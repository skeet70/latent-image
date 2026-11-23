#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub mod machine;
#[cfg(target_arch = "arm")]
#[allow(unsafe_code)]
mod stock;

#[cfg(test)]
mod tests;
