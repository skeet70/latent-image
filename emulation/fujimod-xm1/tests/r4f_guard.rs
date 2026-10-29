//! Synthetic boot-ROM fixtures for the R4F guard on the X-M1 session: each ROM
//! is authored here, starts at the reset vector and spins after its subject
//! instruction.

use fujimod_xm1::machine::Machine;
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};

const RUN: &[u8] = include_bytes!("../data/xm1-150.json");
const ROM_BYTES: usize = 16384;
/// `b .`
const SPIN: u32 = 0xeaff_fffe;

fn machine(program: &[u32]) -> Machine {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(RUN).unwrap();
    let mut rom = vec![0u8; ROM_BYTES];
    for (index, word) in program.iter().chain(std::iter::once(&SPIN)).enumerate() {
        rom[4 * index..4 * index + 4].copy_from_slice(&word.to_le_bytes());
    }
    let update = ValidatedUpdate(vec![0xff; document.update_flash_offset + 2048]);
    let mut session = run::open(&document, &update, &rom).unwrap();
    session.board.set_execution_policy(document.execution_policy.into()).unwrap();
    Machine::new(document, session).unwrap()
}

fn shutter_machine(program: &[u32]) -> Machine {
    let machine = machine(program);
    let table = &machine.document.firmware.key_table;
    for (code, row, pin, interrupt) in [(7u32, 7, 2u32, 10u16), (8, 6, 3, 11)] {
        let mut descriptor = [0u8; 28];
        descriptor[..4].copy_from_slice(&code.to_le_bytes());
        descriptor[9] = 4;
        descriptor[12..16].copy_from_slice(&pin.to_le_bytes());
        descriptor[16..18].copy_from_slice(&1u16.to_le_bytes());
        descriptor[18..20].copy_from_slice(&interrupt.to_le_bytes());
        machine.session.board.write_physical(table.base.0 + row * table.row_bytes, &descriptor).unwrap();
    }
    machine
}

#[test]
fn physical_full_shutter_holds_both_contacts_without_changing_firmware_ram() {
    use fujimod_xm1::guest::PhysicalShutterPosition::{Full, Half, Released};

    let mut machine = shutter_machine(&[]);
    let base = machine.document.firmware.key_table.base.0;
    let before = machine.read(base, 8 * 28).unwrap();
    for (position, expected) in [(Released, 0xc), (Half, 0x8), (Full, 0), (Half, 0x8), (Released, 0xc)] {
        machine.set_shutter_position(position).unwrap();
        machine.run(1_000);
        machine.require_running().unwrap();
        let pads = machine.read(0xfff89014, 4).unwrap();
        assert_eq!(u32::from_le_bytes(pads.try_into().unwrap()) & 0xc, expected);
        assert_eq!(machine.read(base, 8 * 28).unwrap(), before);
    }
}

#[test]
fn a_stopped_machine_refuses_a_physical_shutter_transition() {
    use fujimod_xm1::guest::PhysicalShutterPosition::Full;

    let mut machine = shutter_machine(&[0xe3a0_1006, 0xe3a0_2003, 0xe710_f211]);
    let before = machine.read(0xfff89014, 4).unwrap();
    machine.run(10_000);
    assert!(machine.require_running().is_err());
    assert!(machine.set_shutter_position(Full).is_err());
    assert_eq!(machine.read(0xfff89014, 4).unwrap(), before);
}
