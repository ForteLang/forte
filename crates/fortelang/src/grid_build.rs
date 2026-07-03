//! Lower a `device` definition to a dawcore Grid node graph. The device's
//! `param`s are resolved at the instantiation site (compile-time binding —
//! runtime-automatable custom params arrive with forte-core), so the emitted
//! graph is fully baked and runs on the existing per-voice Grid interpreter.

use std::collections::HashMap;

use crate::ast::{Arg, Call, DeviceAst, NodeArg, NodeExpr};
use crate::diag::{Diag, Pos};
use dawcore::model::{GridConn, GridGraph, GridModule, GridModuleKind};

/// NoteIn output ports.
const NOTE_PORTS: &[(&str, usize)] = &[("freq", 0), ("gate", 1), ("vel", 2)];

struct Prim {
    kind: GridModuleKind,
    /// (arg name, input port, default source: Some((node0 port)) = NoteIn)
    inputs: &'static [(&'static str, usize, Option<usize>)],
    /// (arg name, param index, default)
    params: &'static [(&'static str, usize, f32)],
    /// (arg name, param index, choices, values)
    options: &'static [(&'static str, usize, &'static [&'static str], &'static [f32])],
}

/// Values chosen so `(v * 3.999) as u8` lands on the intended index.
const IDX: [f32; 4] = [0.1, 0.35, 0.6, 0.85];

fn prim(name: &str) -> Option<Prim> {
    Some(match name {
        "osc" => Prim {
            kind: GridModuleKind::Osc,
            inputs: &[("freq", 0, Some(0)), ("mod", 1, None)], // freq defaults to note.freq
            params: &[],
            options: &[(
                "shape",
                0,
                &["sine", "saw", "square", "tri"],
                &[IDX[0], IDX[1], IDX[2], IDX[3]],
            )],
        },
        "lfo" => Prim {
            kind: GridModuleKind::Lfo,
            inputs: &[],
            params: &[("rate", 0, 0.3)],
            options: &[(
                "shape",
                1,
                &["sine", "tri", "saw", "square"],
                &[IDX[0], IDX[1], IDX[2], IDX[3]],
            )],
        },
        "adsr" => Prim {
            kind: GridModuleKind::Adsr,
            inputs: &[("gate", 0, Some(1))], // defaults to note.gate
            params: &[("a", 0, 0.05), ("d", 1, 0.3), ("s", 2, 0.6), ("r", 3, 0.25)],
            options: &[],
        },
        "noise" => Prim {
            kind: GridModuleKind::Noise,
            inputs: &[],
            params: &[],
            options: &[],
        },
        "shaper" => Prim {
            kind: GridModuleKind::Shaper,
            inputs: &[("in", 0, None), ("mod", 1, None)],
            params: &[("drive", 0, 0.3)],
            // engine decodes with (v * 2.999) as u8
            options: &[("mode", 1, &["tanh", "clip", "fold"], &[0.1, 0.5, 0.9])],
        },
        "svf" => Prim {
            kind: GridModuleKind::Filter,
            inputs: &[("in", 0, None), ("mod", 1, None)],
            params: &[("cutoff", 0, 0.65), ("reso", 1, 0.2)],
            options: &[],
        },
        "gain" => Prim {
            kind: GridModuleKind::Gain,
            inputs: &[("in", 0, None), ("mod", 1, None)],
            params: &[("level", 0, 0.8)],
            options: &[],
        },
        "mix" => Prim {
            kind: GridModuleKind::Mix,
            inputs: &[("a", 0, None), ("b", 1, None)],
            params: &[],
            options: &[],
        },
        _ => return None,
    })
}

struct Builder<'a> {
    graph: GridGraph,
    named: HashMap<&'a str, usize>,
    params: HashMap<&'a str, f32>,
}

