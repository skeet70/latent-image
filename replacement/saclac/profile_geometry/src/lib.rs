#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

//! saclac's L4 layer: the image-5 DCF profile tables, pipeline geometry and selection that
//! task 43's controllers, measurement round, search and shot sequence share.

pub mod numeric;
pub mod profile;
#[allow(unsafe_code)]
pub mod stock;

#[cfg(test)]
mod tests;
