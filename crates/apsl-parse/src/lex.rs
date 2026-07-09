
use apsl_core::ast::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Tok {
    pub kind: TokKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokKind {
    Ident(String),
    TypeName(String),
    IntLit(i128),
    RatLit(i128, u128),
    StrLit(String),
    BoolLit(bool),
    Colon,
    Arrow,
    FatArrow,
    Comma,
    Dot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    BracketPair,
    Semi,
    At,
    Eq,
    Ne,
    Lt,
    Le,
    SubtypeOf,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Indent,
    Dedent,
    Newline,
    Eof,
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub msg: String,
    pub span: Span,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lex error at line {} col {}: {}", self.span.line, self.span.col, self.msg)
    }
}

pub fn lex(src: &str) -> Result<Vec<Tok>, LexError> {
    let mut out = Vec::new();
    let mut indent_stack: Vec<usize> = vec![0];
    let mut line = 1u32;
    let mut col = 1u32;
    let mut chars = src.chars().peekable();
    let mut at_line_start = true;
    let mut paren_depth = 0i32;

    while let Some(&c) = chars.peek() {
        if at_line_start && paren_depth == 0 {
            let mut indent = 0usize;
            let line_start_col = 1u32;
            while let Some(&c2) = chars.peek() {
                if c2 == ' ' { chars.next(); indent += 1; }
                else if c2 == '\t' {
                    return Err(LexError {
                        msg: "tab in indentation; APSL requires spaces only".into(),
                        span: Span { line, col: line_start_col, len: 1 },
                    });
                } else { break; }
            }
            col = (indent + 1) as u32;
            if let Some(&c3) = chars.peek() {
                if c3 == '\n' || c3 == '#' {
                } else {
                    let top = *indent_stack.last().unwrap();
                    use std::cmp::Ordering::*;
                    match indent.cmp(&top) {
                        Greater => {
                            indent_stack.push(indent);
                            out.push(Tok { kind: TokKind::Indent, span: Span { line, col: line_start_col, len: 0 } });
                        }
                        Less => {
                            while *indent_stack.last().unwrap() > indent {
                                indent_stack.pop();
                                out.push(Tok { kind: TokKind::Dedent, span: Span { line, col: line_start_col, len: 0 } });
                            }
                            if *indent_stack.last().unwrap() != indent {
                                return Err(LexError {
                                    msg: format!("inconsistent indentation: {}", indent),
                                    span: Span { line, col: line_start_col, len: 0 },
                                });
                            }
                        }
                        Equal => {}
                    }
                }
            }
            at_line_start = false;
            continue;
        }

        match c {
            ' ' | '\t' => { chars.next(); col += 1; }
            '\n' => {
                chars.next();
                if paren_depth == 0 {
                    let prev_is_separator = out.last()
                        .map(|t| matches!(t.kind, TokKind::Newline | TokKind::Indent | TokKind::Dedent))
                        .unwrap_or(true);
                    if !prev_is_separator {
                        out.push(Tok { kind: TokKind::Newline, span: Span { line, col, len: 1 } });
                    }
                }
                line += 1;
                col = 1;
                at_line_start = true;
            }
            '\r' => { chars.next(); }
            '#' => {
                while let Some(&c2) = chars.peek() {
                    if c2 == '\n' { break; }
                    chars.next();
                }
            }
            '"' => {
                let start = Span { line, col, len: 0 };
                chars.next(); col += 1;
                let mut s = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2 == '"' { chars.next(); col += 1; break; }
                    if c2 == '\\' {
                        chars.next(); col += 1;
                        match chars.next() {
                            Some('"') => { s.push('"'); col += 1; }
                            Some('\\') => { s.push('\\'); col += 1; }
                            Some('n') => { s.push('\n'); col += 1; }
                            Some('r') => { s.push('\r'); col += 1; }
                            Some('t') => { s.push('\t'); col += 1; }
                            Some(other) => { s.push(other); col += 1; }
                            None => return Err(LexError {
                                msg: "unterminated string".into(),
                                span: start,
                            }),
                        }
                    } else {
                        s.push(c2);
                        chars.next(); col += 1;
                    }
                }
                out.push(Tok { kind: TokKind::StrLit(s), span: start });
            }
            c if c.is_ascii_digit() => {
                let start = Span { line, col, len: 0 };
                let mut buf = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2.is_ascii_digit() { buf.push(c2); chars.next(); col += 1; }
                    else { break; }
                }
                let prev_is_dot = out.last().map(|t| matches!(t.kind, TokKind::Dot)).unwrap_or(false);
                if !prev_is_dot {
                    if chars.peek() == Some(&'.') {
                        let mut clone = chars.clone();
                        clone.next();
                        if clone.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                            buf.push('.'); chars.next(); col += 1;
                            while let Some(&c2) = chars.peek() {
                                if c2.is_ascii_digit() { buf.push(c2); chars.next(); col += 1; }
                                else { break; }
                            }
                        }
                    }
                }
                if chars.peek() == Some(&'e') || chars.peek() == Some(&'E') {
                    buf.push('e'); chars.next(); col += 1;
                    if chars.peek() == Some(&'-') || chars.peek() == Some(&'+') {
                        buf.push(*chars.peek().unwrap()); chars.next(); col += 1;
                    }
                    while let Some(&c2) = chars.peek() {
                        if c2.is_ascii_digit() { buf.push(c2); chars.next(); col += 1; }
                        else { break; }
                    }
                }
                let kind = if buf.contains('.') || buf.contains('e') {
                    let (p, q) = decimal_or_scientific_to_rat(&buf).ok_or_else(|| LexError {
                        msg: format!("malformed number `{}`", buf),
                        span: start.clone(),
                    })?;
                    TokKind::RatLit(p, q)
                } else {
                    let n: i128 = buf.parse().map_err(|_| LexError {
                        msg: format!("integer `{}` does not fit in i128", buf),
                        span: start.clone(),
                    })?;
                    TokKind::IntLit(n)
                };
                out.push(Tok { kind, span: start });
            }
            c if is_ident_start(c) => {
                let start = Span { line, col, len: 0 };
                let mut buf = String::new();
                buf.push(c); chars.next(); col += 1;
                while let Some(&c2) = chars.peek() {
                    if is_ident_continue(c2) { buf.push(c2); chars.next(); col += 1; }
                    else { break; }
                }
                if let Some(&c2) = chars.peek() {
                    if c2 == '?' || c2 == '!' { buf.push(c2); chars.next(); col += 1; }
                }
                let kind = match buf.as_str() {
                    "true"  => TokKind::BoolLit(true),
                    "false" => TokKind::BoolLit(false),
                    _ if buf.chars().next().unwrap().is_ascii_uppercase() => TokKind::TypeName(buf),
                    _ => TokKind::Ident(buf),
                };
                out.push(Tok { kind, span: start });
            }
            _ => {
                let start = Span { line, col, len: 0 };
                let mut consume_one = || { chars.next(); col += 1; };
                match c {
                    ':' => { consume_one(); out.push(Tok { kind: TokKind::Colon, span: start }); }
                    '(' => { consume_one(); paren_depth += 1; out.push(Tok { kind: TokKind::LParen, span: start }); }
                    ')' => { consume_one(); paren_depth -= 1; out.push(Tok { kind: TokKind::RParen, span: start }); }
                    '[' => {
                        consume_one();
                        if chars.peek() == Some(&']') {
                            chars.next(); col += 1;
                            out.push(Tok { kind: TokKind::BracketPair, span: start });
                        } else {
                            out.push(Tok { kind: TokKind::LBracket, span: start });
                        }
                    }
                    ']' => { consume_one(); out.push(Tok { kind: TokKind::RBracket, span: start }); }
                    '{' => { consume_one(); out.push(Tok { kind: TokKind::LBrace, span: start }); }
                    '}' => { consume_one(); out.push(Tok { kind: TokKind::RBrace, span: start }); }
                    ',' => { consume_one(); out.push(Tok { kind: TokKind::Comma, span: start }); }
                    '.' => { consume_one(); out.push(Tok { kind: TokKind::Dot, span: start }); }
                    ';' => { consume_one(); out.push(Tok { kind: TokKind::Semi, span: start }); }
                    '@' => { consume_one(); out.push(Tok { kind: TokKind::At, span: start }); }
                    '+' => { consume_one(); out.push(Tok { kind: TokKind::Plus, span: start }); }
                    '*' => { consume_one(); out.push(Tok { kind: TokKind::Star, span: start }); }
                    '/' => { consume_one(); out.push(Tok { kind: TokKind::Slash, span: start }); }
                    '-' => {
                        consume_one();
                        if chars.peek() == Some(&'>') { chars.next(); col += 1; out.push(Tok { kind: TokKind::Arrow, span: start }); }
                        else { out.push(Tok { kind: TokKind::Minus, span: start }); }
                    }
                    '=' => {
                        consume_one();
                        if chars.peek() == Some(&'>') { chars.next(); col += 1; out.push(Tok { kind: TokKind::FatArrow, span: start }); }
                        else { out.push(Tok { kind: TokKind::Eq, span: start }); }
                    }
                    '!' => {
                        consume_one();
                        if chars.peek() == Some(&'=') { chars.next(); col += 1; out.push(Tok { kind: TokKind::Ne, span: start }); }
                        else { return Err(LexError { msg: "stray '!'".into(), span: start }); }
                    }
                    '<' => {
                        consume_one();
                        if chars.peek() == Some(&':') { chars.next(); col += 1; out.push(Tok { kind: TokKind::SubtypeOf, span: start }); }
                        else if chars.peek() == Some(&'=') { chars.next(); col += 1; out.push(Tok { kind: TokKind::Le, span: start }); }
                        else { out.push(Tok { kind: TokKind::Lt, span: start }); }
                    }
                    '>' => {
                        consume_one();
                        if chars.peek() == Some(&'=') { chars.next(); col += 1; out.push(Tok { kind: TokKind::Ge, span: start }); }
                        else { out.push(Tok { kind: TokKind::Gt, span: start }); }
                    }
                    '→' => { consume_one(); out.push(Tok { kind: TokKind::Arrow, span: start }); }
                    '⇒' => { consume_one(); out.push(Tok { kind: TokKind::FatArrow, span: start }); }
                    '∀' => { consume_one(); out.push(Tok { kind: TokKind::Ident("forall".into()), span: start }); }
                    '∃' => { consume_one(); out.push(Tok { kind: TokKind::Ident("exists".into()), span: start }); }
                    '∈' => { consume_one(); out.push(Tok { kind: TokKind::Ident("in".into()), span: start }); }
                    '⊆' => { consume_one(); out.push(Tok { kind: TokKind::Ident("subset".into()), span: start }); }
                    '∪' => { consume_one(); out.push(Tok { kind: TokKind::Ident("union".into()), span: start }); }
                    '∩' => { consume_one(); out.push(Tok { kind: TokKind::Ident("intersect".into()), span: start }); }
                    '∧' => { consume_one(); out.push(Tok { kind: TokKind::Ident("and".into()), span: start }); }
                    '∨' => { consume_one(); out.push(Tok { kind: TokKind::Ident("or".into()), span: start }); }
                    '¬' => { consume_one(); out.push(Tok { kind: TokKind::Ident("not".into()), span: start }); }
                    '≤' => { consume_one(); out.push(Tok { kind: TokKind::Le, span: start }); }
                    '≥' => { consume_one(); out.push(Tok { kind: TokKind::Ge, span: start }); }
                    '≠' => { consume_one(); out.push(Tok { kind: TokKind::Ne, span: start }); }
                    'ε' => { consume_one(); out.push(Tok { kind: TokKind::Ident("e".into()), span: start }); }
                    'δ' => { consume_one(); out.push(Tok { kind: TokKind::Ident("d".into()), span: start }); }
                    _ => return Err(LexError {
                        msg: format!("unexpected character `{}`", c),
                        span: start,
                    }),
                }
            }
        }
    }

    if !out.is_empty() && !matches!(out.last().unwrap().kind, TokKind::Newline) {
        out.push(Tok { kind: TokKind::Newline, span: Span { line, col, len: 0 } });
    }
    while indent_stack.len() > 1 {
        indent_stack.pop();
        out.push(Tok { kind: TokKind::Dedent, span: Span { line, col, len: 0 } });
    }
    out.push(Tok { kind: TokKind::Eof, span: Span { line, col, len: 0 } });
    Ok(out)
}

