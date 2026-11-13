#![no_std]
#![forbid(unsafe_code)]

pub mod jobs;
pub mod acquisition;
pub mod children;
pub mod lanes;
pub mod round;
pub mod stock;
pub mod planner;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod acquisition_tests;
#[cfg(test)]
mod child_tests;
#[cfg(test)]
mod round_tests;
#[cfg(test)]
mod stock_tests;
#[cfg(test)]
mod planner_tests;
