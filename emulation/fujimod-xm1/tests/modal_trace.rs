#[path = "common/modal_lifetime_fixture.rs"]
mod lifetime_fixture;

use fujimod_emulator::board::{bus::{Bus, Mode}, factory::{self, Graph, MemoryMap, Processor, Ram}, memory::BusMapping};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, arm, timeline::Ratio};
use fujimod_emulator_exr2_plan::Resources;
use fujimod_xm1::{
    machine::{Machine, RunOutcome},
    modal_permit::{CodeRange, Layout, ObservationLimit, STOCK_END, STOCK_ENTRY},
    modal_trace::{BeginRequest, Config, SourceImage},
    provenance::Guest,
    run::RunDocument,
    trace::CaptureScope,
};
use std::{collections::BTreeMap, num::{NonZeroU64, NonZeroUsize}};

fn synthetic_stock() -> Vec<u8> {
    let mut words = vec![0xe1a0_0000u32; ((STOCK_END.0 - STOCK_ENTRY.0) / 4) as usize];
    words[0] = 0xe92d_4ff0;
    words[1] = 0xea00_001b;
    words[((0x00bf_b0a0 - STOCK_ENTRY.0) / 4) as usize] = 0xe8bd_8ff0;
    words.into_iter().flat_map(u32::to_le_bytes).collect()
}

fn machine(entry_cores: [bool; 2], aliased_stack: bool) -> Machine {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let mut mappings = Vec::new();
    for core in 0..2 {
        mappings.extend([
            MemoryMap { core, address: 0, physical: 0, size: 0x10000, permissions: 7 },
            MemoryMap { core, address: 0x00bf_b000, physical: 0x00bf_b000, size: 0x1000, permissions: 7 },
        ]);
        if aliased_stack {
            mappings.push(MemoryMap { core, address: 0x4000_0000, physical: 0, size: 0x10000, permissions: 7 });
        }
    }
    let graph = Graph {
        processors: (0..2).map(|core| Processor {
            model: Model::CortexR5F, rprofile: Some(Default::default()), mprofile: None, released: true,
            registers: vec![
                (Register::Pc, if entry_cores[core] { u64::from(STOCK_ENTRY.0) } else { 0x1000 }),
                (Register::Pstate, 0xd3),
                (Register::Arm32(arm::Register::R(0)), 75),
                (Register::Arm32(arm::Register::R(1)), 7),
                (Register::Arm32(arm::Register::R(2)), 6),
                (Register::Arm32(arm::Register::R(13)), if aliased_stack { 0x4000_8000 } else { 0x8000 } + core as u64 * 0x1000),
                (Register::Arm32(arm::Register::R(14)), 0x2000 + core as u64 * 0x1000),
            ],
        }).collect(),
        ram: vec![Ram { base: 0, size: 0x10000 }, Ram { base: 0x00bf_b000, size: 0x1000 }],
        dma_mappings: vec![BusMapping::ram(0, 0x10000), BusMapping::ram(0x00bf_b000, 0x1000)],
        decoders: vec![], peripherals: vec![], mappings,
        instructions_per_tick: Ratio::new(24, 1).unwrap(),
    };
    let mut board = graph.build(factory::Options {
        engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20,
    }, Bus::new(Mode::Strict), None).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    board.write_physical(u64::from(STOCK_ENTRY.0), &synthetic_stock()).unwrap();
    for pc in [0x1000, 0x2000, 0x3000] {
        board.write_physical(pc, &0xeaff_fffeu32.to_le_bytes()).unwrap();
    }
    let session = Session {
        definition: serde_json::Value::Null, board, controls: None, contact_driver: None,
        resources: Resources::default(), nodes: BTreeMap::new(), contacts: BTreeMap::new(), recording_frames: Default::default(),
    };
    let mut document = RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap();
    document.execution.quantum = 1;
    Machine::new(document, session).unwrap()
}

fn baseline(machine: &Machine) -> Config {
    Config::new(Layout::stock_baseline(), synthetic_stock(), None,
        machine.session.board.mappings().to_vec(), ObservationLimit(NonZeroU64::new(128).unwrap())).unwrap()
}

#[test]
fn begin_and_finish_preserve_user_sites_and_publish_disabled_evidence() {
    let mut machine = machine([false, false], false);
    let sites = BTreeMap::from([(0x1000, "user-loop".to_owned())]);
    machine.traces.arm_primary(&machine.session, sites.clone()).unwrap();
    assert!(!machine.traces.modal_observation_report().enabled);
    let config = baseline(&machine);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    assert!(machine.traces.modal_observation_report().enabled);
    machine.traces.end_modal_observation(&machine.session).unwrap();
    let report = machine.traces.modal_observation_report();
    assert!(!report.enabled);
    assert_eq!(report.counts.observations, 0);
    assert!(report.failure.is_none());
    assert!(!report.physical_fetch_coverage);
    assert!(!report.task_identity_verified);
    assert_eq!(machine.traces.sites, sites);
    assert!(machine.traces.armed());
    assert!(machine.traces.end_modal_observation(&machine.session).is_err());
    machine.traces.disarm(&machine.session).unwrap();
}

