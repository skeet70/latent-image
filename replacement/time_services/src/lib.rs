#![no_std]
#![deny(unsafe_code)]

mod timers;
pub use timers::*;
pub mod interrupts;
pub mod driver;
#[cfg(target_arch = "arm")]
#[allow(unsafe_code)]
mod stock;
#[cfg(target_arch = "arm")]
pub use stock::{cancel_timer, reload_timer, start_timer};

#[cfg(test)]
mod tests;
