#![forbid(unsafe_code)]

//! The gain class a capture request selects and the three gains it takes: a table class, a
//! request's own triple, or a blend of two classes across the adjustment area's temperature span.

use super::ports::Adjustments;
use super::AdjustmentKey;

/// A gain class: 0..=0x3d select the gain tables; some also name a request triple or a blend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GainClass(pub u32);

impl GainClass {
    const BLENDED: Self = Self(0);
    const FIXED_FIRST: Self = Self(11);
    const FIXED_SECOND: Self = Self(12);
    /// Above every preset; resolves to the request's preset.
    const UNRESOLVED: Self = Self(17);
    const FIXED_THIRD: Self = Self(24);
    const CUSTOM: Self = Self(61);
    /// The two classes a blend runs between, cold end first.
    const BLEND_ENDS: [Self; 2] = [Self(18), Self(5)];
}

/// The three gains a class selects.
pub type Gains = [u16; 3];

/// What a gain request carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GainRequest {
    pub mode: u32,
    pub variant: u32,
    /// Selects the custom gains' source when the class is the custom one.
    pub custom_source: u32,
    /// Selects the second bank of sensitivity entries when nonzero.
    pub bank: u32,
    /// The request's triple for classes 11 and 12.
    pub fixed: Gains,
    /// 0 none, 1..=4 a sensitivity preset, 2 also remaps scene classes.
    pub preset: u32,
    pub temperature: u16,
    pub column: i16,
    pub level: i16,
    /// The request's triple for the blended class and class 24.
    pub custom: Gains,
    /// 1 or 2..=3 select the class variant sets.
    pub variant_set: u32,
    pub forced: bool,
    pub variant_subset: u32,
}

/// The gain tables and the class settings outside this layer.
pub trait GainSources {
    /// 0xef3434.
    fn class_gains(&mut self, class: GainClass, column: i16) -> Gains;
    /// 0xef4fc8.
    fn custom_gains(&mut self, source: u32, column: i16) -> Gains;
    /// The class a forced setting byte names (0xef4fd8).
    fn forced_class(&mut self, setting: u8) -> GainClass;
    /// 0x772f28's third value, added to a request's temperature before a blend.
    fn temperature_offset(&mut self) -> u16;
}

pub trait GainPorts: Adjustments + GainSources {}

impl<T: Adjustments + GainSources> GainPorts for T {}

const FORCE_FLAGS: AdjustmentKey = AdjustmentKey(0xa8);
const FORCED_CLASS: AdjustmentKey = AdjustmentKey(0xb0);
/// Bit 2 of the flags forces every class; bit 6 forces a forcing request's class.
const FORCE_ALL: u8 = 1 << 2;
const FORCE_REQUESTED: u8 = 1 << 6;

const fn has_triple(gains: Gains) -> bool { gains[0] != 0 && gains[1] != 0 && gains[2] != 0 }

/// The adjustment entries naming a sensitivity class, normal then alternate, first bank then second.
const SENSITIVITY_KEYS: u32 = 0x793b;

/// The class a sensitivity preset 1..=4 names; 5 for any other preset.
fn sensitivity_class(adjustments: &impl Adjustments, alternate: bool, second_bank: bool, preset: u32) -> u32 {
    if !(1..=4).contains(&preset) {
        return 5;
    }
    let key = SENSITIVITY_KEYS + 8 * u32::from(alternate) + 4 * u32::from(second_bank) + preset - 1;
    u32::from(adjustments.byte(AdjustmentKey(key)))
}

/// The base class a scene class takes under preset 2, by mode.
fn scene_class(base: u32, mode: u32) -> Option<u32> {
    let offset = match mode {
        5..=7 => 0,
        25 => 1,
        18 => 2,
        21 => 3,
        34 => 4,
        37 => 5,
        40 => 6,
        _ => return None,
    };
    let first = match base {
        2 | 20 => 27,
        14 => 34,
        42 => 44,
        52 => 54,
        _ => return None,
    };
    Some(first + offset)
}

/// The class a variant's sensitivity entry gives, remapped through the variant's set.
fn variant_class(sensitivity: u32, variant: u32) -> u32 {
    match sensitivity {
        0 => if variant == 7 { 24 } else { 0 },
        1..=4 => sensitivity,
        5..=9 => match variant {
            0 => match sensitivity { 6..=9 => sensitivity - 5, _ => 0 },
            2..=6 => variant + 4,
            7 => 24,
            8 => 11,
            9 => 12,
            _ => 5,
        },
        _ if variant == 0 => 0,
        10 if variant == 7 => 24,
        11..=14 => sensitivity - 10,
        _ => 0,
    }
}