#[test]
fn active_modal_refuses_point_mutation_and_capture_without_losing_evidence() {
    let mut machine = machine([false, false], false);
    let sites = BTreeMap::from([(0x1000, "user-loop".to_owned())]);
    machine.traces.arm(&machine.session, sites.clone()).unwrap();
    let config = baseline(&machine);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    assert!(machine.traces.arm(&machine.session, BTreeMap::new()).is_err());
    assert!(machine.traces.arm_primary(&machine.session, BTreeMap::new()).is_err());
    assert!(machine.traces.add_sites(&machine.session, BTreeMap::from([(0x2000, "other".into())])).is_err());
    assert!(machine.traces.remove_sites(&machine.session, &[Guest(0x1000)]).is_err());
    assert!(machine.traces.disarm(&machine.session).is_err());
    assert!(machine.traces.begin_capture_observation(CaptureScope::Capture).is_err());
    let config = baseline(&machine);
    assert!(machine.traces.begin_modal_observation(&machine.session, config).is_err());
    assert_eq!(machine.traces.sites, sites);
    assert!(machine.traces.modal_observation_report().enabled);
    assert!(machine.traces.modal_observation_report().failure.is_none());
    machine.traces.end_modal_observation(&machine.session).unwrap();
}

#[test]
fn capture_observation_refuses_modal_begin() {
    let mut machine = machine([false, false], false);
    machine.traces.begin_capture_observation(CaptureScope::Capture).unwrap();
    let config = baseline(&machine);
    assert!(machine.traces.begin_modal_observation(&machine.session, config).is_err());
    assert!(!machine.traces.modal_observation_active());
    assert!(machine.traces.capture_observation_active());
}

#[test]
fn source_mismatch_refuses_begin_without_replacing_user_points() {
    let mut machine = machine([false, false], false);
    let sites = BTreeMap::from([(0x1000, "user-loop".to_owned())]);
    machine.traces.arm(&machine.session, sites.clone()).unwrap();
    machine.session.board.write_physical(u64::from(STOCK_END.0 - 4), &0xeaff_fffeu32.to_le_bytes()).unwrap();
    let config = baseline(&machine);
    assert!(machine.traces.begin_modal_observation(&machine.session, config).is_err());
    assert!(!machine.traces.modal_observation_active());
    assert_eq!(machine.traces.sites, sites);
    assert!(machine.traces.armed());
}

#[test]
fn both_cores_arm_new_returns_and_capture_prehook_facts_without_user_hits() {
    let mut machine = machine([true, true], false);
    machine.traces.bound_full_registers(NonZeroUsize::new(1).unwrap());
    let config = baseline(&machine);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    assert!(matches!(machine.run(8), RunOutcome::Completed));
    machine.require_running().unwrap();
    machine.traces.end_modal_observation(&machine.session).unwrap();
    let report = machine.traces.modal_observation_report();
    assert_eq!(report.counts.entries, 2);
    assert_eq!(report.counts.fallback_handoffs, 2);
    assert_eq!(report.counts.returns, 2);
    assert_eq!(report.counts.active_frames, 0);
    assert_eq!(report.stock_fetches.len(), 4);
    assert_eq!(report.captured_domains.len(), 2);
    for core in 0..2 {
        let domain = report.captured_domains.iter().find(|domain| domain.core.0 == core).unwrap();
        assert_eq!(domain.pc, Guest(0x00bf_b0a0));
        assert_eq!((domain.controller, domain.state, domain.event), (75, 7, 6));
    }
    assert!(machine.traces.hits.is_empty());
    assert!(!machine.traces.armed());
}

#[test]
fn existing_user_resume_marker_does_not_skip_modal_entry_or_duplicate_user_hit() {
    let mut machine = machine([true, false], false);
    machine.traces.arm(&machine.session, BTreeMap::from([(STOCK_ENTRY.0, "entry".into())])).unwrap();
    machine.traces.stop_labels.insert("entry".into());
    machine.traces.bound_full_registers(NonZeroUsize::new(1).unwrap());
    assert!(matches!(machine.run(1), RunOutcome::Stopped));
    assert_eq!(machine.instructions(), 0);
    assert_eq!(machine.traces.hits.len(), 1);
    let config = baseline(&machine);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    machine.traces.stop_labels.clear();
    assert!(matches!(machine.run(8), RunOutcome::Completed));
    machine.require_running().unwrap();
    machine.traces.end_modal_observation(&machine.session).unwrap();
    assert_eq!(machine.traces.modal_observation_report().counts.entries, 1);
    assert_eq!(machine.traces.modal_observation_report().counts.returns, 1);
    assert_eq!(machine.traces.hits.len(), 1);
}

#[test]
fn incomplete_finish_is_sticky_and_keeps_required_points_and_frames() {
    let mut machine = machine([true, false], false);
    let config = baseline(&machine);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    assert!(matches!(machine.run(1), RunOutcome::Completed));
    assert!(machine.traces.end_modal_observation(&machine.session).is_err());
    let report = machine.traces.modal_observation_report();
    assert!(report.enabled);
    assert_eq!(report.counts.entries, 1);
    assert_eq!(report.counts.active_frames, 1);
    assert!(report.failure.is_some());
    let failure = report.failure.unwrap().to_owned();
    assert!(machine.traces.end_modal_observation(&machine.session).is_err());
    assert_eq!(machine.traces.modal_observation_report().failure, Some(failure.as_str()));
    assert!(machine.traces.armed());
    assert!(machine.traces.disarm(&machine.session).is_err());
    let instructions = machine.instructions();
    assert!(matches!(machine.run(1), RunOutcome::Failed));
    assert_eq!(machine.instructions(), instructions);
}

#[test]
fn mapped_stack_alias_is_sampled_at_its_physical_translation() {
    let mut machine = machine([true, false], true);
    let config = baseline(&machine);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    assert!(matches!(machine.run(8), RunOutcome::Completed));
    machine.require_running().unwrap();
    machine.traces.end_modal_observation(&machine.session).unwrap();
    assert_eq!(machine.traces.modal_observation_report().counts.returns, 1);
}

