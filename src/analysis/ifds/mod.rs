//! Deterministic summary-based IFDS/IDE framework.
//!
//! IFDS is the *Interprocedural Finite Distributive Subset* framework. IDE is
//! its *Interprocedural Distributive Environment* generalization. This module
//! supplies the common, analysis-independent procedure-summary fixpoint; taint,
//! effects, and other domains provide their own facts and transfer semantics.

mod domain;
mod solver;

pub use domain::{
    CalleeRef, IfdsCapEvent, IfdsCaps, IfdsIssue, IfdsOutput, Lattice, SummaryCtx, SummaryDomain,
};
pub use solver::{solve_summaries, solve_summaries_with_resolver};

#[cfg(test)]
mod tests;
