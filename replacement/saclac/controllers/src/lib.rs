#![no_std]
#![forbid(unsafe_code)]

pub mod lifecycle;
pub mod adjustment;
pub mod statistics;
pub mod stock;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod adjustment_tests;
#[cfg(test)]
mod configuration_tests;
