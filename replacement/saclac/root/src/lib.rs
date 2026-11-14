#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

//! saclac's composition root. `Services` binds each module's port traits to the module that
//! implements them, and what no merged module implements to the stock entries and memory in
//! `stock`.

pub mod geometry;
pub mod controller_geometry;
pub mod controller_source;
mod plan_services;
mod controller_replies;
mod calculation_memory;
pub mod calculation_services;
pub mod peer_context;
pub mod acquisition;
mod controller_records;
pub mod controller_services;
pub mod lens;
pub mod lens_com;
pub mod records;
pub mod scene;
pub mod search_runtime;
pub mod services;
pub mod stages;
#[allow(unsafe_code)]
pub mod stock;

pub use services::Services;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod lens_com_tests;
#[cfg(test)]
mod lens_tests;
#[cfg(test)]
mod records_tests;
#[cfg(test)]
mod stages_tests;
#[cfg(test)]
mod acquisition_tests;
#[cfg(test)]
mod peer_context_tests;
