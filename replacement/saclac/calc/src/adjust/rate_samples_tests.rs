use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::rate_samples::*;
use crate::exposure::fakes::World;
use crate::exposure::ports::StockMemory;

const STATE: u32 = 0x16d_6cbc;
const BUFFER: u32 = 0x5f00_0000;
const RESULTS: u32 = 0x5e00_0002;
const ALIGNED: u32 = 0x5e00_0004;
const TABLES: [u32; 7] = [0x4100_0000, 0x4100_1000, 0x4100_2000, 0x4100_3000, 0x4100_4000, 0x4100_5000, 0x4100_6000];

fn setup() -> RecorderSetup {
    RecorderSetup { buffer: Some(SharedAddress(BUFFER)), results: Some(SharedAddress(RESULTS)), tables: TABLES.map(|table| Some(SharedAddress(table))) }
}

fn byte(world: &mut World, at: u32) -> u8 { world.read_u8(SharedAddress(at)) }

fn word(world: &World, at: u32) -> u32 { world.u32_at(SharedAddress(at)) }

#[test]
fn set_up_refuses_a_missing_buffer_results_area_or_table() {
    let mut world = World::default();
    assert_eq!(set_up(&mut world, &RecorderSetup { buffer: None, ..setup() }), Err(Refused));
    assert_eq!(set_up(&mut world, &RecorderSetup { results: None, ..setup() }), Err(Refused));
    let mut tables = setup().tables;
    tables[6] = None;
    assert_eq!(set_up(&mut world, &RecorderSetup { tables, ..setup() }), Err(Refused));
    assert!(world.stores.is_empty());
}

#[test]
fn set_up_keeps_the_tables_and_points_at_the_buffer() {
    let mut world = World::default();
    world.write_u32(SharedAddress(TABLES[0] + 4), 0xa1b2_c3d4);
    world.write_u32(SharedAddress(TABLES[6] + 0x214), 0x55);
    world.write_u32(SharedAddress(STATE), 9);
    world.stores.clear();
    assert_eq!(set_up(&mut world, &setup()), Ok(()));
    assert_eq!(word(&world, STATE), 0);
    assert_eq!(word(&world, STATE + 0x134 + 4), 0xa1b2_c3d4);
    assert_eq!(word(&world, STATE + 0x3a8 + 0x214), 0x55);
    assert_eq!(word(&world, STATE + 0x5c0), BUFFER);
    assert_eq!(word(&world, STATE + 0x6cc), ALIGNED);
    assert_eq!(word(&world, STATE + 0x690), BUFFER + 4);
    assert_eq!(word(&world, STATE + 0x6ac), BUFFER + 0x491a0);
    assert_eq!(word(&world, STATE + 0x130), 3);
}

#[test]
fn set_up_marks_columns_unfilled_and_writes_the_starting_results() {
    let mut world = World::default();
    set_up(&mut world, &setup()).unwrap();
    assert_eq!(word(&world, BUFFER + 4 + 0xc0f0 + 48 * 0x3f0), 0xfdfd_fdfd);
    assert_eq!(word(&world, BUFFER + 0x2ec24 + 0x46e0), 0xfdfd_fdfd);
    assert_eq!(world.u16_at(SharedAddress(BUFFER + 0x2ea2c)), 0xfdfd);
    assert_eq!(byte(&mut world, BUFFER + 0x4fffe + 0x10), 0xfd);
    assert_eq!(word(&world, ALIGNED + 0x1554 + 0x18 * 8 + 8), 0x61);
    assert_eq!(byte(&mut world, ALIGNED + 0x2a8), 0xff);
    assert_eq!(byte(&mut world, ALIGNED + 0x94c + 0x2a7), 0xff);
    assert_eq!(byte(&mut world, ALIGNED + 0x162c + 8 + 3), 2);
}

#[test]
fn set_up_counts_leading_channels_up_to_the_first_flagged_one() {
    let mut world = World::default();
    for (index, flag) in [0x80u8, 0x80, 0x00, 0x80, 0x84, 0x80].into_iter().enumerate() {
        world.write_u8(SharedAddress(TABLES[0] + index as u32), flag);
    }
    world.write_u8(SharedAddress(TABLES[0] + 20), 0x04);
    set_up(&mut world, &setup()).unwrap();
    assert_eq!(byte(&mut world, STATE + 8), 3);
    assert_eq!(byte(&mut world, STATE + 7), 0);
}

fn recorder(recorded: u8) -> World {
    let mut world = World::default();
    set_up(&mut world, &setup()).unwrap();
    world.write_u8(SharedAddress(STATE + 4), recorded);
    world
}

#[test]
fn a_column_is_recorded_into_flagged_channels_and_the_extra_samples() {
    let mut world = recorder(4);
    world.write_u8(SharedAddress(STATE + 0x134 + 8 * 2), 1);
    world.write_u8(SharedAddress(STATE + 0x17c), 2);
    world.write_u32(SharedAddress(STATE + 0x178), 1);
    let records = BUFFER + 0x2ec24;
    assert_eq!(record_column(&mut world, &[10, 11, -12, 13, 14, 15, 16, 17, 18], 3), Ok(()));
    assert_eq!(word(&world, records + 3 * 0x3f0 + 12), -12i32 as u32);
    assert_eq!(word(&world, records + 2 * 0x3f0 + 12), 0);
    assert_eq!(word(&world, records + 0x1f80 + 12), 17);
    assert_eq!(word(&world, records + 0x1f80 + 0x3f0 + 12), 18);
    assert_eq!(word(&world, records + 0x46e0 + 12), 17);
    assert_eq!(record_column(&mut world, &[], 4), Err(Refused));
}

#[test]
fn the_scan_keeps_each_allowed_columns_extremes_and_the_count() {
    let mut world = recorder(5);
    world.write_u8(SharedAddress(STATE + 5), 5);
    let column = BUFFER + 4 + 0x3f0 * 2;
    for (index, value) in [7u32, 30, 2, 30, 9].into_iter().enumerate() {
        world.write_u32(SharedAddress(column + 4 * index as u32), value);
    }
    world.write_u8(SharedAddress(ALIGNED + 0x550 + 2), 3);
    world.write_u8(SharedAddress(ALIGNED + 0x5fa + 2), 3);
    world.write_u8(SharedAddress(ALIGNED + 0x550 + 3), 3);
    assert_eq!(scan_extremes(&mut world, 3), Ok(()));
    assert_eq!((byte(&mut world, ALIGNED + 0x550 + 2), word(&world, ALIGNED + 4 * 2)), (1, 30));
    assert_eq!((byte(&mut world, ALIGNED + 0x5fa + 2), word(&world, ALIGNED + 0x2a8 + 4 * 2)), (2, 2));
    assert_eq!(word(&world, ALIGNED + 4 * 4), 0);
    assert_eq!(byte(&mut world, STATE + 4), 3);
    assert_eq!(byte(&mut world, STATE + 5), 3);
    assert_eq!(scan_extremes(&mut world, 4), Err(Refused));
    assert_eq!(scan_extremes(&mut world, 0), Err(Refused));
}
