#![allow(unsafe_code)]

//! The root's stock boundary: X-M1 1.50 entries and memory the bound modules reach that no merged
//! module owns. Entries are entered and memory read through the shared layer's `Machine`.

pub mod acquisition;
pub mod shot;
pub mod callbacks;
pub(crate) mod capture_moves;
pub(crate) mod capture_statistics;
pub mod statistics;
mod commands;
pub mod stages;
pub mod geometry;
mod geometry_services;
mod geometry_radial;
mod geometry_numeric;

/// Lens control's resource index (0x76d9c8).
pub const RESOURCE_INDEX: u32 = 0x76_d9c8;
/// The configuration lookup selection the bring-up applies the resource index to (0x740b18).
pub const SELECT_LOOKUP: u32 = 0x74_0b18;
/// saclac's fault reaction (0xf46c20).
pub const FAULT: u32 = 0xf4_6c20;
/// The word the live drive's callback records from Lens34, read by the reply at 0xf14f14.
pub const LIVE_DRIVE_RESULT: u32 = 0x95_4dbc;
/// The live intake's cancel/command origin (efe668 -> f238c0), read by f238e0.
pub const LIVE_DRIVE_ORIGIN: u32 = 0x16b_5b14;
/// Waiting bits, deferred count and lens-stop byte published by the S1 router.
pub const S1_PROGRESS: u32 = 0x169_d508;
/// The word the S1 drive's callback 0xf28e94 stores (0xf28eb8), read by the reply at 0xf28f94.
pub const S1_DRIVE_REPORT: u32 = 0x16b_750c;
/// The profile-record handle the focus controller holds (0x16be004 + 0x18), under which
/// 0xf32a48 clears the focus history.
pub const FOCUS_CONTROLLER_RECORD: u32 = 0x16b_e01c;
/// The word 0x742a6c stores a lens request's callback in before it requests.
pub const LENS_REQUEST_CALLBACK: u32 = 0x96_bd04;
/// Current receipt's status word at 169d370+10 (f2e65c), not a stored command parameter.
pub const CURRENT_COMPLETION_STATUS: u32 = 0x169_d380;
/// Calc's frame-header source at 16cd228+14 (f176d0..f176e0).
pub const CALC_FRAME_HEADER_SOURCE: u32 = 0x16c_d23c;
/// Root rendezvous waiter task words: lens at +0 and adjustment at +4 (f47288..f472f4).
pub const RENDEZVOUS_WAITERS: u32 = 0x16c_2214;
/// Shared adjustment service: invokes the supplied callback after synchronizing (f472a0).
pub const REQUEST_ADJUSTMENT_SYNCHRONIZATION: u32 = 0x74_4c48;
/// Subject selection registers task at16a049c+8 before eeccc0 (f157e0..f157fc).
pub const SUBJECT_SELECTION_WAITER: u32 = 0x16a_04a4;
pub const START_SUBJECT_SELECTION: u32 = 0xee_ccc0;
pub const END_SUBJECT_SELECTION: u32 = 0xee_cddc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ControlDistance(pub(crate) i16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlDistanceLimit(pub i32);

/// Retained control-distance limit primitive (0x1005700).
pub fn control_distance_limit(machine: &impl xm1_saclac_shared::stock::Machine) -> ControlDistanceLimit {
    ControlDistanceLimit(machine.enter(0x100_5700, &[]) as i32)
}