#[test]
fn changed_return_word_fails_without_losing_the_active_frame() {
    let mut machine = machine([true, false], false);
    let config = baseline(&machine);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    assert!(matches!(machine.run(1), RunOutcome::Completed));
    machine.session.board.write_physical(0x2000, &0xe1a0_0000u32.to_le_bytes()).unwrap();
    assert!(matches!(machine.run(8), RunOutcome::Failed));
    let report = machine.traces.modal_observation_report();
    assert!(report.enabled);
    assert_eq!(report.counts.entries, 1);
    assert_eq!(report.counts.returns, 0);
    assert_eq!(report.counts.active_frames, 1);
    assert!(report.failure.is_some());
    assert!(machine.traces.end_modal_observation(&machine.session).is_err());
}

#[test]
fn separate_linked_prehook_records_facts_without_creating_a_permit() {
    let mut machine = machine([false, false], false);
    let seam = CodeRange::new(Guest(0x4000), Guest(0x4038)).unwrap();
    let owned = CodeRange::new(Guest(0x4040), Guest(0x4080)).unwrap();
    let layout = Layout::new(seam, owned).unwrap();
    let mut linked = vec![0; 0x100];
    for word in linked.chunks_exact_mut(4) { word.copy_from_slice(&0xe1a0_0000u32.to_le_bytes()); }
    linked[0x90..0x94].copy_from_slice(&0xe12f_ff1eu32.to_le_bytes());
    machine.session.board.write_physical(0x4000, &linked).unwrap();
    let delta = (0x4000i64 - i64::from(STOCK_ENTRY.0) - 8) as u32;
    let branch = 0xea00_0000 | ((delta >> 2) & 0x00ff_ffff);
    machine.session.board.write_physical(u64::from(STOCK_ENTRY.0), &branch.to_le_bytes()).unwrap();
    for core in 0..2 { machine.session.board.write_register(core, Register::Pc, 0x4090).unwrap(); }
    let config = Config::new(layout, synthetic_stock(), Some(SourceImage::new(Guest(0x4000), linked).unwrap()),
        machine.session.board.mappings().to_vec(), ObservationLimit(NonZeroU64::new(128).unwrap())).unwrap()
        .with_owned_prehook(Some(Guest(0x4090))).unwrap();
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    assert!(matches!(machine.run(4), RunOutcome::Completed));
    machine.require_running().unwrap();
    machine.traces.end_modal_observation(&machine.session).unwrap();
    let report = machine.traces.modal_observation_report();
    assert_eq!(report.counts.observations, 0);
    assert_eq!(report.counts.entries, 0);
    assert_eq!(report.captured_domains.len(), 2);
    assert!(report.stock_fetches.is_empty());
}

#[test]
fn either_complete_helper_range_refuses_execution_without_an_entry_permit() {
    for pc in [0x4094, 0x40bc] {
        let mut machine = machine([false, false], false);
        let seam = CodeRange::new(Guest(0x4000), Guest(0x4038)).unwrap();
        let owned = CodeRange::new(Guest(0x4040), Guest(0x4080)).unwrap();
        let layout = Layout::new(seam, owned).unwrap().with_helpers([
            CodeRange::new(Guest(0x4090), Guest(0x40a0)).unwrap(),
            CodeRange::new(Guest(0x40b0), Guest(0x40c0)).unwrap(),
        ]).unwrap();
        let linked: Vec<_> = [0xe3a0_007bu32; 64].into_iter().flat_map(u32::to_le_bytes).collect();
        machine.session.board.write_physical(0x4000, &linked).unwrap();
        let delta = (0x4000i64 - i64::from(STOCK_ENTRY.0) - 8) as u32;
        let branch = 0xea00_0000 | ((delta >> 2) & 0x00ff_ffff);
        machine.session.board.write_physical(u64::from(STOCK_ENTRY.0), &branch.to_le_bytes()).unwrap();
        machine.session.board.write_register(0, Register::Pc, pc).unwrap();
        let config = Config::new(layout, synthetic_stock(), Some(SourceImage::new(Guest(0x4000), linked).unwrap()),
            machine.session.board.mappings().to_vec(), ObservationLimit(NonZeroU64::new(128).unwrap())).unwrap()
            .with_owned_prehook(Some(Guest(0x4090))).unwrap();
        machine.traces.begin_modal_observation(&machine.session, config).unwrap();
        assert!(matches!(machine.run(4), RunOutcome::Failed));
        assert!(machine.traces.modal_observation_report().failure.is_some());
        assert_eq!(machine.session.board.read_register(0, Register::Arm32(arm::Register::R(0))).unwrap(), 75);
    }
}

#[test]
fn request_rejects_zero_limit_partial_scope_and_unbound_prehook() {
    let machine = machine([false, false], false);
    for fields in [
        serde_json::json!({"limit": 0}),
        serde_json::json!({"limit": 65537}),
        serde_json::json!({"limit": 1, "seam": [0x4000, 0x4038]}),
        serde_json::json!({"limit": 1, "owned_prehook": 0x4000}),
    ] {
        let mut request = serde_json::json!({"stock": synthetic_stock(), "linked": null, "seam": null, "owned": null});
        request.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
        let request: BeginRequest = serde_json::from_value(request).unwrap();
        assert!(request.into_config(machine.session.board.mappings().to_vec()).is_err());
    }
    let request: BeginRequest = serde_json::from_value(serde_json::json!({
        "stock": synthetic_stock(), "linked": null, "seam": null, "owned": null, "limit": 1,
    })).unwrap();
    assert!(request.into_config(machine.session.board.mappings().to_vec()).is_ok());
    assert!(SourceImage::new(Guest(0xffff_fffc), vec![0; 8]).is_err());
    assert!(SourceImage::new(Guest(1), vec![0; 4]).is_err());
    assert!(SourceImage::new(Guest(0x4000), vec![0; 1024 * 1024 + 4]).is_err());
}

