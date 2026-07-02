//! Minimal Language Server (stdio JSON-RPC, no async runtime): full-text
//! document sync + push diagnostics from the Forte compiler. Enough for
//! "errors appear as you type" in VSCode (SRS-LSP-001 first slice).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};

use serde_json::{json, Value};

pub fn run() -> i32 {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let mut docs: HashMap<String, String> = HashMap::new();
    let mut exit_code = 1; // per spec: exit without shutdown -> 1

    while let Some(msg) = read_message(&mut reader) {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => {
                respond(
                    &mut writer,
                    id,
                    json!({
                        "capabilities": {
                            "textDocumentSync": 1, // full
                            "positionEncoding": "utf-16",
                        },
                        "serverInfo": {"name": "forte-lsp", "version": env!("CARGO_PKG_VERSION")},
                    }),
                );
            }
            "initialized" => {}
            "shutdown" => {
                exit_code = 0;
                respond(&mut writer, id, Value::Null);
            }
            "exit" => break,
            "textDocument/didOpen" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                let text = params["textDocument"]["text"].as_str().unwrap_or("").to_string();
                publish(&mut writer, &uri, &text);
                docs.insert(uri, text);
            }
            "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                // full sync: the last content change carries the whole text
                if let Some(text) = params["contentChanges"]
                    .as_array()
                    .and_then(|a| a.last())
                    .and_then(|c| c["text"].as_str())
                {
                    publish(&mut writer, &uri, text);
                    docs.insert(uri, text.to_string());
                }
            }
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                docs.remove(uri);
                notify(
                    &mut writer,
                    "textDocument/publishDiagnostics",
                    json!({"uri": uri, "diagnostics": []}),
                );
            }
            _ => {
                // politely reject unknown requests; ignore unknown notifications
                if let Some(id) = id {
                    respond_err(&mut writer, id, -32601, &format!("method not found: {method}"));
                }
            }
        }
    }
    exit_code
}

fn publish(writer: &mut impl Write, uri: &str, text: &str) {
    let diags: Vec<Value> = match crate::compile_str(text) {
        Ok(_) => Vec::new(),
        Err(ds) => ds
            .iter()
            .map(|d| {
                let line = d.pos.line.saturating_sub(1);
                let col = d.pos.col.saturating_sub(1);
                json!({
                    "range": {
                        "start": {"line": line, "character": col},
                        "end":   {"line": line, "character": col + 1},
                    },
                    "severity": 1,
                    "code": d.code,
                    "source": "forte",
                    "message": d.message,
                })
            })
            .collect(),
    };
    notify(
        writer,
        "textDocument/publishDiagnostics",
        json!({"uri": uri, "diagnostics": diags}),
    );
}

// ---- framing ---------------------------------------------------------------

fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None; // EOF
        }
        let line = line.trim_end();
        if line.is_empty() {
            break; // end of headers
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            content_length = v.trim().parse().ok();
        }
    }
    let len = content_length?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

fn send(writer: &mut impl Write, v: Value) {
    let body = v.to_string();
    let _ = write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = writer.flush();
}

fn respond(writer: &mut impl Write, id: Option<Value>, result: Value) {
    send(writer, json!({"jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": result}));
}

fn respond_err(writer: &mut impl Write, id: Value, code: i64, message: &str) {
    send(
        writer,
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    );
}

fn notify(writer: &mut impl Write, method: &str, params: Value) {
    send(writer, json!({"jsonrpc": "2.0", "method": method, "params": params}));
}
