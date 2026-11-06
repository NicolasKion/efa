//! pyfa's graphs: their definitions, the math behind every curve, and sampling.
//!
//! The numbers come from refine through [`snapshot::FitSnapshot`]; everything here is plain
//! computation, so the UI can sample freely.

pub mod capsim;
pub mod defs;
pub mod math;
pub mod plot;
pub mod profiles;
pub mod snapshot;
