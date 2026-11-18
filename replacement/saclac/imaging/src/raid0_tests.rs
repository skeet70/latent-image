use std::vec::Vec;

use crate::driver::{Client, Event, Fault, Operation, Refusal, Register, Unit};
use crate::fake::FakeIsp;
use crate::raid0::*;

const fn r(offset: u16) -> Register { Register::Raid0(offset) }

fn parameter(operation: u32) -> Event {
    Event::Refused { client: Client::S1, unit: Unit::Raid0, operation: Operation(operation), fault: Fault::Parameter }
}

#[test]
fn stage_selectors_change_only_their_two_bits() {
    let mut isp = FakeIsp::new();
    isp.preset(r(0), 0xffff_ffff);
    set_stage(&mut isp, Stage::Third, StageMode::On);
    assert_eq!(isp.value(r(0)), Some(0xffff_ffdf));
    isp.preset(r(0), 0);
    set_stage(&mut isp, Stage::Fourth, StageMode::Alternate);
    assert_eq!(isp.value(r(0)), Some(0x200));
    assert_eq!(isp.stored_record(Unit::Raid0).operation, Operation(7));
}

#[test]
fn gates_and_path_share_the_control_register() {
    let mut isp = FakeIsp::new();
    isp.preset(r(0), 0x0007_0005);
    set_path(&mut isp, Path { source: PathSource::Second, enabled: false });
    assert_eq!(isp.value(r(0)), Some(0x0001_0005));
    set_gates(&mut isp, Gates { first: true, second: false });
    assert_eq!(isp.value(r(0)), Some(0x0001_1005));
    assert_eq!(gates(&mut isp), Gates { first: true, second: false });
    isp.preset(r(0), 0x0006_0000);
    assert_eq!(path(&mut isp), Path { source: PathSource::Third, enabled: true });
    set_gates(&mut isp, Gates { first: false, second: true });
    set_path(&mut isp, Path { source: PathSource::Fourth, enabled: true });
    assert_eq!(isp.writes(), [
        (r(0), 0x0001_0005), (r(0), 0x0001_1005),
        (r(0), 0x0006_2000), (r(0), 0x0007_2000),
    ]);
}

#[test]
fn coordinates_pack_two_per_register() {
    let mut isp = FakeIsp::new();
    assert_eq!(set_origin(&mut isp, Client::S1, [0x100, 0x200, 0x300]), Ok(()));
    assert_eq!(set_extent(&mut isp, Client::S1, [1, 2, 3, 4]), Ok(()));
    assert_eq!(set_area(&mut isp, Client::S1, [1, 2, 3, 4]), Ok(()));
    assert_eq!(isp.writes(), [
        (r(0x04), 0x0100_0200), (r(0x08), 0x0300_0000),
        (r(0x10), 0x0001_0002), (r(0x14), 0x0003_0004),
        (r(0x40), 0x0002_0001), (r(0x44), 0x0004_0003),
    ]);
}

#[test]
fn an_out_of_range_coordinate_is_logged_and_writes_nothing() {
    let mut isp = FakeIsp::new();
    assert_eq!(set_origin(&mut isp, Client::S1, [0, 0x2000, 0]), Err(Refusal::Parameter));
    assert_eq!(isp.logs(), [parameter(0x0c)]);
    assert!(isp.writes().is_empty());
}

#[test]
fn format_and_division_fields() {
    let mut isp = FakeIsp::new();
    assert_eq!(set_format(&mut isp, Client::S1, Format { first: 5, second: 11, third: 15, fourth: 3 }), Ok(()));
    assert_eq!(set_division(&mut isp, Client::S1, Division { mode: DivisionMode::Fourth, count: 15 }), Ok(()));
    assert_eq!(isp.writes(), [(r(0x0c), 0x003f_005d), (r(0x18), 0x3f)]);
    assert_eq!(set_format(&mut isp, Client::S1, Format { first: 6, second: 0, third: 0, fourth: 0 }), Err(Refusal::Parameter));
    assert_eq!(set_division(&mut isp, Client::S1, Division { mode: DivisionMode::First, count: 0 }), Err(Refusal::Parameter));
    assert_eq!(isp.logs(), [parameter(0x0e), parameter(0x12)]);
}

