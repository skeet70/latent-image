//! The exposure evaluation engine: it prepares the selection bank and the tables calc evaluates
//! for one exposure, builds the evaluation and correction requests, and keeps what the shot reads
//! back.

pub mod adjustment;
pub mod context;
pub mod correction;
pub mod evaluation;
pub mod levels;
pub mod ports;
pub mod prepare;
pub mod program;
pub mod settings;
pub mod snapshot;
pub mod tables;

#[cfg(test)]
pub mod fakes;
#[cfg(test)]
mod adjustment_tests;
#[cfg(test)]
mod correction_tests;
#[cfg(test)]
mod evaluation_tests;
#[cfg(test)]
mod levels_tests;
#[cfg(test)]
mod program_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod tables_tests;
