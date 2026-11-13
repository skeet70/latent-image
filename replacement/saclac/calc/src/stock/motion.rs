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

/// Set by the measurement pipeline and read back by the motion layer (0xf9d3f4).
pub const fn pipeline_flag(section: SharedAddress) -> SharedAddress { at(section, 0x45dac) }

/// Ten times the ticks since preparation, when the pipeline asks (0xf9cdcc).
pub const fn elapsed(section: SharedAddress) -> SharedAddress { at(section, 0x44c10) }

/// The contrast results 0x6c3c0 into the frame, addressed without the alias (0xfa3dc4).
pub const fn results(frame: SharedAddress) -> SharedAddress { SharedAddress(frame.0 + 0x6c3c0) }

pub const RESULT_AREAS: u32 = 5;

/// Non-zero once the results hold a measurement.
pub const fn results_present(results: SharedAddress) -> SharedAddress { at(results, 0x4) }

/// Where the peak check records the threshold it compared with (0xf9623c).
pub const fn results_threshold(results: SharedAddress) -> SharedAddress { at(results, 0x970) }

/// 1 for an area the results measured.
pub const fn result_measured(results: SharedAddress, area: u32) -> SharedAddress { at(results, 0xc + 4 * area) }

/// The area's two levels the peak check compares (0xf96258, 0xf96264).
pub const fn result_levels(results: SharedAddress, area: u32) -> [SharedAddress; 2] {
    [at(results, 0x8a4 + 4 * area), at(results, 0x8b8 + 4 * area)]
}

/// Whether the peak check runs at all (0xf9ed5c).
pub const PEAK_CHECK: Property = Property(0x92ca);
/// Two bytes: the level an area must reach (0xf96230).
pub const PEAK_THRESHOLD: Property = Property(0x944f);

/// Two bytes each: the bounds a step offset's scale is clamped to (0xfa3598, 0xfa35a8).
pub const STEP_SCALE_HIGH: Property = Property(0x3e_270d);
pub const STEP_SCALE_LOW: Property = Property(0x3e_270f);
/// The scale selector for kinds outside 5..=18 (0xfa35e8).
pub const STEP_SCALE_SELECTOR: Property = Property(0x9301);

/// Two bytes: the scale of a step offset for `kind` (0xfa35d0, 0xfa35f0).
pub const fn step_scale(kind: u32, selector: u32) -> Property {
    if kind.wrapping_sub(5) <= 13 { Property(((kind - 5) + 0x4ee2) * 2) } else { Property(0x9305 + 2 * selector) }
}
