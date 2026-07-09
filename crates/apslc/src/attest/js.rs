
use super::common::{js_import_legal, record, trunc, wrapper_legal, Fault, Violation};

const KW_BEFORE_REGEX: &[&str] = &[
    "return", "typeof", "instanceof", "in", "of", "new", "delete", "void", "do", "else", "yield",
    "await", "case",
];

fn regex_ctx(last: Option<u8>) -> bool {
    match last {
        None => true,
        Some(c) => {
            !(c.is_ascii_alphanumeric()
                || matches!(c, b')' | b']' | b'}' | b'.' | b'_' | b'"' | b'\'' | b'`'))
        }
    }
}

enum Frame {
    Code { interp: bool, brace: i32 },
    Template { start: usize, sline: usize, has: bool },
}

pub fn scan(path: &str, text: &str, out: &mut Vec<Violation>, faults: &mut Vec<Fault>, base_line: usize) {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0usize;
    let mut line = base_line;
    let mut last: Option<u8> = None;
    let mut stack: Vec<Frame> = vec![Frame::Code {
        interp: false,
        brace: 0,
    }];

    while i < n {
        let c = b[i];

        if let Some(Frame::Template { .. }) = stack.last() {
            if c == b'\\' {
                if i + 1 < n && b[i + 1] == b'\n' {
                    line += 1;
                }
                i += 2;
                continue;
            }
            if c == b'\n' {
                line += 1;
                i += 1;
                continue;
            }
            if c == b'`' {
                let tok_end = i + 1;
                if let Some(Frame::Template { start, sline, has }) = stack.pop() {
                    if has
                        && !(wrapper_legal(b, start, tok_end) || js_import_legal(b, start, tok_end))
                    {
                        out.push(Violation {
                            path: path.to_string(),
                            line: sline,
                            rule: "bare-template",
                            snippet: trunc(&text[start..tok_end], 100),
                            lang: "js",
                        });
                    }
                }
                last = Some(b'`');
                i = tok_end;
                continue;
            }
            if c == b'$' && i + 1 < n && b[i + 1] == b'{' {
                stack.push(Frame::Code {
                    interp: true,
                    brace: 0,
                });
                last = None;
                i += 2;
                continue;
            }
            if !c.is_ascii_whitespace() {
                if let Some(Frame::Template { has, .. }) = stack.last_mut() {
                    *has = true;
                }
            }
            i += 1;
            continue;
        }

        if c == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if matches!(c, b' ' | b'\t' | b'\r') {
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
            i += 2;
            while i + 1 < n && !(b[i] == b'*' && b[i + 1] == b'/') {
                if b[i] == b'\n' {
                    line += 1;
                }
                i += 1;
            }
            if i + 1 >= n && !(i < n && b[i] == b'*') {
                faults.push(Fault {
                    path: path.to_string(),
                    line,
                    reason: "unterminated block comment".to_string(),
                });
                return;
            }
            i += 2;
            continue;
        }
        if c == b'`' {
            stack.push(Frame::Template {
                start: i,
                sline: line,
                has: false,
            });
            last = None;
            i += 1;
            continue;
        }
        if let Some(Frame::Code { interp: true, brace }) = stack.last_mut() {
            if c == b'{' {
                *brace += 1;
                last = Some(b'{');
                i += 1;
                continue;
            }
            if c == b'}' {
                if *brace > 0 {
                    *brace -= 1;
                    last = Some(b'}');
                    i += 1;
                    continue;
                }
                stack.pop();
                last = Some(b'}');
                i += 1;
                continue;
            }
        }
        if c == b'"' || c == b'\'' {
            let mut j = i + 1;
            while j < n {
                if b[j] == b'\\' {
                    if j + 1 < n && b[j + 1] == b'\n' {
                        line += 1;
                    }
                    j += 2;
                    continue;
                }
                if b[j] == b'\n' {
                    break;
                }
                if b[j] == c {
                    break;
                }
                j += 1;
            }
            if j >= n || b[j] != c {
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
                &text[i + 1..j],
                &text[i..tok_end],
                "js",
            );
            last = Some(b'"');
            i = tok_end;
            continue;
        }
        if c == b'/' {
            if regex_ctx(last) {
                let mut j = i + 1;
                let mut inclass = false;
                let mut ok = false;
                while j < n {
                    let ch = b[j];
                    if ch == b'\\' {
                        j += 2;
                        continue;
                    }
                    if ch == b'\n' {
                        break;
                    }
                    if ch == b'[' {
                        inclass = true;
                    } else if ch == b']' {
                        inclass = false;
                    } else if ch == b'/' && !inclass {
                        ok = true;
                        break;
                    }
                    j += 1;
                }
                if ok {
                    j += 1;
                    while j < n && b[j].is_ascii_alphabetic() {
                        j += 1;
                    }
                    last = Some(b'x');
                    i = j;
                    continue;
                }
            }
            last = Some(b'/');
            i += 1;
            continue;
        }
        if c.is_ascii_alphanumeric() || c == b'_' || c == b'$' {
            let mut j = i;
            while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'$') {
                j += 1;
            }
            let word = &text[i..j];
            last = if KW_BEFORE_REGEX.contains(&word) {
                None
            } else {
                Some(b'x')
            };
            i = j;
            continue;
        }
        last = Some(c);
        i += 1;
    }
    if stack.len() != 1 {
        faults.push(Fault {
            path: path.to_string(),
            line,
            reason: "unterminated template literal".to_string(),
        });
    }
}
