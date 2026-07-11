use apsl_core::ast::CxExpr;

use crate::algebra::{normalize, Weight};
use crate::NodeStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKind {
    NestedIteration,
    SequentialFoldOverLookup,
    RecursionGrowsInput,
    OverBound,
}

pub fn hint_for_status(s: &NodeStatus) -> String {
    match s {
        NodeStatus::Ok => String::new(),
        NodeStatus::Exceeds { hint, .. } => hint.clone(),
        NodeStatus::Mismatch { hint } => hint.clone(),
    }
}

pub fn classify(e: &CxExpr) -> HintKind {
    let poly = normalize(e);
    for t in &poly {
        for w in t.factors.values() {
            if *w > Weight::NLogN {
                return HintKind::NestedIteration;
            }
        }
    }
    HintKind::OverBound
}

pub fn hint_for_kind(k: HintKind) -> String {
    match k {
        HintKind::NestedIteration => "\
Inner iteration over the same data as outer iteration produces O(n^2).
Fix by one of:
  (a) sort the data once (O(n log n)) and use a linear sweep,
  (b) group_by a key (O(n)) then map over groups,
  (c) build a lookup keyed by the inner-loop value and map over it."
            .into(),
        HintKind::SequentialFoldOverLookup => "\
fold with a non-associative reducer is sequential. If the reducer is
associative and has an identity, use map_reduce; otherwise pre-sort the
input and use a linear sweep."
            .into(),
        HintKind::RecursionGrowsInput => "\
A recursive call's argument is not structurally smaller than its caller's.
Structural recursion requires the argument to decrease toward a base case
on every call."
            .into(),
        HintKind::OverBound => "\
The body's derived complexity exceeds O(n log n). Refactor so the work
done per input element is O(log n) or less."
            .into(),
    }
}
