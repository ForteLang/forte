//! `forte repl` — type a pattern, hear it immediately. The session keeps a
//! live engine; every input hot-swaps the loop without stopping the
//! transport, exactly like `forte play`'s file watching but at line
//! granularity. `:save` turns the jam into a real .forte file.

use std::io::{BufRead, Write};

use dawcore::command::Command;
use dawcore::sync::full_sync;

pub struct Session {
    pub tempo: f64,
    pub instrument: String,
    pub inserts: Vec<String>,
    pub imports: Vec<String>,
    pub devices: Vec<String>,
    pub lets: Vec<String>,
    pub last_pattern: Option<String>,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            tempo: 120.0,
            instrument: "polymer()".into(),
            inserts: Vec::new(),
            imports: Vec::new(),
            devices: Vec::new(),
            lets: Vec::new(),
            last_pattern: None,
        }
    }
}

pub enum Action {
    None,
    Msg(String),
    Play(String), // full song source to hot-swap
    Stop,
    Save(String),
    Quit,
}

impl Session {
    /// Render the whole session as a valid .forte source around `pattern`.
    pub fn source_for(&self, pattern: &str) -> String {
        let mut s = String::new();
        for im in &self.imports {
            s.push_str(im);
            s.push('\n');
        }
        for d in &self.devices {
            s.push_str(d);
            s.push_str("\n\n");
        }
        s.push_str(&format!("song \"repl\" {{\n  tempo {}bpm\n", self.tempo));
        for l in &self.lets {
            s.push_str("  ");
            s.push_str(l);
            s.push('\n');
        }
        s.push_str("\n  track Repl {\n");
        s.push_str(&format!("    instrument {}\n", self.instrument));
        for fx in &self.inserts {
            s.push_str(&format!("    insert {fx}\n"));
        }
        s.push_str(&format!("    play {pattern} at bars(1..8)\n  }}\n}}\n"));
        s
    }

    fn placeholder(&self) -> &str {
        self.last_pattern.as_deref().unwrap_or("beat`----`")
    }

    /// Validate a candidate session change by compiling the resulting source.
    fn validated(&self, source: &str) -> Result<(), String> {
        crate::compile_with_loader(source, &crate::FsLoader, ".")
            .map(|_| ())
            .map_err(|ds| ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
    }

    pub fn eval(&mut self, input: &str) -> Action {
        let input = input.trim();
        if input.is_empty() || input.starts_with("//") {
            return Action::None;
        }

        // ---- directives ----------------------------------------------------
        if let Some(rest) = input.strip_prefix(':') {
            let (cmd, arg) = rest.split_once(' ').unwrap_or((rest, ""));
            return match cmd {
                "help" | "h" => Action::Msg(HELP.into()),
                "quit" | "q" | "exit" => Action::Quit,
                "stop" => Action::Stop,
                "show" => Action::Msg(self.source_for(self.placeholder())),
                "save" if !arg.is_empty() => Action::Save(arg.to_string()),
                "tempo" => match arg.parse::<f64>() {
                    Ok(t) if (20.0..=400.0).contains(&t) => {
                        self.tempo = t;
                        Action::Play(self.source_for(self.placeholder().to_string().as_str()))
                    }
                    _ => Action::Msg(format!("tempo は 20..400 で: {arg}")),
                },
                "inst" if !arg.is_empty() => {
                    let old = std::mem::replace(&mut self.instrument, arg.to_string());
                    match self.validated(&self.source_for(self.placeholder())) {
                        Ok(()) => Action::Play(self.source_for(self.placeholder().to_string().as_str())),
                        Err(e) => {
                            self.instrument = old;
                            Action::Msg(e)
                        }
                    }
                }
                "fx" if arg == "clear" => {
                    self.inserts.clear();
                    Action::Play(self.source_for(self.placeholder().to_string().as_str()))
                }
                "fx" if !arg.is_empty() => {
                    self.inserts.push(arg.to_string());
                    match self.validated(&self.source_for(self.placeholder())) {
                        Ok(()) => Action::Play(self.source_for(self.placeholder().to_string().as_str())),
                        Err(e) => {
                            self.inserts.pop();
                            Action::Msg(e)
                        }
                    }
                }
                _ => Action::Msg(format!("不明なコマンド :{cmd}(:help で一覧)")),
            };
        }

        // ---- session declarations ------------------------------------------
        if input.starts_with("let ") {
            self.lets.push(input.to_string());
            return match self.validated(&self.source_for(self.placeholder())) {
                Ok(()) => Action::Msg(format!("定義しました: {input}")),
                Err(e) => {
                    self.lets.pop();
                    Action::Msg(e)
                }
            };
        }
        if input.starts_with("import ") {
            self.imports.push(input.to_string());
            return match self.validated(&self.source_for(self.placeholder())) {
                Ok(()) => Action::Msg(format!("import しました")),
                Err(e) => {
                    self.imports.pop();
                    Action::Msg(e)
                }
            };
        }
        if input.starts_with("device ") || input.starts_with("device\t") {
            self.devices.push(input.to_string());
            return match self.validated(&self.source_for(self.placeholder())) {
                Ok(()) => Action::Msg("device を定義しました".into()),
                Err(e) => {
                    self.devices.pop();
                    Action::Msg(e)
                }
            };
        }

        // ---- anything else is a pattern: play it now -----------------------
        let src = self.source_for(input);
        match self.validated(&src) {
            Ok(()) => {
                self.last_pattern = Some(input.to_string());
                Action::Play(src)
            }
            Err(e) => Action::Msg(e),
        }
    }
}

const HELP: &str = "\
パターンを打つと即ループ再生されます(再生を止めずに差し替え):
  beat`x--- x-x-`             ステップ
  notes`C4:1/2 E4:1/2 G4:1`   ノート列
  prog`Am | F | C | G`        進行(ブロックコード)
  chords(prog`…`) / arp(prog`…`, rate: 0.25) / bass(prog`…`)
宣言(セッションに積まれます):
  let name = beat`…`          → 以後 name だけで再生
  device Name : Instrument { … }   (複数行 OK)
  import { X } from \"./lib.forte\"
コマンド:
  :tempo 140      :inst polymer(wave: \"saw\")   :inst WarmLead(cutoff: 0.7)
  :fx reverb(mix: 0.3)   :fx clear
  :show(現在のソース)  :save jam.forte(曲として保存)
  :stop  :quit";

/// Count how far `line` leaves us inside braces / backticks / block comments,
/// so multi-line device blocks and literals can be collected.
fn scan_line(line: &str, depth: &mut i32, in_backtick: &mut bool, in_block: &mut bool) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if *in_block {
            if c == '*' && chars.get(i + 1) == Some(&'/') {
                *in_block = false;
                i += 1;
            }
        } else if *in_backtick {
            if c == '`' {
                *in_backtick = false;
            }
        } else if in_string {
            if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '/' if chars.get(i + 1) == Some(&'/') => break,
                '/' if chars.get(i + 1) == Some(&'*') => {
                    *in_block = true;
                    i += 1;
                }
                '`' => *in_backtick = true,
                '"' => in_string = true,
                '{' => *depth += 1,
                '}' => *depth -= 1,
                _ => {}
            }
        }
        i += 1;
    }
}

