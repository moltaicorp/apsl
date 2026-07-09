
use super::common::{trunc, wrapper_legal, Fault, Violation};

const PREFIXES: &[&str] = &["r", "u", "f", "b", "rf", "fr", "rb", "br"];

fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}
fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn line_of(line_starts: &[usize], pos: usize) -> usize {
    line_starts.partition_point(|&s| s <= pos)
}

fn skip_string(b: &[u8], mut i: usize) -> usize {
    let n = b.len();
    let qc = b[i];
    let triple = i + 2 < n && b[i + 1] == qc && b[i + 2] == qc;
    if triple {
        i += 3;
        while i < n {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if i + 2 < n && b[i] == qc && b[i + 1] == qc && b[i + 2] == qc {
                return i + 3;
            }
            i += 1;
        }
        return n;
    }
    i += 1;
    while i < n {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == qc || b[i] == b'\n' {
            return i + 1;
        }
        i += 1;
    }
    n
}

struct StrTok {
    prefix_start: usize,
    tok_end: usize,
    is_f: bool,
    is_bytes: bool,
    inner_start: usize,
    inner_end: usize,
}

struct Folded {
    line: usize,
    prefix_start: usize,
    tok_end: usize,
    is_f: bool,
    is_bytes: bool,
    any_nonempty: bool,
    fbodies: Vec<(usize, usize)>,
}

struct Cx<'a> {
    path: &'a str,
    text: &'a str,
    b: &'a [u8],
    line_starts: &'a [usize],
}

impl<'a> Cx<'a> {
    fn find_field_close(&self, start: usize, hard_end: usize) -> (usize, Option<usize>) {
        let b = self.b;
        let mut j = start;
        let mut nest: i32 = 0;
        let mut spec: Option<usize> = None;
        while j < hard_end {
            let c = b[j];
            if c == b'\'' || c == b'"' {
                j = skip_string(b, j).min(hard_end);
                continue;
            }
            if c == b'(' || c == b'[' || c == b'{' {
                nest += 1;
            } else if c == b')' || c == b']' {
                nest -= 1;
            } else if c == b'}' {
                if nest == 0 {
                    return (j, spec);
                }
                nest -= 1;
            } else if c == b':' && nest == 0 && spec.is_none() {
                spec = Some(j);
            }
            j += 1;
        }
        (hard_end, spec)
    }

