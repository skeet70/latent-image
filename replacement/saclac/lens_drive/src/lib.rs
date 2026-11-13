#![no_std]
#![forbid(unsafe_code)]

//! saclac's lens children: the bring-up child the root enters for LensBringUp and LensStop,
//! the live child the live router enters and the S1 child the S1 router enters. Every
//! request they make can reach a lens drive, so the module runs only in the emulator.

pub mod bring_up;
pub mod children;
pub mod live;
pub mod ports;
pub mod s1;
pub mod stock;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod bring_up_tests;
#[cfg(test)]
mod children_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod s1_tests;
#[cfg(test)]
mod stock_tests;
#[cfg(test)]
mod test_ports;

use xm1_saclac_interface::{ParameterValue, Parameters};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Complete,
    Pending,
    /// The child has no entry for the event.
    Inactive,
    Fault,
}

/// A completion's payload word; zero past the payload's end.
fn parameter_word(parameters: Parameters<'_>, index: usize) -> ParameterValue {
    let bytes = parameters.bytes();
    let word = bytes
        .get(index * 4..index * 4 + 4)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map_or(0, u32::from_le_bytes);
    ParameterValue(word)
}
