use std::vec::Vec;

use xm1_saclac_interface::stock::record_handle_from_a2;
use xm1_saclac_interface::RecordHandle;

use crate::adjust::focus_profile::*;

#[derive(Clone, Copy, Default)]
struct Record {
    armed: bool,
    selection: u8,
    scene_kind: u8,
    follow: u16,
    level: i16,
    priority: u8,
    area_kind: u8,
    auxiliary: u8,
}

struct Fields {
    record: Record,
    shared_scene: u32,
    shared_follow: u16,
}

impl Fields {
    fn new(record: Record) -> Self { Self { record, shared_scene: 0x55, shared_follow: 0x77 } }
}

impl RecordFields for Fields {
    fn selection_callback_armed(&self, _: RecordHandle) -> bool { self.record.armed }
    fn measurement_selection(&self, _: RecordHandle) -> MeasurementSelection { MeasurementSelection(self.record.selection) }
    fn scene_kind(&self, _: RecordHandle) -> u8 { self.record.scene_kind }
    fn shared_scene(&self) -> u32 { self.shared_scene }
    fn follow_word(&self, _: RecordHandle) -> u16 { self.record.follow }
    fn shared_follow_word(&self) -> u16 { self.shared_follow }
    fn level(&self, _: RecordHandle) -> i16 { self.record.level }
    fn priority(&self, _: RecordHandle) -> u8 { self.record.priority }
    fn area_kind(&self, _: RecordHandle) -> u8 { self.record.area_kind }
    fn area_auxiliary(&self, _: RecordHandle) -> u8 { self.record.auxiliary }
}

const RECORD: RecordHandle = record_handle_from_a2(0x0102_0304);

#[test]
fn an_armed_record_takes_its_scene_from_the_shared_block() {
    let own = Record { scene_kind: 9, ..Record::default() };
    let armed = Record { armed: true, ..own };
    let armed_selected = Record { selection: 1, ..armed };
    assert_eq!(area_scene(&Fields::new(own), RECORD), 9);
    assert_eq!(area_scene(&Fields::new(armed), RECORD), 0);
    assert_eq!(area_scene(&Fields::new(armed_selected), RECORD), 0x55);
    assert_eq!(scene(&Fields::new(own), RECORD), 9);
    assert_eq!(scene(&Fields::new(armed), RECORD), 0x55);
    assert_eq!(adjustment_word(&Fields::new(own), RECORD), 9);
    assert_eq!(adjustment_word(&Fields::new(armed), RECORD), 0);
    assert_eq!(follow_word(&Fields::new(Record { follow: 3, ..own }), RECORD), 3);
    assert_eq!(follow_word(&Fields::new(Record { follow: 3, ..armed }), RECORD), 0x77);
}

#[test]
fn the_default_level_stands_in_for_a_selected_or_unset_record() {
    let level_of = |record| level(&Fields::new(record), RECORD, -40);
    assert_eq!(level_of(Record { level: 12, ..Record::default() }), 12);
    assert_eq!(level_of(Record { level: 0, ..Record::default() }), -40);
    assert_eq!(level_of(Record { level: 12, selection: 1, ..Record::default() }), -40);
    assert_eq!(level_of(Record { level: 12, selection: 2, ..Record::default() }), 12);
}

#[test]
fn priority_is_one_unless_a_selected_record_sets_its_own() {
    let priority_of = |record| priority(&Fields::new(record), RECORD);
    assert_eq!(priority_of(Record { priority: 5, selection: 0, ..Record::default() }), 1);
    assert_eq!(priority_of(Record { priority: 5, selection: 2, ..Record::default() }), 5);
    assert_eq!(priority_of(Record { priority: 0, selection: 1, ..Record::default() }), 1);
}

#[test]
fn context_category_follows_activity_then_aspect() {
    assert_eq!(context_category(true, 0), 4);
    assert_eq!(context_category(false, 0), 2);
    assert_eq!(context_category(false, 1), 3);
}

struct Lens(Option<u32>);

