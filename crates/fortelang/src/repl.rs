//! `forte repl` — type a pattern, hear it immediately; layer tracks like a
//! loop station. The session keeps a live engine; every input hot-swaps the
//! arrangement without stopping the transport. `:save` turns the jam into a
//! real multi-track .forte file, and `:undo` rewinds any step.

use std::io::{BufRead, Write};

use dawcore::command::Command;
use dawcore::sync::full_sync;

#[derive(Clone)]
pub struct TrackState {
    pub name: String,
    pub instrument: String,
    pub inserts: Vec<String>,
    pub pattern: Option<String>,
    pub volume: Option<f64>,
    pub pan: Option<f64>,
}

impl TrackState {
    fn new(name: &str) -> Self {
        TrackState {
            name: name.into(),
            instrument: "polymer()".into(),
            inserts: Vec::new(),
            pattern: None,
            volume: None,
            pan: None,
        }
    }
}

#[derive(Clone)]
pub struct Session {
    pub tempo: f64,
    pub imports: Vec<String>,
    pub devices: Vec<String>,
    pub lets: Vec<String>,
    pub tracks: Vec<TrackState>,
    pub current: usize,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            tempo: 120.0,
            imports: Vec::new(),
            devices: Vec::new(),
            lets: Vec::new(),
            tracks: vec![TrackState::new("Main")],
            current: 0,
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
    fn cur(&mut self) -> &mut TrackState {
        &mut self.tracks[self.current]
    }

    /// Render the whole session as a valid multi-track .forte source.
    pub fn source(&self) -> String {
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
        let mut any = false;
        for t in &self.tracks {
            let Some(pattern) = &t.pattern else { continue };
            any = true;
            s.push_str(&format!("\n  track {} {{\n    instrument {}\n", t.name, t.instrument));
            for fx in &t.inserts {
                s.push_str(&format!("    insert {fx}\n"));
            }
            if let Some(v) = t.volume {
                s.push_str(&format!("    volume {v}\n"));
            }
            if let Some(p) = t.pan {
                s.push_str(&format!("    pan {p}\n"));
            }
            s.push_str(&format!("    play {pattern} at bars(1..8)\n  }}\n"));
        }
        if !any {
            // a song needs at least one track; silence until a pattern arrives
            s.push_str("\n  track Main {\n    instrument polymer()\n    play beat`----` at bars(1..8)\n  }\n");
        }
        s.push('}');
        s.push('\n');
        s
    }

