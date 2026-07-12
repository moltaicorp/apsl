#![forbid(unsafe_code)]

pub mod discharge;
pub mod encode;
pub mod explain;
pub mod solver;

pub use discharge::{discharge_node, ClauseResult, ClauseStatus, DischargeReport};
pub use encode::{encode_vc, EmptyTypeOracle, Smt2Script, TypeOracle};
pub use explain::explain;
pub use solver::{default_solver, Cvc5Solver, Model, NullSolver, Solver, SolverResult, Z3Solver};
