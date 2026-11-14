#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

//! saclac's composition root. `Services` binds each module's port traits to the module that
//! implements them, and what no merged module implements to the stock entries and memory in
//! `stock`.

pub mod geometry;
pub mod lens;
pub mod lens_com;
pub mod records;
pub mod scene;
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
