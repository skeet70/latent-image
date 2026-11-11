use fujimod_emulator::board::{
    Board,
    bus::{Bus, Mode},
    decoder::{Choice, Decoder},
    factory::{self, Graph, MemoryMap, Processor, Ram},
    memory::BusMapping,
    observation::{
        AccessBytes, ByteOrder, CoreIndex, CpuDataAttribution, FetchAttribution, GuestAddress,
        InstructionCores, InstructionExecutionState, InstructionGuardFailure, InstructionPoints, MemoryWatch,
        PhysicalAddress, PhysicalInstructionGuard, ViewPolicy, WatchKinds, WatchRange,
    },
};
use fujimod_emulator_cpu::{Cpu, ExecutionPolicy, ExecutionRefusal, Hook, HookRange, Model, Register, Run, arm, timeline::Ratio};
use fujimod_emulator_cpu_unicorn::rprofile;
use fujimod_emulator_devices::{Cx, Device};
use fujimod_xm1::modal_permit::{CoreId, Cpsr};
use fujimod_xm1::{modal_lifetime::{LifetimeFailure, LifetimeMemory, LifetimeMonitor, LifetimeRequest, Word},
    modal_task::PhysicalExtent, provenance::Guest};

const CODE: u64 = 0x1000;
const ALIAS: u64 = 0x4000_1000;
const DECODER: u64 = 0x1000_0000;
const DATA: u64 = 0x3000;

struct DecodeInput;
impl Device for DecodeInput {
    fn name(&self) -> &'static str { "synthetic-guard-decode-input" }
    fn read(&mut self, _: &mut Cx<'_>, _: u64, _: u8) -> u64 { 0 }
    fn write(&mut self, _: &mut Cx<'_>, _: u64, _: u8, _: u64) {}
}

fn board(program: &[u32]) -> (Board, usize) {
    let mut mappings = Vec::new();
    for core in 0..2 {
        mappings.extend([
            MemoryMap { core, address: 0, physical: 0, size: 0x10000, permissions: 7 },
            MemoryMap { core, address: 0x4000_0000, physical: 0, size: 0x10000, permissions: 7 },
            MemoryMap { core, address: DECODER, physical: CODE, size: 0x1000, permissions: 7 },
        ]);
    }
    let choice = |physical| Choice {
        processors: (0..2).map(|core| MemoryMap {
            core, address: DECODER, physical, size: 0x1000, permissions: 7,
        }).collect(),
        dma: vec![],
    };
    let graph = Graph {
        processors: (0..2).map(|_| Processor {
            model: Model::CortexR5F, mprofile: None, released: true,
            rprofile: Some(rprofile::core::Options {
                tcm: rprofile::tcm::Options {
                    atcm: rprofile::tcm::BankOptions { bytes: 0x1000, reset: 0 },
                    btcm: rprofile::tcm::BankOptions { bytes: 0x1000, reset: 0 },
                    ..Default::default()
                },
                ..Default::default()
            }),
            registers: vec![(Register::Pc, CODE), (Register::Pstate, 0xd3), (arm::CORE[0], 7), (arm::CORE[1], DATA)],
        }).collect(),
        ram: vec![Ram { base: 0, size: 0x10000 }],
        mappings,
        dma_mappings: vec![BusMapping::ram(0, 0x10000)],
        decoders: vec![Decoder { initial: 0, choices: vec![choice(CODE), choice(0x2000)] }],
        peripherals: vec![],
        instructions_per_tick: Ratio::new(1, 1).unwrap(),
    };
    let mut bus = Bus::new(Mode::Strict);
    let input = bus.register(0x2000_0000, 0x1000, Box::new(DecodeInput)).unwrap();
    let mut board = graph.build(factory::Options {
        engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20,
    }, bus, None).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    let bytes: Vec<_> = program.iter().flat_map(|word| word.to_le_bytes()).collect();
    board.write_physical(CODE, &bytes).unwrap();
    board.write_physical(0x2000, &bytes).unwrap();
    (board, input)
}

fn range(start: u64, end: u64) -> WatchRange {
    WatchRange { start, end, kinds: WatchKinds::CPU_FETCH }
}

fn watch(board: &Board, cores: InstructionCores) -> Box<MemoryWatch> {
    watch_with_sources(board, cores, CpuDataAttribution::Writes)
}

fn watch_with_sources(board: &Board, cores: InstructionCores, sources: CpuDataAttribution) -> Box<MemoryWatch> {
    watch_with_policy(board, cores, sources, InstructionExecutionState::A32Little)
}

fn watch_with_policy(
    board: &Board, cores: InstructionCores, sources: CpuDataAttribution, state: InstructionExecutionState,
) -> Box<MemoryWatch> {
    let protected = range(CODE, CODE + 12);
    let attribution = FetchAttribution::new(sources, vec![protected]).unwrap()
        .with_instruction_guards(vec![PhysicalInstructionGuard::arm32_little(
            protected, GuestAddress(CODE), cores,
        ).unwrap().with_execution_state(state)]).unwrap();
    board.memory_watch_begin_with_fetch_attribution(vec![
        range(CODE, CODE + 12),
        WatchRange { start: DATA, end: DATA + 8, kinds: WatchKinds::CPU_WRITE },
    ], attribution, 4, board.mappings().to_vec(), ViewPolicy::Fixed).unwrap()
}

fn run(board: &Board, core: usize, pc: u64, count: usize) {
    board.cpu(core).unwrap().run(Run {
        begin: pc, until: None, instructions: Some(count), watchdog_us: 0,
    }).unwrap();
}

fn word(board: &Board, address: u64) -> u32 {
    let mut bytes = [0; 4];
    board.read_physical(address, &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn points(board: &Board, core: usize, pc: u64) -> Box<InstructionPoints> {
    board.instruction_points_begin(core, vec![pc]).unwrap()
}

#[test]
fn canonical_fetches_execute_register_and_memory_effects_on_both_cores() {
    for core in 0..2 {
        let (board, _) = board(&[0xe2800001, 0xe5810000, 0xeafffffe]);
        let mut watch = watch(&board, InstructionCores::Both);
        run(&board, core, CODE, 2);
        assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 8);
        assert_eq!(word(&board, DATA), 8);
        assert!(watch.instruction_guard_status().unwrap().first_failure.is_none());
        assert_eq!(watch.records.borrow().row_count(), 1);
        assert!(watch.records.borrow().violation().is_none());
        board.memory_watch_end(&mut watch).unwrap();
    }
}

#[test]
fn alias_fetches_refuse_before_register_or_memory_effects_on_both_cores() {
    for core in 0..2 {
        for instruction in [0xe2800001, 0xe5810000] {
            let (board, _) = board(&[instruction, 0xeafffffe]);
            let mut watch = watch(&board, InstructionCores::Both);
            run(&board, core, ALIAS, 1);
            assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 7);
            assert_eq!(word(&board, DATA), 0);
            assert_eq!(board.read_register(core, Register::Pc).unwrap(), ALIAS);
            let failure = watch.instruction_guard_status().unwrap().first_failure.unwrap();
            assert_eq!(failure.kind, InstructionGuardFailure::Alias);
            assert_eq!(failure.core, Some(CoreIndex(core)));
            assert_eq!(watch.records.borrow().row_count(), 0);
            run(&board, core, CODE, 1);
            assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 7);
            assert_eq!(word(&board, DATA), 0);
            run(&board, core, 0x2000, 1);
            assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 7);
            assert_eq!(word(&board, DATA), 0);
            assert_eq!(watch.instruction_guard_status().unwrap().first_failure, Some(failure));
            board.memory_watch_end(&mut watch).unwrap();
        }
    }
}

#[test]
fn decoder_alternative_aliases_are_hooked_on_both_cores() {
    for core in 0..2 {
        let (board, _) = board(&[0xe2800001, 0xeafffffe]);
        let mut watch = watch(&board, InstructionCores::Both);
        run(&board, core, DECODER, 1);
        assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 7);
        assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, InstructionGuardFailure::Alias);
        board.memory_watch_end(&mut watch).unwrap();
    }
}

#[test]
fn nonadmitted_core_refuses_canonical_instructions() {
    let (board, _) = board(&[0xe2800001, 0xeafffffe]);
    let mut watch = watch(&board, InstructionCores::Core0);
    run(&board, 1, CODE, 1);
    assert_eq!(board.read_register(1, arm::CORE[0]).unwrap(), 7);
    assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, InstructionGuardFailure::CoreNotAdmitted);
    board.memory_watch_end(&mut watch).unwrap();
}

#[derive(Clone, Copy)]
enum ObserverOrder { FetchFirst, PointFirst }