/// The variant-set class a class 1..=4 takes; others keep theirs.
fn variant_set_class(class: u32, set: u32, subset: u32) -> u32 {
    let column = match class {
        1 => 0,
        2 | 4 => 1,
        3 => 2,
        _ => return class,
    };
    let first = match (set, subset) {
        (2, 1) => 41,
        (2, 2) => 51,
        (2, _) => 13,
        (3, _) => 19,
        _ => return class,
    };
    first + column
}

/// The gain class of a request (0xfe0ccc).
pub fn gain_class(ports: &mut impl GainPorts, request: &GainRequest) -> GainClass {
    const THRESHOLD: AdjustmentKey = AdjustmentKey(0x7902);
    let adjustments = &*ports;
    let threshold = adjustments.signed_byte(THRESHOLD);
    let custom_mode = request.mode == 28;
    let variant = if custom_mode { 7 } else { request.variant };
    let enabled = if custom_mode || request.variant == 7 {
        adjustments.byte(AdjustmentKey(0x844b))
    } else {
        match request.variant_set {
            1 => adjustments.byte(AdjustmentKey(0x7cce)),
            2 | 3 => adjustments.byte(AdjustmentKey(match request.variant_subset {
                1 => 0x8449,
                2 => 0x844a,
                _ => 0x7ccf,
            })),
            _ => 1,
        }
    };
    let preset = if enabled == 0 { 1 } else { request.preset };
    let unpreset = preset == 0;
    let class = match variant {
        8 if !custom_mode => 11,
        9 if !custom_mode => 12,
        10 => 61,
        _ if (11..=13).contains(&request.mode) && unpreset => {
            let flags = adjustments.byte(AdjustmentKey(0x794c));
            let bit = if request.mode == 13 { 2 } else { 1 };
            if flags & bit == 0 { 16 } else { 0 }
        }
        _ if request.mode == 24 && unpreset => {
            if adjustments.byte(AdjustmentKey(0x794d)) & 1 == 0 { 22 } else { 0 }
        }
        _ => {
            let low_level = i32::from(request.level) < 2 * i32::from(((i32::from(threshold) * 5) << 17) >> 17);
            if request.mode == 47 && low_level && unpreset {
                match variant {
                    2..=6 => variant + 4,
                    7 => 24,
                    8 => 11,
                    9 => 12,
                    _ => 5,
                }
            } else {
                let alternate = variant == 7 || request.mode == 28;
                let sensitivity = sensitivity_class(adjustments, alternate, request.bank != 0, preset);
                let class = variant_class(sensitivity, variant);
                if matches!(request.variant_set, 2 | 3) {
                    variant_set_class(class, request.variant_set, request.variant_subset)
                } else {
                    class
                }
            }
        }
    };
    let class = if preset == 2 { scene_class(class, request.mode).unwrap_or(class) } else { class };
    let class = if request.mode == 25 {
        let flags = adjustments.byte(AdjustmentKey(0x794e));
        if preset == 0 {
            if flags & 1 == 0 { 23 } else { 0 }
        } else if flags & 2 != 0 {
            17
        } else if preset != 2 {
            23
        } else {
            match (request.variant_set, request.variant_subset) {
                (2 | 3, 1) => 45,
                (2 | 3, 2) => 55,
                (2 | 3, _) => 35,
                _ => 28,
            }
        }
    } else if variant == 7 && preset != 0 && adjustments.byte(AdjustmentKey(0x794f)) & 1 == 0 {
        0
    } else {
        class
    };
    let class = if ports.byte(FORCE_FLAGS) & FORCE_ALL != 0 {
        let setting = ports.byte(FORCED_CLASS);
        ports.forced_class(setting).0
    } else {
        class
    };
    GainClass(if class == 0 && preset != 0 { GainClass::UNRESOLVED.0 } else { class })
}

/// The gains of a request and the class they come from (0xfe170c).
pub fn gains(ports: &mut impl GainPorts, request: &GainRequest) -> (Gains, GainClass) {
    let class = gain_class(ports, request);
    let class = if class == GainClass::UNRESOLVED && (1..=4).contains(&request.preset) { GainClass(request.preset) } else { class };
    let class = if ports.byte(FORCE_FLAGS) & FORCE_REQUESTED != 0 && request.forced {
        let setting = ports.byte(FORCED_CLASS);
        ports.forced_class(setting)
    } else {
        class
    };
    let table = |ports: &mut _, class| GainSources::class_gains(ports, class, request.column);
    let gains = match class {
        GainClass::BLENDED if has_triple(request.custom) => request.custom,
        GainClass::BLENDED => blend(ports, request),
        GainClass::FIXED_FIRST | GainClass::FIXED_SECOND if has_triple(request.fixed) => request.fixed,
        GainClass::FIXED_FIRST | GainClass::FIXED_SECOND => table(ports, GainClass::BLEND_ENDS[1]),
        GainClass::FIXED_THIRD if has_triple(request.custom) => request.custom,
        GainClass::FIXED_THIRD => table(ports, GainClass::BLEND_ENDS[1]),
        GainClass::CUSTOM => ports.custom_gains(request.custom_source, request.column),
        _ => table(ports, class),
    };
    (gains, class)
}

