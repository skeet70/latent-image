#![no_std]
#![forbid(unsafe_code)]

//! saclac's calculation library: requests to calc (queue 44), calc1 (queue 45) and calcsub
//! (queue 46), and the computations its measurement and shot modules share.

pub mod adjust;
pub mod capture_requests;
pub mod diagnostics;
pub mod exposure;
pub mod finish;
pub mod geometry;
pub mod library_state;
pub mod policy;
pub mod profile;
pub mod record_policy;
pub mod requests;
pub mod scene_levels;
pub mod scene_recognition;
pub mod selection;
pub mod stock;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod capture_requests_tests;
#[cfg(test)]
mod diagnostics_tests;
#[cfg(test)]
mod finish_tests;
#[cfg(test)]
mod geometry_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod profile_tests;
#[cfg(test)]
mod record_policy_tests;
#[cfg(test)]
mod requests_tests;
#[cfg(test)]
mod scene_levels_tests;
#[cfg(test)]
mod selection_tests;
