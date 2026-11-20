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
// The file service and the display scanout; the loading compositions link Root, the family and
// the loading owner only.
#[cfg(all(target_arch = "arm", not(loading_owner)))]
extern crate xm1_file_service;
#[cfg(all(target_arch = "arm", not(loading_owner)))]
extern crate xm1_display_scanout;
// The task60 loading owner and one explicit policy; linked only by the
// `saclac-image-loading-*` bank compositions.
#[cfg(all(target_arch = "arm", loading_owner))]
extern crate xm1_executable_loading;
#[cfg(all(target_arch = "arm", loading_owner))]
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

