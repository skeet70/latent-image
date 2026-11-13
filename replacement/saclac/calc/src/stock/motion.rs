//! The focus section of the client's dump frame the motion layer traces into, and the
//! configuration ids it reads.

use xm1_saclac_interface::calc::SharedAddress;

use crate::exposure::ports::Property;

/// The section starts 0x116cc into the frame and is addressed through its alias with bit 30
/// clear (0xfa3dd8, 0x740e0c).
pub const fn section(frame: SharedAddress) -> SharedAddress { SharedAddress((frame.0 + 0x116cc) & !0x4000_0000) }

/// Closes each traced list.
pub const TERMINATOR: u32 = 0xfdfd_fdfd;

const fn at(section: SharedAddress, offset: u32) -> SharedAddress { SharedAddress(section.0 + offset) }

/// The lens position a sample's targets were taken around (0xfa3068).
pub const fn sample_position(section: SharedAddress, index: u32) -> SharedAddress { at(section, 0x2ec24 + 4 * index) }

/// The drive time of that position (0xfa309c).
pub const fn sample_time(section: SharedAddress, index: u32) -> SharedAddress { at(section, 0x44608 + 4 * index) }

/// The drive planning record: the start time, then the eleven planning words 0xf969a8 stores.
pub const fn drive_planning(section: SharedAddress, word: u32) -> SharedAddress { at(section, 0x44a28 + 4 * word) }

/// The speed schedule a replanning reads: frames, then speeds (0xfa34c4, 0xfa34cc).
pub const fn schedule_frame(section: SharedAddress, index: u32) -> SharedAddress { at(section, 0x44a58 + 4 * index) }

pub const fn schedule_speed(section: SharedAddress, index: u32) -> SharedAddress { at(section, 0x44aac + 4 * index) }

/// Percent of the sampled midpoint's distance from the target that corrects a sample target
/// (0xf96f84).
pub const SAMPLE_CORRECTION_SCALE: Property = Property(0x9276);
/// Signed bytes: the bias the correction adds in the drive's direction, and the base every
/// correction starts from (0xf96f90, 0xf96ff4).
pub const SAMPLE_CORRECTION_BIAS: Property = Property(0x9274);
pub const SAMPLE_CORRECTION_BASE: Property = Property(0x9275);

/// The fewest frames a measured run must span before its speed replaces the scheduled one
/// (0xfa337c).
pub const MEASURED_RUN_FRAMES: Property = Property(0x9280);
