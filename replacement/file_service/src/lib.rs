#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub mod completion;
pub mod handles;
pub mod media_io;
pub mod requests;
pub mod routing;
#[allow(unsafe_code)]
pub mod stock;
#[allow(unsafe_code)]
pub mod stock_media_io;
#[allow(unsafe_code)]
pub mod stock_requests;
#[allow(unsafe_code)]
pub mod stock_workers;
pub mod workers;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod routing_tests;
#[cfg(test)]
mod workers_tests;
#[cfg(test)]
mod media_io_tests;