/// Instantiate `dev` with the arguments given at the `instrument` call site.
pub fn instantiate(dev: &DeviceAst, call: &Call) -> Result<GridGraph, Diag> {
    // resolve param values: defaults, then call-site overrides (range-checked)
    let mut params: HashMap<&str, f32> = HashMap::new();
    for p in &dev.params {
        let (lo, hi) = p.range.unwrap_or((0.0, 1.0));
        if !(lo..=hi).contains(&p.default) {
            return Err(Diag::new(
                "E-TYPE-002",
                p.pos,
                format!("param {} の既定値 {} が範囲 {lo}..{hi} の外です", p.name, p.default),
            ));
        }
        params.insert(p.name.as_str(), p.default as f32);
    }
    for (key, arg) in &call.args {
        let Some(p) = dev.params.iter().find(|p| p.name == *key) else {
            let names: Vec<&str> = dev.params.iter().map(|p| p.name.as_str()).collect();
            return Err(Diag::new(
                "E-DEV-002",
                call.pos,
                format!("{} に '{key}' というパラメータはありません(使えるもの: {})", dev.name, names.join(", ")),
            ));
        };
        let Arg::Num(n, apos) = arg else {
            return Err(Diag::new("E-TYPE-004", call.pos, format!("{}.{key} は数値で指定します", dev.name)));
        };
        let (lo, hi) = p.range.unwrap_or((0.0, 1.0));
        if !(lo..=hi).contains(n) {
            return Err(Diag::new(
                "E-TYPE-002",
                *apos,
                format!("{}.{key} = {n} は範囲 {lo}..{hi} の外です", dev.name),
            ));
        }
        params.insert(p.name.as_str(), *n as f32);
    }

    let mut b = Builder {
        graph: GridGraph {
            modules: vec![GridModule {
                kind: GridModuleKind::NoteIn,
                pos: (20.0, 60.0),
                params: Vec::new(),
            }],
            conns: Vec::new(),
        },
        named: HashMap::new(),
        params,
    };

    for (name, expr, pos) in &dev.nodes {
        let src = b.build_expr(expr, dev)?;
        if b.named.insert(name.as_str(), src.0).is_some() {
            return Err(Diag::new("E-GRID-002", *pos, format!("node '{name}' が重複しています")));
        }
        // named refs always read output port 0 of the produced node; note
        // ports keep their own port via build_expr when used inline
        if src.1 != 0 {
            return Err(Diag::new(
                "E-GRID-003",
                *pos,
                format!("node '{name}' に note ポートを直接束縛できません(式の中で使ってください)"),
            ));
        }
    }

    let Some(out_expr) = &dev.out else {
        return Err(Diag::new("E-GRID-001", dev.pos, format!("device {} に out がありません", dev.name)));
    };
    let src = b.build_expr(out_expr, dev)?;
    let out_idx = b.add_module(GridModuleKind::Out, Vec::new());
    b.graph.conns.push(GridConn { from: src, to: (out_idx, 0) });

    Ok(b.graph)
}

impl<'a> Builder<'a> {
    fn add_module(&mut self, kind: GridModuleKind, params: Vec<f32>) -> usize {
        let i = self.graph.modules.len();
        self.graph.modules.push(GridModule {
            kind,
            pos: (140.0 + 130.0 * i as f32, 40.0 + 70.0 * (i % 3) as f32),
            params,
        });
        i
    }

