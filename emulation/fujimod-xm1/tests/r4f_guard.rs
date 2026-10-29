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

fn stop(program: &[u32]) -> Option<String> {
    let mut machine = machine(program);
    machine.run(10_000);
    machine.failure
}

#[test]
fn ordinary_arm_code_runs_under_the_guard() {
    // mov r1, #6; mov r2, #3; mul r0, r1, r2; mrc p15,1,r3,c0,c0,1 (CLIDR)
    assert_eq!(stop(&[0xe3a0_1006, 0xe3a0_2003, 0xe000_0291, 0xee30_3f30]), None);
}

#[test]
fn each_r5f_only_behaviour_stops_the_run() {
    let cases = [
        (0xe710_f211, "A32 SDIV/UDIV"), // sdiv r0, r1, r2
        (0xe730_f211, "A32 SDIV/UDIV"), // udiv r0, r1, r2
        (0xee10_0fb0, "ID register read c0,c0,5"), // mrc p15,0,r0,c0,c0,5 (MPIDR)
        (0xee10_0f10, "ID register read c0,c0,0"), // mrc p15,0,r0,c0,c0,0 (MIDR)
        (0xf591_f000, "PLDW"), // pldw [r1]
        (0xee1f_0f30, "c15 access"), // mrc p15,0,r0,c15,c0,1
    ];
    for (word, expected) in cases {
        let failure = stop(&[0xe3a0_1006, 0xe3a0_2003, word]).unwrap_or_default();
        assert!(failure.contains("R4F guard") && failure.contains(expected), "{word:#010x}: {failure}");
    }
}

#[test]
fn an_enabled_mpu_region_above_eleven_stops_the_run() {
    // mov r0, #12; mcr p15,0,r0,c6,c2,0; mov r1, #0x3f; mcr p15,0,r1,c6,c1,2
    let failure = stop(&[0xe3a0_000c, 0xee06_0f12, 0xe3a0_103f, 0xee06_1f51]).unwrap_or_default();
    assert!(failure.contains("enabled MPU region 12"), "{failure}");
    // mov r0, #11; mcr p15,0,r0,c6,c2,0; mov r1, #0x3f; mcr p15,0,r1,c6,c1,2
    assert_eq!(stop(&[0xe3a0_000b, 0xee06_0f12, 0xe3a0_103f, 0xee06_1f51]), None);
}

fn shutter_machine() -> Machine {
    let machine = machine(&[]);
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

    let mut machine = shutter_machine();
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

    let mut machine = shutter_machine();
    let before = machine.read(0xfff89014, 4).unwrap();
    machine.failure = Some("synthetic strict stop".into());
    assert!(machine.set_shutter_position(Full).is_err());
    assert_eq!(machine.read(0xfff89014, 4).unwrap(), before);
}
