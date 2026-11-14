#![forbid(unsafe_code)]

//! Readout timing of capture profiles: frame and exposure line counts in eighths of a line,
//! their intervals in microseconds, and the shutter a profile's line grid can realise.

use super::ports::{Adjustments, ControlState, ControlTables, LoadTest, LoadTestSetting, Profiles, Reference, Runtime};
use super::{round_tenth, round_tenth_wide, AdjustmentKey, ProfileHandle, ProfileKind, Rectangle, ShutterLevel, Unsupported};

pub trait ControlPorts: Adjustments + Profiles + ControlTables + LoadTest + ControlState + Runtime {}

impl<T: Adjustments + Profiles + ControlTables + LoadTest + ControlState + Runtime> ControlPorts for T {}

/// The pixel clock every timing here runs off.
const PIXEL_CLOCK_MHZ: u32 = 54;

/// The load tests under which configured and exposed profiles stretch their frame to the
/// configured limit.
const FRAME_LIMIT_TESTS: [LoadTestSetting; 3] = [LoadTestSetting(0x83), LoadTestSetting(0x8a), LoadTestSetting(0x8b)];

/// Milliseconds, low byte first.
const FRAME_LIMIT: AdjustmentKey = AdjustmentKey(0xca);
/// Sixteenths of a line added to a shutter-following exposure.
const EXPOSURE_MARGIN: AdjustmentKey = AdjustmentKey(0x94c2);

/// A frame-rate selection; 0 lets the exposure interval follow the shutter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateSelection(pub u32);

fn reference(ports: &impl ControlTables, profile: ProfileHandle) -> Reference { ports.reference(ports.reference_index(profile)) }

/// One second's rate times two to the level's stops; zero from -32 stops down and from 64 up.
fn shutter_rate(ports: &impl ControlTables, level: ShutterLevel) -> u64 {
    let stops = (level.0 / 100) as i16;
    let step = (level.0 % 100 + 100) as u32;
    let rate = u64::from(ports.shutter_rate(step));
    if stops >= 0 {
        rate.checked_shl(stops as u32).unwrap_or(0)
    } else {
        let scale = 0x1_0000u64.checked_shr(u32::from(stops.unsigned_abs())).filter(|_| stops > -32).unwrap_or(0);
        (rate * scale) >> 16
    }
}

/// The shutter's exposure in eighths of a line, rounded on the tenth, at least 1.
fn exposure_lines(ports: &impl ControlTables, profile: ProfileHandle, level: ShutterLevel) -> u32 {
    const EIGHTHS_PER_SECOND_PER_KHZ: u64 = 8 * 10_000_000_000;
    let line = u64::from(reference(ports, profile).line_length as u16);
    let rate = shutter_rate(ports, level).wrapping_mul(line);
    if rate == 0 {
        return 1;
    }
    let lines = round_tenth_wide(EIGHTHS_PER_SECOND_PER_KHZ * u64::from(PIXEL_CLOCK_MHZ * 1000) / rate);
    if lines == 0 { 1 } else { lines as u32 }
}

/// The configured frame limit in microseconds, within [1000, the profile's frame period].
fn configured_limit(ports: &impl ControlPorts, profile: ProfileHandle) -> u32 {
    let configured = u32::from(ports.halfword(FRAME_LIMIT)) * 1000;
    configured.min(reference(ports, profile).frame_period).max(1000)
}

fn frame_limit_test(ports: &mut impl LoadTest) -> bool { ports.load_test().is_some_and(|setting| FRAME_LIMIT_TESTS.contains(&setting)) }

/// The reference's line count stretched from its frame period to `limit` microseconds.
fn stretched_lines(reference: &Reference, limit: u32) -> u32 {
    let scaled = reference.frame_period.wrapping_mul(reference.lines.wrapping_mul(10));
    round_tenth(scaled.checked_div(limit).unwrap_or(0))
}

const fn even(lines: u32) -> u32 { lines & !1 }

