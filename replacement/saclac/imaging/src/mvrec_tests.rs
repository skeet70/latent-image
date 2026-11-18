use crate::driver::{Bank, Client, Event, Fault, Operation, Refusal, Register, Unit};
use crate::fake::FakeIsp;
use crate::mvrec::*;

const fn first(offset: u16) -> Register { Register::Mvrec(Bank::First, offset) }
const fn second(offset: u16) -> Register { Register::Mvrec(Bank::Second, offset) }

fn parameter(operation: u32) -> Event {
    Event::Refused { client: Client::MV, unit: Unit::Mvrec, operation: Operation(operation), fault: Fault::Parameter }
}

#[test]
fn dimensions_are_stored_less_one_in_the_chosen_bank() {
    let mut isp = FakeIsp::new();
    assert_eq!(set_dimensions(&mut isp, Client::MV, Bank::Second, [0x1000, 1]), Ok(()));
    assert_eq!(isp.writes(), [(second(0x04), 0x0000_0fff)]);
    assert_eq!(set_dimensions(&mut isp, Client::MV, Bank::First, [0, 1]), Err(Refusal::Parameter));
    assert_eq!(isp.logs(), [parameter(5)]);
}

#[test]
fn geometry_offsets_and_window() {
    let mut isp = FakeIsp::new();
    set_geometry(&mut isp, Bank::First, Geometry { enabled: true, first: 0x1234, second: 0xabcd });
    assert_eq!(set_offset(&mut isp, Client::MV, Bank::First, Offset::Third, [0x1fff, 2]), Ok(()));
    assert_eq!(set_window(&mut isp, Client::MV, Bank::First, Window { position: [3, 0x7fff], repeat: 8, count: 32 }), Ok(()));
    assert_eq!(isp.writes(), [
        (first(0x08), 0x8000_1234), (first(0x0c), 0xabcd),
        (first(0x18), 0x0002_1fff),
        (first(0x1c), 0x7fff_0003), (first(0x20), 0x0020_0008),
    ]);
    assert_eq!(set_offset(&mut isp, Client::MV, Bank::First, Offset::Second, [0x2000, 0]), Err(Refusal::Parameter));
    assert_eq!(set_window(&mut isp, Client::MV, Bank::First, Window { position: [0, 0], repeat: 9, count: 0 }), Err(Refusal::Parameter));
    assert_eq!(isp.logs(), [parameter(11), parameter(15)]);
}

#[test]
fn the_window_reads_back_its_fields() {
    let mut isp = FakeIsp::new();
    isp.preset(second(0x1c), 0xffff_ffff);
    isp.preset(second(0x20), 0xffff_ffff);
    assert_eq!(window(&mut isp, Bank::Second), Window { position: [0x7fff, 0x7fff], repeat: 0xf, count: 0x3f });
}

#[test]
fn table_entries_keep_seventeen_bits() {
    let mut isp = FakeIsp::new();
    load_table(&mut isp, Bank::First, &[0xffff_ffff; 31]);
    assert_eq!(isp.value(first(0x30)), Some(0x1_ffff));
    assert_eq!(isp.value(first(0x30 + 4 * 30)), Some(0x1_ffff));
    assert_eq!(isp.writes().len(), 31);
}

#[test]
fn secondary_geometry_needs_multiples_of_eight() {
    let mut isp = FakeIsp::new();
    let geometry = SecondaryGeometry { width: 0x800, height: 0x10, offset: 0x7f8, stride: 0x18 };
    assert_eq!(set_secondary_geometry(&mut isp, Client::MV, Bank::First, geometry), Ok(()));
    assert_eq!(isp.writes(), [(first(0xb0), 0x000f_07ff), (first(0xb4), 0x07f8_0017)]);
    let geometry = SecondaryGeometry { width: 0x804, ..geometry };
    assert_eq!(set_secondary_geometry(&mut isp, Client::MV, Bank::First, geometry), Err(Refusal::Parameter));
    let geometry = SecondaryGeometry { width: 0x10, offset: 0x7fc, ..geometry };
    assert_eq!(set_secondary_geometry(&mut isp, Client::MV, Bank::First, geometry), Err(Refusal::Parameter));
    assert_eq!(isp.logs(), [parameter(19), parameter(19)]);
}

#[test]
fn readback_packs_region_source_and_mode() {
    let mut isp = FakeIsp::new();
    let readback = Readback { enabled: true, region: 0xff, source: ReadbackSource::Fourth, mode: ReadbackMode::Fifth };
    assert_eq!(set_readback(&mut isp, Client::MV, Bank::Second, readback), Ok(()));
    assert_eq!(isp.writes(), [(second(0xb8), 0x01ff_0034)]);
    let readback = Readback { enabled: false, region: 0x100, ..readback };
    assert_eq!(set_readback(&mut isp, Client::MV, Bank::Second, readback), Err(Refusal::Parameter));
    assert_eq!(isp.logs(), [parameter(0x15)]);
}

#[test]
fn address_and_stride_alignment() {
    let mut isp = FakeIsp::new();
    assert_eq!(set_address(&mut isp, Client::MV, Bank::First, Address { address: 0x3fff_fff0, stride: 0x1ff8 }), Ok(()));
    assert_eq!(isp.writes(), [(first(0xc0), 0x3fff_fff0), (first(0xc4), 0x1ff8)]);
    assert_eq!(set_address(&mut isp, Client::MV, Bank::First, Address { address: 0x4000_0000, stride: 8 }), Err(Refusal::Parameter));
    assert_eq!(set_address(&mut isp, Client::MV, Bank::First, Address { address: 0x10, stride: 0x1ffc }), Err(Refusal::Parameter));
    assert_eq!(isp.logs(), [parameter(23), parameter(23)]);
}

#[test]
fn the_nibble_checks_both_values_and_programs_the_low_one() {
    let mut isp = FakeIsp::new();
    assert_eq!(set_nibble(&mut isp, Client::MV, Bank::First, 7, 15), Ok(()));
    assert_eq!(set_nibble(&mut isp, Client::MV, Bank::First, 7, 16), Err(Refusal::Parameter));
    assert_eq!(isp.writes(), [(first(0xf4), 7)]);
}
