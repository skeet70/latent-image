#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub mod catalog;
pub mod completion;
pub mod counts;
pub mod handles;
pub mod paths;
pub mod requests;
pub mod routing;
pub mod search;
pub mod selection;
#[allow(unsafe_code)]
pub mod stock;
#[allow(unsafe_code)]
pub mod stock_catalog;
#[allow(unsafe_code)]
pub mod stock_counts;
#[allow(unsafe_code)]
pub mod stock_requests;
#[allow(unsafe_code)]
pub mod stock_paths;
#[allow(unsafe_code)]
pub mod stock_search;
#[allow(unsafe_code)]
pub mod stock_selection;
#[allow(unsafe_code)]
pub mod stock_volume;
#[allow(unsafe_code)]
pub mod stock_workers;
pub mod volume;
pub mod workers;

#[cfg(test)]
mod catalog_tests;
#[cfg(test)]
mod counts_tests;
#[cfg(test)]
mod paths_tests;
#[cfg(test)]
mod routing_tests;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod volume_tests;
#[cfg(test)]
mod workers_tests;