#[test]
fn observation_limit_keeps_rejected_fetch_evidence_bounded() {
    let mut machine = machine([true, false], false);
    let config = Config::new(Layout::stock_baseline(), synthetic_stock(), None,
        machine.session.board.mappings().to_vec(), ObservationLimit(NonZeroU64::new(1).unwrap())).unwrap();
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    assert!(matches!(machine.run(8), RunOutcome::Failed));
    let report = machine.traces.modal_observation_report();
    assert_eq!(report.counts.observations, 1);
    assert_eq!(report.counts.entries, 1);
    assert_eq!(report.stock_fetches.len(), 1);
    assert!(report.failure.is_some());
    assert!(report.enabled);
}

#[test]
fn changed_stock_instruction_is_refused_before_execution() {
    let mut machine = machine([true, false], false);
    let config = baseline(&machine);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    machine.session.board.write_physical(u64::from(STOCK_ENTRY.0 + 4), &0xe3a0_0063u32.to_le_bytes()).unwrap();
    assert!(matches!(machine.run(8), RunOutcome::Failed));
    assert_eq!(machine.session.board.read_register(0, Register::Pc).unwrap(), u64::from(STOCK_ENTRY.0 + 4));
    assert_eq!(machine.session.board.read_register(0, Register::Arm32(arm::Register::R(0))).unwrap(), 75);
    let report = machine.traces.modal_observation_report();
    assert_eq!(report.counts.entries, 1);
    assert_eq!(report.counts.returns, 0);
    assert!(report.failure.is_some());
}

#[test]
fn stock_handoff_checks_registers_outside_the_captured_domain() {
    let mut machine = machine([true, false], false);
    let config = baseline(&machine);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    assert!(matches!(machine.run(1), RunOutcome::Completed));
    machine.session.board.write_register(0, Register::Arm32(arm::Register::R(12)), 1).unwrap();
    assert!(matches!(machine.run(8), RunOutcome::Failed));
    let report = machine.traces.modal_observation_report();
    assert_eq!(report.counts.entries, 1);
    assert_eq!(report.counts.fallback_handoffs, 0);
    assert_eq!(report.counts.active_frames, 1);
    assert!(report.failure.is_some());
}

#[test]
fn failed_user_point_rearm_retains_active_modal_evidence_and_refuses_advancement() {
    let mut machine = machine([true, false], false);
    let config = baseline(&machine);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    assert!(matches!(machine.run(8), RunOutcome::Completed));
    machine.require_running().unwrap();
    machine.traces.sites.insert(0x1001, "unaligned-user-site".into());
    assert!(machine.traces.end_modal_observation(&machine.session).is_err());
    let report = machine.traces.modal_observation_report();
    assert!(report.enabled);
    assert_eq!(report.counts.entries, 1);
    assert_eq!(report.counts.returns, 1);
    assert_eq!(report.counts.active_frames, 0);
    assert_eq!(report.stock_fetches.len(), 2);
    assert_eq!(report.captured_domains.len(), 1);
    let failure = report.failure.unwrap().to_owned();
    assert!(machine.traces.end_modal_observation(&machine.session).is_err());
    assert_eq!(machine.traces.modal_observation_report().failure, Some(failure.as_str()));
    let instructions = machine.instructions();
    assert!(matches!(machine.run(1), RunOutcome::Failed));
    assert_eq!(machine.instructions(), instructions);
    let report = machine.traces.modal_observation_report();
    assert!(report.enabled);
    assert_eq!(report.failure, Some(failure.as_str()));
    assert_eq!(report.counts.returns, 1);
    assert_eq!(report.stock_fetches.len(), 2);
    assert_eq!(report.captured_domains.len(), 1);
}

#[derive(Clone, Copy)]
enum CurrentStoreCase { Valid, WrongTarget, Underflow, WrongCore }

struct InstalledCurrentCase {
    machine: Machine,
    config: Config,
    current: Guest,
    node: fujimod_xm1::modal_lifetime::Word,
}

