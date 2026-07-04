//! Minimal Language Server (stdio JSON-RPC, no async runtime): full-text
//! document sync + push diagnostics from the Forte compiler. Enough for
//! "errors appear as you type" in VSCode (SRS-LSP-001 first slice).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};

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
                            "completionProvider": {},
                            "hoverProvider": true,
                            "documentFormattingProvider": true,
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
            "textDocument/formatting" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                let result = docs.get(uri).and_then(|text| {
                    let formatted = crate::fmt::format(text).ok()?;
                    if formatted == *text {
                        return Some(Value::Array(vec![]));
                    }
                    let lines = text.lines().count() as u64 + 1;
                    Some(json!([{
                        "range": {"start": {"line": 0, "character": 0},
                                   "end": {"line": lines, "character": 0}},
                        "newText": formatted,
                    }]))
                });
                respond(&mut writer, id, result.unwrap_or(Value::Null));
            }
            "textDocument/completion" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                let items = docs.get(uri).map(|t| completions(t)).unwrap_or_default();
                respond(&mut writer, id, json!(items));
            }
            "textDocument/hover" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let ch = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let doc = docs.get(uri).and_then(|t| hover(t, line, ch));
                respond(
                    &mut writer,
                    id,
                    doc.map(|md| json!({"contents": {"kind": "markdown", "value": md}}))
                        .unwrap_or(Value::Null),
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
    // resolve imports relative to the document when it lives on disk
    let base_dir = uri
        .strip_prefix("file://")
        .and_then(|p| std::path::Path::new(p).parent())
        .map(|p| p.to_string_lossy().into_owned());
    let result = match &base_dir {
        Some(dir) => crate::check_with_loader(text, &crate::FsLoader, dir),
        None => crate::check_with_loader(text, &crate::NoLoader, ""),
    };
    let diags: Vec<Value> = match result {
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

// ---- completion / hover ------------------------------------------------------

/// (word, hover markdown, completion detail). One table drives both features.
const DOCS: &[(&str, &str, &str)] = &[
    ("song", "曲の定義: `song \"名前\" { tempo / meter / key / let / section / track / return }`", "keyword"),
    ("track", "トラック: `track 名前 { instrument … play … }`", "keyword"),
    ("return", "リターントラック: `return 名前 { insert reverb(...) }` — `send 名前 0.3` で送る", "keyword"),
    ("section", "名前付き区間: `section verse = bars(1..8)` → `play x at verse`", "keyword"),
    ("device", "自作音源/エフェクト: `device 名前 : Instrument|Effect { param / node / out }`(Effect の入力は audio.in、insert で使う)", "keyword"),
    ("tempo", "テンポ: `tempo 120bpm`", "keyword"),
    ("meter", "拍子: `meter 4/4`", "keyword"),
    ("key", "キー: `key D minor`", "keyword"),
    ("play", "配置: `play パターン at bars(1..8)` / `at セクション名`", "keyword"),
    ("audio", "録音テイクの配置: `audio take at bars(2..3)`(要 `import take from \"./t.frec\"`)", "keyword"),
    ("send", "ポストフェーダーセンド: `send Space 0.35`", "keyword"),
    ("automate", "ボリュームオートメーション: `automate volume from 0.2 to 0.8 over bars(1..8)`(over にセクション名も可)", "keyword"),
    ("modulate", "LFO モジュレーション: `modulate cutoff with lfo(rate: 0.3, amount: 0.4, shape: \"sine\")`", "keyword"),
    ("sampler", "ビルトインサンプラー: `sampler(sample: \"Kick\"|\"Snare\"|\"Hat\")` / `sampler(take: x, root: A3, start:, end:, loop:, reverse:)`", "instrument"),
    ("kit", "録音テイクのドラムキット: `kit(C2: kickTake, D2: snareTake, gain: 0.9)`(各パッドは原速再生)", "instrument"),
    ("polymer", "2osc 減算シンセ: wave(sine/saw/square/tri), cutoff, reso, attack, decay, sustain, release, detune, sub, filtenv", "instrument"),
    ("grid", "モジュラー音源(既定パッチ): `grid()`", "instrument"),
    ("filter", "マルチモードフィルタ: type(lp/hp/bp/notch), cutoff, reso", "effect"),
    ("eq", "3 バンド EQ: low, mid, high", "effect"),
    ("drive", "ディストーション: drive", "effect"),
    ("delay", "ピンポンディレイ: time, fdbk, mix", "effect"),
    ("reverb", "FDN リバーブ: size, decay, mix", "effect"),
    ("chords", "進行をブロックコードで鳴らす: `chords(進行)`", "pattern fn"),
    ("arp", "アルペジオ: `arp(進行, rate: 0.25, style: \"up|down|updown\")`", "pattern fn"),
    ("bass", "ルート音ライン: `bass(進行, rate: 0.5)`", "pattern fn"),
    ("beat", "ステップ列: `beat\\`x--- X-x-\\``(x=ヒット X=アクセント -=休符)", "literal"),
    ("notes", "ノート列: `notes\\`C4:1/2 [E4 G4]:1 _:1\\``", "literal"),
    ("prog", "コード進行: `prog\\`Em | C G | D\\``(| が小節)。類似検索の対象になる", "literal"),
    ("osc", "DSP: オシレータ `osc(shape: \"saw\", freq: note.freq)`", "dsp"),
    ("noise", "DSP: ホワイトノイズ `noise()`(決定論的 — 同じソースは同じビット)", "dsp"),
    ("sample", "DSP: 録音テイクを音源に `sample(take: <takeスロット>, start:, end:, loop:, reverse:)`(device 冒頭で `take voice` を宣言)", "dsp"),
    ("shaper", "DSP: ウェーブシェイパー `shaper(in:, drive: 0.4, mode: \"tanh|clip|fold\")`", "dsp"),
    ("lfo", "DSP: LFO `lfo(rate: 0.3, shape: \"sine\")`", "dsp"),
    ("adsr", "DSP: エンベロープ `adsr(a,d,s,r, gate: note.gate)`", "dsp"),
    ("svf", "DSP: フィルタ `svf(in:, cutoff:, reso:, mod:)`", "dsp"),
    ("gain", "DSP: ゲイン `gain(in:, level:, mod:)`", "dsp"),
    ("mix", "DSP: 2 入力加算 `mix(a:, b:)`", "dsp"),
];

/// Static vocabulary plus names defined in this document (lets, sections,
/// devices, returns).
fn completions(text: &str) -> Vec<Value> {
    let mut items: Vec<Value> = DOCS
        .iter()
        .map(|(w, doc, detail)| {
            json!({"label": w, "kind": 14, "detail": detail,
                   "documentation": {"kind": "markdown", "value": doc}})
        })
        .collect();
    if let Ok(file) = crate::parser::parse(text) {
        for d in &file.devices {
            items.push(json!({"label": d.name, "kind": 7, "detail": "user device"}));
        }
        if let Some(song) = &file.song {
            for l in &song.lets {
                items.push(json!({"label": l.name, "kind": 6, "detail": format!("let ({})", l.value.kind)}));
            }
            for s in &song.sections {
                items.push(json!({"label": s.name, "kind": 6,
                                   "detail": format!("section bars({}..{})", s.bars.0, s.bars.1)}));
            }
            for r in &song.returns {
                items.push(json!({"label": r.name, "kind": 6, "detail": "return"}));
            }
        }
    }
    items
}

fn hover(text: &str, line: usize, ch: usize) -> Option<String> {
    let l = text.lines().nth(line)?;
    let chars: Vec<char> = l.chars().collect();
    if ch > chars.len() {
        return None;
    }
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut start = ch.min(chars.len().saturating_sub(1));
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = start;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    DOCS.iter().find(|(w, _, _)| *w == word).map(|(w, doc, _)| format!("**{w}** — {doc}"))
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
