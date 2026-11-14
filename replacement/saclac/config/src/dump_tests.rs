use crate::channel::Channel;
use crate::dump::*;
use crate::fake::Area;
use crate::ports::{Derivations, ExportQuery};
use crate::table::{Row, Step};

/// Records which derivations the dump enters.
#[derive(Default)]
struct Scales(Vec<&'static str>);

impl Derivations for Scales {
    fn export_word(&mut self, _: Channel, _: ExportQuery) -> i16 {
        self.0.push("export word");
        0
    }
    fn adjust_direction(&mut self, word: i16, _: u32, _: u32) -> i16 {
        self.0.push("adjust direction");
        word
    }
    fn scaled_limit(&mut self, _: Channel, _: u32, _: u32) -> u16 {
        self.0.push("scaled limit");
        0
    }
    fn channel_scale(&mut self, _: Channel) -> u16 {
        self.0.push("channel scale");
        77
    }
    fn table_scale(&mut self, step: u32, _: Channel) -> u16 {
        self.0.push("table scale");
        500 + step as u16
    }
    fn curve_position(&mut self, _: i16, _: Channel) -> u32 {
        self.0.push("curve position");
        0
    }
    fn curve_value(&mut self, _: u32, _: Channel) -> i16 {
        self.0.push("curve value");
        0
    }
    fn channel_word(&mut self, _: Channel, _: u32, _: u32) -> i16 {
        self.0.push("channel word");
        0
    }
    fn reference_word(&mut self, _: u32, _: Channel) -> i16 {
        self.0.push("reference word");
        0
    }
    fn apply_reference(&mut self, _: i16, _: Channel) { self.0.push("apply reference"); }
    fn lens_table(&mut self, _: u32, _: u32, _: u32) -> u32 {
        self.0.push("lens table");
        0
    }
    fn lens_limits(&mut self, _: u32, _: [u32; 11]) -> u32 {
        self.0.push("lens limits");
        0
    }
}

#[derive(Default)]
struct Sources {
    selections: Vec<(u32, i16)>,
}

impl DumpDerivations for Sources {
    fn transformed_value(&mut self, step: Step, _: Row) -> u16 { 100 + step.0 as u16 }
    fn transformed_word(&mut self, step: Step, _: Row) -> u16 { 200 + step.0 as u16 }
    fn lens_position_flags(&mut self) -> u32 { 0x40 }
    fn selection_word(&mut self, _: Channel, mode: u32, flags: u32, word: i16) -> u32 {
        assert_eq!(flags, 0x40);
        self.selections.push((mode, word));
        self.selections.len() as u32
    }
}

const BANK_SIZE: u32 = 0x3e0f82;

/// Channel 1's base word is -2, its mode-1 word 7, the words after it 10, 20, .., 60 and its
/// reference words 0x7fff.
fn area() -> Area {
    let area = Area::default().with(BANK_SIZE, 3).with_half(0x3e0317 + 2, 0xfffe).with_half(0x3e03e0 + 2, 7);
    let added = [0x3e04a9, 0x3e0572, 0x3e063b, 0x3e0704, 0x3e07f7, 0x3e08c0];
    let area = (1..).zip(added).fold(area, |area, (index, at)| area.with_half(at + 2, 10 * index));
    [0x3e07cd, 0x3e07e2, 0x3e0989].into_iter().fold(area, |area, at| area.with_half(at + 2, 0x7fff))
}

#[test]
fn the_step_table_holds_the_banks_steps() {
    let (mut scales, mut sources) = (Scales::default(), Sources::default());
    let dump = configuration_dump(&area(), &mut scales, &mut sources, Channel(1));
    assert_eq!(dump.count, 3);
    assert_eq!(dump.steps[2], StepDump { rounded: 0, value: 0, transformed_value: 102, transformed_word: 202, scale: 502 });
    assert_eq!(dump.steps[3], StepDump::default());
    assert_eq!(dump.channel_scale, 77);
    assert_eq!(scales.0, ["table scale", "table scale", "table scale", "channel scale"]);
}

#[test]
fn selection_words_adjust_the_base_and_its_sums_with_wrapping_halfwords() {
    let mut sources = Sources::default();
    let dump = configuration_dump(&area(), &mut Scales::default(), &mut sources, Channel(1));
    assert_eq!(dump.words[3], 0xffff_fffe, "signed words read sign-extended");
    assert_eq!(
        sources.selections,
        [(2, -2), (1, 7), (2, 8), (2, 18), (2, 28), (2, 38), (2, 48), (2, 58), (2, 0x7ffd), (2, 0x7ffd), (2, 0x7ffd)]
    );
    assert_eq!(dump.selections, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(dump.references, Some([0x7fff; 3]));
    assert_eq!(dump.reference_selections, Some([9, 10, 11]));
}

#[test]
fn channels_above_9_have_no_reference_words_and_request_no_reference_selections() {
    let mut sources = Sources::default();
    let dump = configuration_dump(&area(), &mut Scales::default(), &mut sources, Channel(10));
    assert_eq!(dump.references, None);
    assert_eq!(dump.reference_selections, None);
    assert_eq!(sources.selections.len(), 8);
}

#[test]
fn unset_optional_limits_read_0xffff_and_the_others_0() {
    let dump = configuration_dump(&Area::default(), &mut Scales::default(), &mut Sources::default(), Channel(1));
    assert_eq!(dump.limits, [[0xffff; 10], [0xffff; 10], [0xffff; 10], [0; 10], [0; 10]]);
}

#[test]
fn the_dump_keeps_signed_record_values_and_both_shifted_floors() {
    let area = area().with_half(0x3f3d14 + 22, 0xfffe)
        .with_half(0x3e15a6, 9).with_half(0x3e15a8, 13).with(0x3e162f, 3)
        .with_half(0x3e0b40 + 2, 0x101).with_half(0x3e0c09 + 2, 0x203)
        .with_half(0x3e0cd2 + 2, 0x405).with_half(0x3e0d9b + 2, 0x807);
    let dump = configuration_dump(&area, &mut Scales::default(), &mut Sources::default(), Channel(1));
    assert_eq!(dump.record_word, -2);
    assert_eq!(dump.floors, [72, 104]);
    assert_eq!(dump.late_words, [0x101, 0x203, 0x405, 0x807]);
}