/// The blend of the two end classes at the request's temperature across the adjustment span.
fn blend(ports: &mut impl GainPorts, request: &GainRequest) -> Gains {
    let temperature = request.temperature.wrapping_add(ports.temperature_offset()) as i16;
    let [cold, warm] = GainClass::BLEND_ENDS;
    let warm = ports.class_gains(warm, request.column);
    let cold = ports.class_gains(cold, request.column);
    let adjustments = &*ports;
    let low = (u16::from(adjustments.byte(AdjustmentKey(0x791e))) * 10) as i16;
    let high = (u16::from(adjustments.byte(AdjustmentKey(0x791f))) * 10) as i16;
    if temperature <= low {
        return cold;
    }
    if temperature >= high {
        return warm;
    }
    let span = i32::from(high) - i32::from(low);
    let offset = i32::from(temperature) - i32::from(low);
    core::array::from_fn(|channel| {
        let delta = i32::from(warm[channel]) - i32::from(cold[channel]);
        (i32::from(cold[channel]) + delta * offset / span) as u16
    })
}

/// The first adjustment entries of the three blend weights each class names.
const BLEND_KEYS: [u16; 42] = [
    0x81db, 0x81e4, 0x81ea, 0x821a, 0x821d, 0x8223, 0x8226, 0x8229, 0x822c, 0x822f, 0x8232, 0x8235, 0x8238, 0x81e7,
    0x81e1, 0x8220, 0x81f0, 0x81f3, 0x81f6, 0x81f9, 0x81ff, 0x8202, 0x8205, 0x8208, 0x820e, 0x8211, 0x8214, 0x8217,
    0x81de, 0x81ed, 0x81fc, 0x820b, 0x8241, 0x8244, 0x8253, 0x8247, 0x824a, 0x824d, 0x8256, 0x8250, 0x823b, 0x823e,
];
/// The first of the fifteen blend outputs, three entries apart.
const BLEND_OUTPUTS: u32 = 0x8265;
/// What a blend reads as when its class or weight is out of range.
const NO_BLEND: u8 = 10;

/// The blend class of a request's mode and variant (0xfe0020).
fn blend_class(mode: u32, variant: u32, sensitivity: u32, preset: u32) -> u32 {
    let banded = |bands: [u32; 5]| match sensitivity {
        0 => bands[0],
        1 => bands[1],
        3 => bands[3],
        4 => bands[4],
        _ => bands[2],
    };
    let class = match mode {
        0 if preset != 0 => 0,
        0 | 0x2b | 0x2c | 0x39 => banded([0x1c, 0xe, 1, 0xd, 2]),
        1..=4 => 3,
        10 => 4,
        11 => 0xf,
        16 => 5,
        17 => 6,
        18 => 9,
        21 => 8,
        25 => 7,
        28 => 10,
        30 => 11,
        32 => 12,
        0x2d => banded([0x1e, 0x14, 0x15, 0x16, 0x17]),
        0x2e => banded([0x1d, 0x10, 0x11, 0x12, 0x13]),
        0x2f => banded([0x1f, 0x18, 0x19, 0x1a, 0x1b]),
        0x33 => 0x28,
        0x34 => 0x29,
        _ => 0x2a,
    };
    match variant {
        1..=6 => 0x1f + variant,
        7..=12 => 0x27,
        14 => 0x26,
        _ => class,
    }
}

/// The blend weight a request's class gives in `column` 0..=2 (0xfe044c).
pub fn blend_weight(adjustments: &impl Adjustments, mode: u32, variant: u32, column: u32, sensitivity: u32, preset: u32) -> u8 {
    let Some(&first) = BLEND_KEYS.get(blend_class(mode, variant, sensitivity, preset) as usize) else {
        return NO_BLEND;
    };
    let output = adjustments.byte(AdjustmentKey(u32::from(first) + column));
    match output {
        1..=15 => adjustments.byte(AdjustmentKey(BLEND_OUTPUTS + 3 * u32::from(output - 1))),
        _ => NO_BLEND,
    }
}