fn installed_current_case(kernel: fujimod_xm1::modal_permit::CoreId, case: CurrentStoreCase) -> InstalledCurrentCase {
    use fujimod_xm1::{modal_lifetime::Word, modal_task::source_layouts};
    let request = lifetime_fixture::owned_union_request(false);
    let layout = source_layouts()[kernel.0];
    let core = if matches!(case, CurrentStoreCase::WrongCore) { 1 - kernel.0 } else { kernel.0 };
    let pc = [0x43d48u64, 0xd3d04][kernel.0];
    let node = Word(layout.nodes.start().0 + 0x4000_0000 + 56);
    let base = match case {
        CurrentStoreCase::WrongTarget => layout.selected.start().0 + 0x4000_0004,
        CurrentStoreCase::Underflow => 4,
        _ => layout.selected.start().0 + 0x4000_0000,
    };
    let mappings: Vec<_> = (0..2).flat_map(|core| [
        MemoryMap { core, address: 0, physical: 0, size: 0x0130_0000, permissions: 7 },
        MemoryMap { core, address: 0x4000_0000, physical: 0, size: 0x0130_0000, permissions: 7 },
        MemoryMap { core, address: 0x0146_7000, physical: 0x0146_7000, size: 0x1000, permissions: 7 },
    ]).collect();
    let graph = Graph {
        processors: (0..2).map(|index| Processor {
            model: Model::CortexR5F, rprofile: Some(Default::default()), mprofile: None, released: index == core,
            registers: vec![(Register::Pc, pc), (Register::Pstate, 0x93),
                (arm::CORE[0], u64::from(base)), (arm::CORE[1], u64::from(node.0)),
                (Register::Arm32(arm::Register::R(13)), 0x8000)],
        }).collect(),
        ram: vec![Ram { base: 0, size: 0x0130_0000 }, Ram { base: 0x0146_7000, size: 0x1000 }],
        dma_mappings: vec![BusMapping::ram(0, 0x0130_0000), BusMapping::ram(0x0146_7000, 0x1000)],
        decoders: vec![], peripherals: vec![], mappings,
        instructions_per_tick: Ratio::new(1, 1).unwrap(),
    };
    let mut board = graph.build(factory::Options {
        engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20,
    }, Bus::new(Mode::Strict), None).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    let store = |address, value: &serde_json::Value| {
        let bytes: Vec<u8> = serde_json::from_value(value.clone()).unwrap();
        board.write_physical(address, &bytes).unwrap();
    };
    store(u64::from(STOCK_ENTRY.0), &request["stock"]);
    for image in request["context_sources"].as_array().unwrap().iter()
        .chain(request["lifetime"]["readonly"].as_array().unwrap()) {
        store(image["start"].as_u64().unwrap(), &image["bytes"]);
    }
    board.write_physical(u64::from(layout.selected.start().0), &node.0.to_le_bytes()).unwrap();
    let config = serde_json::from_value::<BeginRequest>(request).unwrap()
        .into_config(board.mappings().to_vec()).unwrap();
    let session = Session {
        definition: serde_json::Value::Null, board, controls: None, contact_driver: None,
        resources: Resources::default(), nodes: BTreeMap::new(), contacts: BTreeMap::new(), recording_frames: Default::default(),
    };
    let mut document = RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap();
    document.execution.quantum = 1;
    InstalledCurrentCase { machine: Machine::new(document, session).unwrap(), config, current: layout.current.start(), node }
}

#[test]
fn installed_owned_current_stores_execute_only_at_the_bound_core_and_effective_address() {
    use fujimod_xm1::modal_permit::CoreId;
    for kernel in [CoreId(0), CoreId(1)] {
        for case in [CurrentStoreCase::Valid, CurrentStoreCase::WrongTarget, CurrentStoreCase::Underflow, CurrentStoreCase::WrongCore] {
            let InstalledCurrentCase { mut machine, config, current, node } = installed_current_case(kernel, case);
            machine.traces.begin_modal_observation(&machine.session, config).unwrap();
            let outcome = machine.run(4);
            let mut bytes = [0; 4];
            machine.session.board.read_physical(u64::from(current.0), &mut bytes).unwrap();
            let report = serde_json::to_value(machine.traces.modal_observation_report()).unwrap();
            if matches!(case, CurrentStoreCase::Valid) {
                assert!(matches!(outcome, RunOutcome::Completed));
                machine.require_running().unwrap();
                assert_eq!(u32::from_le_bytes(bytes), node.0);
                assert_eq!(report["lifetime"]["checked_context_writes"], serde_json::json!(1));
                assert_eq!(report["lifetime"]["pending_stores"], serde_json::json!(0));
                assert!(report["failure"].is_null());
            } else {
                assert!(matches!(outcome, RunOutcome::Failed));
                assert_eq!(bytes, [0; 4]);
                assert!(!report["failure"].is_null());
            }
        }
    }
}

fn prepare_exclusive_control(machine: &Machine) {
    let code:Vec<_>=[0xe191_2f9fu32,0xe181_3f90,0xeaff_fffe].into_iter().flat_map(u32::to_le_bytes).collect();
    machine.session.board.write_physical(0x1000,&code).unwrap();
}

fn reserve_control_word(machine: &Machine,core:usize) {
    let board=&machine.session.board;
    board.write_register(core,arm::CORE[0],0x1234_5678).unwrap();
    board.write_register(core,arm::CORE[1],0x3000).unwrap();
    board.cpu(core).unwrap().run(fujimod_emulator_cpu::Run {
        begin:0x1000,until:None,instructions:Some(1),watchdog_us:0,
    }).unwrap();
}

fn require_exclusive_store_success(machine: &Machine,core:usize) {
    let board=&machine.session.board;
    board.cpu(core).unwrap().run(fujimod_emulator_cpu::Run {
        begin:0x1004,until:None,instructions:Some(1),watchdog_us:0,
    }).unwrap();
    assert_eq!(board.read_register(core,arm::CORE[3]).unwrap(),0);
    let mut bytes=[0;4];
    board.read_physical(0x3000,&mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes),0x1234_5678);
}

#[test]
fn baseline_modal_observer_begin_end_preserve_reservations_against_no_observer() {
    for core in 0..2 {
        for enabled in [false,true] {
            let mut machine=machine([false,false],true);
            prepare_exclusive_control(&machine);
            let config=baseline(&machine);
            reserve_control_word(&machine,core);
            if enabled { machine.traces.begin_modal_observation(&machine.session,config).unwrap(); }
            require_exclusive_store_success(&machine,core);
            reserve_control_word(&machine,core);
            if enabled { machine.traces.end_modal_observation(&machine.session).unwrap(); }
            require_exclusive_store_success(&machine,core);
            assert!(!machine.traces.modal_observation_report().enabled);
        }
    }
}

fn execute_lifetime_control_instruction(machine:&mut Machine,core:usize,pc:u64) {
    machine.session.board.write_register(core,Register::Pc,pc).unwrap();
    assert!(matches!(machine.run(1),RunOutcome::Completed));
    machine.require_running().unwrap();
}

