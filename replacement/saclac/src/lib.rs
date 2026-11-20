#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

#[cfg(target_arch = "arm")]
pub extern crate xm1_rtos_own_first_component;
#[cfg(target_arch = "arm")]
pub extern crate xm1_rtos_global_lock_component;
#[cfg(target_arch = "arm")]
extern crate xm1_image5_decoder;
#[cfg(target_arch = "arm")]
extern crate xm1_rtos_objects_bank_a;
#[cfg(target_arch = "arm")]
extern crate xm1_rtos_objects_bank_b;
#[cfg(target_arch = "arm")]
extern crate xm1_file_service;
#[cfg(target_arch = "arm")]
extern crate xm1_display_scanout;
#[cfg(target_arch = "arm")]
extern crate xm1_executable_loading;
#[cfg(target_arch = "arm")]
extern crate xm1_loading_policy;

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