#[test]
fn settled_entry_identity_is_fresh_across_same_pc_resume_loops_and_observer_orders() {
    for core in 0..2 {
        for order in [ObserverOrder::FetchFirst, ObserverOrder::PointFirst] {
            let (board, _) = board(&[0xeafffffe]);
            let (mut watch, mut points) = match order {
                ObserverOrder::FetchFirst => {
                    let watch = watch(&board, InstructionCores::Both);
                    (watch, points(&board, core, CODE))
                }
                ObserverOrder::PointFirst => {
                    let points = points(&board, core, CODE);
                    (watch(&board, InstructionCores::Both), points)
                }
            };
            run(&board, core, CODE, 1);
            assert_eq!(board.cpu(core).unwrap().instruction_progress(), Some(0));
            assert_eq!(points.event(), Some(CODE));
            let first = points.event_context().unwrap();
            let fetch = watch.fetch_event(first.core, first.entry.id).unwrap();
            assert_eq!(fetch.entry, first.entry);
            assert_eq!(fetch.physical, PhysicalAddress(CODE));
            assert_eq!(fetch.size, AccessBytes(4));
            assert_eq!(fetch.byte_order, ByteOrder::Little);
            assert_eq!(watch.fetch_event(first.core, first.entry.id), Some(fetch));
            points.resume(CODE, CODE);
            run(&board, core, CODE, 1);
            assert_eq!(points.event(), None);
            assert!(watch.fetch_event(first.core, first.entry.id).is_none());
            run(&board, core, CODE, 1);
            assert_eq!(board.cpu(core).unwrap().instruction_progress(), Some(0));
            assert_eq!(points.event(), Some(CODE));
            let later = points.event_context().unwrap();
            assert_ne!(later.entry.id, first.entry.id);
            assert_eq!(later.entry.address, first.entry.address);
            assert_eq!(later.entry.progress, first.entry.progress);
            assert_eq!(watch.fetch_event(later.core, later.entry.id).unwrap().entry, later.entry);
            assert!(watch.fetch_event(first.core, first.entry.id).is_none());
            assert!(watch.instruction_guard_status().unwrap().first_failure.is_none());
            run(&board, core, 0x2000, 1);
            assert_eq!(points.event(), None);
            board.instruction_points_end(core, &mut points).unwrap();
            board.memory_watch_end(&mut watch).unwrap();
        }
    }
}

#[test]
fn resume_suppression_does_not_skip_tcm_revalidation() {
    let (board, _) = board(&[0xe2800001, 0xeafffffe]);
    let mut points = points(&board, 0, CODE);
    let mut watch = watch(&board, InstructionCores::Both);
    run(&board, 0, CODE, 1);
    let stopped = points.event_context().unwrap();
    assert!(watch.fetch_event(stopped.core, stopped.entry.id).is_some());
    board.cpu(0).unwrap().coprocessor(arm::Coprocessor {
        cp: 15, opc1: 0, crn: 9, crm: 1, opc2: 1, secure: 0, wide: false,
    }, Some(CODE | 1)).unwrap();
    board.write_memory(0, CODE, &0xe2800001u32.to_le_bytes()).unwrap();
    points.resume(CODE, CODE);
    run(&board, 0, CODE, 1);
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 7);
    assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, InstructionGuardFailure::Tcm);
    assert!(watch.fetch_event(stopped.core, stopped.entry.id).is_none());
    board.instruction_points_end(0, &mut points).unwrap();
    board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn unknown_backing_and_tcm_are_refused_before_unprotected_prefix_filtering() {
    for tcm in [false, true] {
        let (board, _) = board(&[0xeafffffe]);
        if tcm {
            board.cpu(0).unwrap().coprocessor(arm::Coprocessor {
                cp: 15, opc1: 0, crn: 9, crm: 1, opc2: 1, secure: 0, wide: false,
            }, Some(1)).unwrap();
        } else {
            board.cpu(0).unwrap().unmap(0, 0x10000).unwrap();
            board.cpu(0).unwrap().map_zeroed(0, 0x10000, 7).unwrap();
        }
        board.write_memory(0, CODE - 12, &0xe2800001u32.to_le_bytes()).unwrap();
        let mut watch = watch(&board, InstructionCores::Both);
        run(&board, 0, CODE - 12, 1);
        assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 7);
        assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind,
            if tcm { InstructionGuardFailure::Tcm } else { InstructionGuardFailure::UnresolvedBacking });
        board.memory_watch_end(&mut watch).unwrap();
    }
}

#[test]
fn thumb_width_and_big_endian_are_refused_before_effects() {
    for (pstate, pc, bytes, failure) in [
        (0xf3, CODE | 1, vec![0x07, 0x20, 0xfe, 0xe7], InstructionGuardFailure::InvalidWidth),
        (0x2d3, CODE, 0xe2800001u32.to_le_bytes().to_vec(), InstructionGuardFailure::ByteOrder),
    ] {
        let (board, _) = board(&[0xeafffffe]);
        board.write_physical(CODE, &bytes).unwrap();
        board.write_register(0, Register::Pstate, pstate).unwrap();
        let mut watch = watch(&board, InstructionCores::Both);
        run(&board, 0, pc, 1);
        assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 7);
        assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, failure);
        board.memory_watch_end(&mut watch).unwrap();
    }
}

#[test]
fn committed_and_restored_decoder_views_latch_refusal_before_any_next_instruction() {
    let (mut board, input) = board(&[0xe2800001, 0xeafffffe]);
    let mut watch = watch(&board, InstructionCores::Both);
    board.device_input::<DecodeInput, _>(input, |_, cx| cx.select_memory(0, 1)).unwrap();
    board.run_for_instructions(1, 1).unwrap();
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 7);
    let failure = watch.instruction_guard_status().unwrap().first_failure.unwrap();
    assert_eq!(failure.kind, InstructionGuardFailure::ViewChanged);
    board.device_input::<DecodeInput, _>(input, |_, cx| cx.select_memory(0, 0)).unwrap();
    board.run_for_instructions(1, 1).unwrap();
    run(&board, 0, CODE, 1);
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 7);
    assert_eq!(watch.instruction_guard_status().unwrap().first_failure, Some(failure));
    board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn accepted_non_data_fetches_keep_constant_retained_rows_and_shared_lookup_ownership() {
    let (board, _) = board(&[0xeafffffe]);
    let mut watch = watch(&board, InstructionCores::Both);
    run(&board, 0, CODE, 10000);
    run(&board, 1, CODE, 10000);
    let status = watch.instruction_guard_status().unwrap();
    assert!(status.accepted_entries >= 20000);
    assert!(status.first_failure.is_none());
    let records = watch.records.borrow();
    assert_eq!(records.row_count(), 0);
    assert!(records.rows_since(0).unwrap().is_empty());
    assert!(records.rows_since(1).is_err());
    assert!(records.violation().is_none());
    drop(records);
    board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn non_fixed_guard_installation_refuses_without_changing_guest_state() {
    let (board, _) = board(&[0xe2800001]);
    let protected = range(CODE, CODE + 4);
    let attribution = FetchAttribution::new(CpuDataAttribution::Writes, vec![protected]).unwrap()
        .with_instruction_guards(vec![PhysicalInstructionGuard::arm32_little(
            protected, GuestAddress(CODE), InstructionCores::Both,
        ).unwrap()]).unwrap();
    assert!(board.memory_watch_begin_with_fetch_attribution(vec![
        WatchRange { start: DATA, end: DATA + 4, kinds: WatchKinds::CPU_WRITE },
    ], attribution, 4, board.mappings().to_vec(), ViewPolicy::Observe).is_err());
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 7);
    assert_eq!(word(&board, DATA), 0);
}

