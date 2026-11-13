#![no_std]
#![forbid(unsafe_code)]

//! saclac's calculation library: the requests saclac posts to calc (queue 44) and calc1
//! (queue 45), and the computations its measurement and shot modules share.

pub mod adjust;
pub mod capture_requests;
pub mod exposure;
pub mod finish;
pub mod geometry;
pub mod policy;
pub mod profile;
pub mod requests;
pub mod scene_recognition;
pub mod selection;
pub mod stock;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod capture_requests_tests;
#[cfg(test)]
mod finish_tests;
#[cfg(test)]
mod geometry_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod profile_tests;
#[cfg(test)]
mod requests_tests;
#[cfg(test)]
mod selection_tests;
