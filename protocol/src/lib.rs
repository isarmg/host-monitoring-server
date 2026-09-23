//! Current JSON wire types shared by the remote host-monitor and Host worker.
//!
//! This crate deliberately contains no collection, validation, persistence, or HTTP logic. The
//! Client owns construction at platform boundaries and the Host worker owns trust-boundary
//! validation; both sides use these exact DTOs so their wire representations cannot drift
//! independently even though they live in separately versioned repositories.

#![forbid(unsafe_code)]

mod hardware;
pub mod json_u64;
mod pairing;
mod report;
pub use hardware::*;

pub use pairing::*;
pub use report::*;
