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

/// A lifetime monitor over a synthetic 223-publication request, past its
/// install entry, whose retired extent is [CODE, CODE+8).
fn retired_monitor() -> LifetimeMonitor {
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
    let mut monitor = LifetimeMonitor::new(config, &memory).unwrap();
    monitor.observe_entry(CoreId(0), Guest(INSTALL), Word(words[0])).unwrap();
    monitor
}

#[test]
fn retired_lifetime_extents_refuse_canonical_and_aliased_fetches_on_both_cores() {
    for (core, pc) in [(0, CODE), (1, CODE), (0, ALIAS), (1, ALIAS), (0, DECODER), (1, DECODER)] {
        let (board, _) = board(&[0xe2800001, 0xeafffffe]);
        let protected = range(0x2000, 0x2004);
        let attribution = FetchAttribution::new(CpuDataAttribution::None, vec![protected]).unwrap()
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
        let mut monitor = retired_monitor();
        assert_eq!(monitor.observe_row(fetch), Err(LifetimeFailure::RetiredFetch));
        assert_eq!(monitor.receipt().failure, Some(LifetimeFailure::RetiredFetch));
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
