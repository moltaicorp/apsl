
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod common;
mod html;
mod js;
mod json;
mod python;
mod rust;
mod shell;
mod toml;
mod yaml;

use common::{Fault, Violation};

const SELF_REL: &str = "ci/no_strings_gate.py";

fn is_law(path: &str) -> bool {
    if path == SELF_REL {
        return true;
    }
    if path.starts_with("ci/strings/")
        || path.starts_with("ci/hooks/")
        || path.starts_with(".githooks/")
    {
        return true;
    }
    let base = path.rsplit('/').next().unwrap_or(path);
    base == "mv.rs" || base == "mv.mjs" || path.ends_with("mv/__init__.py")
}

const WALK_SKIP: &[&str] = &[
    ".git",
    "target",
    ".venv",
    "__pycache__",
    "node_modules",
    "artifacts",
    ".cargo",
    ".pytest_cache",
    ".apsl-bin",
    "dist",
];

fn tracked_files(repo: &Path) -> Vec<String> {
    if let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("ls-files")
        .output()
    {
        if out.status.success() {
            let files: Vec<String> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|p| !p.is_empty() && !is_law(p))
                .map(|p| p.to_string())
                .collect();
            if !files.is_empty() {
                return files;
            }
        }
    }
    let mut files = Vec::new();
    walk(repo, repo, &mut files);
    files
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if WALK_SKIP.contains(&name.as_str()) {
                continue;
            }
            walk(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if !is_law(&rel) {
                out.push(rel);
            }
        }
    }
}

fn detect_lang(path: &str, text: &str) -> Option<&'static str> {
    let ext = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let lang = match ext.as_str() {
        "py" => Some("python"),
        "rs" => Some("rust"),
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => Some("js"),
        "html" | "htm" => Some("html"),
        "yaml" | "yml" => Some("yaml"),
        "json" => Some("json"),
        "toml" => Some("toml"),
        "sh" | "bash" => Some("shell"),
        _ => None,
    };
    if lang.is_some() {
        return lang;
    }
    if text.starts_with("#!") {
        let first = text.lines().next().unwrap_or("");
        if first.contains("python") {
            return Some("python");
        }
        if first.contains("bash") || first.contains("zsh") || first.trim_end().ends_with("sh") {
            return Some("shell");
        }
    }
    None
}

fn scan_file(path: &str, lang: &str, text: &str, out: &mut Vec<Violation>, faults: &mut Vec<Fault>) {
    match lang {
        "python" => python::scan(path, text, out, faults),
        "rust" => rust::scan(path, text, out, faults),
        "js" => js::scan(path, text, out, faults, 1),
        "shell" => shell::scan(path, text, out, faults),
        "html" => html::scan(path, text, out, faults),
        "json" => json::scan(path, text, out, faults),
        "yaml" => yaml::scan(path, text, out, faults),
        "toml" => toml::scan(path, text, out, faults),
        _ => {}
    }
}

struct Scan {
    violations: Vec<Violation>,
    faults: Vec<Fault>,
    unhandled: BTreeMap<String, usize>,
}

fn run_scan(repo: &Path) -> Scan {
    let files = tracked_files(repo);
    let mut violations: Vec<Violation> = Vec::new();
    let mut faults: Vec<Fault> = Vec::new();
    let mut unhandled: BTreeMap<String, usize> = BTreeMap::new();

    for path in &files {
        let full = repo.join(path);
        let bytes = match std::fs::read(&full) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let text = match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(_) => {
                *unhandled.entry("<binary>".to_string()).or_insert(0) += 1;
                continue;
            }
        };
        match detect_lang(path, &text) {
            Some(lang) => scan_file(path, lang, &text, &mut violations, &mut faults),
            None => {
                let ext = Path::new(path)
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
                    .unwrap_or_else(|| {
                        Path::new(path)
                            .file_name()
                            .map(|f| f.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    });
                *unhandled.entry(ext).or_insert(0) += 1;
            }
        }
    }

    violations.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.rule.cmp(b.rule))
    });
    Scan {
        violations,
        faults,
        unhandled,
    }
}

const KEYS_TO_TYPES: &str = "ci/strings/keys_to_types.py";

