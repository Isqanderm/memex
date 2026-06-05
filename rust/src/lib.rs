//! Memex library — exposes core modules for use by binaries and tests.

// Public API items are intentionally exposed for external consumers (binaries, tests, FFI).
// Clippy cannot see through crate boundaries, so suppress dead_code / unused_imports here.
#![allow(dead_code, unused_imports)]

pub mod config;
pub mod error;
pub mod db;
pub mod search;
pub mod ingestion;
pub mod llm;
pub mod memory;
pub mod api;
