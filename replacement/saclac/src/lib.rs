#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

#[cfg(target_arch = "arm")]
pub extern crate xm1_rtos_own_first_component;
#[cfg(target_arch = "arm")]
pub extern crate xm1_rtos_global_lock_component;
#[cfg(target_arch = "arm")]
extern crate xm1_image5_decoder;

pub mod engine;
pub mod driver;
pub mod machine0;
#[allow(unsafe_code)]
pub mod stock;
#[cfg(target_arch = "arm")]
#[allow(unsafe_code)]
mod task_stack;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod driver_tests;

