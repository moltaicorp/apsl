
use super::common::{is_wrap_content, trunc, Fault, Violation};
use super::js;

fn line_of(line_starts: &[usize], pos: usize) -> usize {
    line_starts.partition_point(|&s| s <= pos)
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let n = b.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;
    while i < n {
        if b[i] == b'&' {
            if let Some(semi) = s[i + 1..].find(';') {
                let ent = &s[i + 1..i + 1 + semi];
                let decoded = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{00A0}'),
                    _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                        u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                    }
                    _ if ent.starts_with('#') => {
                        ent[1..].parse::<u32>().ok().and_then(char::from_u32)
                    }
                    _ => None,
                };
                if let Some(ch) = decoded {
                    out.push(ch);
                    i += 1 + semi + 1;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

struct Ctx<'a> {
    path: &'a str,
}

impl Ctx<'_> {
    fn flag(&self, line: usize, raw: &str, value: &str, out: &mut Vec<Violation>) {
        let v = value.trim();
        if v.is_empty() || is_wrap_content(v) {
            return;
        }
        out.push(Violation {
            path: self.path.to_string(),
            line,
            rule: "bare-string",
            snippet: trunc(raw, 100),
            lang: "html",
        });
    }
}

fn find_end_tag(b: &[u8], name: &str, from: usize) -> Option<usize> {
    let n = b.len();
    let nm = name.as_bytes();
    let mut i = from;
    while i + 1 < n {
        if b[i] == b'<' && b[i + 1] == b'/' {
            let mut k = i + 2;
            while k < n && is_ws(b[k]) {
                k += 1;
            }
            if k + nm.len() <= n && b[k..k + nm.len()].eq_ignore_ascii_case(nm) {
                let mut m = k + nm.len();
                while m < n && is_ws(b[m]) {
                    m += 1;
                }
                if m < n && b[m] == b'>' {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

pub fn scan(path: &str, text: &str, out: &mut Vec<Violation>, faults: &mut Vec<Fault>) {
    let b = text.as_bytes();
    let n = b.len();
    let mut line_starts: Vec<usize> = vec![0];
    for (idx, &ch) in b.iter().enumerate() {
        if ch == b'\n' {
            line_starts.push(idx + 1);
        }
    }
    let cx = Ctx { path };
    let mut tagstack: Vec<String> = Vec::new();
    let mut i = 0usize;

    while i < n {
        let data_start = i;
        while i < n && b[i] != b'<' {
            i += 1;
        }
        if i > data_start {
            let cur = tagstack.last().map(|s| s.as_str());
            match cur {
                Some("script") => {
                    let data = unescape(&text[data_start..i]);
                    if !data.trim().is_empty() {
                        cx.flag(line_of(&line_starts, data_start), data.trim(), &data, out);
                    }
                }
                Some("style") => {}
                _ => {
                    let data = unescape(&text[data_start..i]);
                    if !data.trim().is_empty() {
                        cx.flag(line_of(&line_starts, data_start), data.trim(), &data, out);
                    }
                }
            }
        }
        if i >= n {
            break;
        }
        let line = line_of(&line_starts, i);
        if text[i..].starts_with("<!--") {
            i = match text[i + 4..].find("-->") {
                Some(p) => i + 4 + p + 3,
                None => n,
            };
            continue;
        }
        if text[i..].starts_with("<!") || text[i..].starts_with("<?") {
            i = match text[i + 2..].find('>') {
                Some(p) => i + 2 + p + 1,
                None => n,
            };
            continue;
        }
        if text[i..].starts_with("</") {
            let mut k = i + 2;
            while k < n && is_ws(b[k]) {
                k += 1;
            }
            let ns = k;
            while k < n && !is_ws(b[k]) && b[k] != b'>' {
                k += 1;
            }
            let tag = text[ns..k].to_ascii_lowercase();
            while k < n && b[k] != b'>' {
                k += 1;
            }
            if k < n {
                k += 1;
            }
            if let Some(idx) = tagstack.iter().rposition(|t| *t == tag) {
                tagstack.remove(idx);
            }
            i = k;
            continue;
        }
        if i + 1 < n && b[i + 1].is_ascii_alphabetic() {
            let mut k = i + 1;
            while k < n && !is_ws(b[k]) && b[k] != b'/' && b[k] != b'>' {
                k += 1;
            }
            let tag = text[i + 1..k].to_ascii_lowercase();
            let mut self_closing = false;
            loop {
                while k < n && is_ws(b[k]) {
                    k += 1;
                }
                if k >= n {
                    break;
                }
                if b[k] == b'>' {
                    k += 1;
                    break;
                }
                if b[k] == b'/' {
                    if k + 1 < n && b[k + 1] == b'>' {
                        self_closing = true;
                        k += 2;
                        break;
                    }
                    k += 1;
                    continue;
                }
                let ns = k;
                while k < n && !is_ws(b[k]) && b[k] != b'=' && b[k] != b'>' && b[k] != b'/' {
                    k += 1;
                }
                let name = text[ns..k].to_ascii_lowercase();
                while k < n && is_ws(b[k]) {
                    k += 1;
                }
                let mut value: Option<String> = None;
                if k < n && b[k] == b'=' {
                    k += 1;
                    while k < n && is_ws(b[k]) {
                        k += 1;
                    }
                    if k < n && (b[k] == b'"' || b[k] == b'\'') {
                        let q = b[k];
                        k += 1;
                        let vs = k;
                        while k < n && b[k] != q {
                            k += 1;
                        }
                        value = Some(unescape(&text[vs..k]));
                        if k < n {
                            k += 1;
                        }
                    } else {
                        let vs = k;
                        while k < n && !is_ws(b[k]) && b[k] != b'>' {
                            k += 1;
                        }
                        value = Some(unescape(&text[vs..k]));
                    }
                }
                if !name.is_empty() {
                    cx.flag(line, &format!("@{}", name), &name, out);
                }
                if let Some(val) = &value {
                    cx.flag(line, &format!("{}=\"{}\"", name, val), val, out);
                }
            }
            if !self_closing {
                if tag == "script" || tag == "style" {
                    tagstack.push(tag.clone());
                    let body_start = k;
                    let end = find_end_tag(b, &tag, body_start).unwrap_or(n);
                    if tag == "script" {
                        js::scan(
                            path,
                            &text[body_start..end],
                            out,
                            faults,
                            line_of(&line_starts, body_start),
                        );
                    }
                    let mut m = end;
                    while m < n && b[m] != b'>' {
                        m += 1;
                    }
                    if m < n {
                        m += 1;
                    }
                    if let Some(idx) = tagstack.iter().rposition(|t| *t == tag) {
                        tagstack.remove(idx);
                    }
                    i = m;
                    continue;
                }
                tagstack.push(tag);
            }
            i = k;
            continue;
        }
        let data = "<";
        let cur = tagstack.last().map(|s| s.as_str());
        if cur != Some("style") && cur != Some("script") {
            cx.flag(line, data, data, out);
        }
        i += 1;
    }
}
