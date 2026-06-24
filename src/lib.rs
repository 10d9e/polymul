//! Crate root.
//!
//! FROZEN FILE — do not edit as part of autoresearch. This only wires the
//! editable `algorithm` module to the frozen `harness`.

pub mod algorithm;
pub mod harness;

/// The stable contract the harness depends on.
pub use algorithm::{plan_new, poly_mul, Plan};
