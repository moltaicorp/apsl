
use super::common::{is_wrap_content, trunc, Fault, Violation};

pub fn scan(path: &str, text: &str, out: &mut Vec<Violation>, faults: &mut Vec<Fault>) {
    if let Err(e) = serde_json::from_str::<serde_json::Value>(text) {
        faults.push(Fault {
            path: path.to_string(),
            line: e.line(),
            reason: format!("json parse error: {}", e),
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
        if c == b'"' {
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
                faults.push(Fault {
                    path: path.to_string(),
                    line,
                    reason: "unterminated json string".to_string(),
                });
                return;
            }
            let tok_end = j + 1;
            let decoded = serde_json::from_str::<String>(&text[i..tok_end])
                .unwrap_or_else(|_| text[i + 1..j].to_string());
            if !decoded.is_empty() && !is_wrap_content(&decoded) {
                out.push(Violation {
                    path: path.to_string(),
                    line,
                    rule: "bare-string",
                    snippet: trunc(&text[i..tok_end], 100),
                    lang: "json",
                });
            }
            i = tok_end;
            continue;
        }
        i += 1;
    }
}
