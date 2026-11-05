//! Synthetic boot-ROM fixtures for the R4F guard on the X-M1 session: each ROM
//! is authored here, starts at the reset vector and spins after its subject
//! instruction.

use fujimod_xm1::machine::Machine;
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use fujimod_emulator_devices::exr2::gpio::Gpio;

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
    for (code, binding, interrupt) in [
        (7u32, &machine.document.firmware.shutter.half, 31u16),
        (8, &machine.document.firmware.shutter.full, 33),
    ] {
        let mut descriptor = [0u8; 28];
        descriptor[..4].copy_from_slice(&code.to_le_bytes());
        descriptor[9] = 4;
        descriptor[12..16].copy_from_slice(&u32::from(binding.pad.pin).to_le_bytes());
        descriptor[16..18].copy_from_slice(&1u16.to_le_bytes());
        descriptor[18..20].copy_from_slice(&interrupt.to_le_bytes());
        machine.session.board.write_physical(table.base.0 + binding.row.get() * table.row_bytes, &descriptor).unwrap();
    }
    machine
}

fn pad_levels(machine: &Machine) -> [u32; 16] {
    let node = machine.session.node(&machine.document.gpio.0).unwrap();
    machine.session.board.inspect_device::<Gpio, _>(node, |gpio| Ok(gpio.pads().input_levels())).unwrap()
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
        assert_eq!(pad_levels(&machine)[0] & 0xc, expected);
        assert_eq!(machine.read(base, 8 * 28).unwrap(), before);
    }
}

#[test]
fn a_stopped_machine_refuses_a_physical_shutter_transition() {
    use fujimod_xm1::guest::PhysicalShutterPosition::Full;

    let mut machine = shutter_machine(&[0xe3a0_1006, 0xe3a0_2003, 0xe710_f211]);
    let before = pad_levels(&machine);
    machine.run(10_000);
    assert!(machine.require_running().is_err());
    assert!(machine.set_shutter_position(Full).is_err());
    assert_eq!(pad_levels(&machine), before);
}

fn ram_program(machine: &Machine, core: usize, words: &[u32]) {
    use fujimod_emulator_cpu::{Register, arm};
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    machine.session.board.write_physical(0x2000, &bytes).unwrap();
    machine.session.board.write_register(core, Register::Pc, 0x4000_2000).unwrap();
    machine.session.board.write_register(core, Register::Pstate, 0xd3).unwrap();
    machine.session.board.write_register(core, arm::CORE[0], 9).unwrap();
    machine.session.board.write_register(core, arm::CORE[1], 3).unwrap();
}

fn assert_guard_refusal(machine: &Machine) {
    let failure = machine.require_running().unwrap_err();
    assert!(failure.contains("R4F guard:"), "{failure}");
}

fn run_core(machine: &mut Machine, core: usize) {
    use fujimod_emulator_cpu::{Register, Run};
    let board = &machine.session.board;
    let cpu = board.cpu(core).unwrap();
    let pc = board.read_register(core, Register::Pc).unwrap();
    let thumb = board.read_register(core, Register::Pstate).unwrap() & 0x20 != 0;
    cpu.set_loop_window(true).unwrap();
    let result = cpu.run(Run { begin: pc | u64::from(thumb), until: None, watchdog_us: 0, instructions: Some(100_000) })
        .map(|_| ()).map_err(|error| error.to_string());
    machine.failure = machine.guard.scan(board).and(result).err();
}

#[test]
fn translation_admission_refuses_r5_only_instructions_on_each_core() {
    for core in 0..2 {
        for word in [0xe710_f110, 0xe730_f110, 0x0710_f110, 0xf590_f000, 0xee10_0fb0, 0xee1f_0f10, 0xee09_0f12] {
            let mut machine = machine(&[]);
            ram_program(&machine, core, &[word, SPIN]);
            run_core(&mut machine, core);
            assert_guard_refusal(&machine);
            assert_eq!(machine.session.board.read_register(core, fujimod_emulator_cpu::arm::CORE[0]).unwrap(), 9);
        }
    }
}

#[test]
fn host_rewrite_of_previously_admitted_ram_is_refused() {
    for core in 0..2 {
        let mut machine = machine(&[]);
        ram_program(&machine, core, &[0xe1a0_0000, SPIN]);
        run_core(&mut machine, core);
        machine.require_running().unwrap();
        ram_program(&machine, core, &[0xe710_f110, SPIN]);
        run_core(&mut machine, core);
        assert_guard_refusal(&machine);
        assert_eq!(machine.session.board.read_register(core, fujimod_emulator_cpu::arm::CORE[0]).unwrap(), 9);
    }
}

#[test]
fn guest_rewrite_of_previously_admitted_ram_is_refused() {
    for core in 0..2 {
        let mut machine = machine(&[]);
        ram_program(&machine, core, &[0xe1a0_0000, SPIN]);
        run_core(&mut machine, core);
        machine.require_running().unwrap();
        let writer = [0xe582_3000u32, 0xe12f_ff12];
        machine.session.board.write_physical(0x3000, &writer.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>()).unwrap();
        for (register, value) in [(fujimod_emulator_cpu::Register::Pc, 0x4000_3000), (fujimod_emulator_cpu::arm::CORE[2], 0x4000_2000), (fujimod_emulator_cpu::arm::CORE[3], 0xe710_f110)] {
            machine.session.board.write_register(core, register, value).unwrap();
        }
        run_core(&mut machine, core);
        assert_guard_refusal(&machine);
        assert_eq!(machine.session.board.read_register(core, fujimod_emulator_cpu::arm::CORE[0]).unwrap(), 9);
    }
}

#[test]
fn admission_does_not_veto_exact_loop_acceleration() {
    use fujimod_emulator_cpu::{arm, loops::LoopMode};
    let mut machine = machine(&[]);
    ram_program(&machine, 0, &[0xe592_1000, 0xe311_0001, 0x0aff_fffc, SPIN]);
    machine.session.board.write_register(0, arm::CORE[2], 0x4000_4000).unwrap();
    machine.session.board.write_physical(0x4000, &0u32.to_le_bytes()).unwrap();
    machine.session.board.set_loop_mode(LoopMode::Accelerate).unwrap();
    run_core(&mut machine, 0);
    machine.require_running().unwrap();
    assert!(machine.session.board.loop_statistics().unwrap()[0]["loop_accelerated_instructions"].as_u64().unwrap() > 0);
}

#[test]
fn thumb_coprocessor_admission_precedes_register_effects() {
    for core in 0..2 {
        let mut machine = machine(&[]);
        ram_program(&machine, core, &[SPIN]);
        machine.session.board.write_physical(0x2000, &[0x10, 0xee, 0xb0, 0x0f, 0xfe, 0xe7]).unwrap();
        machine.session.board.write_register(core, fujimod_emulator_cpu::Register::Pstate, 0xf3).unwrap();
        run_core(&mut machine, core);
        assert_guard_refusal(&machine);
        assert_eq!(machine.session.board.read_register(core, fujimod_emulator_cpu::arm::CORE[0]).unwrap(), 9);
    }
}

#[test]
fn an_enabled_r5_only_mpu_region_is_still_refused() {
    for core in 0..2 {
        let mut machine = machine(&[]);
        ram_program(&machine, core, &[0xe3a0_000c, 0xee06_0f12, 0xe3a0_0001, 0xee06_0f51, SPIN]);
        run_core(&mut machine, core);
        assert_guard_refusal(&machine);
    }
}
