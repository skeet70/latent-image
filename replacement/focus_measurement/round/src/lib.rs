#![no_std]
#![forbid(unsafe_code)]

pub mod jobs;
pub mod acquisition;
pub mod children;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod acquisition_tests;
#[cfg(test)]
mod child_tests;
