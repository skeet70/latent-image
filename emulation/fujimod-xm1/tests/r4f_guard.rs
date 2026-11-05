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

struct RamGuard {
    board: fujimod_emulator::board::Board,
    guard: fujimod_xm1::r4f::Guard,
}

impl RamGuard {
    fn new(words: &[u32]) -> Self {
        use fujimod_emulator::board::{bus::{Bus, Mode}, factory::{self, Graph, MemoryMap, Processor, Ram}, memory::BusMapping};
        use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, arm, timeline::Ratio};
        let graph = Graph {
            processors: (0..2).map(|_| Processor {
                model: Model::CortexR5F, mprofile: None, released: true, rprofile: Some(Default::default()),
                registers: vec![(Register::Pc, 0x4000_2000), (Register::Pstate, 0xd3), (arm::CORE[0], 9), (arm::CORE[1], 3)],
            }).collect(),
            ram: vec![Ram { base: 0, size: 0x10000 }],
            mappings: (0..2).flat_map(|core| [0, 0x4000_0000].map(|address| MemoryMap {
                core, address, physical: 0, size: 0x10000, permissions: 7,
            })).collect(),
            dma_mappings: vec![BusMapping::ram(0, 0x10000)],
            decoders: vec![], peripherals: vec![], instructions_per_tick: Ratio::new(1, 1).unwrap(),
        };
        let board = graph.build(factory::Options {
            engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20,
        }, Bus::new(Mode::Strict), None).unwrap();
        board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
        let mut fixture = Self { board, guard: Default::default() };
        fixture.program(0x2000, words);
        fixture.guard.arm(&fixture.board).unwrap();
        fixture
    }

    fn program(&self, address: u64, words: &[u32]) {
        let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        self.board.write_physical(address, &bytes).unwrap();
    }

    fn run(&mut self, core: usize, pc: u64) -> Result<(), String> {
        let cpu = self.board.cpu(core).unwrap();
        cpu.set_loop_window(true).unwrap();
        let result = cpu.run(fujimod_emulator_cpu::Run {
            begin: pc, until: None, watchdog_us: 0, instructions: Some(100_000),
        }).map(|_| ()).map_err(|error| error.to_string());
        self.guard.scan(&self.board).and(result)
    }

    fn r0(&self, core: usize) -> u64 {
        self.board.read_register(core, fujimod_emulator_cpu::arm::CORE[0]).unwrap()
    }
}

fn assert_guard_refusal(result: Result<(), String>) {
    let failure = result.unwrap_err();
    assert!(failure.contains("R4F guard:"), "{failure}");
}

#[test]
fn translation_admission_refuses_r5_only_instructions_on_each_core() {
    for core in 0..2 {
        for word in [0xe710_f110, 0xe730_f110, 0x0710_f110, 0xf590_f000, 0xee10_0fb0, 0xee1f_0f10, 0xee09_0f12] {
            let mut fixture = RamGuard::new(&[word, SPIN]);
            assert_guard_refusal(fixture.run(core, 0x4000_2000));
            assert_eq!(fixture.r0(core), 9);
        }
    }
}

#[test]
fn host_rewrite_of_previously_admitted_ram_is_refused() {
    for core in 0..2 {
        let mut fixture = RamGuard::new(&[0xe280_0001, SPIN]);
        fixture.run(core, 0x4000_2000).unwrap();
        assert_eq!(fixture.r0(core), 10);
        fixture.program(0x2000, &[0xe710_f110, SPIN]);
        assert_guard_refusal(fixture.run(core, 0x4000_2000));
        assert_eq!(fixture.r0(core), 10);
    }
}

#[test]
fn guest_rewrite_of_previously_admitted_ram_is_refused() {
    use fujimod_emulator_cpu::arm;
    for core in 0..2 {
        let mut fixture = RamGuard::new(&[0xe280_0001, SPIN]);
        fixture.run(core, 0x4000_2000).unwrap();
        assert_eq!(fixture.r0(core), 10);
        fixture.program(0x3000, &[0xe582_3000, 0xe12f_ff12]);
        fixture.board.write_register(core, arm::CORE[2], 0x4000_2000).unwrap();
        fixture.board.write_register(core, arm::CORE[3], 0xe710_f110).unwrap();
        assert_guard_refusal(fixture.run(core, 0x4000_3000));
        assert_eq!(fixture.r0(core), 10);
    }
}

#[test]
fn admission_does_not_veto_exact_loop_acceleration() {
    use fujimod_emulator_cpu::{arm, loops::LoopMode};
    let mut fixture = RamGuard::new(&[0xe592_1000, 0xe311_0001, 0x0aff_fffc, SPIN]);
    fixture.board.write_register(0, arm::CORE[2], 0x4000_4000).unwrap();
    fixture.board.set_loop_mode(LoopMode::Accelerate).unwrap();
    fixture.run(0, 0x4000_2000).unwrap();
    assert!(fixture.board.loop_statistics().unwrap()[0]["loop_accelerated_instructions"].as_u64().unwrap() > 0);
}

#[test]
fn thumb_coprocessor_admission_precedes_register_effects() {
    for core in 0..2 {
        let mut fixture = RamGuard::new(&[SPIN]);
        fixture.board.write_physical(0x2000, &[0x10, 0xee, 0xb0, 0x0f, 0xfe, 0xe7]).unwrap();
        assert_guard_refusal(fixture.run(core, 0x4000_2001));
        assert_eq!(fixture.r0(core), 9);
    }
}

#[test]
fn an_enabled_r5_only_mpu_region_is_still_refused() {
    for core in 0..2 {
        let mut fixture = RamGuard::new(&[0xe3a0_000c, 0xee06_0f12, 0xe3a0_003f, 0xee06_0f51, SPIN]);
        assert_guard_refusal(fixture.run(core, 0x4000_2000));
    }
}