fn complete_control_publication_cycle(machine:&mut Machine,core:usize) {
    let runtime=u64::from(lifetime_fixture::RUNTIME);
    execute_lifetime_control_instruction(machine,core,runtime+0x200);
    for index in 0..223u64 {
        machine.session.board.write_register(core,arm::CORE[1],0x0120_0000+index*4).unwrap();
        machine.session.board.write_register(core,arm::CORE[3],index+1).unwrap();
        execute_lifetime_control_instruction(machine,core,runtime+0x204);
    }
    execute_lifetime_control_instruction(machine,core,runtime+0x208);
    for index in (0..223u64).rev() {
        machine.session.board.write_register(core,arm::CORE[1],0x0120_0000+index*4).unwrap();
        machine.session.board.write_register(core,arm::CORE[3],0).unwrap();
        execute_lifetime_control_instruction(machine,core,runtime+0x20c);
    }
}

#[test]
fn installed_lifetime_guarded_modal_begin_end_preserve_reservations_against_no_observer() {
    for core in 0..2 {
        for enabled in [false,true] {
            let InstalledCurrentCase{mut machine,config,..}=installed_current_case(
                fujimod_xm1::modal_permit::CoreId(core),CurrentStoreCase::Valid);
            prepare_exclusive_control(&machine);
            reserve_control_word(&machine,core);
            if enabled { machine.traces.begin_modal_observation(&machine.session,config).unwrap(); }
            require_exclusive_store_success(&machine,core);
            complete_control_publication_cycle(&mut machine,core);
            reserve_control_word(&machine,core);
            if enabled {
                machine.traces.end_modal_observation(&machine.session).unwrap();
                let report=serde_json::to_value(machine.traces.modal_observation_report()).unwrap();
                assert_eq!(report["lifetime"]["phase"],serde_json::json!("ended"));
                assert_eq!(report["lifetime"]["forward_writes"],serde_json::json!(223));
                assert_eq!(report["lifetime"]["reverse_writes"],serde_json::json!(223));
                assert!(report["failure"].is_null());
            }
            require_exclusive_store_success(&machine,core);
            assert!(!machine.traces.modal_observation_report().enabled);
        }
    }
}

fn passive_control_watch(machine: &Machine) -> Box<fujimod_emulator::board::observation::MemoryWatch> {
    use fujimod_emulator::board::observation::{
        CpuDataAttribution, FetchAttribution, GuestAddress, InstructionCores, PassiveFetchAttribution, PassiveRetention,
        PhysicalInstructionGuard, ViewPolicy, WatchKinds, WatchRange,
    };
    let protected = WatchRange { start: 0x3000, end: 0x3004, kinds: WatchKinds::WRITES };
    machine.session.board.memory_watch_begin_with_passive_fetch_attribution(
        vec![protected.clone()],
        FetchAttribution::new(CpuDataAttribution::None, vec![
            WatchRange { start: 0x1000, end: 0x1008, kinds: WatchKinds::CPU_FETCH },
        ]).unwrap().with_instruction_guards(vec![PhysicalInstructionGuard::arm32_little(
            WatchRange { start: 0x1000, end: 0x1008, kinds: WatchKinds::CPU_FETCH },
            GuestAddress(0x1000), InstructionCores::Both,
        ).unwrap()]).unwrap(),
        3, machine.session.board.mappings().to_vec(), ViewPolicy::Fixed,
        PassiveRetention::new(vec![
            protected,
            WatchRange { start: 0x4000, end: 0x4004, kinds: WatchKinds::WRITES },
        ], 2).unwrap().with_fetch_attribution(PassiveFetchAttribution::new(CpuDataAttribution::Writes, vec![
            WatchRange { start: 0x1000, end: 0x1008, kinds: WatchKinds::CPU_FETCH },
            WatchRange { start: 0x2000, end: 0x2008, kinds: WatchKinds::CPU_FETCH },
        ]).unwrap()),
    ).unwrap()
}

fn require_protected_native_refusal(machine: &mut Machine,
    watch: &fujimod_emulator::board::observation::MemoryWatch) {
    machine.session.board.run_for_instructions(8, 1).unwrap();
    assert!(watch.records.borrow().violation().is_some());
    assert!(watch.instruction_guard_status().unwrap().first_failure.is_some());
    for core in 0..2 {
        assert!(machine.session.board.cpu(core).unwrap().execution_refused());
    }
    let counts = machine.session.board.instruction_counts();
    machine.session.board.run_for_instructions(8, 1).unwrap();
    assert_eq!(machine.session.board.instruction_counts(), counts);
}

#[derive(Clone, Copy)]
enum PublicAdvance { Run, RunUntil }

fn require_protected_public_refusal(machine: &mut Machine,
    watch: &fujimod_emulator::board::observation::MemoryWatch, api: PublicAdvance) {
    let outcome = match api {
        PublicAdvance::Run => machine.run(8),
        PublicAdvance::RunUntil => machine.run_until(8, None),
    };
    assert!(matches!(outcome, RunOutcome::Failed));
    let cause = machine.native_execution_refusal().unwrap();
    assert_eq!(cause.core, fujimod_xm1::modal_permit::CoreId(0));
    assert!(machine.require_running().is_err());
    let counts = machine.session.board.instruction_counts();
    assert!(matches!(machine.run(8), RunOutcome::Failed));
    assert!(matches!(machine.run_until(8, None), RunOutcome::Failed));
    assert_eq!(machine.session.board.instruction_counts(), counts);
    assert_eq!(machine.native_execution_refusal().unwrap().core, cause.core);
    require_protected_native_refusal(machine, watch);
}

