#![cfg_attr(not(test), no_std)]

pub mod clock;
pub mod ids;
pub mod wake;

#[cfg(target_arch = "arm")]
pub mod kernel;
#[cfg(target_arch = "arm")]
mod platform;