/// The profile's frame in eighths of a line; `None` for an undeclared kind.
fn frame_lines(ports: &mut impl ControlPorts, profile: ProfileHandle) -> Option<u32> {
    let kind = ports.kind(profile)?;
    let reference = reference(ports, profile);
    let lines = match kind {
        ProfileKind::Untimed => return Some(0),
        ProfileKind::Tabled => reference.lines,
        ProfileKind::Configured | ProfileKind::ConfiguredAlternate => {
            if frame_limit_test(ports) {
                even(stretched_lines(&reference, configured_limit(ports, profile)))
            } else {
                u32::from(ports.configured_lines())
            }
        }
        ProfileKind::RateLimited => {
            let limit = if frame_limit_test(ports) {
                configured_limit(ports, profile)
            } else {
                ports.rate_threshold(profile).wrapping_mul(1000)
            };
            even(if limit < reference.frame_period { stretched_lines(&reference, limit) } else { reference.lines })
        }
        ProfileKind::Exposed => {
            if frame_limit_test(ports) {
                even(stretched_lines(&reference, configured_limit(ports, profile)))
            } else {
                reference.lines
            }
        }
    };
    Some(lines << 3)
}

/// Microseconds of `eighths` lines.
fn interval(reference: &Reference, eighths: u32) -> u32 { reference.line_length.wrapping_mul(eighths >> 3) / PIXEL_CLOCK_MHZ }

const fn shutter_following(kind: ProfileKind) -> bool {
    matches!(kind, ProfileKind::Configured | ProfileKind::ConfiguredAlternate | ProfileKind::Exposed)
}

/// The profile's frame interval in microseconds (0x1005f60).
pub fn frame_interval(ports: &mut impl ControlPorts, profile: ProfileHandle) -> Result<u32, Unsupported> {
    match ports.kind(profile) {
        Some(kind) if shutter_following(kind) => {}
        _ => return Err(Unsupported),
    }
    let frame = frame_lines(ports, profile).ok_or(Unsupported)?;
    Ok(interval(&reference(ports, profile), frame))
}

/// Twice a line's duration in microseconds, rounded on the tenth (0x1004d68).
pub fn line_pair_interval(ports: &mut impl ControlPorts, profile: ProfileHandle) -> Result<u32, Unsupported> {
    match ports.kind(profile) {
        Some(kind) if shutter_following(kind) => {}
        _ => return Err(Unsupported),
    }
    let tenths = reference(ports, profile).line_length.wrapping_mul(10) / PIXEL_CLOCK_MHZ;
    Ok(round_tenth(tenths * 2))
}

/// The interval of one exposure in microseconds: the configured kind's frame, or the exposed
/// kind's frame at `rate`, or with no rate its shutter rounded to whole lines plus the margin and
/// at least the frame (0x10061ec).
pub fn exposure_interval(
    ports: &mut impl ControlPorts,
    profile: ProfileHandle,
    level: ShutterLevel,
    rate: RateSelection,
) -> Result<u32, Unsupported> {
    let reference = reference(ports, profile);
    let eighths = match ports.kind(profile) {
        Some(ProfileKind::Configured) => frame_lines(ports, profile).ok_or(Unsupported)?,
        Some(ProfileKind::Exposed) if rate.0 == 0 => {
            let frame = frame_lines(ports, profile).ok_or(Unsupported)?;
            let margin = u32::from(ports.byte(EXPOSURE_MARGIN)) << 4;
            round_to_line(exposure_lines(ports, profile, level)).wrapping_add(margin).max(frame)
        }
        Some(ProfileKind::Exposed) => {
            let per_rate = reference.lines.wrapping_mul(reference.frame_period);
            let divisor = u32::from(ports.channel_divisor(rate.0));
            round_tenth(per_rate.checked_div(divisor).unwrap_or(0)) << 3
        }
        _ => return Err(Unsupported),
    };
    Ok(interval(&reference, eighths))
}