fn is_ident_start(c: char) -> bool { c.is_ascii_alphabetic() || c == '_' }
fn is_ident_continue(c: char) -> bool { c.is_ascii_alphanumeric() || c == '_' }

fn decimal_or_scientific_to_rat(buf: &str) -> Option<(i128, u128)> {
    let (mantissa, exp): (&str, i32) = if let Some(idx) = buf.find(|c: char| c == 'e' || c == 'E') {
        let (m, e) = buf.split_at(idx);
        let e = &e[1..];
        (m, e.parse().ok()?)
    } else {
        (buf, 0)
    };
    let (int_part, frac_part) = if let Some(idx) = mantissa.find('.') {
        let (a, b) = mantissa.split_at(idx);
        (a, &b[1..])
    } else {
        (mantissa, "")
    };
    let combined = format!("{}{}", int_part, frac_part);
    let num: i128 = combined.parse().ok()?;
    let frac_len = frac_part.len() as i32;
    let net_exp = exp - frac_len;
    let (p, q) = if net_exp >= 0 {
        let factor = 10i128.checked_pow(net_exp as u32)?;
        (num.checked_mul(factor)?, 1u128)
    } else {
        let factor = 10u128.checked_pow((-net_exp) as u32)?;
        (num, factor)
    };
    Some(reduce(p, q))
}

