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

use ports::{LensFlags, Probe, Probes};
use xm1_saclac_interface::{ParameterValue, Parameters};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Complete,
    Pending,
    /// The child has no entry for the event.
    Inactive,
    Fault,
}

fn flags(probes: Probes) -> LensFlags {
    let mut bits = 0;
    if probes.bit1 == Probe::Forced { bits |= 2; }
    if probes.bit0 == Probe::Forced { bits |= 1; }
    if probes.bit2 == Probe::Forced { bits |= 4; }
    LensFlags(bits)
}

/// The step replies' flags: X-M1 1.50 0xf14fa0 assigns bit 0 where the other replies OR it,
/// so a forced bit-0 probe clears bit 1.
fn step_flags(probes: Probes) -> LensFlags {
    let mut bits = if probes.bit1 == Probe::Forced { 2 } else { 0 };
    if probes.bit0 == Probe::Forced { bits = 1; }
    if probes.bit2 == Probe::Forced { bits |= 4; }
    LensFlags(bits)
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
