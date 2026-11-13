#![no_std]
#![forbid(unsafe_code)]

pub mod contrast;
pub mod live;
pub mod requests;
pub mod stock;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod contrast_tests;
#[cfg(test)]
mod live_tests;
