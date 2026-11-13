#![no_std]
#![forbid(unsafe_code)]

pub mod lifecycle;
pub mod adjustment;
pub mod stock;
pub mod replies;
pub mod children;
pub mod synchronous;
pub mod search;
pub mod records;
pub mod capture;
pub mod exports;
pub mod root;
pub mod live;
pub mod s1;
pub mod live_view;
pub mod layers;
pub mod live_profile;
mod live_autofocus;
pub mod live_statistics;
pub mod live_statistics_profile;
pub mod s1_statistics;
#[cfg(test)]
mod s1_statistics_tests;
#[cfg(test)]
mod live_statistics_tests;
#[cfg(test)]
mod live_statistics_profile_tests;
#[cfg(test)]
mod live_view_tests;
#[cfg(test)]
mod live_profile_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod s1_tests;
#[cfg(test)]
mod root_tests;
#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod export_tests;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod adjustment_tests;
#[cfg(test)]
mod reply_tests;
#[cfg(test)]
mod synchronous_tests;
#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod child_contract_tests;
#[cfg(test)]
mod record_tests;