    /// Validate a candidate session by compiling its source.
    fn validated(&self) -> Result<(), String> {
        crate::compile_with_loader(&self.source(), &crate::FsLoader, ".")
            .map(|_| ())
            .map_err(|ds| ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
    }

    /// Apply a mutation transactionally: clone, mutate, compile-check; commit
    /// only on success so a typo can never poison the session.
    fn try_mutate(
        &mut self,
        undo: &mut Vec<Session>,
        f: impl FnOnce(&mut Session),
        ok: impl FnOnce(&Session) -> Action,
    ) -> Action {
        let mut candidate = self.clone();
        f(&mut candidate);
        match candidate.validated() {
            Ok(()) => {
                undo.push(self.clone());
                *self = candidate;
                ok(self)
            }
            Err(e) => Action::Msg(e),
        }
    }

    pub fn eval(&mut self, input: &str, undo: &mut Vec<Session>) -> Action {
        let input = input.trim();
        if input.is_empty() || input.starts_with("//") {
            return Action::None;
        }

        // ---- directives ----------------------------------------------------
        if let Some(rest) = input.strip_prefix(':') {
            let (cmd, arg) = rest.split_once(' ').map(|(c, a)| (c, a.trim())).unwrap_or((rest.trim(), ""));
            return match cmd {
                "help" | "h" => Action::Msg(HELP.into()),
                "quit" | "q" | "exit" => Action::Quit,
                "stop" => Action::Stop,
                "show" => Action::Msg(self.source()),
                "save" if !arg.is_empty() => Action::Save(arg.to_string()),
                "undo" => match undo.pop() {
                    Some(prev) => {
                        *self = prev;
                        Action::Play(self.source())
                    }
                    None => Action::Msg("これ以上戻れません".into()),
                },
                "tracks" => Action::Msg(
                    self.tracks
                        .iter()
                        .enumerate()
                        .map(|(i, t)| {
                            format!(
                                "{}{}\t{}\t{}",
                                if i == self.current { "▶ " } else { "  " },
                                t.name,
                                t.instrument,
                                t.pattern.as_deref().unwrap_or("(空)")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                "track" if !arg.is_empty() => {
                    if let Some(i) = self.tracks.iter().position(|t| t.name == arg) {
                        self.current = i;
                        Action::Msg(format!("▶ {arg}(既存トラックに切替)"))
                    } else {
                        self.tracks.push(TrackState::new(arg));
                        self.current = self.tracks.len() - 1;
                        Action::Msg(format!("▶ {arg}(新規トラック — パターンを打つと重なります)"))
                    }
                }
                "drop" if !arg.is_empty() => {
                    let Some(i) = self.tracks.iter().position(|t| t.name == arg) else {
                        return Action::Msg(format!("トラック '{arg}' はありません"));
                    };
                    self.try_mutate(
                        undo,
                        |s| {
                            s.tracks.remove(i);
                            if s.tracks.is_empty() {
                                s.tracks.push(TrackState::new("Main"));
                            }
                            s.current = s.current.min(s.tracks.len() - 1);
                        },
                        |s| Action::Play(s.source()),
                    )
                }
                "tempo" => match arg.parse::<f64>() {
                    Ok(t) if (20.0..=400.0).contains(&t) => {
                        self.try_mutate(undo, |s| s.tempo = t, |s| Action::Play(s.source()))
                    }
                    _ => Action::Msg(format!("tempo は 20..400 で: {arg}")),
                },
                "inst" if !arg.is_empty() => {
                    let a = arg.to_string();
                    self.try_mutate(undo, |s| s.cur().instrument = a, |s| Action::Play(s.source()))
                }
                "vol" => match arg.parse::<f64>() {
                    Ok(v) if (0.0..=1.0).contains(&v) => {
                        self.try_mutate(undo, |s| s.cur().volume = Some(v), |s| Action::Play(s.source()))
                    }
                    _ => Action::Msg("vol は 0..1 で".into()),
                },
                "pan" => match arg.parse::<f64>() {
                    Ok(v) if (-1.0..=1.0).contains(&v) => {
                        self.try_mutate(undo, |s| s.cur().pan = Some(v), |s| Action::Play(s.source()))
                    }
                    _ => Action::Msg("pan は -1..1 で".into()),
                },
                "fx" if arg == "clear" => {
                    self.try_mutate(undo, |s| s.cur().inserts.clear(), |s| Action::Play(s.source()))
                }
                "fx" if !arg.is_empty() => {
                    let a = arg.to_string();
                    self.try_mutate(undo, |s| s.cur().inserts.push(a), |s| Action::Play(s.source()))
                }
                _ => Action::Msg(format!("不明なコマンド :{cmd}(:help で一覧)")),
            };
        }

        // ---- session declarations ------------------------------------------
        if input.starts_with("let ") {
            let a = input.to_string();
            return self.try_mutate(undo, |s| s.lets.push(a), |_| Action::Msg("定義しました".into()));
        }
        if input.starts_with("import ") {
            let a = input.to_string();
            return self.try_mutate(undo, |s| s.imports.push(a), |_| Action::Msg("import しました".into()));
        }
        if input.starts_with("device ") || input.starts_with("device\t") {
            let a = input.to_string();
            return self
                .try_mutate(undo, |s| s.devices.push(a), |_| Action::Msg("device を定義しました".into()));
        }

        // ---- anything else is a pattern for the current track --------------
        let a = input.to_string();
        self.try_mutate(undo, |s| s.cur().pattern = Some(a), |s| Action::Play(s.source()))
    }
}

const HELP: &str = "\
パターンを打つと現在のトラックで即ループ再生(再生は止まりません):
  beat`x--- x-x-` / notes`C4:1/2 …` / prog`Am | F | C | G`
  chords(…) / arp(…, rate: 0.25) / bass(…)
重ねる(ループステーション):
  :track Bass        トラックを作成/切替(以後のパターン・:inst はそのトラックへ)
  :tracks            一覧(▶ が現在)   :drop Bass  削除
宣言(セッションに積む): let 名前 = … / device … { … } / import …
調整: :tempo 140  :inst polymer(wave: \"saw\")  :fx reverb(mix: 0.3)  :fx clear
      :vol 0.7  :pan -0.3
その他: :undo(一手戻る) :show :save jam.forte :stop :quit";

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
    println!("forte repl — パターンを打てば鳴ります。:track で重ねる(:help / :quit)");

    let mut session = Session::default();
    let mut undo: Vec<Session> = Vec::new();
    let mut prev_tracks = 0usize;
    let mut playing = false;
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        print!("forte:{}> ", session.tracks[session.current].name);
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
        match session.eval(&buf, &mut undo) {
            Action::None => {}
            Action::Msg(m) => println!("{m}"),
            Action::Stop => {
                audio.handle.send(Command::Stop);
                playing = false;
                println!("stopped");
            }
            Action::Quit => return 0,
            Action::Save(path) => {
                match std::fs::write(&path, session.source()) {
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
                        println!(
                            "♪ playing ({} bpm, {} tracks, loop {} beats)",
                            p.tempo,
                            p.tracks.len(),
                            len
                        );
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
