
use super::common::{record, trunc, Fault, Violation};

fn count_nl(s: &str) -> usize {
    s.bytes().filter(|&x| x == b'\n').count()
}

pub fn scan(path: &str, text: &str, out: &mut Vec<Violation>, faults: &mut Vec<Fault>) {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut prev = b'\n';
    while i < n {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            prev = b'\n';
            i += 1;
            continue;
        }
        if c == b'#' && matches!(prev, b' ' | b'\t' | b'\n' | b';' | b'&' | b'|' | b'(') {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'<' && i + 1 < n && b[i + 1] == b'<' && !(i + 2 < n && b[i + 2] == b'<') {
            let mut k = i + 2;
            let dash = k < n && b[k] == b'-';
            if dash {
                k += 1;
            }
            while k < n && matches!(b[k], b' ' | b'\t') {
                k += 1;
            }
            let q = if k < n && matches!(b[k], b'"' | b'\'') {
                Some(b[k])
            } else {
                None
            };
            if q.is_some() {
                k += 1;
            }
            let ds = k;
            while k < n && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                k += 1;
            }
            let delim = &text[ds..k];
            if let Some(qc) = q {
                if k < n && b[k] == qc {
                    k += 1;
                }
            }
            if delim.is_empty() {
                prev = b'<';
                i += 2;
                continue;
            }
            while k < n && b[k] != b'\n' {
                k += 1;
            }
            if k < n {
                k += 1;
            }
            let hd_line = line + 1;
            let body_start = k;
            let mut terminated = false;
            while k < n {
                let ls = k;
                while k < n && b[k] != b'\n' {
                    k += 1;
                }
                let lntext = &text[ls..k];
                let cmp = if dash { lntext.trim() } else { lntext };
                if cmp == delim {
                    let body = &text[body_start..ls];
                    if !body.trim().is_empty() {
                        out.push(Violation {
                            path: path.to_string(),
                            line: hd_line,
                            rule: "bare-heredoc",
                            snippet: trunc(body, 100),
                            lang: "shell",
                        });
                    }
                    terminated = true;
                    break;
                }
                if k < n {
                    k += 1;
                }
            }
            if !terminated {
                faults.push(Fault {
                    path: path.to_string(),
                    line,
                    reason: format!("unterminated here-doc <<{}", delim),
                });
                return;
            }
            line += count_nl(&text[i..k]);
            prev = b'\n';
            i = k;
            continue;
        }
        if c == b'`' {
            let mut j = i + 1;
            while j < n && b[j] != b'`' {
                if b[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if b[j] == b'\n' {
                    line += 1;
                }
                j += 1;
            }
            if j >= n {
                faults.push(Fault {
                    path: path.to_string(),
                    line,
                    reason: "unterminated backtick".to_string(),
                });
                return;
            }
            prev = b'`';
            i = j + 1;
            continue;
        }
        if c == b'\'' {
            let mut j = i + 1;
            while j < n && b[j] != b'\'' {
                if b[j] == b'\n' {
                    line += 1;
                }
                j += 1;
            }
            if j >= n {
                faults.push(Fault {
                    path: path.to_string(),
                    line,
                    reason: "unterminated single-quote".to_string(),
                });
                return;
            }
            record(
                out,
                path,
                line,
                b,
                i,
                j + 1,
                &text[i + 1..j],
                &text[i..j + 1],
                "shell",
            );
            prev = b'\'';
            i = j + 1;
            continue;
        }
        if c == b'"' || (c == b'$' && i + 1 < n && b[i + 1] == b'\'') {
            let q0 = if c == b'"' { i } else { i + 1 };
            let quote = b[q0];
            let mut j = q0 + 1;
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
                if b[j] == quote {
                    break;
                }
                j += 1;
            }
            if j >= n || b[j] != quote {
                faults.push(Fault {
                    path: path.to_string(),
                    line,
                    reason: "unterminated quote".to_string(),
                });
                return;
            }
            record(
                out,
                path,
                line,
                b,
                i,
                j + 1,
                &text[q0 + 1..j],
                &text[i..j + 1],
                "shell",
            );
            prev = quote;
            i = j + 1;
            continue;
        }
        prev = c;
        i += 1;
    }
}