#[test]
fn a_straddling_thumb_word_is_refused_before_link_register_effects() {
    let (board, _) = board(&[0xeafffffe]);
    board.write_physical(CODE + 2, &[0x00, 0xf0, 0x00, 0xf8]).unwrap();
    board.write_register(0, Register::Pstate, 0xf3).unwrap();
    board.write_register(0, arm::CORE[14], 0x1234).unwrap();
    let protected = range(CODE + 4, CODE + 8);
    let attribution = FetchAttribution::new(CpuDataAttribution::Writes, vec![protected]).unwrap()
        .with_instruction_guards(vec![PhysicalInstructionGuard::arm32_little(
            protected, GuestAddress(CODE + 4), InstructionCores::Both,
        ).unwrap()]).unwrap();
    let mut watch = board.memory_watch_begin_with_fetch_attribution(vec![
        WatchRange { start: DATA, end: DATA + 4, kinds: WatchKinds::CPU_WRITE },
    ], attribution, 4, board.mappings().to_vec(), ViewPolicy::Fixed).unwrap();
    run(&board, 0, CODE + 3, 1);
    assert_eq!(board.read_register(0, arm::CORE[14]).unwrap(), 0x1234);
    assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, InstructionGuardFailure::Straddling);
    board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn stale_supplied_view_is_refused_against_the_actual_instruction_backing() {
    let (board, _) = board(&[0xe2800001]);
    let protected = range(0x2000, 0x2004);
    let attribution = FetchAttribution::new(CpuDataAttribution::Writes, vec![protected]).unwrap()
        .with_instruction_guards(vec![PhysicalInstructionGuard::arm32_little(
            protected, GuestAddress(CODE), InstructionCores::Both,
        ).unwrap()]).unwrap();
    let maps = (0..2).map(|core| MemoryMap {
        core, address: CODE, physical: 0x2000, size: 0x1000, permissions: 7,
    }).collect();
    let mut watch = board.memory_watch_begin_with_fetch_attribution(vec![
        WatchRange { start: DATA, end: DATA + 4, kinds: WatchKinds::CPU_WRITE },
    ], attribution, 4, maps, ViewPolicy::Fixed).unwrap();
    run(&board, 0, CODE, 1);
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 7);
    assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, InstructionGuardFailure::BackingChanged);
    board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn healthy_guard_leaves_unprotected_execution_without_entry_metadata() {
    let (board, _) = board(&[0xe2800001, 0xeafffffd]);
    let mut watch = watch_with_sources(&board, InstructionCores::Both, CpuDataAttribution::None);
    for core in 0..2 {
        run(&board, core, 0x2000, 10000);
        assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 5007);
        assert!(board.cpu(core).unwrap().instruction_entry().is_none());
    }
    assert_eq!(watch.instruction_guard_status().unwrap().hook_entries, 0);
    assert_eq!(watch.records.borrow().row_count(), 0);
    run(&board, 0, ALIAS, 1);
    assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, InstructionGuardFailure::Alias);
    run(&board, 1, 0x2000, 2);
    assert_eq!(board.read_register(1, arm::CORE[0]).unwrap(), 5007);
    board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn cache_only_mode_keeps_ordinary_writes_without_fetch_provenance_fields() {
    let (board, _) = board(&[0xe5810000, 0xeafffffe]);
    let mut watch = watch_with_sources(&board, InstructionCores::Both, CpuDataAttribution::None);
    run(&board, 0, 0x2000, 1);
    assert_eq!(word(&board, DATA), 7);
    let records = watch.records.borrow();
    assert!(records.violation().is_none());
    let rows = records.rows_since(0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["pc"].as_u64(), Some(0x2000));
    assert!(rows[0].get("core_instructions").is_none());
    assert!(rows[0].get("latest_fetch").is_none());
    drop(records);
    assert!(watch.instruction_guard_status().unwrap().first_failure.is_none());
    board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn a_point_without_its_exact_fetch_cannot_reuse_an_earlier_guarded_token() {
    let (board, _) = board(&[0xe2800001, 0xeafffffe]);
    let mut watch = watch_with_sources(&board, InstructionCores::Both, CpuDataAttribution::None);
    let mut guarded = points(&board, 0, CODE);
    run(&board, 0, CODE, 1);
    let earlier = guarded.event_context().unwrap();
    assert!(watch.fetch_event(earlier.core, earlier.entry.id).is_some());
    let mut missing = points(&board, 0, 0x2000);
    run(&board, 0, 0x2000, 1);
    let later = missing.event_context().unwrap();
    assert_ne!(earlier.entry.id, later.entry.id);
    assert!(watch.fetch_event(later.core, later.entry.id).is_none());
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 7);
    board.instruction_points_end(0, &mut missing).unwrap();
    board.instruction_points_end(0, &mut guarded).unwrap();
    board.memory_watch_end(&mut watch).unwrap();
}

#[cfg(not(target_os = "emscripten"))]
#[test]
fn intrinsic_shared_worker_gate_adds_no_unprotected_instruction_callbacks() {
    use std::cell::Cell;
    struct RefuseEntry {
        gate: ExecutionRefusal,
        observed: Cell<Option<fujimod_emulator_cpu::InstructionEntry>>,
        cpu: *const dyn Cpu,
    }
    unsafe fn refuse(_: u64, _: u32, data: *mut std::ffi::c_void) {
        let entry = unsafe { &*data.cast::<RefuseEntry>() };
        entry.observed.set(unsafe { &*entry.cpu }.instruction_entry());
        entry.gate.refuse();
        unsafe { &*entry.cpu }.stop_before_instruction().unwrap();
    }
    let memory = rprofile::memory::SharedMemory::default();
    let cpus: Vec<_> = (0..2).map(|_| rprofile::worker::spawn(rprofile::worker::Config {
        translation_cache_bytes: 1 << 20, options: Default::default(), memory: memory.clone(),
    }).unwrap()).collect();
    let gate = ExecutionRefusal::default();
    for cpu in &cpus {
        cpu.map_zeroed(0x1000, 0x2000, 7).unwrap();
        cpu.write_memory(0x1000, &0xe2800001u32.to_le_bytes()).unwrap();
        cpu.write_memory(0x2000, &[0x01,0x00,0x80,0xe2,0xfd,0xff,0xff,0xea]).unwrap();
        cpu.write_register(Register::Pstate, &0xd3u32.to_le_bytes()).unwrap();
        cpu.write_register(arm::CORE[0], &7u32.to_le_bytes()).unwrap();
        cpu.arm_execution_refusal(gate.clone()).unwrap();
    }
    let observed = RefuseEntry { gate: gate.clone(), observed: Cell::new(None), cpu: &cpus[0] };
    let hook = unsafe { cpus[0].install_hook(Hook::Instruction {
        callback: refuse, data: (&observed as *const RefuseEntry).cast_mut().cast(),
        range: HookRange::Inclusive { begin: 0x1000, end: 0x1000 },
    }) }.unwrap();
    for cpu in &cpus {
        let before = cpu.exit_counters().unwrap();
        cpu.run(Run { begin: 0x2000, until: None, instructions: Some(10000), watchdog_us: 0 }).unwrap();
        let after = cpu.exit_counters().unwrap();
        assert_eq!(after["worker_callback_instruction"], before["worker_callback_instruction"]);
        assert!(cpu.instruction_entry().is_none());
    }
    assert!(observed.observed.get().is_none());
    cpus[0].run(Run { begin: 0x1000, until: None, instructions: Some(1), watchdog_us: 0 }).unwrap();
    assert!(observed.observed.get().is_some());
    for cpu in &cpus {
        cpu.run(Run { begin: 0x2000, until: None, instructions: Some(2), watchdog_us: 0 }).unwrap();
        let mut value = [0; 4];
        cpu.read_register(arm::CORE[0], &mut value).unwrap();
        assert_eq!(u32::from_le_bytes(value), 5007);
    }
    cpus[0].remove_hook(hook).unwrap();
    for cpu in &cpus { cpu.disarm_execution_refusal(&gate).unwrap(); }
}

#[cfg(not(target_os = "emscripten"))]
#[test]
fn an_interactive_midblock_refusal_stops_the_next_unobserved_instruction() {
    unsafe fn read(_: u64, _: u32, data: *mut std::ffi::c_void) -> u64 {
        unsafe { &*data.cast::<ExecutionRefusal>() }.refuse();
        0
    }
    unsafe fn write(_: u64, _: u32, _: u64, _: *mut std::ffi::c_void) {}
    let cpu = rprofile::worker::spawn(rprofile::worker::Config {
        translation_cache_bytes: 1 << 20, options: Default::default(),
        memory: rprofile::memory::SharedMemory::default(),
    }).unwrap();
    cpu.map_zeroed(0x2000, 0x1000, 7).unwrap();
    let code: Vec<_> = [0xe5921000u32, 0xe2800001, 0xeafffffe]
        .into_iter().flat_map(u32::to_le_bytes).collect();
    cpu.write_memory(0x2000, &code).unwrap();
    cpu.write_register(Register::Pstate, &0xd3u32.to_le_bytes()).unwrap();
    cpu.write_register(arm::CORE[0], &7u32.to_le_bytes()).unwrap();
    cpu.write_register(arm::CORE[2], &0x4000u32.to_le_bytes()).unwrap();
    cpu.set_execution_policy(ExecutionPolicy::Interactive).unwrap();
    let gate = ExecutionRefusal::default();
    unsafe { cpu.map_mmio(0x4000, 0x1000, fujimod_emulator_cpu::Mmio {
        read, write, data: (&gate as *const ExecutionRefusal).cast_mut().cast(),
    }) }.unwrap();
    cpu.arm_execution_refusal(gate.clone()).unwrap();
    cpu.run(Run { begin: 0x2000, until: None, instructions: Some(100), watchdog_us: 0 }).unwrap();
    let mut value = [0; 4];
    cpu.read_register(arm::CORE[0], &mut value).unwrap();
    assert!(gate.is_refused());
    assert_eq!(u32::from_le_bytes(value), 7);
    assert_eq!(cpu.exit_counters().unwrap()["worker_callback_instruction"], 0);
    cpu.disarm_execution_refusal(&gate).unwrap();
    cpu.unmap(0x4000, 0x1000).unwrap();
}

#[test]
fn an_observed_branch_cannot_supply_fetch_provenance_for_a_later_unobserved_store() {
    let (board, _) = board(&[0xea0003fe]);
    let stores: Vec<_> = [0xe5810000u32, 0xe2800001, 0xeafffffe]
        .into_iter().flat_map(u32::to_le_bytes).collect();
    board.write_physical(0x2000, &stores).unwrap();
    let mut watch = watch(&board, InstructionCores::Both);
    run(&board, 0, CODE, 8);
    let records = watch.records.borrow();
    let rows = records.rows_since(0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["pc"].as_u64(), Some(0x2000));
    assert_eq!(rows[0]["core_instructions"].as_u64(), Some(2));
    assert!(rows[0]["latest_fetch"].is_null());
    assert!(records.violation().is_some());
    drop(records);
    assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, InstructionGuardFailure::ObservationFailed);
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 7);
    board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn an_initial_unobserved_store_retains_its_row_before_missing_fetch_refusal() {
    let (board, _) = board(&[0xe5810000, 0xe2800001, 0xeafffffe]);
    let mut watch = watch(&board, InstructionCores::Both);
    run(&board, 0, 0x2000, 8);
    let records = watch.records.borrow();
    let rows = records.rows_since(0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["pc"].as_u64(), Some(0x2000));
    assert_eq!(rows[0]["core_instructions"].as_u64(), Some(1));
    assert!(rows[0]["latest_fetch"].is_null());
    assert!(records.violation().is_some());
    drop(records);
    assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, InstructionGuardFailure::ObservationFailed);
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 7);
    board.memory_watch_end(&mut watch).unwrap();
}

