//! Shared library for balls GitHub plugins.
//!
//! Holds the code both `balls-plugin-github` (forge — pull requests) and
//! the future `balls-plugin-github-issues` (issue tracker) use: error
//! types, auth-dir-parameterized token I/O, the base GitHub HTTP client,
//! the shared half of plugin config (`repo`, `api_base`), and the
//! protocol-level types every plugin needs to deserialize a Task and
//! emit a SyncReport.
//!
//! Boundary invariant: this crate has zero references to any
//! per-plugin `external.<name>.*` projection — those are owned by
//! each plugin's own crate. The boundary is enforced by a unit test
//! (`projection_boundary_test`) that grep-asserts the source tree.

pub mod auth;
pub mod config_base;
pub mod error;
pub mod http;
pub mod types;

#[cfg(test)]
mod projection_boundary_test;
