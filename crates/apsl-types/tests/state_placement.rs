use apsl_core::ast::Decl;
use apsl_parse::parse_str;
use apsl_types::{node_placement, state_kind, NodePlacement, StateKind};

fn placement(source: &str) -> NodePlacement {
    let program = parse_str(source).unwrap();
    let node = program
        .decls
        .iter()
        .find_map(|declaration| match declaration {
            Decl::Node(node) => Some(node.as_ref()),
            _ => None,
        })
        .unwrap();
    node_placement(node)
}

#[test]
fn abstract_state_is_positional_and_fixed_or_absent_state_is_fungible() {
    assert_eq!(
        placement("work : Int -> Int\n  cx O(1) idem\n"),
        NodePlacement::Fungible
    );
    assert_eq!(
        placement(
            "type Method = String\n\nwork : Int -> Int\n  state method : Method = \"GET\"\n  cx O(1) idem\n"
        ),
        NodePlacement::Fungible
    );
    assert_eq!(
        placement(
            "type Endpoint = String\n\nwork : Int -> Int\n  state origin : Endpoint\n  cx O(1) idem\n"
        ),
        NodePlacement::Positional
    );
}

#[test]
fn missing_default_is_abstract_and_present_default_is_fixed() {
    let program = parse_str(
        "type Label = String\n\nwork : Int -> Int\n  state dynamic : Label\n  state constant : Label = \"fixed\"\n  cx O(1) idem\n",
    )
    .unwrap();
    let node = program
        .decls
        .iter()
        .find_map(|declaration| match declaration {
            Decl::Node(node) => Some(node.as_ref()),
            _ => None,
        })
        .unwrap();
    assert_eq!(state_kind(&node.state[0]), StateKind::Abstract);
    assert_eq!(state_kind(&node.state[1]), StateKind::Fixed);
}