/// Interactive loop on stdin/stdout. Returns the process exit code.
pub fn run() -> i32 {
    let mut audio = crate::audio::start();
    if audio.silent {
        eprintln!("audio: 出力デバイスなし — 無音で走ります({})", audio.device_name);
    }
    println!("forte repl — パターンを打てば鳴ります(:help でヘルプ, :quit で終了)");

    let mut session = Session::default();
    let mut prev_tracks = 0usize;
    let mut playing = false;
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        print!("forte> ");
        let _ = std::io::stdout().flush();
        // collect a full input (multi-line for device blocks / literals)
        let mut buf = String::new();
        let (mut depth, mut bt, mut blk) = (0i32, false, false);
        loop {
            let Some(Ok(line)) = lines.next() else {
                return 0; // EOF
            };
            scan_line(&line, &mut depth, &mut bt, &mut blk);
            buf.push_str(&line);
            buf.push('\n');
            if depth <= 0 && !bt && !blk {
                break;
            }
            print!("  ...> ");
            let _ = std::io::stdout().flush();
        }

        audio.handle.collect_garbage();
        match session.eval(&buf) {
            Action::None => {}
            Action::Msg(m) => println!("{m}"),
            Action::Stop => {
                audio.handle.send(Command::Stop);
                playing = false;
                println!("stopped");
            }
            Action::Quit => return 0,
            Action::Save(path) => {
                let src = session.source_for(session.placeholder().to_string().as_str());
                match std::fs::write(&path, &src) {
                    Ok(()) => println!("saved: {path}"),
                    Err(e) => println!("{path}: {e}"),
                }
            }
            Action::Play(src) => {
                match crate::compile_with_loader(&src, &crate::FsLoader, ".") {
                    Ok(p) => {
                        full_sync(&mut audio.handle, &p);
                        for slot in p.tracks.len()..prev_tracks {
                            audio.handle.send(Command::RemoveTrack { slot });
                        }
                        prev_tracks = p.tracks.len();
                        let len = dawcore::bounce::arrangement_len(&p);
                        audio.handle.send(Command::SetLoop { enabled: true, start: 0.0, end: len });
                        audio.handle.send(Command::SetLaunchQuant(0.0));
                        if !playing {
                            audio.handle.send(Command::Play);
                            playing = true;
                        }
                        println!("♪ playing ({} bpm, loop {} beats)", p.tempo, len);
                    }
                    Err(ds) => {
                        for d in ds {
                            println!("{d}");
                        }
                    }
                }
            }
        }
    }
}
