#![no_std]
#![forbid(unsafe_code)]

pub mod contrast;
pub mod intake;
pub mod live;
pub mod requests;
pub mod stock;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod contrast_tests;
#[cfg(test)]
mod intake_tests;
#[cfg(test)]
mod live_tests;
