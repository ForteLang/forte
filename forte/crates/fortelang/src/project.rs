//! The project read side (ADR D-15): enumerate a `forte init` package —
//! meta, blocks, songs, instruments, albums, vendored packages — as the
//! JSON a project-first GUI binds its explorer to.
//!
//! Read-only by design: writes go through `forte edit` (per file,
//! body-addressed), so any host that renders this inventory can already
//! edit everything it shows. Files that fail to parse are reported as
//! `{file, error}` entries instead of aborting the scan — a project
//! explorer must show broken files, not hide the project behind them.

use std::path::{Path, PathBuf};

use crate::ast::{FileAst, SongAst};
use crate::parser::parse;
use serde_json::{json, Value};

/// Inventory the package rooted at `root` (the directory `forte init`
/// creates — `package.forte` is the marker and is required).
pub fn project_json(root: &Path) -> Result<Value, String> {
    let meta_src = std::fs::read_to_string(root.join("package.forte")).map_err(|_| {
        format!(
            "{} に package.forte がありません(forte init で作るか、パッケージのルートで実行してください)",
            root.display()
        )
    })?;
    let meta = parse(&meta_src)
        .ok()
        .and_then(|ast| ast.blocks.last().map(|b| b.body.clone()))
        .ok_or("package.forte に meta block がありません(block Name { desc … version … })")?;

    Ok(json!({
        "name": meta.name,
        "desc": meta.desc.clone().unwrap_or_default(),
        "version": meta.version.clone().unwrap_or_default(),
        "tags": meta.tags,
        "license": meta.license.clone().unwrap_or_default(),
        "songs": scan(root, "songs", song_entry),
        "blocks": scan(root, "blocks", block_entry),
        "instruments": scan(root, "instruments", device_entry),
        "albums": albums(root),
        "packages": vendored(root),
    }))
}

/// Parse every `.forte` file under `<root>/<sub>` (sorted, recursive one
/// level is enough: init's layout is flat) into explorer entries.
fn scan(root: &Path, sub: &str, entry: fn(&str, &FileAst) -> Value) -> Vec<Value> {
    let mut files: Vec<_> = std::fs::read_dir(root.join(sub))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "forte"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
        .iter()
        .map(|p| {
            let rel = format!("{sub}/{}", p.file_name().unwrap_or_default().to_string_lossy());
            let src = match std::fs::read_to_string(p) {
                Ok(s) => s,
                Err(e) => return json!({"file": rel, "error": e.to_string()}),
            };
            match parse(&src) {
                Ok(ast) => entry(&rel, &ast),
                Err(ds) => json!({"file": rel, "error": ds[0].to_string()}),
            }
        })
        .collect()
}

fn imports_json(ast: &FileAst) -> Vec<Value> {
    ast.imports.iter().map(|i| json!({"names": i.names, "from": i.path})).collect()
}

fn tracks_json(body: &SongAst) -> Vec<Value> {
    body.tracks.iter().map(|t| json!({"name": t.name, "line": t.pos.line})).collect()
}

/// A body's span in bars (max end bar over sections, placements and every
/// track's plays/audio) — what a GUI needs to place the block on a timeline.
fn body_bars(body: &SongAst) -> u32 {
    let at_end = |at: &crate::ast::AtRef| match at {
        crate::ast::AtRef::Bars(_, b) => *b,
        crate::ast::AtRef::Section(name, _) => body
            .sections
            .iter()
            .find(|s| &s.name == name)
            .map(|s| s.bars.1)
            .unwrap_or(0),
    };
    let mut end = 0u32;
    for s in &body.sections {
        end = end.max(s.bars.1);
    }
    for p in &body.places {
        end = end.max(at_end(&p.at));
    }
    for t in &body.tracks {
        for p in &t.plays {
            end = end.max(at_end(&p.at));
        }
        for a in &t.audios {
            end = end.max(at_end(&a.at));
        }
    }
    end.max(1)
}

