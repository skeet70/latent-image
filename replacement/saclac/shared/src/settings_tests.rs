use std::collections::HashMap;

use xm1_saclac_interface::stock::record_handle_from_a2;
use xm1_saclac_interface::RecordHandle;

use crate::settings::*;

struct Area(HashMap<u32, u8>);

impl Adjustments for Area {
    fn byte(&self, key: AdjustmentKey) -> u8 { self.0.get(&key.0).copied().unwrap_or(0) }
}

fn area(bytes: &[(u32, u8)]) -> Area { Area(bytes.iter().copied().collect()) }

#[test]
fn adjustment_bytes_read_signed_and_in_little_endian_pairs() {
    let area = area(&[(0x10, 0xff), (0x11, 0x7f), (0x3e_0243, 0x34), (0x3e_0244, 0x12)]);
    assert_eq!(signed_byte(&area, AdjustmentKey(0x10)), -1);
    assert_eq!(signed_byte(&area, AdjustmentKey(0x11)), 127);
    assert_eq!(halfword(&area, AdjustmentKey(0x10)), 0x7fff);
    assert_eq!(halfword(&area, LENS_ADJUSTMENT), 0x1234);
}

struct Headers { selection: u8, at47: u8 }

impl ProfileHeaders for Headers {
    fn header(&mut self, _: RecordHandle, byte: HeaderByte) -> u8 {
        match byte {
            HeaderByte::MeasurementSelection => self.selection,
            HeaderByte::At47 => self.at47,
        }
    }
}

#[test]
fn an_unselected_mode_needs_the_adjustment_no_selection_and_modes_four_to_six() {
    let record = record_handle_from_a2(0x0102_0304);
    let admitted = area(&[(UNSELECTED_MODES.0, 1)]);
    for mode in 0..=10 {
        let expected = (4..=6).contains(&mode);
        assert_eq!(unselected_mode(record, &admitted, &mut Headers { selection: 0, at47: mode }), expected, "{mode}");
    }
    assert!(!unselected_mode(record, &admitted, &mut Headers { selection: 1, at47: 5 }));
    assert!(!unselected_mode(record, &area(&[]), &mut Headers { selection: 0, at47: 5 }));
}

#[test]
fn tenths_classes_follow_the_threshold_table() {
    for (value, class) in [
        (0, 6),
        (-5, 6),
        (319, 6),
        (320, 7),
        (359, 7),
        (360, 8),
        (600, 15),
        (1289, 35),
        (1290, 36),
        (2559, 36),
        (2560, 6),
        (-320, 36),
    ] {
        assert_eq!(tenths_class(value), class, "{value}");
    }
}

#[test]
fn unrecognized_pairs_share_the_three_four_class() {
    for (pair, expected) in [
        ([3, 4], Aspect::ThreeFour),
        ([2, 3], Aspect::TwoThree),
        ([9, 16], Aspect::NineSixteen),
        ([1, 1], Aspect::Square),
        ([4, 3], Aspect::ThreeFour),
        ([16, 9], Aspect::ThreeFour),
        ([0, 0], Aspect::ThreeFour),
    ] {
        assert_eq!(aspect(AspectPair(pair)), expected, "{pair:?}");
    }
}

#[test]
fn capture_modes_run_from_one_to_ten() {
    assert_eq!(CaptureMode::new(0), None);
    assert_eq!(CaptureMode::new(1).map(CaptureMode::get), Some(1));
    assert_eq!(CaptureMode::new(10).map(CaptureMode::get), Some(10));
    assert_eq!(CaptureMode::new(11), None);
}

#[derive(Default)]
struct Conversion {
    unmapped: bool,
    calls: Vec<&'static str>,
}

impl Conversions for Conversion {
    fn window_variant(&mut self, mode: CaptureMode, aspect: Aspect) -> Option<WindowVariant> {
        self.calls.push("variant");
        (!self.unmapped).then_some(WindowVariant(mode.get() * 10 + aspect as u8))
    }

    fn value(&mut self, variant: WindowVariant, _: Aspect) -> u32 {
        self.calls.push("value");
        1000 + u32::from(variant.0)
    }

    fn alternate(&mut self, variant: WindowVariant, _: Aspect) -> u32 {
        self.calls.push("alternate");
        2000 + u32::from(variant.0)
    }
}

fn window(mode: u32, pair: [u16; 2], conversion: &mut Conversion) -> Result<WindowScale, NoVariant> {
    capture_window(CaptureMode::new(mode).unwrap(), AspectPair(pair), conversion)
}

#[test]
fn scaled_modes_read_the_variant_then_its_value_and_alternate() {
    let mut conversion = Conversion::default();
    let scaled = window(2, [2, 3], &mut conversion);
    assert_eq!(scaled, Ok(WindowScale::Scaled(CaptureWindow { alternate: 2021, value: 1021 })));
    assert_eq!(conversion.calls, ["variant", "value", "alternate"]);
}

#[test]
fn each_mode_scales_only_its_aspects() {
    let scaled_aspects = |mode| {
        [[3, 4], [2, 3], [9, 16], [1, 1]].map(|pair| matches!(window(mode, pair, &mut Conversion::default()), Ok(WindowScale::Scaled(_))))
    };
    for mode in 1..=3 {
        assert_eq!(scaled_aspects(mode), [false, true, true, true], "{mode}");
    }
    for mode in 4..=5 {
        assert_eq!(scaled_aspects(mode), [true, false, false, false], "{mode}");
    }
    for mode in 6..=7 {
        assert_eq!(scaled_aspects(mode), [false, false, true, false], "{mode}");
    }
    for mode in 8..=10 {
        assert_eq!(scaled_aspects(mode), [false; 4], "{mode}");
    }
}

#[test]
fn an_unrecognized_pair_is_never_scaled() {
    let mut conversion = Conversion::default();
    assert_eq!(window(4, [4, 3], &mut conversion), Ok(WindowScale::Unscaled));
    assert!(conversion.calls.is_empty());
}

#[test]
fn a_scaled_aspect_without_a_variant_is_refused() {
    let mut conversion = Conversion { unmapped: true, ..Conversion::default() };
    assert_eq!(window(6, [9, 16], &mut conversion), Err(NoVariant));
    assert_eq!(conversion.calls, ["variant"]);
}