fn isolated_core() -> rprofile::core::Core {
    let raw = fujimod_emulator_cpu_unicorn::Library::linked().unwrap()
        .open(Model::CortexR5F, 1 << 20).unwrap();
    let cpu = rprofile::core::Core::new(raw, Default::default(), rprofile::core::Wiring {
        memory: rprofile::memory::Memory::default(),
    }).unwrap();
    cpu.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    cpu.write_register(Register::Pstate, &0xd3u32.to_le_bytes()).unwrap();
    cpu.write_register(arm::CORE[0], &7u32.to_le_bytes()).unwrap();
    cpu
}

fn register_word(cpu: &dyn Cpu, register: Register) -> u32 {
    let mut bytes = [0; 4];
    cpu.read_register(register, &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn an_exact_first_entry_intrinsic_refusal_counts_no_admitted_block_tail() {
    unsafe fn refuse(_: fujimod_emulator_cpu::Block, data: *mut std::ffi::c_void) {
        unsafe { &*data.cast::<ExecutionRefusal>() }.refuse();
    }
    let cpu = isolated_core();
    cpu.map_zeroed(CODE, 0x1000, 7).unwrap();
    let code: Vec<_> = [0xe2800001u32, 0xe2800002, 0xe2800004, 0xeafffffe]
        .into_iter().flat_map(u32::to_le_bytes).collect();
    cpu.write_memory(CODE, &code).unwrap();
    let gate = ExecutionRefusal::default();
    cpu.arm_execution_refusal(gate.clone()).unwrap();
    let hook = unsafe { cpu.install_hook(Hook::Block {
        callback: refuse, data: (&gate as *const ExecutionRefusal).cast_mut().cast(),
        range: HookRange::Inclusive { begin: CODE, end: CODE },
    }) }.unwrap();
    cpu.run(Run { begin: CODE, until: None, instructions: Some(32), watchdog_us: 0 }).unwrap();
    assert_eq!(cpu.instruction_progress(), Some(0));
    assert_eq!(register_word(&cpu, arm::CORE[0]), 7);
    assert_eq!(register_word(&cpu, Register::Pc), CODE as u32);
    assert!(cpu.instruction_entry().is_none());
    cpu.remove_hook(hook).unwrap();
    cpu.disarm_execution_refusal(&gate).unwrap();
    cpu.run(Run { begin: CODE, until: None, instructions: Some(1), watchdog_us: 0 }).unwrap();
    assert!(gate.is_refused());
    assert_eq!(cpu.instruction_progress(), Some(1));
    assert_eq!(register_word(&cpu, arm::CORE[0]), 8);
}

#[test]
fn an_exact_shared_core_midblock_refusal_counts_only_the_entered_prefix() {
    struct PeerRefusal {
        gate: ExecutionRefusal,
        peer: *const dyn Cpu,
    }
    unsafe fn refuse(_: u64, _: u32, data: *mut std::ffi::c_void) {
        let refusal = unsafe { &*data.cast::<PeerRefusal>() };
        refusal.gate.refuse();
        unsafe { &*refusal.peer }.stop_before_instruction().unwrap();
    }
    unsafe fn read(_: u64, _: u32, data: *mut std::ffi::c_void) -> u64 {
        let refusal = unsafe { &*data.cast::<PeerRefusal>() };
        unsafe { &*refusal.peer }.run(Run {
            begin: 0x5000, until: None, instructions: Some(1), watchdog_us: 0,
        }).unwrap();
        42
    }
    unsafe fn write(_: u64, _: u32, _: u64, _: *mut std::ffi::c_void) {}
    let cpu = isolated_core();
    let peer = isolated_core();
    cpu.map_zeroed(0x2000, 0x1000, 7).unwrap();
    peer.map_zeroed(0x5000, 0x1000, 7).unwrap();
    let code: Vec<_> = [0xe2800001u32, 0xe5921000, 0xe2800004, 0xeafffffe]
        .into_iter().flat_map(u32::to_le_bytes).collect();
    cpu.write_memory(0x2000, &code).unwrap();
    cpu.write_register(arm::CORE[2], &0x4000u32.to_le_bytes()).unwrap();
    peer.write_memory(0x5000, &0xe2800001u32.to_le_bytes()).unwrap();
    let gate = ExecutionRefusal::default();
    let refusal = PeerRefusal { gate: gate.clone(), peer: &peer };
    let data = (&refusal as *const PeerRefusal).cast_mut().cast();
    let hook = unsafe { peer.install_hook(Hook::Instruction {
        callback: refuse, data, range: HookRange::Inclusive { begin: 0x5000, end: 0x5000 },
    }) }.unwrap();
    unsafe { cpu.map_mmio(0x4000, 0x1000, fujimod_emulator_cpu::Mmio { read, write, data }) }.unwrap();
    cpu.arm_execution_refusal(gate.clone()).unwrap();
    peer.arm_execution_refusal(gate.clone()).unwrap();
    cpu.run(Run { begin: 0x2000, until: None, instructions: Some(32), watchdog_us: 0 }).unwrap();
    assert_eq!(cpu.instruction_progress(), Some(2));
    assert_eq!(register_word(&cpu, arm::CORE[0]), 8);
    assert_eq!(register_word(&cpu, arm::CORE[1]), 42);
    assert_eq!(register_word(&cpu, Register::Pc), 0x2008);
    assert_eq!(peer.instruction_progress(), Some(0));
    assert_eq!(register_word(&peer, arm::CORE[0]), 7);
    peer.remove_hook(hook).unwrap();
    cpu.disarm_execution_refusal(&gate).unwrap();
    peer.disarm_execution_refusal(&gate).unwrap();
    cpu.unmap(0x4000, 0x1000).unwrap();
}

#[test]
fn dropping_an_unended_guard_refuses_both_cores_even_while_records_are_borrowed() {
    let (board, _) = board(&[0xe2800001, 0xe2800004, 0xeafffffe]);
    let watch = watch(&board, InstructionCores::Both);
    let records = watch.records.clone();
    let inspected = records.borrow();
    drop(watch);
    for core in 0..2 {
        run(&board, core, CODE, 2);
        run(&board, core, 0x2000, 2);
        assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 7);
        assert_eq!(board.cpu(core).unwrap().instruction_progress(), Some(0));
    }
    assert_eq!(inspected.row_count(), 0);
}

#[test]
fn dropping_a_guard_after_successful_end_leaves_execution_available() {
    let (board, _) = board(&[0xe2800001, 0xeafffffe]);
    let mut watch = watch(&board, InstructionCores::Both);
    board.memory_watch_end(&mut watch).unwrap();
    drop(watch);
    for core in 0..2 {
        run(&board, core, 0x2000, 1);
        assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 8);
    }
}

#[test]
fn a_hook_removal_failure_followed_by_drop_keeps_both_cores_refused() {
    use std::cell::{Cell, RefCell};
    struct EndDuringEntry {
        board: *const Board,
        watch: RefCell<Option<Box<MemoryWatch>>>,
        refused: Cell<bool>,
    }
    unsafe fn end(_: u64, _: u32, data: *mut std::ffi::c_void) {
        let context = unsafe { &*data.cast::<EndDuringEntry>() };
        let mut watch = context.watch.borrow_mut().take().unwrap();
        context.refused.set(unsafe { &*context.board }.memory_watch_end(&mut watch).is_err());
        drop(watch);
    }
    let (board, _) = board(&[0xe2800001, 0xe2800004, 0xeafffffe]);
    let context = EndDuringEntry {
        board: &board, watch: RefCell::new(Some(watch(&board, InstructionCores::Both))),
        refused: Cell::new(false),
    };
    let hook = unsafe { board.cpu(0).unwrap().install_hook(Hook::Instruction {
        callback: end, data: (&context as *const EndDuringEntry).cast_mut().cast(),
        range: HookRange::Inclusive { begin: 0x2000, end: 0x2000 },
    }) }.unwrap();
    run(&board, 0, 0x2000, 8);
    assert!(context.refused.get());
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 8);
    assert_eq!(board.cpu(0).unwrap().instruction_progress(), Some(1));
    run(&board, 0, CODE, 2);
    run(&board, 1, 0x2000, 2);
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 8);
    assert_eq!(board.read_register(1, arm::CORE[0]).unwrap(), 7);
    assert_eq!(board.cpu(0).unwrap().instruction_progress(), Some(0));
    assert_eq!(board.cpu(1).unwrap().instruction_progress(), Some(0));
    board.cpu(0).unwrap().remove_hook(hook).unwrap();
}

#[derive(Clone, Copy)]
struct RefusedTaskExecution {
    cpsr: Cpsr,
    begin: GuestAddress,
    instruction: [u8; 4],
    failure: InstructionGuardFailure,
}

fn refused_task_execution_states() -> [RefusedTaskExecution; 4] {
    let add = 0xe2800001u32.to_le_bytes();
    [
        RefusedTaskExecution { cpsr: Cpsr(0x93), begin: GuestAddress(CODE), instruction: add, failure: InstructionGuardFailure::ExecutionState },
        RefusedTaskExecution { cpsr: Cpsr(0x11), begin: GuestAddress(CODE), instruction: add, failure: InstructionGuardFailure::ExecutionState },
        RefusedTaskExecution { cpsr: Cpsr(0x33), begin: GuestAddress(CODE | 1), instruction: [0x09,0x20,0xfe,0xe7], failure: InstructionGuardFailure::InvalidWidth },
        RefusedTaskExecution { cpsr: Cpsr(0x213), begin: GuestAddress(CODE), instruction: add, failure: InstructionGuardFailure::ByteOrder },
    ]
}

