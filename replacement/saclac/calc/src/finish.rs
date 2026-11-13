//! The library functions the shot's finish calls.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::exposure::ports::{ImageReport, ProfileAccess, Properties, StockMemory};
use crate::exposure::settings;
use crate::stock::finish as stock;

/// The scene kind that takes the scene rate factor.
const RATE_SCENE: u8 = 28;
/// Record byte +0x48 that takes the scene rate factor.
const RATE_RECORD_KIND: u8 = 7;
const BANDS: usize = 4;

/// Image 4's lens services the target selection reads.
pub trait TargetLens {
    /// The lens resource's configured value in tens; 0 past the lens's resources (0x747bf8).
    fn resource_value(&mut self, resource: u32) -> u32;
    /// 0x749be4.
    fn geometry_value(&mut self, position: i16, reference: u32, control: u32, lens_control: u32, kind: u32) -> u32;
    /// 0x746864.
    fn mapped_value(&mut self, value: u32, bank: u32) -> u32;
}

/// Where a target's drive rate stands against its band.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateClass {
    Below,
    Within,
    Above,
}

impl RateClass {
    pub const fn code(self) -> u32 {
        match self {
            Self::Below => 0,
            Self::Within => 1,
            Self::Above => 2,
        }
    }
}

/// What the finish asks the target selection about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetQuery {
    pub record: RecordHandle,
    pub lens_reference: u32,
    pub position: i16,
    pub control: u32,
    pub kind: u32,
    pub lens_control: u32,
    /// The client's frame the selection traces into.
    pub frame: Option<SharedAddress>,
}

fn trace(ports: &mut impl StockMemory, frame: Option<SharedAddress>, word: usize, value: u32) {
    if let Some(frame) = frame {
        ports.write_u32(stock::target_trace(frame, word), value);
    }
}

/// The class a band's bounds fix without comparing the rate.
fn fixed_class(lower: u16, upper: u16) -> Option<RateClass> {
    match (lower, upper) {
        (_, 0) => Some(RateClass::Above),
        (u16::MAX, u16::MAX) => Some(RateClass::Below),
        (0, u16::MAX) => Some(RateClass::Within),
        _ => None,
    }
}

/// The target's drive rate class (0xf60efc): the lens's geometry value scaled by the configured
/// factor against the bounds of the band the lens resource's value falls in.
pub fn target_selection(ports: &mut (impl Properties + ProfileAccess + ImageReport + StockMemory + TargetLens), query: TargetQuery) -> RateClass {
    let frame = query.frame;
    let record = query.record;
    let mut factor = ports.property(stock::RATE_FACTOR);
    trace(ports, frame, 0, 0);
    trace(ports, frame, 1, 0);
    trace(ports, frame, 2, 4);
    if ports.property(stock::FIXED_TARGET) == 1 {
        let class = match ports.property(stock::FIXED_TARGET_CLASS) {
            0 => RateClass::Above,
            1 => RateClass::Within,
            _ => RateClass::Below,
        };
        trace(ports, frame, 3, class.code());
        return class;
    }
    if ports.profile().read::<u8, _>(record, &group_00::AT_0041) == 1 {
        if ports.fourth_report_word() == 1 {
            trace(ports, frame, 3, RateClass::Above.code());
            return RateClass::Above;
        }
        factor = ports.property(stock::REPORTED_RATE_FACTOR);
    }
    let value = ports.resource_value(query.lens_reference);
    let band = (0..BANDS).find(|band| value < ports.property(stock::band_threshold(*band)).wrapping_mul(100)).unwrap_or(BANDS);
    let [first, upper] = stock::band_bounds(band).map(|id| settings::word(ports, id));
    let lower = first.min(upper);
    let rate = ports.geometry_value(query.position, query.lens_reference, query.control, query.lens_control, query.kind);
    let scene_rate = ports.profile().header(record, SharedHeaderByte::At31) == RATE_SCENE || ports.profile().read::<u8, _>(record, &group_00::AT_0048) == RATE_RECORD_KIND;
    if scene_rate {
        factor = ports.property(stock::SCENE_RATE_FACTOR);
    }
    let mapped = ports.mapped_value(query.control, query.lens_reference);
    let scaled = factor.wrapping_mul(mapped).wrapping_mul(rate) / 10_000;
    let class = fixed_class(lower, upper).unwrap_or(if scaled >= u32::from(upper) {
        RateClass::Above
    } else if scaled >= u32::from(lower) {
        RateClass::Within
    } else {
        RateClass::Below
    });
    let adjustment = ports.property(stock::TARGET_ADJUSTMENT) & 0xf;
    let answer = if matches!(adjustment, 1 | 2) && ports.profile().header(record, SharedHeaderByte::At46) != 0 {
        RateClass::Below
    } else if adjustment == 2 && class == RateClass::Below {
        RateClass::Within
    } else {
        class
    };
    trace(ports, frame, 0, rate);
    trace(ports, frame, 1, scaled);
    trace(ports, frame, 2, class.code());
    trace(ports, frame, 3, answer.code());
    answer
}
