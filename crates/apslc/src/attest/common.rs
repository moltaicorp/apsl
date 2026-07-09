
use std::sync::OnceLock;

use regex::Regex;

pub const WRAPPERS: &[&str] = &[
    "mv", "prose", "state", "state_opt", "prose_lit", "stateOpt", "proseLit",
];

pub fn is_wrapper(name: &str) -> bool {
    WRAPPERS.contains(&name)
}

pub fn wrap_content() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?s)^\s*(?:mv|prose|state|state_opt|prose_lit|stateOpt|proseLit)!?\(\s*(?:"[^"]*"|'[^']*')\s*\)\s*$"#,
        )
        .expect("wrap_content regex")
    })
}

pub fn is_wrap_content(s: &str) -> bool {
    wrap_content().is_match(s)
}

#[derive(Clone)]
pub struct Violation {
    pub path: String,
    pub line: usize,
    pub rule: &'static str,
    pub snippet: String,
    pub lang: &'static str,
}

pub struct Fault {
    pub path: String,
    pub line: usize,
    pub reason: String,
}

pub fn trunc(s: &str, n: usize) -> String {
    let mut t = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\r' => {}
            '\n' => t.push_str("\\n"),
            '\t' => t.push_str("\\t"),
            other => t.push(other),
        }
    }
    if t.chars().count() <= n {
        return t;
    }
    let mut out: String = t.chars().take(n - 1).collect();
    out.push('…');
    out
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

pub fn preceding_call(b: &[u8], tok_start: usize) -> Option<String> {
    let mut j = tok_start as isize - 1;
    while j >= 0 && is_ws(b[j as usize]) {
        j -= 1;
    }
    if j < 0 || b[j as usize] != b'(' {
        return None;
    }
    j -= 1;
    while j >= 0 && is_ws(b[j as usize]) {
        j -= 1;
    }
    if j >= 0 && b[j as usize] == b'!' {
        j -= 1;
        while j >= 0 && is_ws(b[j as usize]) {
            j -= 1;
        }
    }
    let end = (j + 1) as usize;
    while j >= 0 && is_ident_byte(b[j as usize]) {
        j -= 1;
    }
    let start = (j + 1) as usize;
    if start >= end {
        None
    } else {
        Some(String::from_utf8_lossy(&b[start..end]).into_owned())
    }
}

pub fn sole_arg_close(b: &[u8], tok_end: usize) -> bool {
    let mut j = tok_end;
    while j < b.len() && is_ws(b[j]) {
        j += 1;
    }
    j < b.len() && b[j] == b')'
}

pub fn preceding_word(b: &[u8], tok_start: usize) -> Option<String> {
    let mut j = tok_start as isize - 1;
    while j >= 0 && is_ws(b[j as usize]) {
        j -= 1;
    }
    let end = (j + 1) as usize;
    while j >= 0 && is_ident_byte(b[j as usize]) {
        j -= 1;
    }
    let start = (j + 1) as usize;
    if start >= end {
        None
    } else {
        Some(String::from_utf8_lossy(&b[start..end]).into_owned())
    }
}

pub fn wrapper_legal(b: &[u8], tok_start: usize, tok_end: usize) -> bool {
    preceding_call(b, tok_start).map_or(false, |n| is_wrapper(&n)) && sole_arg_close(b, tok_end)
}

pub fn js_import_legal(b: &[u8], tok_start: usize, _tok_end: usize) -> bool {
    if let Some(w) = preceding_word(b, tok_start) {
        if w == "from" || w == "import" {
            return true;
        }
    }
    matches!(
        preceding_call(b, tok_start).as_deref(),
        Some("require") | Some("import")
    )
}

#[allow(clippy::too_many_arguments)]
pub fn record(
    out: &mut Vec<Violation>,
    path: &str,
    line: usize,
    b: &[u8],
    tok_start: usize,
    tok_end: usize,
    value: &str,
    raw: &str,
    lang: &'static str,
) {
    if wrapper_legal(b, tok_start, tok_end) {
        return;
    }
    if value.is_empty() {
        return;
    }
    if lang == "js" && js_import_legal(b, tok_start, tok_end) {
        return;
    }
    out.push(Violation {
        path: path.to_string(),
        line,
        rule: "bare-string",
        snippet: trunc(raw, 100),
        lang,
    });
}
