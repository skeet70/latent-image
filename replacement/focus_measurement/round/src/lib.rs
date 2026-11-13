#![no_std]
#![forbid(unsafe_code)]

pub mod jobs;
pub mod acquisition;
pub mod children;
pub mod lanes;
pub mod history;
pub mod round;
pub mod image;
pub mod live;
pub mod stock;
pub mod planner;
pub mod profile;
pub mod record;
pub mod pipeline;
pub mod window;
pub mod auxiliary;
pub mod source;

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
#[cfg(test)]
mod image_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod fakes;
#[cfg(test)]
mod record_tests;
#[cfg(test)]
mod window_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod auxiliary_tests;
#[cfg(test)]
mod source_tests;