/// Eighths rounded half up to a whole line.
const fn round_to_line(eighths: u32) -> u32 { (eighths & !7) + if eighths & 7 > 3 { 8 } else { 0 } }

/// The shutter the profile's line grid realises for `level`: the exposure rounded so the frame's
/// remaining lines are a multiple of two lines, at least two (0x1005df8).
pub fn quantized_shutter(ports: &mut impl ControlPorts, profile: ProfileHandle, level: ShutterLevel) -> Result<ShutterLevel, Unsupported> {
    let reference = reference(ports, profile);
    let frame = frame_lines(ports, profile).ok_or(Unsupported)?;
    let exposure = exposure_lines(ports, profile, level).max(16);
    let blank = if exposure >= frame { 16 } else { (frame - exposure).next_multiple_of(16) };
    let exposed = frame.wrapping_sub(blank);
    let nanoseconds = ((u64::from(reference.line_length) * 1000).wrapping_mul(u64::from(exposed)) >> 3) / u64::from(PIXEL_CLOCK_MHZ);
    let rate = 1.0 / (nanoseconds as u32 as f32 / 1.0e9);
    let stops = ports.log(rate) / ports.log(2.0);
    let thousandths = (stops * 1000.0) as i32 as i16;
    let hundredths = thousandths / 10 + i16::from(thousandths % 10 > 4);
    Ok(ShutterLevel(i32::from(hundredths)))
}

/// The profile's crop rectangle; the exposed kind takes its reference's left edge and one column
/// less (0x1004d04).
pub fn readout_window(ports: &mut impl ControlPorts, profile: ProfileHandle, preserve_axes: bool) -> Rectangle {
    let kind = ports.kind(profile);
    let crop = ports.crop(profile, preserve_axes);
    if kind != Some(ProfileKind::Exposed) {
        return crop;
    }
    Rectangle { left: u32::from(reference(ports, profile).left), width: crop.width.wrapping_sub(1), ..crop }
}

/// 0x1004ce0.
pub fn left_reference(ports: &impl ControlTables, profile: ProfileHandle) -> u16 { reference(ports, profile).left_reference }

/// The adjusted reference ratio of a group entry: its second halfword per thousand of its first,
/// rounded on the tenth (0x1007e44).
pub fn reference_scale(ports: &(impl ControlTables + Adjustments), group: u32, entry: u32) -> u32 {
    let [denominator, numerator] = ports.scale_keys(group, entry);
    let denominator = ports.halfword(denominator).max(1);
    let numerator = u32::from(ports.halfword(numerator)) * 1000;
    round_tenth(numerator / u32::from(denominator))
}

/// The decade table's value at `value`'s step scaled by ten to its decades.
fn decade_scale(ports: &impl ControlTables, value: i16) -> u64 {
    let decades = value / 10_000;
    let step = u32::from((value % 10_000 + 10_000) as u16);
    let base = u64::from(ports.decade(step));
    let power = 10u64.wrapping_pow(u32::from(decades.unsigned_abs()));
    if decades >= 0 { base.wrapping_mul(power) } else { base / power }
}

/// The control state's sample period of the profile converted through the reference ratio and the
/// decade table, in tenths (0x10065fc).
pub fn converted_sample_period(ports: &mut impl ControlPorts, profile: ProfileHandle) -> u16 {
    const RANGE: i64 = 0xfff;
    let (period, _) = ports.sample_period(profile);
    let period = (round_tenth(u32::from(period)) * 10) as u16;
    let fraction = ((RANGE - i64::from(period)) * 100_000_000) as u64 / RANGE as u64;
    let sample = ports.reference_ratio(fraction as u32).unwrap_or(0);
    let unit = ports.reference_ratio(100_000_000).unwrap_or(0);
    let difference = (i64::from(sample) - i64::from(unit)).wrapping_mul(-200_000) / 200_000_000;
    let level = round_tenth_wide(difference as u64) as u32 as i16;
    round_tenth_wide(decade_scale(ports, level) / 10) as u16
}
