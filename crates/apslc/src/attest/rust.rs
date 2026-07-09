
use std::sync::OnceLock;

use regex::Regex;

use super::common::{record, Fault, Violation};

fn rust_char() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|.)|[^'\\\n])'").expect("rust_char")
    })
}

fn boundary(b: &[u8], i: usize) -> bool {
    i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')
}

fn window(b: &[u8], base: usize) -> &str {
    let mut end = (base + 128).min(b.len());
    while end > base && (b[end - 1] & 0xC0) == 0x80 {
        end -= 1;
    }
    std::str::from_utf8(&b[base..end]).unwrap_or("")
}

fn find_sub(b: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from > b.len() {
        return None;
    }
    let last = b.len().checked_sub(needle.len())?;
    (from..=last).find(|&k| &b[k..k + needle.len()] == needle)
}

pub fn scan(path: &str, text: &str, out: &mut Vec<Violation>, faults: &mut Vec<Fault>) {
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
        if c == b'/' && i + 1 < n && b[i + 1] == b'/' {
            i += 2;
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && i + 1 < n && b[i + 1] == b'*' {
            let mut depth = 1;
            i += 2;
            while i < n && depth > 0 {
                if b[i] == b'\n' {
                    line += 1;
                    i += 1;
                    continue;
                }
                if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                    continue;
                }
                if b[i] == b'*' && i + 1 < n && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                    continue;
                }
                i += 1;
            }
            continue;
        }
        if boundary(b, i) {
            let mut k = i;
            if k < n && b[k] == b'b' {
                k += 1;
            }
            if k < n && b[k] == b'r' {
                k += 1;
                let hs = k;
                while k < n && b[k] == b'#' {
                    k += 1;
                }
                let hashes = k - hs;
                if k < n && b[k] == b'"' {
                    let content_start = k + 1;
                    let mut closer = vec![b'"'];
                    closer.extend(std::iter::repeat(b'#').take(hashes));
                    match find_sub(b, &closer, content_start) {
                        None => {
                            faults.push(Fault {
                                path: path.to_string(),
                                line,
                                reason: "unterminated raw string".to_string(),
                            });
                            return;
                        }
                        Some(end) => {
                            let tok_end = end + closer.len();
                            let raw = &text[i..tok_end];
                            let value = &text[content_start..end];
                            record(out, path, line, b, i, tok_end, value, raw, "rust");
                            line += raw.bytes().filter(|&x| x == b'\n').count();
                            i = tok_end;
                            continue;
                        }
                    }
                }
            }
        }
        if c == b'"' || (c == b'b' && i + 1 < n && b[i + 1] == b'"' && boundary(b, i)) {
            let q = if c == b'"' { i } else { i + 1 };
            let mut j = q + 1;
            while j < n {
                if b[j] == b'\\' {
                    if j + 1 < n && b[j + 1] == b'\n' {
                        line += 1;
                    }
                    j += 2;
                    continue;
                }
                if b[j] == b'\n' {
                    line += 1;
                    j += 1;
                    continue;
                }
                if b[j] == b'"' {
                    break;
                }
                j += 1;
            }
            if j >= n || b[j] != b'"' {
                faults.push(Fault {
                    path: path.to_string(),
                    line,
                    reason: "unterminated string".to_string(),
                });
                return;
            }
            let tok_end = j + 1;
            record(
                out,
                path,
                line,
                b,
                i,
                tok_end,
                &text[q + 1..j],
                &text[i..tok_end],
                "rust",
            );
            i = tok_end;
            continue;
        }
        if c == b'\'' || (c == b'b' && i + 1 < n && b[i + 1] == b'\'' && boundary(b, i)) {
            let base = if c == b'\'' { i } else { i + 1 };
            if let Some(m) = rust_char().find(window(b, base)) {
                i = base + m.end();
                continue;
            }
            i = base + 1;
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
}