    fn parse_string(&self, prefix_start: usize, quote_pos: usize, is_f: bool, is_bytes: bool) -> StrTok {
        let b = self.b;
        let n = b.len();
        let qc = b[quote_pos];
        let triple = quote_pos + 2 < n && b[quote_pos + 1] == qc && b[quote_pos + 2] == qc;
        let qlen = if triple { 3 } else { 1 };
        let inner_start = quote_pos + qlen;
        let at_close = |i: usize| -> bool {
            if triple {
                i + 2 < n && b[i] == qc && b[i + 1] == qc && b[i + 2] == qc
            } else {
                i < n && b[i] == qc
            }
        };
        let mut i = inner_start;
        while i < n {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if is_f && b[i] == b'{' {
                if i + 1 < n && b[i + 1] == b'{' {
                    i += 2;
                    continue;
                }
                let (close, _) = self.find_field_close(i + 1, n);
                i = if close < n { close + 1 } else { n };
                continue;
            }
            if is_f && b[i] == b'}' {
                if i + 1 < n && b[i + 1] == b'}' {
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            if at_close(i) {
                break;
            }
            if !triple && b[i] == b'\n' {
                break;
            }
            i += 1;
        }
        let inner_end = i;
        let tok_end = if i < n && at_close(i) { i + qlen } else { (i + qlen).min(n) };
        StrTok {
            prefix_start,
            tok_end,
            is_f,
            is_bytes,
            inner_start,
            inner_end,
        }
    }

    fn try_string(&self, i: usize) -> Option<StrTok> {
        let b = self.b;
        let n = b.len();
        if b[i] == b'\'' || b[i] == b'"' {
            return Some(self.parse_string(i, i, false, false));
        }
        if is_alpha(b[i]) {
            let mut j = i;
            while j < n && b[j].is_ascii_alphabetic() {
                j += 1;
            }
            if j < n && (b[j] == b'\'' || b[j] == b'"') {
                let word = self.text[i..j].to_ascii_lowercase();
                if PREFIXES.contains(&word.as_str()) {
                    let is_f = word.contains('f');
                    let is_bytes = word.contains('b');
                    return Some(self.parse_string(i, j, is_f, is_bytes));
                }
            }
        }
        None
    }

    fn adjacent_string(&self, mut j: usize, bracketed: bool) -> Option<usize> {
        let b = self.b;
        let n = b.len();
        loop {
            while j < n && matches!(b[j], b' ' | b'\t' | b'\r' | 0x0c) {
                j += 1;
            }
            if j + 1 < n && b[j] == b'\\' && b[j + 1] == b'\n' {
                j += 2;
                continue;
            }
            if j < n && b[j] == b'\n' {
                if bracketed {
                    j += 1;
                    continue;
                }
                return None;
            }
            if j < n && b[j] == b'#' {
                while j < n && b[j] != b'\n' {
                    j += 1;
                }
                continue;
            }
            break;
        }
        if j >= n {
            return None;
        }
        if b[j] == b'"' || b[j] == b'\'' {
            return Some(j);
        }
        if is_alpha(b[j]) {
            let mut k = j;
            while k < n && b[k].is_ascii_alphabetic() {
                k += 1;
            }
            if k < n && (b[k] == b'"' || b[k] == b'\'') {
                let word = self.text[j..k].to_ascii_lowercase();
                if PREFIXES.contains(&word.as_str()) {
                    return Some(j);
                }
            }
        }
        None
    }

    fn fold(&self, i: usize, bracketed: bool) -> (Folded, usize) {
        let first = self.try_string(i).unwrap();
        let mut folded = Folded {
            line: line_of(self.line_starts, first.prefix_start),
            prefix_start: first.prefix_start,
            tok_end: first.tok_end,
            is_f: first.is_f,
            is_bytes: first.is_bytes,
            any_nonempty: !first.is_f && first.inner_start != first.inner_end,
            fbodies: Vec::new(),
        };
        if first.is_f {
            folded.fbodies.push((first.inner_start, first.inner_end));
        }
        let mut p = first.tok_end;
        while let Some(pos) = self.adjacent_string(p, bracketed) {
            let t = self.try_string(pos).unwrap();
            folded.tok_end = t.tok_end;
            folded.is_f |= t.is_f;
            folded.is_bytes |= t.is_bytes;
            if t.is_f {
                folded.fbodies.push((t.inner_start, t.inner_end));
            } else if t.inner_start != t.inner_end {
                folded.any_nonempty = true;
            }
            p = t.tok_end;
        }
        (folded, p)
    }

    fn walk_fstring_body(&self, start: usize, end: usize, out: &mut Vec<Violation>) {
        let b = self.b;
        let mut i = start;
        while i < end {
            let c = b[i];
            if c == b'{' {
                if i + 1 < end && b[i + 1] == b'{' {
                    i += 2;
                    continue;
                }
                let (close, spec) = self.find_field_close(i + 1, end);
                let expr_end = spec.unwrap_or(close);
                self.scan_flat(i + 1, expr_end, out);
                if let Some(sc) = spec {
                    out.push(Violation {
                        path: self.path.to_string(),
                        line: line_of(self.line_starts, sc),
                        rule: "bare-fstring",
                        snippet: trunc(&self.text[sc..close.min(self.b.len())], 100),
                        lang: "python",
                    });
                    self.walk_fstring_body(sc + 1, close, out);
                }
                i = if close < end { close + 1 } else { end };
                continue;
            }
            if c == b'}' {
                if i + 1 < end && b[i + 1] == b'}' {
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            i += 1;
        }
    }

    fn emit(&self, f: &Folded, is_doc: bool, out: &mut Vec<Violation>) {
        if is_doc || f.is_bytes {
            return;
        }
        if wrapper_legal(self.b, f.prefix_start, f.tok_end) {
            return;
        }
        let raw = &self.text[f.prefix_start..f.tok_end.min(self.b.len())];
        if f.is_f {
            out.push(Violation {
                path: self.path.to_string(),
                line: f.line,
                rule: "bare-fstring",
                snippet: trunc(raw, 100),
                lang: "python",
            });
            for &(s, e) in &f.fbodies {
                self.walk_fstring_body(s, e, out);
            }
        } else if f.any_nonempty {
            out.push(Violation {
                path: self.path.to_string(),
                line: f.line,
                rule: "bare-string",
                snippet: trunc(raw, 100),
                lang: "python",
            });
        }
    }

    fn scan_flat(&self, start: usize, end: usize, out: &mut Vec<Violation>) {
        let b = self.b;
        let mut i = start;
        while i < end {
            let c = b[i];
            if c == b'#' {
                while i < end && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if c == b'"' || c == b'\'' || (is_alpha(c) && self.try_string(i).is_some()) {
                let (folded, next) = self.fold(i, true);
                self.emit(&folded, false, out);
                i = next.max(i + 1);
                continue;
            }
            if is_alpha(c) {
                while i < end && is_ident(b[i]) {
                    i += 1;
                }
                continue;
            }
            i += 1;
        }
    }
}

#[allow(unused_assignments)]
pub fn scan(path: &str, text: &str, out: &mut Vec<Violation>, faults: &mut Vec<Fault>) {
    let _ = faults; // committed Python parses; a SyntaxError-fault is not modeled.
    let b = text.as_bytes();
    let n = b.len();
    let mut line_starts: Vec<usize> = vec![0];
    for (idx, &ch) in b.iter().enumerate() {
        if ch == b'\n' {
            line_starts.push(idx + 1);
        }
    }
    let cx = Cx {
        path,
        text,
        b,
        line_starts: &line_starts,
    };

    let mut i = 0usize;
    let mut depth: i32 = 0;
    let mut line_started = false;
    let mut only_strings = true;
    let mut first_name: Option<String> = None;
    let mut second_name: Option<String> = None;
    let mut name_count = 0usize;
    let mut token_seen = false;
    let mut line_toks: Vec<Folded> = Vec::new();
    let mut doc_eligible = true; // module start

    macro_rules! close_line {
        () => {{
            if line_started {
                let is_doc = doc_eligible && only_strings && !line_toks.is_empty();
                for t in line_toks.drain(..) {
                    cx.emit(&t, is_doc, out);
                }
                doc_eligible = matches!(first_name.as_deref(), Some("def") | Some("class"))
                    || (first_name.as_deref() == Some("async")
                        && second_name.as_deref() == Some("def"));
                line_started = false;
                only_strings = true;
                first_name = None;
                second_name = None;
                name_count = 0;
                token_seen = false;
            }
            line_toks.clear();
        }};
    }

    while i < n {
        let c = b[i];
        if c == b'\n' {
            if depth <= 0 {
                close_line!();
            }
            i += 1;
            continue;
        }
        if c == b'\\' && i + 1 < n && b[i + 1] == b'\n' {
            i += 2;
            continue;
        }
        if c == b' ' || c == b'\t' || c == b'\r' || c == 0x0c {
            i += 1;
            continue;
        }
        if c == b'#' {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'(' || c == b'[' || c == b'{' {
            depth += 1;
            line_started = true;
            only_strings = false;
            token_seen = true;
            i += 1;
            continue;
        }
        if c == b')' || c == b']' || c == b'}' {
            depth -= 1;
            line_started = true;
            only_strings = false;
            token_seen = true;
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' || (is_alpha(c) && cx.try_string(i).is_some()) {
            let (folded, next) = cx.fold(i, depth > 0);
            line_started = true;
            token_seen = true;
            line_toks.push(folded);
            i = next.max(i + 1);
            continue;
        }
        if is_alpha(c) {
            let start = i;
            while i < n && is_ident(b[i]) {
                i += 1;
            }
            let word = text[start..i].to_string();
            line_started = true;
            only_strings = false;
            if name_count == 0 && !token_seen {
                first_name = Some(word);
            } else if name_count == 1 {
                second_name = Some(word);
            }
            name_count += 1;
            token_seen = true;
            continue;
        }
        line_started = true;
        only_strings = false;
        token_seen = true;
        i += 1;
    }
    close_line!();
}
