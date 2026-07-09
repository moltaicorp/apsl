
#![forbid(unsafe_code)]

pub mod lex;
pub mod parse;

pub use lex::{lex, LexError, Tok, TokKind};
pub use parse::{parse_tokens, ParseError};

pub fn parse_str(src: &str) -> Result<apsl_core::Program, ParseError> {
    let toks = lex(src).map_err(|e| ParseError { msg: e.msg, span: e.span })?;
    parse_tokens(toks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input() {
        let p = parse_str("").unwrap();
        assert!(p.decls.is_empty());
    }

    #[test]
    fn type_alias() {
        let p = parse_str("type Email = String\n").unwrap();
        assert_eq!(p.decls.len(), 1);
    }

    #[test]
    fn minimal_node() {
        let src = "n : Int -> Int\n  pre  true\n  post  out = in\n  cx   O(1) idem\n";
        let p = parse_str(src).unwrap();
        assert_eq!(p.decls.len(), 1);
    }

    #[test]
    fn node_with_quantifier_alias() {
        let src = "n : String[] -> Bool\n  post  forall x in in. valid_email?(x)\n  cx    O(n) idem\n";
        let _p = parse_str(src).unwrap();
    }

    #[test]
    fn node_with_glyphs() {
        let src = "n : String[] -> Bool\n  post  ∀x ∈ in. valid_email?(x)\n  cx    O(n) idem\n";
        let _p = parse_str(src).unwrap();
    }

    #[test]
    fn graph_decl() {
        let src = "graph g : Int -> Int\n  flow  in -> a -> out\n";
        let _p = parse_str(src).unwrap();
    }

    #[test]
    fn dedupe_example() {
        let src = std::fs::read_to_string("../../examples/dedupe.apsl").unwrap();
        let p = parse_str(&src).unwrap();
        assert!(p.decls.len() >= 2);
    }

    #[test]
    fn sla_rational_p_over_q() {
        let src = "n : Int -> Int\n  cx O(1) idem\n  sla d <= 1/1000000000\n";
        let p = parse_str(src).unwrap();
        if let apsl_core::ast::Decl::Node(n) = &p.decls[0] {
            let sla = n.sla.as_ref().unwrap();
            assert_eq!(sla.delta, (1, 1_000_000_000));
        } else { panic!("expected node"); }
    }

    #[test]
    fn sla_rational_int_over_scientific() {
        let src = "n : Int -> Int\n  cx O(1) idem\n  sla d <= 1/1e9\n";
        let p = parse_str(src).unwrap();
        if let apsl_core::ast::Decl::Node(n) = &p.decls[0] {
            let sla = n.sla.as_ref().unwrap();
            assert_eq!(sla.delta, (1, 1_000_000_000));
        } else { panic!("expected node"); }
    }

    #[test]
    fn multi_line_flow_accumulates() {
        let src = "graph g : Int -> Int\n  flow in -> a -> c\n  flow in -> b -> c\n  flow c -> out\n";
        let p = parse_str(src).unwrap();
        if let apsl_core::ast::Decl::Graph(g) = &p.decls[0] {
            assert_eq!(g.flow.len(), 3);
        } else { panic!("expected graph"); }
    }

    #[test]
    fn tuple_source_flow_step() {
        let src = "graph g : Int -> Int\n  flow (a, b) -> c -> out\n";
        let p = parse_str(src).unwrap();
        if let apsl_core::ast::Decl::Graph(g) = &p.decls[0] {
            assert_eq!(g.flow.len(), 1);
            let chain = &g.flow[0];
            assert_eq!(chain.len(), 3);
            assert_eq!(chain[0].nodes.len(), 2);
            assert_eq!(chain[0].nodes[0].as_str(), "a");
            assert_eq!(chain[0].nodes[1].as_str(), "b");
            assert_eq!(chain[1].nodes.len(), 1);
            assert_eq!(chain[1].nodes[0].as_str(), "c");
        } else { panic!("expected graph"); }
    }

    #[test]
    fn world_threading_example() {
        let src = std::fs::read_to_string("../../examples/world_threading.apsl").unwrap();
        let _p = parse_str(&src).unwrap();
    }

    #[test]
    fn via_statistical_required_holdout() {
        let src = "n : Int -> Int\n  cx O(1) idem\n  via @statistical holdout=intents_v3\n";
        let p = parse_str(src).unwrap();
        if let apsl_core::ast::Decl::Node(n) = &p.decls[0] {
            let v = n.via.as_ref().unwrap();
            assert_eq!(v.tag.as_str(), "statistical");
            assert_eq!(v.attrs[0].0.as_str(), "holdout");
            assert_eq!(v.attrs[0].1.as_str(), "intents_v3");
        } else { panic!("expected node"); }
    }

    #[test]
    fn via_statistical_missing_holdout_rejected() {
        let src = "n : Int -> Int\n  cx O(1) idem\n  via @statistical\n";
        let err = parse_str(src).unwrap_err();
        assert!(
            err.msg.contains("requires attribute `holdout"),
            "actual error: {}", err.msg,
        );
    }

    #[test]
    fn via_undefined_tag_rejected() {
        let src = "n : Int -> Int\n  cx O(1) idem\n  via @cryptographic algorithm=ed25519\n";
        let err = parse_str(src).unwrap_err();
        assert!(err.msg.contains("no defined semantics"));
    }

    #[test]
    fn state_clause_basic() {
        let src = "n : Int -> Int\n  state origin : String\n  cx O(1) idem\n";
        let p = parse_str(src).unwrap();
        if let apsl_core::ast::Decl::Node(n) = &p.decls[0] {
            assert_eq!(n.state.len(), 1);
            assert_eq!(n.state[0].key.as_str(), "origin");
            assert!(n.state[0].default.is_none());
        } else { panic!("expected node"); }
    }

    #[test]
    fn state_clause_with_default() {
        let src = "n : Int -> Int\n  state ttl : Int = 86400\n  cx O(1) idem\n";
        let p = parse_str(src).unwrap();
        if let apsl_core::ast::Decl::Node(n) = &p.decls[0] {
            assert_eq!(n.state.len(), 1);
            assert_eq!(n.state[0].key.as_str(), "ttl");
            match &n.state[0].default {
                Some(apsl_core::ast::Lit::Int(v)) => assert_eq!(*v, 86400),
                other => panic!("expected Int(86400), got {:?}", other),
            }
        } else { panic!("expected node"); }
    }

    #[test]
    fn state_clause_multiple() {
        let src = "n : Int -> Int\n  state a : String\n  state b : Int\n  cx O(1) idem\n";
        let p = parse_str(src).unwrap();
        if let apsl_core::ast::Decl::Node(n) = &p.decls[0] {
            assert_eq!(n.state.len(), 2);
            assert_eq!(n.state[0].key.as_str(), "a");
            assert_eq!(n.state[1].key.as_str(), "b");
        } else { panic!("expected node"); }
    }

    #[test]
    fn state_clause_string_default() {
        let src = "n : Int -> Int\n  state addr : String = \"https://vault.example.com\"\n  cx O(1) idem\n";
        let p = parse_str(src).unwrap();
        if let apsl_core::ast::Decl::Node(n) = &p.decls[0] {
            assert_eq!(n.state.len(), 1);
            match &n.state[0].default {
                Some(apsl_core::ast::Lit::Str(s)) => assert_eq!(s, "https://vault.example.com"),
                other => panic!("expected Str, got {:?}", other),
            }
        } else { panic!("expected node"); }
    }

    #[test]
    fn node_without_state_has_empty_state() {
        let src = "n : Int -> Int\n  cx O(1) idem\n";
        let p = parse_str(src).unwrap();
        if let apsl_core::ast::Decl::Node(n) = &p.decls[0] {
            assert!(n.state.is_empty());
        } else { panic!("expected node"); }
    }
}
