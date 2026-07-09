
use super::common::{is_wrap_content, trunc, Fault, Violation};

fn find(b: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from > b.len() {
        return None;
    }
    let last = b.len().checked_sub(needle.len())?;
    (from..=last).find(|&k| &b[k..k + needle.len()] == needle)
}

fn toml_string(text: &str, i: usize) -> Option<(String, usize, bool)> {
    let b = text.as_bytes();
    let n = b.len();
    if text[i..].starts_with("\"\"\"") {
        let mut end = find(b, b"\"\"\"", i + 3);
        while let Some(e) = end {
            if e > 0 && b[e - 1] == b'\\' {
                end = find(b, b"\"\"\"", e + 1);
            } else {
                break;
            }
        }
        let e = end?;
        return Some((text[i + 3..e].to_string(), e + 3, false));
    }
    if text[i..].starts_with("'''") {
        let e = find(b, b"'''", i + 3)?;
        return Some((text[i + 3..e].to_string(), e + 3, true));
    }
    if b[i] == b'"' {
        let mut j = i + 1;
        while j < n {
            if b[j] == b'\\' {
                j += 2;
                continue;
            }
            if b[j] == b'"' || b[j] == b'\n' {
                break;
            }
            j += 1;
        }
        if j >= n || b[j] != b'"' {
            return None;
        }
        return Some((text[i + 1..j].to_string(), j + 1, false));
    }
    if b[i] == b'\'' {
        let j = find(b, b"'", i + 1);
        let nl = find(b, b"\n", i + 1);
        match j {
            None => return None,
            Some(j) => {
                if let Some(nl) = nl {
                    if nl < j {
                        return None;
                    }
                }
                return Some((text[i + 1..j].to_string(), j + 1, true));
            }
        }
    }
    None
}

fn toml_unescape(s: &str) -> String {
    s.replace("\\\"", "\"")
        .replace("\\\\", "\\")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
}

fn count_nl(s: &str) -> usize {
    s.bytes().filter(|&x| x == b'\n').count()
}

pub fn scan(path: &str, text: &str, out: &mut Vec<Violation>, faults: &mut Vec<Fault>) {
    if let Err(e) = ::toml::from_str::<::toml::Value>(text) {
        faults.push(Fault {
            path: path.to_string(),
            line: 0,
            reason: format!("toml parse error: {}", e),
        });
        return;
    }
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0usize;
    let mut line = 1usize;
    while i < n {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c == b'#' {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'"' || c == b'\'' {
            match toml_string(text, i) {
                None => {
                    faults.push(Fault {
                        path: path.to_string(),
                        line,
                        reason: "unterminated toml string".to_string(),
                    });
                    return;
                }
                Some((inner, tok_end, literal)) => {
                    let decoded = if literal {
                        inner
                    } else {
                        toml_unescape(&inner)
                    };
                    if !decoded.is_empty() && !is_wrap_content(&decoded) {
                        out.push(Violation {
                            path: path.to_string(),
                            line,
                            rule: "bare-string",
                            snippet: trunc(&text[i..tok_end], 100),
                            lang: "toml",
                        });
                    }
                    line += count_nl(&text[i..tok_end]);
                    i = tok_end;
                    continue;
                }
            }
        }
        i += 1;
    }
}
