#![forbid(unsafe_code)]

mod extract;
mod resolve;
mod search;

pub use resolve::{link, LinkError, LinkResult, ResolvedDep};