#[test]
fn points_are_shifted_and_the_last_is_marked() {
    let mut isp = FakeIsp::new();
    let shift = Shift { low: 1, high: 2 };
    assert_eq!(set_points(&mut isp, Client::S1, &[0x4001_0002, 0x0003_c004, 0x0000_1fff], shift), Ok(()));
    assert_eq!(isp.writes(), [(r(0x100), 0x0003_0003), (r(0x104), 0x0005_c005), (r(0x108), 0x0002_0000), (r(0x108), 0x8002_0000)]);
}

#[test]
fn an_empty_point_list_writes_the_empty_entry_twice() {
    let mut isp = FakeIsp::new();
    assert_eq!(set_points(&mut isp, Client::S1, &[], Shift { low: 0, high: 0 }), Ok(()));
    assert_eq!(isp.writes(), [(r(0x100), 0x9fff_1fff), (r(0x104), 0x9fff_1fff)]);
    let long: Vec<u32> = (0..1025).collect();
    assert_eq!(set_points(&mut isp, Client::S1, &long, Shift { low: 0, high: 0 }), Err(Refusal::Parameter));
}

#[test]
fn pairs_fill_eight_registers() {
    let mut isp = FakeIsp::new();
    assert_eq!(set_pairs(&mut isp, Client::S1, &[0x4001, 0x8002, 0x0003], 0x10), Ok(()));
    let mut expected = std::vec![(r(0x20), 0x8012_4011), (r(0x24), 0x1fff_0013)];
    expected.extend((2..8).map(|index| (r(0x20 + 4 * index), 0x1fff_1fff)));
    assert_eq!(isp.writes(), expected);
}

#[test]
fn an_empty_pair_list_writes_only_the_first_register() {
    let mut isp = FakeIsp::new();
    assert_eq!(set_pairs(&mut isp, Client::S1, &[], 5), Ok(()));
    assert_eq!(isp.writes(), [(r(0x20), 0x1fff_1fff)]);
}

#[test]
fn block_limits_are_checked_and_not_programmed() {
    let mut isp = FakeIsp::new();
    let block = Block { base: 0xf_ffff, stride: 0x3fff, corners: [1, 2, 3, 4], limits: [0x3fff; 4] };
    assert_eq!(set_block(&mut isp, Client::S1, block), Ok(()));
    assert_eq!(isp.writes(), [(r(0x48), 0xf_ffff), (r(0x4c), 0x3fff), (r(0x50), 0x0002_0001), (r(0x54), 0x0004_0003)]);
    let block = Block { limits: [0, 0, 0, 0x4000], ..block };
    assert_eq!(set_block(&mut isp, Client::S1, block), Err(Refusal::Parameter));
    assert_eq!(isp.logs(), [parameter(0x1a)]);
}

#[test]
fn window_registers() {
    let mut isp = FakeIsp::new();
    let window = Window { position: [0x10, 0x20], steps: [0x3f, 1], size: [0x7ff, 2], length: 0x3fff };
    assert_eq!(set_window(&mut isp, Client::S1, window), Ok(()));
    assert_eq!(isp.writes(), [(r(0x64), 0x0010_0020), (r(0x68), 0x003f_0001), (r(0x6c), 0x07ff_0002), (r(0x70), 0x3fff)]);
}

