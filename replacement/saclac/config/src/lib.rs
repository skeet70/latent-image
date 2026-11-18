#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

//! saclac's configuration layers: the image-4 configuration words and limits derived from the
//! adjustment area (L3), the image-5 scan and image-pipeline configuration (L8) and the
//! face-tracking entries (L11), which task 43's controllers, measurement round, search, shot
//! sequence and lens drive share.

pub mod adjustment;
pub mod channel;
pub mod compiled;
pub mod curve;
pub mod dump;
pub mod face;
pub mod limits;
pub mod numeric;
pub mod pipeline;
pub mod ports;
pub mod profile;
pub mod registers;
pub mod scan;
pub mod services;
#[allow(unsafe_code)]
pub mod stock;
pub mod table;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod table_tests;
#[cfg(test)]
mod channel_tests;
#[cfg(test)]
mod limits_tests;
#[cfg(test)]
mod profile_tests;
#[cfg(test)]
mod layer_tests;
#[cfg(test)]
mod pipeline_tests;
#[cfg(test)]
mod compiled_tests;
#[cfg(test)]
mod dump_tests;
#[cfg(test)]
mod stock_tests;
