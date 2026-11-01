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
    assert!(machine.traces.remove_site(&machine.session, Guest(0x1000)).is_err());
    assert!(machine.traces.disarm(&machine.session).is_err());
    assert!(machine.traces.begin_capture_observation().is_err());
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
    machine.traces.begin_capture_observation().unwrap();
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