/// A `songs/*.forte` entry: the song body a Composer opens as a song.
fn song_entry(rel: &str, ast: &FileAst) -> Value {
    let song = ast.song.as_ref().map(|s| {
        json!({
            "name": s.name,
            "desc": s.desc.clone().unwrap_or_default(),
            "tempo": s.tempo.map(|(v, _)| v),
            "sections": s.sections.iter().map(|x| json!({"name": x.name, "bars": [x.bars.0, x.bars.1]})).collect::<Vec<_>>(),
            "tracks": tracks_json(s),
            "places": s.places.len(),
        })
    });
    json!({
        "file": rel,
        "song": song,
        // blocks defined locally in the song file (editable as blocks too)
        "blocks": ast.blocks.iter().map(|b| json!({"name": b.name, "line": b.pos.line})).collect::<Vec<_>>(),
        "imports": imports_json(ast),
    })
}

/// A `blocks/*.forte` entry: each top-level block, editable as a block —
/// the coordinates are exactly the `path` the edit layer takes.
fn block_entry(rel: &str, ast: &FileAst) -> Value {
    let blocks: Vec<Value> = ast
        .blocks
        .iter()
        .map(|b| {
            json!({
                "name": b.name,
                "line": b.pos.line,
                "desc": b.body.desc.clone().unwrap_or_default(),
                "parent": b.parent.as_ref().map(|(p, _)| p.clone()),
                "tempo": b.body.tempo.map(|(v, _)| v),
                "bars": body_bars(&b.body),
                "tracks": tracks_json(&b.body),
                "patterns": b.body.lets.len(),
                "nested": b.body.blocks.iter().map(|n| n.name.clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({"file": rel, "blocks": blocks, "imports": imports_json(ast)})
}

/// An `instruments/*.forte` entry: devices with their `set_arg`-able params.
fn device_entry(rel: &str, ast: &FileAst) -> Value {
    let devices: Vec<Value> = ast
        .devices
        .iter()
        .map(|d| {
            json!({
                "name": d.name,
                "kind": d.kind,
                "line": d.pos.line,
                "params": d.params.iter().map(|p| {
                    let (lo, hi) = p.range.unwrap_or((0.0, 1.0));
                    json!({"name": p.name, "default": p.default, "range": [lo, hi]})
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({"file": rel, "devices": devices, "imports": imports_json(ast)})
}

/// `albums/*/album.forte` + its `.fortesong` pressings, in filename order.
fn albums(root: &Path) -> Vec<Value> {
    let mut dirs: Vec<_> = std::fs::read_dir(root.join("albums"))
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs.iter()
        .filter_map(|d| {
            let meta = std::fs::read_to_string(d.join("album.forte"))
                .ok()
                .and_then(|s| parse(&s).ok())
                .and_then(|a| a.blocks.last().map(|b| b.body.clone()))?;
            let mut tracks: Vec<String> = std::fs::read_dir(d)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .filter(|n| n.ends_with(".fortesong"))
                        .collect()
                })
                .unwrap_or_default();
            tracks.sort();
            Some(json!({
                "dir": format!("albums/{}", d.file_name().unwrap_or_default().to_string_lossy()),
                "title": meta.name,
                "artist": meta.artist.clone().unwrap_or_default(),
                "tracks": tracks,
            }))
        })
        .collect()
}

/// Vendored dependencies: each `packages/<name>_<version>/` with its
/// instruments (project-relative files, `set_arg`-able params) so the
/// palette can offer them without a second scan.
fn vendored(root: &Path) -> Vec<Value> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join("packages"))
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs.iter()
        .map(|d| {
            let name = d.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let sub = format!("packages/{name}");
            let instruments: Vec<Value> = scan(root, &format!("{sub}/instruments"), device_entry);
            json!({ "dir": sub, "name": name, "instruments": instruments })
        })
        .collect()
}
