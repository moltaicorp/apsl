
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use regex::Regex;
use yaml_rust2::parser::{Event, Parser, Tag};
use yaml_rust2::scanner::TScalarStyle;

use super::common::{is_wrap_content, trunc, Fault, Violation};

fn nonstr() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            "^(?:",
            "yes|Yes|YES|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF",
            "|[-+]?0b[0-1_]+|[-+]?0[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+",
            "|[-+]?(?:[0-9][0-9_]*)\\.[0-9_]*(?:[eE][-+][0-9]+)?|\\.[0-9][0-9_]*(?:[eE][-+][0-9]+)?|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\\.[0-9_]*|[-+]?\\.(?:inf|Inf|INF)|\\.(?:nan|NaN|NAN)",
            "|~|null|Null|NULL|",
            "|[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]|[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?",
            "|<<|=|!|&|\\*",
            ")$",
        ))
        .expect("yaml nonstr resolver")
    })
}

enum YNode {
    Scalar {
        value: String,
        is_str: bool,
        line: usize,
    },
    Seq(Vec<Rc<YNode>>),
    Map(Vec<(Rc<YNode>, Rc<YNode>)>),
}

fn scalar_is_str(value: &str, style: TScalarStyle, tag: &Option<Tag>) -> bool {
    if let Some(t) = tag {
        return t.suffix == "str";
    }
    match style {
        TScalarStyle::Plain => !nonstr().is_match(value),
        _ => true, // single/double-quoted, literal, folded → always a string
    }
}

fn walk(node: &Rc<YNode>, path: &str, out: &mut Vec<Violation>) {
    match &**node {
        YNode::Scalar {
            value,
            is_str,
            line,
        } => {
            if *is_str && !value.is_empty() && !is_wrap_content(value) {
                out.push(Violation {
                    path: path.to_string(),
                    line: *line,
                    rule: "bare-string",
                    snippet: trunc(value, 100),
                    lang: "yaml",
                });
            }
        }
        YNode::Map(pairs) => {
            for (k, v) in pairs {
                walk(k, path, out); // keys: state too
                walk(v, path, out); // values: state
            }
        }
        YNode::Seq(items) => {
            for it in items {
                walk(it, path, out);
            }
        }
    }
}

enum Builder {
    Seq(Vec<Rc<YNode>>),
    Map(Vec<(Rc<YNode>, Rc<YNode>)>, Option<Rc<YNode>>),
}

struct Frame {
    builder: Builder,
    aid: usize,
}

fn emit(node: Rc<YNode>, stack: &mut Vec<Frame>, path: &str, out: &mut Vec<Violation>) {
    match stack.last_mut() {
        Some(top) => match &mut top.builder {
            Builder::Seq(v) => v.push(node),
            Builder::Map(pairs, pending) => {
                if pending.is_none() {
                    *pending = Some(node);
                } else {
                    let k = pending.take().unwrap();
                    pairs.push((k, node));
                }
            }
        },
        None => walk(&node, path, out),
    }
}

pub fn scan(path: &str, text: &str, out: &mut Vec<Violation>, faults: &mut Vec<Fault>) {
    let mut parser = Parser::new_from_str(text);
    let mut stack: Vec<Frame> = Vec::new();
    let mut anchors: HashMap<usize, Rc<YNode>> = HashMap::new();
    loop {
        let (ev, mark) = match parser.next_token() {
            Ok(x) => x,
            Err(e) => {
                faults.push(Fault {
                    path: path.to_string(),
                    line: 0,
                    reason: format!("yaml parse error: {}", e),
                });
                return;
            }
        };
        match ev {
            Event::StreamEnd => break,
            Event::Scalar(value, style, aid, tag) => {
                let line = match style {
                    TScalarStyle::Literal | TScalarStyle::Folded => mark.line().saturating_sub(1),
                    _ => mark.line(),
                };
                let node = Rc::new(YNode::Scalar {
                    is_str: scalar_is_str(&value, style, &tag),
                    value,
                    line,
                });
                if aid != 0 {
                    anchors.insert(aid, node.clone());
                }
                emit(node, &mut stack, path, out);
            }
            Event::Alias(aid) => {
                if let Some(n) = anchors.get(&aid) {
                    let n = n.clone();
                    emit(n, &mut stack, path, out);
                }
            }
            Event::SequenceStart(aid, _) => stack.push(Frame {
                builder: Builder::Seq(Vec::new()),
                aid,
            }),
            Event::MappingStart(aid, _) => stack.push(Frame {
                builder: Builder::Map(Vec::new(), None),
                aid,
            }),
            Event::SequenceEnd => {
                if let Some(f) = stack.pop() {
                    let items = match f.builder {
                        Builder::Seq(v) => v,
                        Builder::Map(..) => Vec::new(),
                    };
                    let node = Rc::new(YNode::Seq(items));
                    if f.aid != 0 {
                        anchors.insert(f.aid, node.clone());
                    }
                    emit(node, &mut stack, path, out);
                }
            }
            Event::MappingEnd => {
                if let Some(f) = stack.pop() {
                    let pairs = match f.builder {
                        Builder::Map(p, _) => p,
                        Builder::Seq(_) => Vec::new(),
                    };
                    let node = Rc::new(YNode::Map(pairs));
                    if f.aid != 0 {
                        anchors.insert(f.aid, node.clone());
                    }
                    emit(node, &mut stack, path, out);
                }
            }
            _ => {}
        }
    }
}
