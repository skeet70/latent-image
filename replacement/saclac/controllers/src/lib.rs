#![no_std]
#![forbid(unsafe_code)]

pub mod lifecycle;
pub mod adjustment;
pub mod statistics;
pub mod stock;
pub mod replies;
pub mod children;
pub mod synchronous;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod adjustment_tests;
#[cfg(test)]
mod configuration_tests;
#[cfg(test)]
mod reply_tests;
#[cfg(test)]
mod synchronous_tests;
