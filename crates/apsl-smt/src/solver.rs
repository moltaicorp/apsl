
use std::io::Write;
use std::process::{Command, Stdio};

use crate::encode::Smt2Script;

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub bindings: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum SolverResult {
    Unsat,
    Sat(Model),
    Unknown(String),
    Error(String),
}

pub trait Solver {
    fn check(&self, script: &Smt2Script) -> SolverResult;
}

#[derive(Debug, Clone, Default)]
pub struct NullSolver;

impl Solver for NullSolver {
    fn check(&self, _script: &Smt2Script) -> SolverResult {
        SolverResult::Unknown("no SMT backend available".into())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Z3Solver;

impl Solver for Z3Solver {
    fn check(&self, script: &Smt2Script) -> SolverResult {
        run_subprocess("z3", &["-in", "-smt2", "-t:5000"], &script.text)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Cvc5Solver;

impl Solver for Cvc5Solver {
    fn check(&self, script: &Smt2Script) -> SolverResult {
        run_subprocess("cvc5", &["--lang=smt2", "--tlimit=5000"], &script.text)
    }
}

fn run_subprocess(cmd: &str, args: &[&str], stdin_text: &str) -> SolverResult {
    let mut child = match Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return SolverResult::Error(format!("could not spawn {}: {}", cmd, e)),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let mut s = stdin_text.to_string();
        if !s.ends_with('\n') { s.push('\n'); }
        s.push_str("(get-model)\n(exit)\n");
        if let Err(e) = stdin.write_all(s.as_bytes()) {
            return SolverResult::Error(format!("stdin write failed: {}", e));
        }
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return SolverResult::Error(format!("wait failed: {}", e)),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_result(&stdout)
}

fn parse_result(out: &str) -> SolverResult {
    let trimmed = out.trim();
    let mut lines = trimmed.lines();
    let first = lines.next().unwrap_or("").trim();
    match first {
        "unsat" => SolverResult::Unsat,
        "unknown" => SolverResult::Unknown("solver reported unknown".into()),
        "sat" => {
            let model_text = lines.collect::<Vec<_>>().join("\n");
            SolverResult::Sat(parse_model(&model_text))
        }
        other => SolverResult::Error(format!("unexpected solver output: {}", other)),
    }
}

fn parse_model(text: &str) -> Model {
    let mut bindings = Vec::new();
    let mut cur = text;
    while let Some(pos) = cur.find("define-fun ") {
        cur = &cur[pos + "define-fun ".len()..];
        let name_end = cur.find(|c: char| c.is_whitespace()).unwrap_or(cur.len());
        let name = cur[..name_end].trim_matches('|').to_string();
        if let Some(end) = cur.find(')') {
            if let Some(rest) = cur.get(end + 1..) {
                let after_sort = rest.trim_start();
                if let Some(sp) = after_sort.find(|c: char| c.is_whitespace()) {
                    let after_sort = after_sort[sp..].trim_start();
                    let val_end = after_sort.find(')').unwrap_or(after_sort.len());
                    let val = after_sort[..val_end].trim().to_string();
                    bindings.push((name, val));
                }
            }
        }
    }
    Model { bindings }
}

pub fn default_solver() -> Box<dyn Solver> {
    if probe("z3") { return Box::new(Z3Solver); }
    if probe("cvc5") { return Box::new(Cvc5Solver); }
    Box::new(NullSolver)
}

fn probe(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_solver_returns_unknown() {
        let s = NullSolver;
        let script = Smt2Script { text: String::new() };
        match s.check(&script) {
            SolverResult::Unknown(msg) => assert!(msg.contains("no SMT backend")),
            _ => panic!("expected unknown"),
        }
    }

    #[test]
    fn default_solver_constructible() {
        let _s = default_solver();
    }
}
