#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub mod completion;
pub mod handles;
pub mod requests;
pub mod routing;
#[allow(unsafe_code)]
pub mod stock;
#[allow(unsafe_code)]
pub mod stock_requests;
#[allow(unsafe_code)]
pub mod stock_workers;
pub mod workers;
pub mod search;
#[allow(unsafe_code)]
pub mod stock_search;
#[allow(unsafe_code)]
pub mod stock_volume;
pub mod volume;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod routing_tests;
#[cfg(test)]
mod workers_tests;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod volume_tests;
