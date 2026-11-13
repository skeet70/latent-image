use std::vec::Vec;

use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_27;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::focus_profile::*;
use crate::exposure::fakes::{handle, World};
use crate::stock::property::{self, OffsetTable};

fn record(armed: bool, selection: u8, scene_kind: u8) -> World {
    let mut world = World::default();
    world.header(SharedHeaderByte::At63, u8::from(armed));
    world.select(match selection {
        0 => MeasurementSelection::Unselected,
        1 => MeasurementSelection::Record,
        other => MeasurementSelection::Other(other),
    });
    world.header(SharedHeaderByte::At31, scene_kind);
    world.set_shared(&group_27::AT_073C, 0x55);
    world.set_shared(&group_27::AT_0740, 0x77);
    world.set(&group_00::AT_002E, 3);
    world
}

#[test]
fn an_armed_record_takes_its_scene_from_the_shared_block() {
    let record = handle();
    assert_eq!(area_scene(&mut record_of(false, 0), record), 9);
    assert_eq!(area_scene(&mut record_of(true, 0), record), 0);
    assert_eq!(area_scene(&mut record_of(true, 1), record), 0x55);
    assert_eq!(scene(&mut record_of(false, 0), record), 9);
    assert_eq!(scene(&mut record_of(true, 0), record), 0x55);
    assert_eq!(adjustment_word(&mut record_of(false, 0), record), 9);
    assert_eq!(adjustment_word(&mut record_of(true, 0), record), 0);
    assert_eq!(follow_word(&mut record_of(false, 0), record), 3);
    assert_eq!(follow_word(&mut record_of(true, 0), record), 0x77);
}

fn record_of(armed: bool, selection: u8) -> World { record(armed, selection, 9) }

#[test]
fn the_shared_level_step_stands_in_for_a_selected_or_unset_record() {
    let level_of = |selection: u8, own: i16| {
        let mut world = record(false, selection, 0);
        world.set(&group_00::AT_0022, own);
        world.property_word(property::level_step(RECORD_LEVEL_STEP), (-40i16) as u16);
        level(&mut world, handle())
    };
    assert_eq!(level_of(0, 12), 12);
    assert_eq!(level_of(0, 0), -40);
    assert_eq!(level_of(1, 12), -40);
    assert_eq!(level_of(2, 12), 12);
}

#[test]
fn priority_is_one_unless_a_selected_record_sets_its_own() {
    let priority_of = |selection: u8, own: u8| {
        let mut world = record(false, selection, 0);
        world.set(&group_00::AT_003D, own);
        priority(&mut world, handle())
    };
    assert_eq!(priority_of(0, 5), 1);
    assert_eq!(priority_of(2, 5), 5);
    assert_eq!(priority_of(1, 0), 1);
}

#[test]
fn context_category_follows_activity_then_aspect() {
    assert_eq!(context_category(true, 0), 4);
    assert_eq!(context_category(false, 0), 2);
    assert_eq!(context_category(false, 1), 3);
}

struct Lens(Option<u32>);

impl LensResources for Lens {
    fn resource(&mut self) -> Option<u32> { self.0 }
    fn reference(&mut self) -> u32 { 0xaa }
    fn flags(&mut self) -> u32 { 0xf0 }
    fn channel_bias(&mut self, resource: u32) -> i32 { resource as i32 - 10 }
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

/// A record asking for area kind `kind`, four resources configured, and offset tables whose
/// entries are 1000, 2000 or 3000 plus the resource.
fn area_world(armed: bool, kind: u8) -> World {
    let mut world = record(armed, 0, 7);
    world.header(SharedHeaderByte::At46, kind);
    world.header(SharedHeaderByte::At61, 5);
    world.property(property::AREA_RESOURCES, 4);
    for resource in 0..8 {
        for (table, base) in [(OffsetTable::Standard, 1000), (OffsetTable::First, 2000), (OffsetTable::Second, 3000)] {
            world.property_word(property::area_offset(table, resource), base + resource as u16);
        }
    }
    world
}

fn program(mut world: World, resource: Option<u32>, request: AreaRequest) -> (Option<u16>, Programmed) {
    let mut programmed = Programmed::default();
    let offset = program_area_pair(&mut world, &mut Lens(resource), &mut programmed, handle(), request);
    (offset, programmed)
}

#[test]
fn an_area_pair_carries_the_records_kind_and_returns_its_offset() {
    let (offset, programmed) = program(area_world(false, 2), Some(3), REQUEST);
    let expected = AreaSetup { resource: 3, scene: 7, kind: 2, class: 6, values: [1, 2], flags: 0xf0, auxiliary: 5, last: 9 };
    assert_eq!(programmed.plain, [expected]);
    assert_eq!(programmed.referenced, [ReferencedArea { area: AreaSetup { values: [3, 4], ..expected }, reference: 0xaa }]);
    assert_eq!(offset, Some(3000 + 3 - 7));
}

#[test]
fn a_resource_beyond_the_count_narrows_kind_two_to_kind_one() {
    let (offset, programmed) = program(area_world(false, 2), Some(4), REQUEST);
    assert_eq!(programmed.plain[0].kind, 1);
    assert_eq!(offset, Some(2000 + 4 - 6));
}

#[test]
fn a_forced_or_armed_pair_drops_the_kind_and_an_armed_one_reads_the_standard_offsets() {
    let (forced, programmed) = program(area_world(false, 1), Some(2), AreaRequest { forced: true, ..REQUEST });
    assert_eq!((programmed.plain[0].kind, programmed.plain[0].class), (0, 0));
    assert_eq!(forced, Some(2000 + 2 - 8));
    let (armed, programmed) = program(area_world(true, 1), Some(2), REQUEST);
    assert_eq!((programmed.referenced[0].area.kind, programmed.referenced[0].area.class), (0, 0));
    assert_eq!(programmed.referenced[0].area.scene, 0);
    assert_eq!(armed, Some(1000 + 2 - 8));
}

#[test]
fn nothing_is_programmed_without_a_lens_resource() {
    let (offset, programmed) = program(area_world(false, 1), None, REQUEST);
    assert_eq!(offset, None);
    assert!(programmed.plain.is_empty() && programmed.referenced.is_empty());
}

#[test]
fn the_round_area_uses_the_records_scene_kind_with_no_class() {
    let mut programmed = Programmed::default();
    let mut world = area_world(true, 2);
    program_round_area(&mut world, &mut Lens(None), &mut programmed, handle(), 6, [7, 8]);
    let expected = AreaSetup { resource: 6, scene: 7, kind: 1, class: 0, values: [7, 8], flags: 0xf0, auxiliary: 5, last: 0 };
    assert_eq!(programmed.plain, [expected]);
}