#[test]
fn owned_execution_state_refuses_masked_svc_fiq_thumb_and_big_endian_before_effects() {
    for core in 0..2 {
        for case in refused_task_execution_states() {
            let (board, _) = board(&[0xe2800001, 0xeafffffe]);
            board.write_physical(CODE, &case.instruction).unwrap();
            board.write_register(core, Register::Pstate, u64::from(case.cpsr.0)).unwrap();
            let mut watch = watch_with_policy(
                &board, InstructionCores::Both, CpuDataAttribution::None, InstructionExecutionState::UnmaskedSvcA32Little,
            );
            run(&board, core, case.begin.0, 1);
            assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 7);
            assert_eq!(board.cpu(core).unwrap().instruction_progress(), Some(0));
            assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, case.failure);
            assert_eq!(watch.records.borrow().row_count(), 0);
            board.memory_watch_end(&mut watch).unwrap();
        }
    }
}

#[test]
fn owned_execution_state_admits_nzcv_and_f_bit_variants() {
    for core in 0..2 {
        for cpsr in [Cpsr(0x13), Cpsr(0x53), Cpsr(0xa0000013), Cpsr(0xf0000053)] {
            let (board, _) = board(&[0xe2800001, 0xe5810000, 0xeafffffe]);
            board.write_register(core, Register::Pstate, u64::from(cpsr.0)).unwrap();
            let mut watch = watch_with_policy(
                &board, InstructionCores::Both, CpuDataAttribution::None, InstructionExecutionState::UnmaskedSvcA32Little,
            );
            run(&board, core, CODE, 2);
            assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 8);
            assert_eq!(word(&board, DATA), 8);
            assert!(watch.instruction_guard_status().unwrap().first_failure.is_none());
            board.memory_watch_end(&mut watch).unwrap();
        }
    }
}

#[test]
fn owned_execution_state_is_revalidated_on_the_resume_entry_skipped_by_points() {
    for core in 0..2 {
        for order in [ObserverOrder::FetchFirst, ObserverOrder::PointFirst] {
            for case in refused_task_execution_states() {
                let (board, _) = board(&[0xe2800001, 0xeafffffe]);
                board.write_register(core, Register::Pstate, 0x13).unwrap();
                let (mut watch, mut points) = match order {
                    ObserverOrder::FetchFirst => {
                        let watch = watch_with_policy(
                            &board, InstructionCores::Both, CpuDataAttribution::None, InstructionExecutionState::UnmaskedSvcA32Little,
                        );
                        (watch, points(&board, core, CODE))
                    }
                    ObserverOrder::PointFirst => {
                        let points = points(&board, core, CODE);
                        (watch_with_policy(
                            &board, InstructionCores::Both, CpuDataAttribution::None, InstructionExecutionState::UnmaskedSvcA32Little,
                        ), points)
                    }
                };
                run(&board, core, CODE, 1);
                assert_eq!(points.event(), Some(CODE));
                let stopped = points.event_context().unwrap();
                assert!(watch.fetch_event(stopped.core, stopped.entry.id).is_some());
                points.resume(CODE, CODE);
                board.write_register(core, Register::Pstate, u64::from(case.cpsr.0)).unwrap();
                run(&board, core, case.begin.0, 1);
                assert_eq!(points.event(), None);
                assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 7);
                assert_eq!(board.read_register(core, arm::CORE[1]).unwrap(), DATA);
                assert_eq!(board.cpu(core).unwrap().instruction_progress(), Some(0));
                assert_eq!(watch.instruction_guard_status().unwrap().first_failure.unwrap().kind, case.failure);
                assert!(watch.fetch_event(stopped.core, stopped.entry.id).is_none());
                board.instruction_points_end(core, &mut points).unwrap();
                board.memory_watch_end(&mut watch).unwrap();
            }
        }
    }
}

#[test]
fn owned_resume_admits_nzcv_and_f_bit_changes() {
    let (board, _) = board(&[0xe2800001, 0xeafffffe]);
    board.write_register(0, Register::Pstate, 0x13).unwrap();
    let mut points = points(&board, 0, CODE);
    let mut watch = watch_with_policy(
        &board, InstructionCores::Both, CpuDataAttribution::None, InstructionExecutionState::UnmaskedSvcA32Little,
    );
    run(&board, 0, CODE, 1);
    assert_eq!(points.event(), Some(CODE));
    points.resume(CODE, CODE);
    board.write_register(0, Register::Pstate, 0xa0000053).unwrap();
    run(&board, 0, CODE, 1);
    assert_eq!(points.event(), None);
    assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 8);
    assert_eq!(board.cpu(0).unwrap().instruction_progress(), Some(1));
    assert!(watch.instruction_guard_status().unwrap().first_failure.is_none());
    board.instruction_points_end(0, &mut points).unwrap();
    board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn default_execution_state_still_admits_masked_svc_and_fiq() {
    for cpsr in [Cpsr(0xd3), Cpsr(0x11)] {
        let (board, _) = board(&[0xe2800001, 0xeafffffe]);
        board.write_register(0, Register::Pstate, u64::from(cpsr.0)).unwrap();
        let mut watch = watch_with_sources(&board, InstructionCores::Both, CpuDataAttribution::None);
        run(&board, 0, CODE, 1);
        assert_eq!(board.read_register(0, arm::CORE[0]).unwrap(), 8);
        assert!(watch.instruction_guard_status().unwrap().first_failure.is_none());
        board.memory_watch_end(&mut watch).unwrap();
    }
}

#[test]
fn refused_board_runs_return_without_satisfying_or_retrying_the_instruction_budget() {
    for policy in [ExecutionPolicy::Exact, ExecutionPolicy::Interactive] {
        let (mut board, _) = board(&[0xe2800001, 0xeafffffe]);
        board.set_execution_policy(policy).unwrap();
        let gate = ExecutionRefusal::default();
        for core in 0..2 {
            board.cpu(core).unwrap().arm_execution_refusal(gate.clone()).unwrap();
        }
        gate.refuse();
        for _ in 0..2 {
            board.run_for_instructions(2, 1).unwrap();
            assert_eq!(board.instruction_counts(), vec![0, 0]);
            for core in 0..2 {
                assert!(board.cpu(core).unwrap().execution_refused());
                assert_eq!(board.read_register(core, arm::CORE[0]).unwrap(), 7);
            }
        }
        for core in 0..2 {
            board.cpu(core).unwrap().disarm_execution_refusal(&gate).unwrap();
            assert!(!board.cpu(core).unwrap().execution_refused());
        }
        board.run_for_instructions(2, 1).unwrap();
        assert!(board.instruction_counts().iter().any(|count| *count > 0));
    }
}

struct LifetimeBytes(std::collections::BTreeMap<u32, u8>);
impl LifetimeMemory for LifetimeBytes {
    fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), LifetimeFailure> {
        for (index, byte) in output.iter_mut().enumerate() {
            *byte = self.0.get(&(address.0 + index as u32)).copied().unwrap_or(0);
        }
        Ok(())
    }
}

fn prepared_retired_monitor() -> LifetimeMonitor {
    const INSTALL: u32 = 0x0100_0000;
    let words = [0xe92d_4010u32, 0xe581_3000, 0xe92d_4010, 0xe581_3000];
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    let publications: Vec<_> = (0..223u32).map(|index| serde_json::json!({
        "address": 0x0120_0000 + index * 4, "old": 0, "new": index + 1,
    })).collect();
    let request: LifetimeRequest = serde_json::from_value(serde_json::json!({
        "runtime": [INSTALL, INSTALL + 0x10000], "resident": [INSTALL + 0x8000, INSTALL + 0x8400],
        "readonly": [{"start": INSTALL, "bytes": bytes}],
        "install": {"entry": {"pc": INSTALL, "word": words[0]}, "writer": {"pc": INSTALL + 4, "word": words[1]}},
        "rollback": {"entry": {"pc": INSTALL + 8, "word": words[2]}, "writer": {"pc": INSTALL + 12, "word": words[3]}},
        "publications": publications,
        "context_catalogue": "stock",
        "retired": [[CODE as u32, CODE as u32 + 8]],
    })).unwrap();
    let config = request.into_config(vec![
        PhysicalExtent::new(Guest(0x40000), 0xdae0).unwrap(),
        PhysicalExtent::new(Guest(0xd0000), 0xdaa0).unwrap(),
    ]).unwrap();
    let mut memory = LifetimeBytes(Default::default());
    for (extent, image) in config.readonly_images() {
        memory.0.extend(image.iter().enumerate().map(|(index, byte)| (extent.start().0 + index as u32, *byte)));
    }
    LifetimeMonitor::new(config, &memory).unwrap()
}

fn retired_monitor() -> LifetimeMonitor {
    let mut monitor=prepared_retired_monitor();
    monitor.observe_entry(CoreId(0),Guest(0x0100_0000),Word(0xe92d_4010)).unwrap();
    monitor
}