#[test]
fn passive_overflow_preserves_original_budget_and_later_protected_refusal() {
    use fujimod_emulator::board::observation::{PassiveRetentionStatus, PassiveStopReason};
    let mut machine = machine([false, false], false);
    let code: Vec<_> = [0xe581_0000u32, 0xeaff_fffd].into_iter().flat_map(u32::to_le_bytes).collect();
    machine.session.board.write_physical(0x1000, &code).unwrap();
    machine.session.board.write_register(1, Register::Pc, 0x2000).unwrap();
    let peer_code: Vec<_> = [0xe581_0000u32, 0xeaff_fffe].into_iter().flat_map(u32::to_le_bytes).collect();
    machine.session.board.write_physical(0x2000, &peer_code).unwrap();
    machine.session.board.write_register(1, arm::CORE[1], 0x3000).unwrap();
    machine.session.board.write_register(0, arm::CORE[1], 0x4000).unwrap();
    let mut watch = passive_control_watch(&machine);
    machine.session.board.write_physical(0x4000, &[1; 4]).unwrap();
    assert_eq!(watch.records.borrow().row_count(), 0);
    watch.records.borrow_mut().arm_passive_retention().unwrap();
    machine.session.board.write_physical(0x3000, &[1; 4]).unwrap();
    let before = machine.instructions();
    let original_left_budget = 64;
    assert!(matches!(machine.run(original_left_budget), RunOutcome::Completed));
    machine.require_running().unwrap();
    assert_eq!(machine.instructions() - before, original_left_budget);
    {
        let records = watch.records.borrow();
        assert_eq!(records.passive_retention_status(),
            PassiveRetentionStatus::Stopped(PassiveStopReason::CapacityExceeded));
        assert!(records.violation().is_none());
        let rows = records.rows_since(0).unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["source"], "host-write");
        assert_eq!(rows[0]["physical"], 0x3000);
        let af_rows: Vec<_> = rows.iter().filter(|row| row["physical"] == 0x4000).collect();
        assert_eq!(af_rows.len(), 2);
        assert!(rows.iter().any(|row| row["source"] == "cpu-write" && row["physical"] == 0x3000));
        for row in af_rows {
            assert_eq!(row["source"], "cpu-write");
            assert_eq!(row["latest_fetch"]["physical"], 0x1000);
        }
    }
    machine.session.board.write_physical(0x4000, &[2; 4]).unwrap();
    machine.session.board.write_physical(0x3000, &[2; 4]).unwrap();
    assert_eq!(watch.records.borrow().row_count(), 5);
    machine.session.board.write_register(0, arm::CORE[1], 0x3000).unwrap();
    require_protected_public_refusal(&mut machine, &watch, PublicAdvance::Run);
    machine.session.board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn passive_end_and_refusal_stop_only_af_rows_and_protected_overlap_survives() {
    use fujimod_emulator::board::observation::{PassiveRetentionStatus, PassiveStopReason};
    for reason in [PassiveStopReason::End, PassiveStopReason::Refused] {
        let mut machine = machine([false, false], false);
        let mut watch = passive_control_watch(&machine);
        machine.session.board.write_physical(0x4000, &[1; 4]).unwrap();
        assert_eq!(watch.records.borrow().row_count(), 0);
        watch.records.borrow_mut().arm_passive_retention().unwrap();
        machine.session.board.write_physical(0x4000, &[2; 4]).unwrap();
        watch.records.borrow_mut().stop_passive_retention(reason).unwrap();
        for value in 3..32 {
            machine.session.board.write_physical(0x4000, &[value; 4]).unwrap();
        }
        machine.session.board.write_physical(0x3000, &[3; 4]).unwrap();
        assert!(matches!(machine.run(16), RunOutcome::Completed));
        machine.require_running().unwrap();
        {
            let records = watch.records.borrow();
            assert_eq!(records.passive_retention_status(), PassiveRetentionStatus::Stopped(reason));
            assert_eq!(records.row_count(), 2);
            assert!(records.violation().is_none());
            assert_eq!(records.rows_since(0).unwrap()[1]["physical"], 0x3000);
        }
        assert!(watch.records.borrow_mut().arm_passive_retention().is_err());
        machine.session.board.memory_watch_end(&mut watch).unwrap();
    }
}

#[test]
fn missing_passive_fetch_on_protected_overlap_preserves_canonical_row_and_execution() {
    use fujimod_emulator::board::observation::{PassiveRetentionStatus, PassiveStopReason};
    let mut machine = machine([false, false], false);
    let code: Vec<_> = [0xe581_0000u32, 0xeaff_fffe].into_iter().flat_map(u32::to_le_bytes).collect();
    machine.session.board.write_physical(0x1000, &code).unwrap();
    machine.session.board.write_physical(0x5000, &code).unwrap();
    machine.session.board.write_register(0, Register::Pc, 0x5000).unwrap();
    machine.session.board.write_register(1, Register::Pc, 0x2000).unwrap();
    machine.session.board.write_register(0, arm::CORE[0], 0x1234_5678).unwrap();
    machine.session.board.write_register(0, arm::CORE[1], 0x3000).unwrap();
    let mut watch = passive_control_watch(&machine);
    watch.records.borrow_mut().arm_passive_retention().unwrap();
    let before = machine.instructions();
    assert!(matches!(machine.run(64), RunOutcome::Completed));
    machine.require_running().unwrap();
    assert_eq!(machine.instructions() - before, 64);
    {
        let records = watch.records.borrow();
        assert_eq!(records.passive_retention_status(),
            PassiveRetentionStatus::Stopped(PassiveStopReason::ProvenanceUnavailable));
        assert!(records.violation().is_none());
        let rows = records.rows_since(0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["source"], "cpu-write");
        assert_eq!(rows[0]["physical"], 0x3000);
        assert_eq!(rows[0]["pc"], 0x5000);
        assert_eq!(rows[0]["value"], 0x1234_5678);
        assert!(rows[0].get("latest_fetch").is_none());
        assert!(rows[0].get("core_instructions").is_none());
    }
    let mut bytes = [0; 4];
    machine.session.board.read_physical(0x3000, &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 0x1234_5678);
    watch.records.borrow_mut().stop_passive_retention(PassiveStopReason::End).unwrap();
    assert_eq!(watch.records.borrow().passive_retention_status(),
        PassiveRetentionStatus::Stopped(PassiveStopReason::ProvenanceUnavailable));
    machine.session.board.write_physical(0x4000, &[1; 4]).unwrap();
    assert_eq!(watch.records.borrow().row_count(), 1);
    for value in [1, 2] {
        machine.session.board.write_physical(0x3000, &[value; 4]).unwrap();
    }
    machine.session.board.write_register(0, Register::Pc, 0x1000).unwrap();
    require_protected_public_refusal(&mut machine, &watch, PublicAdvance::RunUntil);
    machine.session.board.memory_watch_end(&mut watch).unwrap();
}

