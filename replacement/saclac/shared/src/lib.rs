#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

//! The saclac helpers more than one module calls. Each module binds the trait it declares to
//! these at saclac's composition root.

pub mod commands;
pub mod display;
pub mod image_registers;
pub mod lens;
pub mod lens_com;
pub mod settings;
pub mod sort;
#[allow(unsafe_code)]
pub mod stock;

#[cfg(test)]
mod commands_tests;
#[cfg(test)]
mod display_tests;
#[cfg(test)]
mod image_registers_tests;
#[cfg(test)]
mod lens_com_tests;
#[cfg(test)]
mod lens_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod sort_tests;
