#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

//! saclac's imaging layers: the image-4 ISP units task 43 programs (L7) and the capture-pipeline
//! layer its modules share (L10).

pub mod auto;
pub mod capture;
pub mod driver;
pub mod mvrec;
pub mod raid0;
#[allow(unsafe_code)]
pub mod stock;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod auto_tests;
#[cfg(test)]
mod driver_tests;
#[cfg(test)]
mod mvrec_tests;
#[cfg(test)]
mod raid0_tests;
