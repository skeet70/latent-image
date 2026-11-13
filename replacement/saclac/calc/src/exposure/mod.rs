//! The exposure evaluation engine: it prepares the selection bank and the tables calc evaluates
//! for one exposure, builds the evaluation and correction requests, and keeps what the shot reads
//! back.

pub mod context;
pub mod levels;
pub mod ports;
pub mod settings;
pub mod snapshot;
pub mod tables;

#[cfg(test)]
pub mod fakes;
#[cfg(test)]
mod levels_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod tables_tests;