#[test]
fn retired_lifetime_extents_refuse_canonical_and_aliased_fetches_on_both_cores() {
    #[derive(Clone,Copy)]
    enum Stage { BeforeInstall, Publishing }
    for (core, pc, sources, stage) in [(0, CODE), (1, CODE), (0, ALIAS), (1, ALIAS), (0, DECODER), (1, DECODER)]
        .into_iter().flat_map(|(core,pc)| [CpuDataAttribution::None,CpuDataAttribution::Writes].map(move |sources| (core,pc,sources)))
        .flat_map(|(core,pc,sources)| [Stage::BeforeInstall,Stage::Publishing].map(move |stage| (core,pc,sources,stage))) {
        let (board, _) = board(&[0xe2800001, 0xeafffffe]);
        let protected = range(0x2000, 0x2004);
        let attribution = FetchAttribution::new(sources, vec![range(CODE,CODE+8),protected]).unwrap()
            .with_instruction_guards(vec![PhysicalInstructionGuard::arm32_little(
                protected, GuestAddress(0x2000), InstructionCores::Both,
            ).unwrap()]).unwrap();
        let mut watch = board.memory_watch_begin_with_fetch_attribution(vec![
            WatchRange { start: DATA, end: DATA + 8, kinds: WatchKinds::WRITES },
            range(CODE, CODE + 8),
        ], attribution, 16, board.mappings().to_vec(), ViewPolicy::Fixed).unwrap();
        run(&board, core, pc, 1);
        let records = watch.records.borrow();
        let rows = records.rows_since(0).unwrap();
        let fetch = rows.iter().find(|row| row["source"] == "cpu-fetch").expect("a retired fetch row");
        assert_eq!(fetch["physical"].as_u64(), Some(CODE));
        assert_eq!(fetch["core"].as_u64(), Some(core as u64));
        let mut monitor = match stage {
            Stage::BeforeInstall => prepared_retired_monitor(),
            Stage::Publishing => retired_monitor(),
        };
        let result = match sources {
            CpuDataAttribution::None => monitor.observe_row(fetch),
            _ => monitor.observe_attributed_row(fetch,board.mappings()),
        };
        let expected = match stage {
            Stage::BeforeInstall => Ok(()),
            Stage::Publishing => Err(LifetimeFailure::RetiredFetch),
        };
        assert_eq!(result,expected);
        assert_eq!(monitor.receipt().failure,expected.err());
        drop(records);
        board.memory_watch_end(&mut watch).unwrap();
    }
}

#[test]
fn fetch_rows_outside_retired_extents_do_not_disappear() {
    let mut monitor = retired_monitor();
    let row = serde_json::json!({"source": "cpu-fetch", "address": 0x2000, "physical": 0x2000, "size": 4, "core": 0, "pc": 0x2000, "instructions": 1});
    assert_eq!(monitor.observe_row(&row), Err(LifetimeFailure::MalformedRow));
}

const RELOAD_CALL: u64 = 0x0063_09ec;
const RELOAD_RETURN: u64 = 0x0063_09f0;
const COPY_STORE: u64 = 0x0072_2134;
const LIVE_SETTINGS: u64 = 0x0096_cc64;
const RELOAD_TARGET: u64 = 0x0096_cc74;

/// Both cores start at the reload call with r5 holding the halfword the copy store writes to the reload target in r12.
fn reload_board() -> Board {
    const BYTES: u64 = 0x0100_0000;
    let graph = Graph {
        processors: (0..2).map(|_| Processor {
            model: Model::CortexR5F, mprofile: None, released: true,
            rprofile: Some(rprofile::core::Options::default()),
            registers: vec![(Register::Pc, RELOAD_CALL), (Register::Pstate, 0xd3), (arm::CORE[5], 0xa9c9), (arm::CORE[12], RELOAD_TARGET)],
        }).collect(),
        ram: vec![Ram { base: 0, size: BYTES as usize }],
        mappings: (0..2).map(|core| MemoryMap { core, address: 0, physical: 0, size: BYTES as usize, permissions: 7 }).collect(),
        dma_mappings: vec![BusMapping::ram(0, BYTES as usize)],
        decoders: vec![],
        peripherals: vec![],
        instructions_per_tick: Ratio::new(1, 1).unwrap(),
    };
    let mut board = graph.build(factory::Options {
        engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20,
    }, Bus::new(Mode::Strict), None).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    // bl COPY_STORE; b . at the return; strh r5,[r12],#2; bx lr at the copy store.
    board.write_physical(RELOAD_CALL, &[0xeb03_c5d0u32, 0xeaff_fffe].map(u32::to_le_bytes).concat()).unwrap();
    board.write_physical(COPY_STORE, &[0xe0cc_50b2u32, 0xe12f_ff1e].map(u32::to_le_bytes).concat()).unwrap();
    board
}

fn reload_monitor() -> LifetimeMonitor {
    const INSTALL: u32 = 0x0100_0000;
    let words = [0xe92d_4010u32, 0xe581_3000, 0xe92d_4010, 0xe581_3000];
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    let publications: Vec<_> = (0..223u32).map(|index| serde_json::json!({
        "address": 0x0120_0000 + index * 4, "old": 0, "new": index + 1,
    })).collect();
    let request: LifetimeRequest = serde_json::from_value(serde_json::json!({
        "runtime": [INSTALL, INSTALL + 0x10000], "resident": [INSTALL + 0x8000, INSTALL + 0x8400],
        "readonly": [{"start": INSTALL, "bytes": bytes}],
        "install": {"entry": {"pc": INSTALL, "word": words[0]}, "writer": {"pc": INSTALL + 4, "word": words[1]}},
        "rollback": {"entry": {"pc": INSTALL + 8, "word": words[2]}, "writer": {"pc": INSTALL + 12, "word": words[3]}},
        "publications": publications,
        "context_catalogue": "stock",
        "settings_writers": [[0x0062_4694, 0x0063_09c4]],
        "settings_reloads": [{"call": RELOAD_CALL, "return": RELOAD_RETURN, "copy_store": COPY_STORE, "target": [RELOAD_TARGET, 0x0096_cd8c]}],
    })).unwrap();
    let config = request.into_config(vec![
        PhysicalExtent::new(Guest(0x40000), 0xdae0).unwrap(),
        PhysicalExtent::new(Guest(0xd0000), 0xdaa0).unwrap(),
    ]).unwrap();
    let mut memory = LifetimeBytes(Default::default());
    for (extent, image) in config.readonly_images() {
        memory.0.extend(image.iter().enumerate().map(|(index, byte)| (extent.start().0 + index as u32, *byte)));
    }
    LifetimeMonitor::new(config, &memory).unwrap()
}

#[test]
fn executed_reload_call_brackets_its_copy_store_in_row_order_on_either_core() {
    for (core, begin, admitted) in [(0, RELOAD_CALL, true), (1, RELOAD_CALL, true), (0, COPY_STORE, false), (1, COPY_STORE, false)] {
        let board = reload_board();
        board.write_register(core, arm::CORE[14], RELOAD_RETURN).unwrap();
        let protected = range(0x2000, 0x2004);
        let attribution = FetchAttribution::new(CpuDataAttribution::None, vec![protected]).unwrap()
            .with_instruction_guards(vec![PhysicalInstructionGuard::arm32_little(
                protected, GuestAddress(0x2000), InstructionCores::Both,
            ).unwrap()]).unwrap();
        let mut watch = board.memory_watch_begin_with_fetch_attribution(vec![
            WatchRange { start: LIVE_SETTINGS, end: LIVE_SETTINGS + 450, kinds: WatchKinds::WRITES },
            range(RELOAD_CALL, RELOAD_CALL + 4),
            range(RELOAD_RETURN, RELOAD_RETURN + 4),
        ], attribution, 16, board.mappings().to_vec(), ViewPolicy::Fixed).unwrap();
        run(&board, core, begin, if begin == RELOAD_CALL { 4 } else { 3 });
        let records = watch.records.borrow();
        let rows = records.rows_since(0).unwrap();
        let shape: Vec<_> = rows.iter().map(|row| (row["source"].as_str().unwrap().to_owned(), row["physical"].as_u64().unwrap(), row["core"].as_u64().unwrap())).collect();
        let store = ("cpu-write".to_owned(), RELOAD_TARGET, core as u64);
        let resume = ("cpu-fetch".to_owned(), RELOAD_RETURN, core as u64);
        let expected = if begin == RELOAD_CALL { vec![("cpu-fetch".to_owned(), RELOAD_CALL, core as u64), store, resume] } else { vec![store, resume] };
        assert_eq!(shape, expected, "core {core} from {begin:#x}");
        let mut monitor = reload_monitor();
        let result = rows.iter().try_for_each(|row| monitor.observe_row(row));
        assert_eq!(result, if admitted { Ok(()) } else { Err(LifetimeFailure::ResourceChanged) }, "core {core} from {begin:#x}");
        assert_eq!(monitor.receipt().admitted_reload_writes, u64::from(admitted));
        drop(records);
        board.memory_watch_end(&mut watch).unwrap();
    }
}

fn set_af_tcm(board: &Board, core: usize, bank: u32, address: u64, enabled: bool) {
    board.cpu(core).unwrap().coprocessor(arm::Coprocessor {
        cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank, secure: 0, wide: false,
    }, Some(address | u64::from(enabled))).unwrap();
}