fn remediate(lang: &str) -> (Option<&'static str>, &'static str) {
    match lang {
        "rust" => (
            Some("ci/strings/wrap_messages_rs.py"),
            "DISPLAY→prose!() (wrap_messages_rs.py), VALUE→state(\"param\") (wrap_values_rs.py)",
        ),
        "python" => (
            Some("ci/strings/wrap_py.py"),
            "DISPLAY→prose_lit(), VALUE→state(\"param\") (wrap_py.py)",
        ),
        "js" => (
            Some("ci/strings/wrap_js.py"),
            "DISPLAY→proseLit(), VALUE→state(\"param\") (wrap_js.py)",
        ),
        "html" => (
            Some("ci/strings/html_to_prose.py"),
            "TEXT/ATTR display→prose!(\"…\") at render time (html_to_prose.py; see its render-seam note)",
        ),
        "yaml" => (
            Some(KEYS_TO_TYPES),
            "KEY→typed schema field, VALUE→state()/mv() (keys_to_types.py)",
        ),
        "json" => (
            Some(KEYS_TO_TYPES),
            "KEY→typed schema field, VALUE→state()/mv() (keys_to_types.py)",
        ),
        "toml" => (
            Some(KEYS_TO_TYPES),
            "KEY→typed schema field, VALUE→state()/mv() (keys_to_types.py)",
        ),
        "shell" => (
            None,
            "VALUE→state(\"param\"), DISPLAY→prose(\"key\") via the mv CLI shim (hand-bind)",
        ),
        _ => (None, "bind to a vault key ref: state()/prose()/mv()"),
    }
}

fn print_report(scan: &Scan) {
    let bar = "=".repeat(78);
    println!("{bar}");
    println!("NO-STRINGS gate — state-binding scan of the committed tree");
    println!("  LAW: every string literal is a VIOLATION unless it is a vault key");
    println!("       reference: mv(\"key\") / prose(\"key\") / state(\"key\").");
    println!("{bar}");

    if !scan.faults.is_empty() {
        let bang = "!".repeat(78);
        println!(
            "\n{bang}\nPARSE FAULTS: {} file(s) the gate could NOT parse (LOUD, never silent):\n{bang}",
            scan.faults.len()
        );
        let mut fs: Vec<&Fault> = scan.faults.iter().collect();
        fs.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        for fa in fs {
            println!("  {}:{}: parse-fault — {}", fa.path, fa.line, fa.reason);
        }
    }

    let line = "─".repeat(78);
    println!(
        "\n{line}\nSTATE-BINDING WORKLIST — {} bare string literal(s)\n{line}",
        scan.violations.len()
    );
    for v in &scan.violations {
        println!("  {}:{}: {} — {}", v.path, v.line, v.rule, v.snippet);
    }

    let mut by_lang: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_ext: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_file: BTreeMap<&str, usize> = BTreeMap::new();
    for v in &scan.violations {
        *by_lang.entry(v.lang).or_insert(0) += 1;
        let ext = Path::new(&v.path)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
            .unwrap_or_else(|| "<none>".to_string());
        *by_ext.entry(ext).or_insert(0) += 1;
        *by_file.entry(v.path.as_str()).or_insert(0) += 1;
    }

    println!("\n{bar}\nSUMMARY\n{bar}");
    println!("TOTAL violations : {}", scan.violations.len());
    println!("PARSE faults     : {}", scan.faults.len());
    println!("\nBy language:");
    let mut langs: Vec<(&&str, &usize)> = by_lang.iter().collect();
    langs.sort_by(|a, b| b.1.cmp(a.1));
    for (lang, c) in langs {
        println!("  {:8} {}", lang, c);
    }
    println!("\nBy file extension:");
    let mut exts: Vec<(&String, &usize)> = by_ext.iter().collect();
    exts.sort_by(|a, b| b.1.cmp(a.1));
    for (ext, c) in exts {
        println!("  {:8} {}", ext, c);
    }
    println!("\nTop-10 files:");
    let mut files: Vec<(&&str, &usize)> = by_file.iter().collect();
    files.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (p, c) in files.iter().take(10) {
        println!("  {:5}  {}", c, p);
    }
    if !scan.unhandled.is_empty() {
        println!("\nOut-of-scope (not one of the 8 in-scope formats — NOT parsed, noted):");
        for (ext, c) in &scan.unhandled {
            println!("  {:14} {} file(s)", ext, c);
        }
    }

    if !scan.violations.is_empty() {
        let mut lang_of: BTreeMap<&str, &str> = BTreeMap::new();
        for v in &scan.violations {
            lang_of.entry(v.path.as_str()).or_insert(v.lang);
        }
        println!(
            "\n{bar}\nREMEDIATION WORKLIST — bind each string to its fate, then re-run the gate."
        );
        println!(
            "  FATES:  VALUE→state(\"param\")   DISPLAY→prose!/prose_lit/proseLit   STRUCTURE→typed field"
        );
        println!("  Per file: <count>  <path>  →  <fate>  |  <transform for the mechanical bulk>");
        println!("{bar}");
        let mut files: Vec<(&&str, &usize)> = by_file.iter().collect();
        files.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (p, c) in files {
            let (script, how) = remediate(lang_of[*p]);
            let cmd = match script {
                Some(s) => format!("python3 {} {} --apply", s, p),
                None => "hand-bind per site".to_string(),
            };
            println!("  {:5}  {}", c, p);
            println!("         → {}", how);
            println!("         $ {}", cmd);
        }
    }

    if !scan.violations.is_empty() || !scan.faults.is_empty() {
        println!("\n{bar}");
        println!(
            "FAULT: {} bare string(s) + {} parse fault(s). RED BY DESIGN.",
            scan.violations.len(),
            scan.faults.len()
        );
        println!("Fix: apply the REMEDIATION WORKLIST above — each file names its fate");
        println!("(VALUE→state / DISPLAY→prose / STRUCTURE→type) and the transform that");
        println!("clears the mechanical bulk. Green comes ONLY from binding each string");
        println!("to a vault key ref — NEVER from weakening this gate. Exit 1.");
        println!("{bar}");
    } else {
        println!("\nCLEAN — every string literal is a vault key reference.\n");
    }
}

