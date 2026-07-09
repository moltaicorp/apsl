
#![forbid(unsafe_code)]

pub mod env;
pub mod infer;
pub mod types;

pub use env::{is_primitive, lambda_slot, primitives};
pub use infer::{type_check, TypeError, TypedProgram};
pub use types::{ast_type_to_ty, instantiate, unify, Env, Scheme, Subst, Ty, TyGen, UnifyError};