impl LensResources for Lens {
    fn resource(&self) -> Option<u32> { self.0 }
    fn reference(&self) -> u32 { 0xaa }
    fn flags(&self) -> u32 { 0xf0 }
    fn channel_bias(&self, resource: u32) -> i32 { resource as i32 - 10 }
}

struct Settings;

impl AreaSettings for Settings {
    fn resource_count(&self) -> u8 { 4 }
    fn area_offset(&self, table: OffsetTable, resource: u32) -> u16 {
        let base = match table { OffsetTable::Standard => 1000, OffsetTable::First => 2000, OffsetTable::Second => 3000 };
        base + resource as u16
    }
}

#[derive(Default)]
struct Programmed {
    plain: Vec<AreaSetup>,
    referenced: Vec<ReferencedArea>,
}

impl AreaConfiguration for Programmed {
    fn configure(&mut self, area: &AreaSetup) { self.plain.push(*area); }
    fn configure_referenced(&mut self, area: &ReferencedArea) { self.referenced.push(*area); }
}

const REQUEST: AreaRequest = AreaRequest { primary: [1, 2], referenced: [3, 4], forced: false, last: 9 };

fn program(record: Record, lens: Lens, request: AreaRequest) -> (Option<u16>, Programmed) {
    let mut programmed = Programmed::default();
    let offset = program_area_pair(&Fields::new(record), &lens, &Settings, &mut programmed, RECORD, request);
    (offset, programmed)
}

#[test]
fn an_area_pair_carries_the_records_kind_and_returns_its_offset() {
    let record = Record { scene_kind: 7, area_kind: 2, auxiliary: 5, ..Record::default() };
    let (offset, programmed) = program(record, Lens(Some(3)), REQUEST);
    let expected = AreaSetup { resource: 3, scene: 7, kind: 2, class: 6, values: [1, 2], flags: 0xf0, auxiliary: 5, last: 9 };
    assert_eq!(programmed.plain, [expected]);
    assert_eq!(programmed.referenced, [ReferencedArea { area: AreaSetup { values: [3, 4], ..expected }, reference: 0xaa }]);
    assert_eq!(offset, Some(3000 + 3 - 7));
}

#[test]
fn a_resource_beyond_the_count_narrows_kind_two_to_kind_one() {
    let record = Record { area_kind: 2, ..Record::default() };
    let (offset, programmed) = program(record, Lens(Some(4)), REQUEST);
    assert_eq!(programmed.plain[0].kind, 1);
    assert_eq!(offset, Some(2000 + 4 - 6));
}

#[test]
fn a_forced_or_armed_pair_drops_the_kind_and_an_armed_one_reads_the_standard_offsets() {
    let record = Record { area_kind: 1, ..Record::default() };
    let (forced, programmed) = program(record, Lens(Some(2)), AreaRequest { forced: true, ..REQUEST });
    assert_eq!((programmed.plain[0].kind, programmed.plain[0].class), (0, 0));
    assert_eq!(forced, Some(2000 + 2 - 8));
    let (armed, programmed) = program(Record { armed: true, ..record }, Lens(Some(2)), REQUEST);
    assert_eq!((programmed.referenced[0].area.kind, programmed.referenced[0].area.class), (0, 0));
    assert_eq!(armed, Some(1000 + 2 - 8));
}

#[test]
fn nothing_is_programmed_without_a_lens_resource() {
    let (offset, programmed) = program(Record::default(), Lens(None), REQUEST);
    assert_eq!(offset, None);
    assert!(programmed.plain.is_empty() && programmed.referenced.is_empty());
}

#[test]
fn the_round_area_uses_the_records_scene_kind_with_no_class() {
    let mut programmed = Programmed::default();
    let record = Record { armed: true, scene_kind: 4, area_kind: 2, auxiliary: 1, ..Record::default() };
    program_round_area(&Fields::new(record), &Lens(None), &Settings, &mut programmed, RECORD, 6, [7, 8]);
    let expected = AreaSetup { resource: 6, scene: 4, kind: 1, class: 0, values: [7, 8], flags: 0xf0, auxiliary: 1, last: 0 };
    assert_eq!(programmed.plain, [expected]);
}
