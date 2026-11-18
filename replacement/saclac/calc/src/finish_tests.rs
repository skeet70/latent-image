use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::{Bytes, SharedHeaderByte};

use crate::exposure::fakes::{handle, Headers, World};
use crate::exposure::ports::{ImageReport, ProfileAccess, Properties, Property, StockMemory};
use crate::finish::{target_selection, RateClass, TargetLens, TargetQuery};
use crate::profile::Profile;
use crate::stock::finish as stock;

const FRAME: SharedAddress = SharedAddress(0x4000_0000);
const LENS_REFERENCE: u32 = 6;
const CONTROL: u32 = 2;

struct Bench {
    world: World,
    resource_value: u32,
    geometry_value: u32,
    mapped_value: u32,
}

impl Properties for Bench {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl ProfileAccess for Bench {
    type Records = Bytes;
    type Entries = Headers;
    fn profile(&mut self) -> Profile<'_, Bytes, Headers> { self.world.profile() }
}

impl ImageReport for Bench {
    fn second_report_word(&mut self) -> u32 { self.world.second_report_word() }
    fn fourth_report_word(&mut self) -> u32 { self.world.fourth_report_word() }
}

impl StockMemory for Bench {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { self.world.read_u8(at) }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.world.read_u16(at) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.world.read_u32(at) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { self.world.write_u8(at, value) }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { self.world.write_u16(at, value) }
    fn write_u32(&mut self, at: SharedAddress, value: u32) { self.world.write_u32(at, value) }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) { self.world.fill(at, bytes, value) }
}

/// Answers the lens queries of this file's target only.
impl TargetLens for Bench {
    fn resource_value(&mut self, resource: u32) -> u32 { if resource == LENS_REFERENCE { self.resource_value } else { 0 } }
    fn geometry_value(&mut self, _position: i16, reference: u32, control: u32, _lens_control: u32, _kind: u32) -> u32 {
        if (reference, control) == (LENS_REFERENCE, CONTROL) { self.geometry_value } else { 0 }
    }
    fn mapped_value(&mut self, value: u32, bank: u32) -> u32 { if (value, bank) == (CONTROL, LENS_REFERENCE) { self.mapped_value } else { 0 } }
}

/// Bands at 200, 300, 400 and 500 in resource value; the second band spans 100 to 500 and the
/// lens is in it; a factor of 10 and a mapped value of 1 make the scaled rate a thousandth of the
/// geometry value.
fn bench(geometry_value: u32) -> Bench {
    let mut world = World::default();
    for (band, threshold) in [2, 3, 4, 5].into_iter().enumerate() {
        world.property(stock::band_threshold(band), threshold);
    }
    let [first, second] = stock::band_bounds(1);
    world.property_word(first, 100);
    world.property_word(second, 500);
    world.property(stock::RATE_FACTOR, 10);
    Bench { world, resource_value: 250, geometry_value, mapped_value: 1 }
}

fn query() -> TargetQuery { TargetQuery { record: handle(), lens_reference: LENS_REFERENCE, position: 0, control: CONTROL, kind: 0, lens_control: 0, frame: Some(FRAME) } }

fn traced(bench: &Bench) -> [u32; 4] { core::array::from_fn(|word| bench.world.u32_at(stock::target_trace(FRAME, word))) }

#[test]
fn the_scaled_rate_is_classed_against_the_band_the_lens_resource_falls_in() {
    for (geometry, class) in [(99_000, RateClass::Below), (100_000, RateClass::Within), (499_000, RateClass::Within), (500_000, RateClass::Above)] {
        let mut bench = bench(geometry);
        assert_eq!(target_selection(&mut bench, query()), class, "geometry value {geometry}");
        assert_eq!(traced(&bench), [geometry, geometry / 1000, class.code(), class.code()]);
    }
}

#[test]
fn bounds_of_zero_or_open_ends_fix_the_class() {
    let [first, second] = stock::band_bounds(1);
    for (lower, upper, class) in [(100, 0, RateClass::Above), (0xffff, 0xffff, RateClass::Below), (0, 0xffff, RateClass::Within)] {
        let mut bench = bench(1_000_000);
        bench.world.property_word(first, lower);
        bench.world.property_word(second, upper);
        assert_eq!(target_selection(&mut bench, query()), class, "bounds {lower} {upper}");
    }
}

#[test]
fn a_fixed_target_takes_its_configured_class_and_traces_only_the_answer() {
    for (configured, class) in [(0, RateClass::Above), (1, RateClass::Within), (2, RateClass::Below)] {
        let mut bench = bench(1_000_000);
        bench.world.property(stock::FIXED_TARGET, 1);
        bench.world.property(stock::FIXED_TARGET_CLASS, configured);
        assert_eq!(target_selection(&mut bench, query()), class);
        assert_eq!(traced(&bench), [0, 0, 4, class.code()]);
    }
}

#[test]
fn a_reported_record_is_above_when_image_four_reports_one_and_otherwise_takes_its_factor() {
    let mut bench = bench(1_000);
    bench.world.set(&group_00::AT_0041, 1);
    bench.world.fourth_report_word = 1;
    assert_eq!(target_selection(&mut bench, query()), RateClass::Above);
    assert_eq!(traced(&bench), [0, 0, 4, 2]);
    bench.world.fourth_report_word = 0;
    bench.geometry_value = 10_000;
    assert_eq!(target_selection(&mut bench, query()), RateClass::Below);
    bench.world.property(stock::REPORTED_RATE_FACTOR, 200);
    assert_eq!(target_selection(&mut bench, query()), RateClass::Within);
}

#[test]
fn scene_twenty_eight_and_record_kind_seven_take_the_scene_factor() {
    let mut scene = bench(10_000);
    scene.world.property(stock::SCENE_RATE_FACTOR, 200);
    scene.world.header(SharedHeaderByte::At31, 28);
    assert_eq!(target_selection(&mut scene, query()), RateClass::Within);
    let mut kind = bench(10_000);
    kind.world.property(stock::SCENE_RATE_FACTOR, 200);
    kind.world.set(&group_00::AT_0048, 7);
    assert_eq!(target_selection(&mut kind, query()), RateClass::Within);
}

#[test]
fn the_adjustment_nibble_lifts_a_low_class_or_holds_an_auxiliary_record_below() {
    let mut lifted = bench(1_000);
    lifted.world.property(stock::TARGET_ADJUSTMENT, 0x12);
    assert_eq!(target_selection(&mut lifted, query()), RateClass::Within);
    assert_eq!(traced(&lifted)[2..], [0, 1]);
    let mut held = bench(1_000_000);
    held.world.property(stock::TARGET_ADJUSTMENT, 1);
    held.world.header(SharedHeaderByte::At46, 3);
    assert_eq!(target_selection(&mut held, query()), RateClass::Below);
}

#[test]
fn without_a_frame_nothing_is_traced() {
    let mut bench = bench(1_000);
    target_selection(&mut bench, TargetQuery { frame: None, ..query() });
    assert!(bench.world.stores.is_empty());
}