fn run_ratchet(scan: &Scan, baseline_path: &Path, bless: bool) -> ExitCode {
    let count = scan.violations.len();
    if bless {
        let _ = std::fs::write(baseline_path, format!("{}\n", count));
        println!(
            "apslc --attest: baseline blessed to {}. Commit {} as part of this reduction.",
            count,
            baseline_path.display()
        );
        return ExitCode::SUCCESS;
    }
    let baseline: Option<usize> = std::fs::read_to_string(baseline_path)
        .ok()
        .and_then(|s| s.trim().parse().ok());
    let baseline = match baseline {
        None => {
            let _ = std::fs::write(baseline_path, format!("{}\n", count));
            println!(
                "apslc --attest: sealed initial baseline at {}. Commit {}. From here it can only ratchet DOWN.",
                count,
                baseline_path.display()
            );
            return ExitCode::SUCCESS;
        }
        Some(b) => b,
    };

    if count > baseline {
        print_report(scan);
        let bar = "=".repeat(78);
        println!("{bar}");
        println!(
            "NO-STRINGS RATCHET: REGRESSION. count {} > baseline {} (+{}).",
            count,
            baseline,
            count - baseline
        );
        println!("A new UNATTESTED STRING was introduced. Bind it before pushing:");
        println!(
            "  VALUE   -> state(\"param\")   (ci/strings/wrap_values_rs.py | wrap_py.py | wrap_js.py)"
        );
        println!("  DISPLAY -> prose!/prose_lit/proseLit   (ci/strings/wrap_messages_rs.py | wrap_py.py)");
        println!("  STRUCT  -> typed schema field           (ci/strings/keys_to_types.py)");
        println!("See the REMEDIATION WORKLIST above for the exact per-file command.");
        println!("{bar}");
        return ExitCode::FAILURE;
    }
    if count < baseline {
        println!(
            "apslc --attest: OK — count {} < baseline {} ({} bound). Lower the ceiling:",
            count,
            baseline,
            baseline - count
        );
        println!(
            "  $ apslc check --attest --ratchet {} --bless",
            baseline_path.display()
        );
        return ExitCode::SUCCESS;
    }
    println!(
        "apslc --attest: OK — held at baseline {}. No new unattested strings. ({} still on the worklist → drive to 0.)",
        count, count
    );
    ExitCode::SUCCESS
}

pub struct AttestOpts {
    pub count: bool,
    pub ratchet: Option<PathBuf>,
    pub bless: bool,
    pub path: Option<String>,
}

impl AttestOpts {
    pub fn parse(args: &[String]) -> AttestOpts {
        let mut opts = AttestOpts {
            count: false,
            ratchet: None,
            bless: false,
            path: None,
        };
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--attest" => {}
                "--count" => opts.count = true,
                "--bless" => opts.bless = true,
                "--ratchet" => {
                    if i + 1 < args.len() {
                        i += 1;
                        opts.ratchet = Some(PathBuf::from(&args[i]));
                    }
                }
                s if s.starts_with('-') => {}
                s => {
                    if opts.path.is_none() {
                        opts.path = Some(s.to_string());
                    }
                }
            }
            i += 1;
        }
        opts
    }
}

fn resolve_repo(opts: &AttestOpts) -> PathBuf {
    if let Some(p) = &opts.path {
        return PathBuf::from(p);
    }
    if let Ok(out) = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
    {
        if out.status.success() {
            let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !top.is_empty() {
                return PathBuf::from(top);
            }
        }
    }
    PathBuf::from(".")
}

pub fn run(opts: &AttestOpts) -> ExitCode {
    let repo = resolve_repo(opts);
    let scan = run_scan(&repo);

    if let Some(baseline_path) = &opts.ratchet {
        return run_ratchet(&scan, baseline_path, opts.bless);
    }
    if opts.count {
        println!("{}", scan.violations.len());
        return ExitCode::SUCCESS;
    }

    print_report(&scan);
    if scan.violations.is_empty() && scan.faults.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