#[test]
fn af_cpu_visible_reads_preserve_private_bank_bytes_on_both_cores() {
    use fujimod_xm1::af_ownership::memory::{ReadCore, ReadProvenance, read_cpu_visible};
    for core in 0..2 {
        for bank in 0..2 {
            let (board, _) = board(&[0xeafffffe]);
            board.write_physical(DATA, &[1, 2, 3, 4]).unwrap();
            set_af_tcm(&board, core, bank, DATA, true);
            board.write_memory(core, DATA, &[5, 6, 7, 8]).unwrap();
            let mut bytes = [0; 4];
            let provenance = read_cpu_visible(&board, board.mappings(), ReadCore::from_index(core).unwrap(), Guest(DATA as u32), &mut bytes).unwrap();
            assert_eq!(bytes, [5, 6, 7, 8]);
            assert!(matches!(provenance, ReadProvenance::Tcm { core: selected, bank: selected_bank, offset: 0, .. } if selected.index() == core && selected_bank.index() == bank as usize));
            assert_eq!(word(&board, DATA), 0x0403_0201);
            let wrong_core = ReadCore::from_index(1 - core).unwrap();
            assert!(read_cpu_visible(&board, board.mappings(), wrong_core, Guest(DATA as u32), &mut bytes).is_err());
        }
    }
}

#[test]
fn af_cpu_visible_reads_follow_live_remap_disable_and_full_span_boundaries() {
    use fujimod_xm1::af_ownership::memory::{ReadCore, ReadProvenance, read_cpu_visible};
    for core in 0..2 {
        for bank in 0..2 {
            let (board, _) = board(&[0xeafffffe]);
            let selected = ReadCore::from_index(core).unwrap();
            let base = 0x7000_0000u64;
            set_af_tcm(&board, core, bank, base, true);
            board.write_memory(core, base + 0xffc, &[9, 8, 7, 6]).unwrap();
            let mut bytes = [0; 4];
            assert!(matches!(read_cpu_visible(&board, board.mappings(), selected, Guest((base + 0xffc) as u32), &mut bytes).unwrap(), ReadProvenance::Tcm { .. }));
            assert_eq!(bytes, [9, 8, 7, 6]);
            for address in [base - 1, base + 0xffd, base + 0x1000] {
                assert!(read_cpu_visible(&board, board.mappings(), selected, Guest(address as u32), &mut bytes).is_err());
            }
            set_af_tcm(&board, core, bank, base + 0x2000, true);
            assert!(read_cpu_visible(&board, board.mappings(), selected, Guest(base as u32), &mut bytes).is_err());
            board.write_memory(core, base + 0x2000, &[4, 3, 2, 1]).unwrap();
            read_cpu_visible(&board, board.mappings(), selected, Guest((base + 0x2000) as u32), &mut bytes).unwrap();
            assert_eq!(bytes, [4, 3, 2, 1]);
            set_af_tcm(&board, core, bank, base + 0x2000, false);
            assert!(read_cpu_visible(&board, board.mappings(), selected, Guest((base + 0x2000) as u32), &mut bytes).is_err());
            assert!(read_cpu_visible(&board, board.mappings(), selected, Guest(0x2000_0000), &mut bytes).is_err());
        }
    }
}

#[test]
fn af_tcm_configuration_refuses_overlap_and_reads_refuse_shared_private_disagreement() {
    use fujimod_xm1::af_ownership::memory::{ReadCore, ReadProvenance, read_cpu_visible};
    for core in 0..2 {
        let (board, _) = board(&[0xeafffffe]);
        set_af_tcm(&board, core, 0, DATA, true);
        board.write_memory(core, DATA, &[9, 8, 7, 6]).unwrap();
        assert!(board.cpu(core).unwrap().coprocessor(arm::Coprocessor {
            cp: 15, opc1: 0, crn: 9, crm: 1, opc2: 1, secure: 0, wide: false,
        }, Some(DATA | 1)).is_err());
        let mut bytes = [0; 4];
        let selected = ReadCore::from_index(core).unwrap();
        assert!(matches!(read_cpu_visible(&board, board.mappings(), selected, Guest(DATA as u32), &mut bytes).unwrap(),
            ReadProvenance::Tcm { bank, .. } if bank.index() == 0));
        assert_eq!(bytes, [9, 8, 7, 6]);
        set_af_tcm(&board, core, 0, DATA, false);
        set_af_tcm(&board, core, 1, DATA, false);
        board.cpu(core).unwrap().unmap(0, 0x10000).unwrap();
        board.cpu(core).unwrap().map_zeroed(0, 0x10000, 7).unwrap();
        board.write_memory(core, DATA, &[1, 2, 3, 4]).unwrap();
        assert!(read_cpu_visible(&board, board.mappings(), ReadCore::from_index(core).unwrap(), Guest(DATA as u32), &mut [0; 4]).is_err());
    }
}

#[test]
fn af_reads_preserve_exclusive_store_success_against_no_read_control() {
    use fujimod_xm1::af_ownership::memory::{ReadCore, read_cpu_visible};
    for core in 0..2 {
        for read in [false, true] {
            let (board, _) = board(&[0xe191_2f9f, 0xe181_3f90, 0xeaff_fffe]);
            let base = 0x7000_0000;
            set_af_tcm(&board, core, 1, base, true);
            board.write_memory(core, base, &[4, 3, 2, 1]).unwrap();
            run(&board, core, CODE, 1);
            assert_eq!(board.read_register(core, arm::CORE[2]).unwrap(), 0);
            if read {
                let selected = ReadCore::from_index(core).unwrap();
                let mut bytes = [0; 4];
                read_cpu_visible(&board, board.mappings(), selected, Guest(DATA as u32), &mut bytes).unwrap();
                assert_eq!(bytes, [0; 4]);
                read_cpu_visible(&board, board.mappings(), selected, Guest(base as u32), &mut bytes).unwrap();
                assert_eq!(bytes, [4, 3, 2, 1]);
            }
            run(&board, core, CODE + 4, 1);
            assert_eq!(board.read_register(core, arm::CORE[3]).unwrap(), 0);
            assert_eq!(word(&board, DATA), 7);
            run(&board, core, CODE, 1);
            if read {
                read_cpu_visible(&board, board.mappings(), ReadCore::from_index(core).unwrap(), Guest(DATA as u32), &mut [0; 4]).unwrap();
            }
            run(&board, core, CODE + 4, 1);
            assert_eq!(board.read_register(core, arm::CORE[3]).unwrap(), 0);
            assert_eq!(word(&board, DATA), 7);
        }
    }
}

#[test]
fn actual_attributed_publication_writes_preserve_guard_progress_and_refusals() {
    const WRITER: u64 = 0x0100_0004;
    const BYTES: usize = 0x0140_0000;
    #[derive(Clone, Copy)]
    enum Case { Valid, WrongValue, UnknownBase, WrongClock, UnknownFetch }
    for case in [Case::Valid, Case::WrongValue, Case::UnknownBase, Case::WrongClock, Case::UnknownFetch] {
        let mut receipts = Vec::new();
        for attributed in [false, true] {
            let mut monitor = retired_monitor();
            let publication = monitor.config().publications()[0];
            let mut registers = [0u32; 13];
            registers[1] = publication.address.0;
            registers[3] = publication.new.0;
            monitor.prepare_store(CoreId(0), Guest(WRITER as u32), Word(0xe581_3000),
                &registers, publication.old, Cpsr(0x13)).unwrap();
            let graph = Graph {
                processors: (0..2).map(|_| Processor {
                    model: Model::CortexR5F, mprofile: None, released: true,
                    rprofile: Some(rprofile::core::Options::default()),
                    registers: vec![(Register::Pc, WRITER), (Register::Pstate, 0x13),
                        (arm::CORE[1], u64::from(registers[1])), (arm::CORE[3], u64::from(registers[3]))],
                }).collect(),
                ram: vec![Ram { base: 0, size: BYTES }],
                mappings: (0..2).flat_map(|core| [0, 0x4000_0000].map(|address|
                    MemoryMap { core, address, physical: 0, size: BYTES, permissions: 7 })).collect(),
                dma_mappings: vec![BusMapping::ram(0, BYTES)],
                decoders: vec![], peripherals: vec![], instructions_per_tick: Ratio::new(1,1).unwrap(),
            };
            let board = graph.build(factory::Options {
                engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20,
            }, Bus::new(Mode::Strict), None).unwrap();
            board.write_physical(WRITER, &0xe581_3000u32.to_le_bytes()).unwrap();
            board.write_physical(u64::from(publication.address.0), &publication.old.0.to_le_bytes()).unwrap();
            let sources = if attributed { CpuDataAttribution::Writes } else { CpuDataAttribution::None };
            let mut watch = board.memory_watch_begin_with_fetch_attribution(
                vec![WatchRange { start: u64::from(publication.address.0),
                    end: u64::from(publication.address.0)+4, kinds: WatchKinds::CPU_WRITE }],
                FetchAttribution::new(sources,vec![range(WRITER,WRITER+4)]).unwrap(),
                4,board.mappings().to_vec(),ViewPolicy::Fixed,
            ).unwrap();
            run(&board,0,WRITER,1);
            assert_eq!(word(&board,u64::from(publication.address.0)),publication.new.0);
            let mut row = watch.records.borrow().rows_since(0).unwrap()[0].clone();
            board.memory_watch_end(&mut watch).unwrap();
            if attributed {
                let fetch = &row["latest_fetch"];
                assert_eq!(fetch["source"], "cpu-fetch");
                assert_eq!(fetch["physical"], WRITER);
                assert_eq!(fetch["core"], row["core"]);
                assert_eq!(fetch["pc"], row["pc"]);
                assert_eq!(fetch["instructions"], row["instructions"]);
                assert_eq!(fetch["tick"], row["tick"]);
                assert!(row["core_instructions"].as_u64().is_some());
                assert_eq!(fetch["core_instructions"], row["core_instructions"]);
            }
            match case {
                Case::Valid => {}
                Case::WrongValue => row["value"] = serde_json::json!(publication.new.0 ^ 1),
                Case::UnknownBase => row["unrecognized"] = serde_json::json!(0),
                Case::WrongClock => row["core_instructions"] = serde_json::json!("invalid"),
                Case::UnknownFetch => {
                    if attributed { row["latest_fetch"]["unrecognized"] = serde_json::json!(0); }
                    else { row["latest_fetch"] = serde_json::json!({"unrecognized":0}); }
                }
            }
            let expected = match case {
                Case::Valid => Ok(()),
                Case::WrongValue => Err(LifetimeFailure::WrongValue),
                _ => Err(LifetimeFailure::MalformedRow),
            };
            let result = if attributed { monitor.observe_attributed_row(&row,board.mappings()) } else { monitor.observe_row(&row) };
            assert_eq!(result,expected);
            assert_eq!(monitor.receipt().forward_writes,u64::from(result.is_ok()));
            assert_eq!(monitor.receipt().pending_stores,usize::from(result.is_err()));
            receipts.push(serde_json::to_value(monitor.receipt()).unwrap());
        }
        assert_eq!(receipts[0],receipts[1]);
    }
}

