#![forbid(unsafe_code)]

pub mod env;
pub mod infer;
pub mod string_strict;
pub mod types;

pub use env::{is_primitive, lambda_slot, primitives};
pub use infer::{type_check, TypeError, TypedProgram};
pub use string_strict::{
    check_state_defaults, check_string_strict, node_placement, state_kind, NodePlacement, StateKind,
};
pub use types::{ast_type_to_ty, instantiate, unify, Env, Scheme, Subst, Ty, TyGen, UnifyError};
