//! Protocol-level test: spawn the real `forte lsp` binary, speak LSP over
//! stdio, and assert diagnostics are pushed for broken and fixed documents.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Lsp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Lsp {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_forte"))
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn forte lsp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Lsp { child, stdin, stdout, next_id: 1 }
    }
    fn send(&mut self, msg: serde_json::Value) {
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.stdin.flush().unwrap();
    }
    fn request(&mut self, method: &str, params: serde_json::Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        id
    }
    fn notify(&mut self, method: &str, params: serde_json::Value) {
        self.send(serde_json::json!({"jsonrpc":"2.0","method":method,"params":params}));
    }
    fn read(&mut self) -> serde_json::Value {
        let mut len = 0usize;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(v) = line.strip_prefix("Content-Length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut buf = vec![0u8; len];
        self.stdout.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }
    /// Read messages until the next publishDiagnostics for `uri`.
    fn diagnostics_for(&mut self, uri: &str) -> Vec<serde_json::Value> {
        loop {
            let msg = self.read();
            if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == uri {
                return msg["params"]["diagnostics"].as_array().unwrap().clone();
            }
        }
    }
}

#[test]
fn lsp_pushes_and_clears_diagnostics() {
    let mut lsp = Lsp::start();

    lsp.request("initialize", serde_json::json!({"capabilities": {}}));
    let init = lsp.read();
    assert_eq!(init["result"]["capabilities"]["textDocumentSync"], 1);
    lsp.notify("initialized", serde_json::json!({}));

    // open a broken document -> one diagnostic with a forte error code
    let uri = "file:///song.forte";
    let broken = r#"song "X" { tempo 120bpm track A { instrument polymer(cutof: 0.5) play beat`x---` at bars(1..2) } }"#;
    lsp.notify(
        "textDocument/didOpen",
        serde_json::json!({"textDocument": {"uri": uri, "languageId": "forte", "version": 1, "text": broken}}),
    );
    let diags = lsp.diagnostics_for(uri);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["code"], "E-DEV-002");
    assert_eq!(diags[0]["source"], "forte");
    assert!(diags[0]["message"].as_str().unwrap().contains("cutoff"));

    // fix it -> diagnostics clear
    let fixed = broken.replace("cutof:", "cutoff:");
    lsp.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": {"uri": uri, "version": 2},
            "contentChanges": [{"text": fixed}],
        }),
    );
    let diags = lsp.diagnostics_for(uri);
    assert!(diags.is_empty(), "{diags:?}");

    // clean shutdown
    lsp.request("shutdown", serde_json::Value::Null);
    let _ = lsp.read();
    lsp.notify("exit", serde_json::Value::Null);
    let status = lsp.child.wait().unwrap();
    assert!(status.success());
}
