use apsl_artifact::{check, Check};
use apsl_parse::parse_str;

#[test]
fn rejects_duplicate_contract_names_before_artifact_construction() {
    let program = parse_str(
        "duplicate : Int -> Int\n  cx O(1) idem\n\n\
         duplicate : Int -> Int\n  cx O(1) idem\n\n\
         graph pipeline : Int -> Int\n  flow in -> duplicate -> out\n",
    )
    .expect("fixture parses");

    let errors = check(&program, &[]).expect_err("duplicate identity must be ambiguous");
    assert!(errors.iter().any(|error| error
        .to_string()
        .contains("duplicate declaration `duplicate`")));
}

#[test]
fn state_check_rejects_a_fixed_default_outside_its_declared_type() {
    let program = parse_str(
        "type Endpoint = String\n\n\
         service : Int -> Int\n  state endpoint : Endpoint = 42\n  cx O(1) idem\n",
    )
    .expect("fixture parses");

    let errors = check(&program, &[Check::State]).expect_err("invalid fixed state must fail");
    assert!(errors.iter().any(|error| error
        .to_string()
        .contains("fixed default does not inhabit `Endpoint`")));
}
