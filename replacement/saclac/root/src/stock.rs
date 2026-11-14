#![allow(unsafe_code)]

//! The root's stock boundary: X-M1 1.50 entries and memory the bound modules reach that no merged
//! module owns. Entries are entered and memory read through the shared layer's `Machine`.

pub mod acquisition;
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
/// The word the S1 drive's callback 0xf28e94 stores (0xf28eb8), read by the reply at 0xf28f94.
pub const S1_DRIVE_REPORT: u32 = 0x16b_750c;
/// The profile-record handle the focus controller holds (0x16be004 + 0x18), under which
/// 0xf32a48 clears the focus history.
pub const FOCUS_CONTROLLER_RECORD: u32 = 0x16b_e01c;
/// The word 0x742a6c stores a lens request's callback in before it requests.
pub const LENS_REQUEST_CALLBACK: u32 = 0x96_bd04;