    /// Build an expression; returns (node index, output port).
    fn build_expr(&mut self, expr: &'a NodeExpr, dev: &'a DeviceAst) -> Result<(usize, usize), Diag> {
        match expr {
            NodeExpr::NotePort(port, pos) => {
                let Some(&(_, idx)) = NOTE_PORTS.iter().find(|(n, _)| n == port) else {
                    return Err(Diag::new(
                        "E-GRID-003",
                        *pos,
                        format!("note.{port} はありません(freq / gate / vel)"),
                    ));
                };
                Ok((0, idx))
            }
            NodeExpr::Ref(name, pos) => {
                if let Some(&idx) = self.named.get(name.as_str()) {
                    return Ok((idx, 0));
                }
                if self.params.contains_key(name.as_str()) {
                    return Err(Diag::new(
                        "E-GRID-003",
                        *pos,
                        format!("param '{name}' は数値引数の位置でのみ使えます(信号入力には node を)"),
                    ));
                }
                Err(Diag::new(
                    "E-GRID-002",
                    *pos,
                    format!("node '{name}' が(この行より前に)定義されていません"),
                ))
            }
            NodeExpr::Call { name, args, pos } => {
                let Some(spec) = prim(name) else {
                    return Err(Diag::new(
                        "E-GRID-004",
                        *pos,
                        format!("DSP プリミティブ '{name}' はありません(osc / noise / lfo / adsr / svf / shaper / gain / mix)"),
                    ));
                };
                // params first (defaults), then wire inputs
                let mut pvals: Vec<f32> = {
                    let max_idx = spec
                        .params
                        .iter()
                        .map(|(_, i, _)| *i)
                        .chain(spec.options.iter().map(|(_, i, _, _)| *i))
                        .max()
                        .map(|m| m + 1)
                        .unwrap_or(0);
                    vec![0.0; max_idx]
                };
                for (_, idx, default) in spec.params {
                    pvals[*idx] = *default;
                }
                for (_, idx, _, values) in spec.options {
                    pvals[*idx] = values[0];
                }

                let mut pending_inputs: Vec<(usize, (usize, usize))> = Vec::new();
                let mut seen_inputs: Vec<usize> = Vec::new();

                for (key, arg) in args {
                    if let Some((_, port, _)) = spec.inputs.iter().find(|(n, _, _)| n == key) {
                        let src = match arg {
                            NodeArg::Expr(e) => self.build_expr(e, dev)?,
                            NodeArg::Num(_, p) | NodeArg::Str(_, p) => {
                                return Err(Diag::new(
                                    "E-GRID-003",
                                    *p,
                                    format!("{name}.{key} は信号入力です(node か note.* を渡してください)"),
                                ))
                            }
                        };
                        pending_inputs.push((*port, src));
                        seen_inputs.push(*port);
                    } else if let Some((_, idx, _)) = spec.params.iter().find(|(n, _, _)| n == key) {
                        let v = self.numeric_arg(name, key, arg, dev)?;
                        pvals[*idx] = v;
                    } else if let Some((_, idx, choices, values)) =
                        spec.options.iter().find(|(n, _, _, _)| n == key)
                    {
                        let NodeArg::Str(s, p) = arg else {
                            return Err(Diag::new(
                                "E-TYPE-004",
                                *pos,
                                format!("{name}.{key} は文字列で指定します({})", choices.join(" / ")),
                            ));
                        };
                        let Some(ci) = choices.iter().position(|c| *c == s.to_ascii_lowercase()) else {
                            return Err(Diag::new(
                                "E-TYPE-005",
                                *p,
                                format!("{name}.{key} に '{s}' は使えません({})", choices.join(" / ")),
                            ));
                        };
                        pvals[*idx] = values[ci];
                    } else {
                        let mut names: Vec<&str> = spec.inputs.iter().map(|(n, _, _)| *n).collect();
                        names.extend(spec.params.iter().map(|(n, _, _)| *n));
                        names.extend(spec.options.iter().map(|(n, _, _, _)| *n));
                        return Err(Diag::new(
                            "E-DEV-002",
                            *pos,
                            format!("{name} に '{key}' という引数はありません(使えるもの: {})", names.join(", ")),
                        ));
                    }
                }

                let idx = self.add_module(spec.kind, pvals);
                for (port, src) in pending_inputs {
                    self.graph.conns.push(GridConn { from: src, to: (idx, port) });
                }
                // unwired inputs with defaults connect to NoteIn
                for (aname, port, default) in spec.inputs {
                    if seen_inputs.contains(port) {
                        continue;
                    }
                    match default {
                        Some(note_port) => self
                            .graph
                            .conns
                            .push(GridConn { from: (0, *note_port), to: (idx, *port) }),
                        None if *aname == "in" || *aname == "a" || *aname == "b" => {
                            return Err(Diag::new(
                                "E-GRID-001",
                                *pos,
                                format!("{name} に必須入力 '{aname}' がありません"),
                            ))
                        }
                        None => {} // optional (mod)
                    }
                }
                Ok((idx, 0))
            }
        }
    }

    fn numeric_arg(
        &self,
        prim_name: &str,
        key: &str,
        arg: &NodeArg,
        _dev: &DeviceAst,
    ) -> Result<f32, Diag> {
        match arg {
            NodeArg::Num(n, pos) => {
                if !(0.0..=1.0).contains(n) {
                    return Err(Diag::new(
                        "E-TYPE-002",
                        *pos,
                        format!("{prim_name}.{key} = {n} は 0..1 の範囲外です"),
                    ));
                }
                Ok(*n as f32)
            }
            NodeArg::Expr(NodeExpr::Ref(name, pos)) => {
                self.params.get(name.as_str()).copied().ok_or_else(|| {
                    Diag::new(
                        "E-GRID-002",
                        *pos,
                        format!("'{name}' は param でも node でもありません"),
                    )
                })
            }
            NodeArg::Str(_, pos) => Err(Diag::new(
                "E-TYPE-004",
                *pos,
                format!("{prim_name}.{key} は数値で指定します"),
            )),
            NodeArg::Expr(e) => {
                let pos = match e {
                    NodeExpr::Call { pos, .. } | NodeExpr::Ref(_, pos) | NodeExpr::NotePort(_, pos) => *pos,
                };
                Err(Diag::new(
                    "E-GRID-003",
                    pos,
                    format!("{prim_name}.{key} は数値パラメータです(信号は接続できません)"),
                ))
            }
        }
    }
}

/// A `Pos` for synthesized diagnostics when none is available.
pub fn no_pos() -> Pos {
    Pos { line: 1, col: 1 }
}