#[test]
fn the_second_extent_keeps_the_step_registers_other_bits() {
    let mut isp = FakeIsp::new();
    isp.preset(r(0x84), 0xffff_ffff);
    let extent = SecondExtent { extent: [1, 2], span: [3, 4], steps: [5, 6] };
    assert_eq!(set_second_extent(&mut isp, Client::S1, extent), Ok(()));
    assert_eq!(isp.writes(), [(r(0x7c), 0x0001_0002), (r(0x80), 0x0003_0004), (r(0x84), 0xc5c6_ffff)]);
}

#[test]
fn the_second_window_replaces_the_step_register() {
    let mut isp = FakeIsp::new();
    isp.preset(r(0x84), 0xffff_ffff);
    let window = SecondWindow { position: [1, 2], steps: [3, 4], length: 5 };
    assert_eq!(set_second_window(&mut isp, Client::S1, window), Ok(()));
    assert_eq!(isp.writes(), [(r(0x78), 0x0001_0002), (r(0x84), 0x0304_0005)]);
}

#[test]
fn tables_are_rotated_or_masked() {
    let mut isp = FakeIsp::new();
    let first = core::array::from_fn(|index| 0xc123_c456 + index as u32);
    let second = core::array::from_fn(|index| 0x8234_8567 + index as u32);
    load_rotated(&mut isp, [&first, &second]);
    let mut expected: Vec<_> = (0..14).map(|index| (r(0x88 + 4 * index), 0x0456_0123 + (u32::from(index) << 16))).collect();
    expected.extend((0..14).map(|index| (r(0xc0 + 4 * index), 0x0567_0234 + (u32::from(index) << 16))));
    assert_eq!(isp.writes(), expected);
    isp.take();
    let first = core::array::from_fn(|index| 0xc123_4000 + index as u32);
    let second = core::array::from_fn(|index| 0x8234_5000 + index as u32);
    let third = core::array::from_fn(|index| 0xf345_6000 + index as u32);
    load_second_tables(&mut isp, [&first, &second, &third]);
    let mut expected: Vec<_> = (0..256).map(|index| (r(0x2800 + 4 * index), 0x0123_4000 + u32::from(index))).collect();
    expected.extend((0..256).map(|index| (r(0x2c00 + 4 * index), 0x0234_5000 + u32::from(index))));
    expected.extend((0..256).map(|index| (r(0x3000 + 4 * index), 0x0005_6000 + u32::from(index))));
    assert_eq!(isp.writes(), expected);
    isp.take();
    load_first_tables(&mut isp, [&first, &second, &third]);
    let mut expected: Vec<_> = (0..256).map(|index| (r(0x1c00 + 4 * index), 0x0123_4000 + u32::from(index))).collect();
    expected.extend((0..256).map(|index| (r(0x2000 + 4 * index), 0x0234_5000 + u32::from(index))));
    expected.extend((0..256).map(|index| (r(0x2400 + 4 * index), 0x3345_6000 + u32::from(index))));
    assert_eq!(isp.writes(), expected);
}

#[test]
fn curves_load_unchanged() {
    let mut isp = FakeIsp::new();
    let curves: Vec<[u32; 64]> = (0..9).map(|curve| core::array::from_fn(|index| 100 * curve + index as u32)).collect();
    load_curves(&mut isp, core::array::from_fn(|index| &curves[index]));
    let expected: Vec<_> = (0u16..9).flat_map(|curve| (0u16..64).map(move |index| (r(0x1100 + 0x100 * curve + 4 * index), u32::from(100 * curve + index)))).collect();
    assert_eq!(isp.writes(), expected);
}

#[test]
fn mode_level_keeps_three_bits() {
    let mut isp = FakeIsp::new();
    set_mode(&mut isp, Mode { enabled: true, level: 9 });
    assert_eq!(isp.value(r(0x74)), Some(0x0001_0001));
    isp.preset(r(0x74), 0x0005_0000);
    assert_eq!(mode(&mut isp), Mode { enabled: false, level: 5 });
    set_gate(&mut isp, true);
    set_output(&mut isp, false);
    assert_eq!((gate(&mut isp), output(&mut isp)), (true, false));
}