#[test]
fn public_machine_consumes_a_standalone_physical_guard_refusal() {
    for api in [PublicAdvance::Run, PublicAdvance::RunUntil] {
        let mut machine = machine([false, false], true);
        machine.session.board.write_register(0, Register::Pc, 0x4000_1000).unwrap();
        machine.session.board.write_register(1, Register::Pc, 0x2000).unwrap();
        let mut watch = passive_control_watch(&machine);
        let counts = machine.session.board.instruction_counts();
        require_protected_public_refusal(&mut machine, &watch, api);
        assert_eq!(machine.session.board.instruction_counts(), counts);
        machine.session.board.memory_watch_end(&mut watch).unwrap();
    }
}

#[test]
fn legitimate_wait_until_deadline_is_not_a_native_execution_failure() {
    let mut machine = machine([false, false], false);
    machine.session.board.write_physical(0x1000, &0xe320_f003u32.to_le_bytes()).unwrap();
    let counts = machine.instructions();
    let deadline = machine.session.board.now() + 2;
    assert!(matches!(machine.run_until(64, Some(deadline)), RunOutcome::Completed));
    assert!(machine.instructions() - counts < 64);
    assert_eq!(machine.session.board.state()["waiting"], serde_json::json!([true, true]));
    assert!(machine.native_execution_refusal().is_none());
    machine.require_running().unwrap();
    for core in 0..2 {
        assert!(!machine.session.board.cpu(core).unwrap().execution_refused());
    }
}

#[test]
fn owned_physical_guard_failure_precedes_generic_native_refusal() {
    let InstalledCurrentCase { mut machine, config, .. } = installed_current_case(
        fujimod_xm1::modal_permit::CoreId(0), CurrentStoreCase::Valid);
    machine.traces.begin_modal_observation(&machine.session, config).unwrap();
    machine.session.board.write_register(0, Register::Pstate, 0xf3).unwrap();
    assert!(matches!(machine.run(8), RunOutcome::Failed));
    assert!(machine.traces.modal_observation_report().failure.is_some());
    assert!(machine.session.board.cpu(0).unwrap().execution_refused());
    assert!(machine.native_execution_refusal().is_none());
    let counts = machine.session.board.instruction_counts();
    assert!(matches!(machine.run_until(8, None), RunOutcome::Failed));
    assert_eq!(machine.session.board.instruction_counts(), counts);
    assert!(machine.native_execution_refusal().is_none());
}

#[test]
fn prelatched_protected_overflow_precedes_zero_budget_and_elapsed_deadline() {
    for api in [PublicAdvance::Run, PublicAdvance::RunUntil] {
        let mut machine = machine([false, false], false);
        let mut watch = passive_control_watch(&machine);
        for value in 0..4 {
            machine.session.board.write_physical(0x3000, &[value; 4]).unwrap();
        }
        let counts = machine.session.board.instruction_counts();
        let outcome = match api {
            PublicAdvance::Run => machine.run(0),
            PublicAdvance::RunUntil => {
                let deadline = machine.session.board.now();
                machine.run_until(8, Some(deadline))
            }
        };
        assert!(matches!(outcome, RunOutcome::Failed));
        assert_eq!(machine.session.board.instruction_counts(), counts);
        require_protected_public_refusal(&mut machine, &watch, api);
        assert_eq!(machine.session.board.instruction_counts(), counts);
        machine.session.board.memory_watch_end(&mut watch).unwrap();
    }
}

#[test]
fn prelatched_native_refusal_precedes_an_outstanding_capture_page() {
    use fujimod_xm1::trace::CaptureCursor;
    let mut machine = machine([false, false], false);
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "loop".into())])).unwrap();
    machine.traces.begin_capture_observation(CaptureScope::Capture).unwrap();
    assert!(matches!(machine.run(1000), RunOutcome::DrainRequired));
    let page = serde_json::to_value(machine.traces.capture_observation_page(CaptureCursor::new(0)).unwrap()).unwrap();
    let mut watch = passive_control_watch(&machine);
    for value in 0..4 {
        machine.session.board.write_physical(0x3000, &[value; 4]).unwrap();
    }
    let counts = machine.session.board.instruction_counts();
    require_protected_public_refusal(&mut machine, &watch, PublicAdvance::Run);
    assert_eq!(machine.session.board.instruction_counts(), counts);
    let retained = serde_json::to_value(machine.traces.capture_observation_page(CaptureCursor::new(0)).unwrap()).unwrap();
    assert_eq!(retained, page);
    machine.session.board.memory_watch_end(&mut watch).unwrap();
}
