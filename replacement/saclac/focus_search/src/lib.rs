#![no_std]
#![forbid(unsafe_code)]

pub mod live;
pub mod requests;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod live_tests;