fn reduce(p: i128, q: u128) -> (i128, u128) {
    let g = gcd(p.unsigned_abs(), q);
    if g == 0 { return (p, q); }
    (p / g as i128, q / g)
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 { let t = b; b = a % b; a = t; }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input() {
        let toks = lex("").unwrap();
        assert_eq!(toks.len(), 1);
        assert!(matches!(toks[0].kind, TokKind::Eof));
    }

    #[test]
    fn ident_and_type_name() {
        let toks = lex("foo Bar").unwrap();
        assert!(matches!(&toks[0].kind, TokKind::Ident(s) if s == "foo"));
        assert!(matches!(&toks[1].kind, TokKind::TypeName(s) if s == "Bar"));
    }

    #[test]
    fn predicate_marker() {
        let toks = lex("unique?").unwrap();
        assert!(matches!(&toks[0].kind, TokKind::Ident(s) if s == "unique?"));
    }

    #[test]
    fn decimal_to_rat() {
        let toks = lex("0.13").unwrap();
        match &toks[0].kind {
            TokKind::RatLit(p, q) => { assert_eq!(*p, 13); assert_eq!(*q, 100); }
            _ => panic!(),
        }
    }

    #[test]
    fn scientific_to_rat() {
        let toks = lex("1e-9").unwrap();
        match &toks[0].kind {
            TokKind::RatLit(p, q) => { assert_eq!(*p, 1); assert_eq!(*q, 1_000_000_000); }
            _ => panic!(),
        }
    }

    #[test]
    fn glyph_aliases() {
        let toks = lex("∀ ε δ →").unwrap();
        assert!(matches!(&toks[0].kind, TokKind::Ident(s) if s == "forall"));
        assert!(matches!(&toks[1].kind, TokKind::Ident(s) if s == "e"));
        assert!(matches!(&toks[2].kind, TokKind::Ident(s) if s == "d"));
        assert!(matches!(&toks[3].kind, TokKind::Arrow));
    }

    #[test]
    fn indent_dedent() {
        let src = "a\n  b\n  c\nd\n";
        let toks = lex(src).unwrap();
        let kinds: Vec<&TokKind> = toks.iter().map(|t| &t.kind).collect();
        assert!(kinds.iter().any(|k| matches!(k, TokKind::Indent)));
        assert!(kinds.iter().any(|k| matches!(k, TokKind::Dedent)));
    }

    #[test]
    fn tab_is_error() {
        let r = lex("\ta\n");
        assert!(r.is_err());
    }

    #[test]
    fn comment_skipped() {
        let toks = lex("# hello\nfoo\n").unwrap();
        assert!(matches!(&toks[0].kind, TokKind::Ident(s) if s == "foo"));
    }

    #[test]
    fn list_type_bracket_pair() {
        let toks = lex("Int[]").unwrap();
        assert!(matches!(&toks[0].kind, TokKind::TypeName(s) if s == "Int"));
        assert!(matches!(&toks[1].kind, TokKind::BracketPair));
    }
}