static CATALOG_LOCK:std::sync::Mutex<()>=std::sync::Mutex::new(());

fn catalog_board() -> Board {
    let document:fujimod_xm1::run::RunDocument=serde_json::from_slice(include_bytes!("../data/xm1-150.json")).unwrap();
    let resolved=fujimod_xm1::run::resolved(&document).unwrap();
    let graph=fujimod_emulator::boot::graph::assemble_with_starts(
        &resolved.reference,document.execution.instructions_per_tick,None,&[],
    ).unwrap();
    graph.validate().unwrap();
    let mut board=graph.build(factory::Options {
        engine:factory::Engine::Unicorn,translation_cache_bytes:1<<20,
    },Bus::new(Mode::Strict),None).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    board
}

#[test]
fn attributed_watch_admits_the_actual_xm1_catalog_without_bridging_backing_holes() {
    let _catalog=CATALOG_LOCK.lock().unwrap();
    let board=catalog_board();
    let guarded=range(0x0103_fd88,0x0103_fd8c);
    let classes=vec![WatchRange{start:0x016f_ed28,end:0x016f_ed2c,kinds:WatchKinds::WRITES}];
    let attribution=|ranges|FetchAttribution::new(CpuDataAttribution::Writes,ranges).unwrap()
        .with_instruction_guards(vec![PhysicalInstructionGuard::arm32_little(
            guarded,GuestAddress(0x0103_fd88),InstructionCores::Both,
        ).unwrap()]).unwrap();
    assert!(board.memory_watch_begin_with_fetch_attribution(
        classes.clone(),attribution(vec![range(0,1<<32)]),4,board.mappings().to_vec(),ViewPolicy::Fixed,
    ).is_err());
    let ranges=fujimod_xm1::af_ownership::attribution_ranges(board.mappings()).unwrap();
    let mut watch=board.memory_watch_begin_with_fetch_attribution(
        classes,attribution(ranges),4,board.mappings().to_vec(),ViewPolicy::Fixed,
    ).unwrap();
    assert_eq!(board.instruction_counts(),vec![0,0]);
    board.memory_watch_end(&mut watch).unwrap();
    assert_eq!(board.instruction_counts(),vec![0,0]);
}

fn catalog_projection_fixture() -> (
    fujimod_emulator::session::Session,
    std::sync::Arc<Vec<MemoryMap>>,
    fujimod_xm1::af_ownership::Plan,
) {
    use fujimod_xm1::af_ownership::{Plan,Site,SiteKind};
    let board=catalog_board();
    let session=fujimod_emulator::session::Session {
        definition:serde_json::Value::Null,board,controls:None,contact_driver:None,
        resources:fujimod_emulator_exr2_plan::Resources::default(),nodes:Default::default(),
        contacts:Default::default(),recording_frames:Default::default(),
    };
    let mappings=std::sync::Arc::new(session.board.mappings().to_vec());
    let required=[
        (0x0103_fd88,SiteKind::Consumer),(0x0103_e988,SiteKind::ModeHelper),
        (0x0103_1664,SiteKind::Primary),(0x0104_01f0,SiteKind::Current),(0x0104_0250,SiteKind::Set),
        (0x0102_dec8,SiteKind::Callback),(0x0104_0288,SiteKind::Clear),(0x0103_a254,SiteKind::Request),
        (0x0072_503c,SiteKind::QueuePost),(0x0072_5318,SiteKind::Dequeue),(0x0102_8b54,SiteKind::Select),
        (0x0102_8bc4,SiteKind::DispatchCallback),(0x0105_44e4,SiteKind::WorkPost),(0x0105_47b8,SiteKind::Callback),
        (0x0102_f1b0,SiteKind::ReservePrimary),(0x0102_f1c8,SiteKind::ReserveSecondary),(0x0102_f1d8,SiteKind::ReserveSecondary),
    ];
    let mut sites:Vec<_>=required.into_iter().map(|(pc,kind)|Site {
        pc,word:0,kind,reads:Vec::new(),register_reads:Vec::new(),
    }).collect();
    sites.sort_unstable_by_key(|site|site.pc);
    let plan=Plan{sites,identity:serde_json::json!({"sdk":"test","viewer":"test","bridge":"test","model":"test","package":"test"})};
    (session,mappings,plan)
}

#[test]
fn blocked_pair_projection_keeps_its_prefix_and_fingerprint_detects_catalog_state_mutations() {
    let _catalog=CATALOG_LOCK.lock().unwrap();
    use fujimod_xm1::af_ownership::Projection;
    let (mut session,mappings,plan)=catalog_projection_fixture();
    let mut projection=Projection::new(plan,&session,mappings.clone()).unwrap();
    projection.sync_retention(fujimod_emulator::board::observation::PassiveRetentionStatus::Unarmed);
    projection.arm(&session,&mappings).unwrap();
    projection.sync_retention(fujimod_emulator::board::observation::PassiveRetentionStatus::Armed);
    session.board.write_register(0,Register::Pstate,0x13).unwrap();
    let mut points=session.board.instruction_points_begin(0,vec![0x0102_f1b0]).unwrap();
    run(&session.board,0,0x0102_f1b0,1);
    let point=points.event_context().unwrap();
    for _ in 0..341 {projection.observe(&session,&mappings,point);}
    let receipt=serde_json::to_value(&projection).unwrap();
    assert_eq!(receipt["status"],"blocked");
    assert_eq!(receipt["context_count"],128);
    assert_eq!(session.board.instruction_counts(),vec![0,0]);
    projection.row(&serde_json::json!({"physical":0x016f_ed28u32,"size":4}));
    assert_eq!(serde_json::to_value(&projection).unwrap()["context_count"],128);
    session.board.instruction_points_end(0,&mut points).unwrap();
    let proof=fujimod_xm1::machine_fingerprint::fingerprint(&session).unwrap();
    assert_eq!(proof["status"],"complete");
    let saved=session.checkpoint(None).unwrap();
    let writable=mappings.iter().find(|mapping|mapping.permissions&2!=0
        && mapping.physical%4==0 && mapping.size>=4).unwrap().physical;
    let mut bytes=[0;4];
    session.board.read_physical(writable,&mut bytes).unwrap();
    bytes[0]^=1;
    session.board.write_physical(writable,&bytes).unwrap();
    let changed=fujimod_xm1::machine_fingerprint::fingerprint(&session).unwrap();
    assert_ne!(changed["machine_sha256"],proof["machine_sha256"]);
    session.restore(&saved).unwrap();
    assert_eq!(fujimod_xm1::machine_fingerprint::fingerprint(&session).unwrap(),proof);
    let register=session.board.read_register(0,arm::CORE[0]).unwrap();
    session.board.write_register(0,arm::CORE[0],register^1).unwrap();
    let changed=fujimod_xm1::machine_fingerprint::fingerprint(&session).unwrap();
    assert_ne!(changed["machine_sha256"],proof["machine_sha256"]);
    session.restore(&saved).unwrap();
    assert_eq!(fujimod_xm1::machine_fingerprint::fingerprint(&session).unwrap(),proof);
    session.board.set_execution_policy(ExecutionPolicy::Interactive).unwrap();
    let changed=fujimod_xm1::machine_fingerprint::fingerprint(&session).unwrap();
    assert_ne!(changed["machine_sha256"],proof["machine_sha256"]);
    session.restore(&saved).unwrap();
    assert_eq!(fujimod_xm1::machine_fingerprint::fingerprint(&session).unwrap(),proof);
    assert_eq!(session.board.instruction_counts(),vec![0,0]);
}
