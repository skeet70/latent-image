#![forbid(unsafe_code)]

//! Adjustment reads, configuration channel checks and the conversions saclac shares.

use xm1_saclac_interface::RecordHandle;

/// A byte address in the adjustment area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdjustmentKey(pub u32);

/// The adjustment area, read a byte at a time (0x69f8fc).
pub trait Adjustments {
    fn byte(&self, key: AdjustmentKey) -> u8;
    /// The lens adjustment halfword (0x76da94).
    fn lens_adjustment(&self) -> u16;
    /// Whether the adjustment byte at 0x887c admits an unselected profile mode (0xf4733c).
    fn admits_unselected_mode(&self) -> bool;
}

pub fn signed_byte(adjustments: &impl Adjustments, key: AdjustmentKey) -> i8 { adjustments.byte(key) as i8 }

/// 0x69fec8.
pub fn halfword(adjustments: &impl Adjustments, key: AdjustmentKey) -> u16 {
    u16::from_le_bytes([adjustments.byte(key), adjustments.byte(AdjustmentKey(key.0 + 1))])
}

/// The profile record's measurement-selection byte; 0 admits an unselected mode (0xf47368).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeasurementSelection(pub u8);

impl MeasurementSelection {
    pub const fn is_unselected(self) -> bool { self.0 == 0 }
}

/// The profile record's mode byte at +0x47 (0xf47380).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileMode(pub u8);

pub trait ProfileHeaders {
    fn measurement_selection(&mut self, record: RecordHandle) -> MeasurementSelection;
    fn mode(&mut self, record: RecordHandle) -> ProfileMode;
}

/// 0xf47330.
pub fn unselected_mode(record: RecordHandle, adjustments: &impl Adjustments, headers: &mut impl ProfileHeaders) -> bool {
    adjustments.admits_unselected_mode()
        && headers.measurement_selection(record).is_unselected()
        && (4..=6).contains(&headers.mode(record).0)
}

/// A configuration channel a lens-drive reply names (0x74a214).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Channel(pub i32);

/// The configuration's channel count bounds the channels a lens-drive reply names (0x74a214).
pub trait Channels {
    fn is_valid(&self, channel: Channel) -> bool;
}

/// The seven configuration words 0xf47300 forwards to the remap (0x74947c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemapArguments(pub [u32; 7]);

/// The remap's halfword result, which its callers store (0xf181bc, 0xf18234, 0xf24154).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemappedWord(pub u16);

/// The configuration word remap 0xf47300 forwards its seven arguments to (0x74947c).
pub trait ChannelRemap {
    fn remap(&mut self, arguments: RemapArguments) -> RemappedWord;
}

/// The tenths thresholds 0x73fef0 compares against, from 0x8184bc.
const TENTHS_THRESHOLDS: [u8; 30] = [
    32, 36, 39, 42, 46, 49, 52, 55, 59, 62, 65, 69, 72, 75, 79, 82, 85, 89, 92, 95, 99, 102, 105, 109, 112, 115, 119, 122,
    125, 129,
];

/// The class of a tenths value, 6..=36 (0x73fef0).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenthsClass(u8);

impl TenthsClass {
    pub const fn get(self) -> u8 { self.0 }
}

/// 0x73fef0.
pub fn tenths_class(value: i32) -> TenthsClass {
    let tenths = (value / 10) as u8;
    if tenths < TENTHS_THRESHOLDS[0] {
        return TenthsClass(6);
    }
    let reached = TENTHS_THRESHOLDS[1..].iter().take_while(|threshold| tenths >= **threshold).count();
    TenthsClass(7 + reached as u8)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AspectPair(pub [u16; 2]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Aspect { ThreeFour, TwoThree, NineSixteen, Square }

impl Aspect {
    const fn recognized(pair: AspectPair) -> Option<Self> {
        match pair.0 {
            [3, 4] => Some(Self::ThreeFour),
            [2, 3] => Some(Self::TwoThree),
            [9, 16] => Some(Self::NineSixteen),
            [1, 1] => Some(Self::Square),
            _ => None,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::ThreeFour => 0,
            Self::TwoThree => 1,
            Self::NineSixteen => 2,
            Self::Square => 3,
        }
    }
}

/// 0x739500.
pub fn aspect(pair: AspectPair) -> Aspect { Aspect::recognized(pair).unwrap_or(Aspect::ThreeFour) }

/// A record mode the capture window table covers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureMode(u8);

impl CaptureMode {
    pub const fn new(mode: u32) -> Option<Self> {
        if mode >= 1 && mode <= 10 { Some(Self(mode as u8)) } else { None }
    }

    pub const fn get(self) -> u8 { self.0 }
}

/// Which aspect classes each capture mode scales, from the table at 0xdca8b0 (rows 1..=10).
const SCALED: [[bool; 4]; 10] = [
    [false, true, true, true],
    [false, true, true, true],
    [false, true, true, true],
    [true, false, false, false],
    [true, false, false, false],
    [false, false, true, false],
    [false, false, true, false],
    [false; 4],
    [false; 4],
    [false; 4],
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowVariant(pub u8);

/// The conversion settings a capture window reads.
pub trait Conversions {
    /// None where the conversion selection maps the pair to no variant (0x73958c returns 255).
    fn window_variant(&mut self, mode: CaptureMode, aspect: Aspect) -> Option<WindowVariant>;
    /// 0x734540.
    fn value(&mut self, variant: WindowVariant, aspect: Aspect) -> u32;
    /// 0x73469c.
    fn alternate(&mut self, variant: WindowVariant, aspect: Aspect) -> u32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureWindow { pub alternate: u32, pub value: u32 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowScale { Unscaled, Scaled(CaptureWindow) }

/// The conversion selection gives a scaled mode and aspect no variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoVariant;

/// The capture window of `mode` at `pair` (0xdca798).
pub fn capture_window(mode: CaptureMode, pair: AspectPair, conversions: &mut impl Conversions) -> Result<WindowScale, NoVariant> {
    let Some(aspect) = Aspect::recognized(pair) else { return Ok(WindowScale::Unscaled) };
    if !SCALED[usize::from(mode.0) - 1][aspect.index()] {
        return Ok(WindowScale::Unscaled);
    }
    let variant = conversions.window_variant(mode, aspect).ok_or(NoVariant)?;
    let value = conversions.value(variant, aspect);
    let alternate = conversions.alternate(variant, aspect);
    Ok(WindowScale::Scaled(CaptureWindow { alternate, value }))
}
