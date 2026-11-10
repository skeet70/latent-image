use fujimod_emulator::board::{bus::{Bus, Mode}, factory::{self, Graph, MemoryMap, Processor, Ram}, memory::BusMapping};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, arm, timeline::Ratio};
use fujimod_emulator_exr2_plan::Resources;
use fujimod_xm1::{machine::{Machine, RunOutcome}, reset_observation::{AttachPhase, ResetObserver, Spec},
    run::RunDocument, trace::{DisarmedPoints, InstructionCount, TaskCapture}};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum FixtureTask { Primary33, Primary34, Secondary7 }
struct Fixture { machine: Machine }
impl Fixture {
    fn new(unsafe_alias: bool) -> Self {
        fujimod_emulator_exr2_board::register_board_builder();
        fujimod_emulator_models::register();
        let mappings = (0..2).flat_map(|core| [
            MemoryMap { core, address: 0, physical: 0, size: 0x100000, permissions: 7 },
            MemoryMap { core, address: 0x40000000, physical: if unsafe_alias && core == 1 { 0x100000 } else { 0 }, size: 0x100000, permissions: 7 },
        ]).collect();
        let graph = Graph {
            processors: (0..2).map(|core| Processor {
                model: Model::CortexR5F, rprofile: Some(Default::default()), mprofile: None, released: true,
                registers: vec![(Register::Pc, if core == 0 { 0x1000 } else { 0x2000 }), (Register::Pstate, 0xd3),
                    (Register::Arm32(arm::Register::R(13)), if core == 0 { 0x8000 } else { 0x9000 })],
            }).collect(),
            ram: vec![Ram { base: 0, size: 0x200000 }],
            dma_mappings: vec![BusMapping::ram(0, 0x200000)], decoders: vec![], peripherals: vec![], mappings,
            instructions_per_tick: Ratio::new(24, 1).unwrap(),
        };
        let mut board = graph.build(factory::Options { engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20 },
            Bus::new(Mode::Strict), None).unwrap();
        board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
        for address in [0x1000, 0x2000] {
            board.write_physical(address, &[0xe2800001u32, 0xeafffffd].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>()).unwrap();
        }
        let session = Session { definition: Value::Null, board, controls: None, contact_driver: None,
            resources: Resources::default(), nodes: BTreeMap::new(), contacts: BTreeMap::new(), recording_frames: Default::default() };
        let mut document = RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap();
        document.execution.quantum = 1;
        Self { machine: Machine::new(document, session).unwrap() }
    }
    fn word(&self, address: u64, value: u32) { self.machine.session.board.write_physical(address, &value.to_le_bytes()).unwrap(); }
    fn task(&self, task: FixtureTask) {
        let (current, node, creation, id, base) = match task {
            FixtureTask::Primary33 => (0x54760, 0x4006060c, 0x5c3ac, 33, 0x7000),
            FixtureTask::Primary34 => (0x54760, 0x40060644, 0x5c3d4, 34, 0x9000),
            FixtureTask::Secondary7 => (0xe4720, 0x400f001c, 0xebf5c, 7, 0x8000),
        };
        self.word(current, node);
        self.word(u64::from((node & !0x40000000) + 8), creation | 0x40000000);
        for (index, word) in [0, 0, 0x1000, 8, 4096, base, 0, 0, id, 1].into_iter().enumerate() {
            self.word(u64::from(creation) + index as u64 * 4, word);
        }
    }
    fn arm(&mut self, address: u32, task: TaskCapture) {
        self.machine.traces.arm_with_task_capture(&self.machine.session, BTreeMap::from([(address, "probe".into())]), task).unwrap();
        self.machine.traces.stop_labels.insert("probe".into());
    }
    fn last_hit(&self) -> Value { serde_json::to_value(self.machine.traces.hits.last().unwrap()).unwrap() }
}

#[test]
fn ordinary_trace_records_actual_core_and_logical_identity_on_both_kernels() {
    for (task, address, expected) in [
        (FixtureTask::Primary33, 0x1000, json!({"kind":"available","kernel":"a","task_id":33,
            "node":"0x4006060c","creation":"0x4005c3ac","stack":{"base":"0x00007000","bytes":4096},
            "context_depth":0,"cpsr":211,"sp":"0x00008000"})),
        (FixtureTask::Secondary7, 0x2000, json!({"kind":"available","kernel":"b","task_id":7,
            "node":"0x400f001c","creation":"0x400ebf5c","stack":{"base":"0x00008000","bytes":4096},
            "context_depth":0,"cpsr":211,"sp":"0x00009000"})),
    ] {
        let mut fixture = Fixture::new(false);
        fixture.task(task);
        fixture.arm(address, TaskCapture::Logical);
        assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
        let hit = fixture.last_hit();
        assert_eq!(hit["task"], expected);
        assert_eq!(hit["core"], if address == 0x1000 { 0 } else { 1 });
        assert_eq!(hit["cpsr"], hit["task"]["cpsr"]);
    }
}

#[test]
fn task_context_is_captured_at_entry_before_later_host_memory_changes() {
    let mut fixture = Fixture::new(false);
    fixture.task(FixtureTask::Primary33);
    fixture.arm(0x1000, TaskCapture::Logical);
    fixture.machine.session.board.run_window(100, 1, None, None).unwrap();
    fixture.task(FixtureTask::Primary34);
    let instructions = InstructionCount::from_board(&fixture.machine.session).unwrap();
    fixture.machine.traces.collect(&fixture.machine.session, instructions).unwrap();
    let hit = fixture.last_hit();
    assert_eq!(hit["task"]["task_id"], 33);
    assert_eq!(hit["task"]["node"], "0x4006060c");
    assert_eq!(hit["task"]["sp"], "0x00008000");
}

#[test]
fn resuming_a_stopped_entry_records_the_next_logical_task_without_stale_reuse() {
    let mut fixture = Fixture::new(false);
    fixture.task(FixtureTask::Primary33);
    fixture.arm(0x1000, TaskCapture::Logical);
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    fixture.task(FixtureTask::Primary34);
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    assert_eq!(fixture.machine.traces.hits.len(), 2);
    assert_eq!(fixture.last_hit()["task"]["task_id"], 34);
    assert_eq!(fixture.last_hit()["task"]["creation"], "0x4005c3d4");
}

#[test]
fn critical_sections_and_irq_context_keep_their_actual_mode_stack_and_depth() {
    for (cpsr, depth, sp) in [(0x53, 0, 0x8000), (0xd3, 0, 0x8000), (0xd2, 1, 0x9800)] {
        let mut fixture = Fixture::new(false);
        fixture.task(FixtureTask::Primary33);
        fixture.word(0x545a4, depth);
        fixture.machine.session.board.write_register(0, Register::Arm32(arm::Register::Cpsr), cpsr).unwrap();
        fixture.machine.session.board.write_register(0, Register::Arm32(arm::Register::R(13)), sp).unwrap();
        fixture.arm(0x1000, TaskCapture::Logical);
        assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
        let hit = fixture.last_hit();
        assert_eq!(hit["task"]["kind"], "available");
        assert_eq!(hit["task"]["cpsr"], cpsr);
        assert_eq!(hit["task"]["context_depth"], depth);
        assert_eq!(hit["task"]["sp"], format!("{sp:#010x}"));
    }
}

#[test]
fn missing_or_transitional_boot_identity_is_explicitly_unavailable_without_stopping_the_trace() {
    let mut fixture = Fixture::new(false);
    fixture.arm(0x1000, TaskCapture::Logical);
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    assert_eq!(fixture.last_hit()["task"], json!({"kind":"unavailable","reason":"no-current-task"}));
    fixture.task(FixtureTask::Primary33);
    fixture.word(0x5c3d0, 0);
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    assert_eq!(fixture.last_hit()["task"], json!({"kind":"unavailable","reason":"invalid-task-identity"}));
}

#[test]
fn unsafe_alias_refuses_before_guest_effects_and_retains_the_raw_hit() {
    let mut fixture = Fixture::new(true);
    fixture.task(FixtureTask::Primary33);
    fixture.arm(0x1000, TaskCapture::Logical);
    assert_eq!(fixture.machine.run(100), RunOutcome::Failed);
    assert_eq!(fixture.machine.session.board.read_register(0, Register::Arm32(arm::Register::R(0))).unwrap(), 0);
    assert_eq!(fixture.machine.session.board.cpu(0).unwrap().instruction_progress(), Some(0));
    let hit = fixture.last_hit();
    assert_eq!(hit["address"], "0x1000");
    assert!(hit.get("task").is_none());
}

#[test]
fn default_off_omits_task_wire_and_disarming_restores_unhooked_execution() {
    let mut fixture = Fixture::new(false);
    fixture.arm(0x1000, TaskCapture::Off);
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    assert!(fixture.last_hit().get("task").is_none());
    fixture.task(FixtureTask::Primary33);
    fixture.arm(0x1000, TaskCapture::Logical);
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    fixture.arm(0x1000, TaskCapture::Off);
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    assert!(fixture.last_hit().get("task").is_none());
    fixture.machine.traces.disarm(&fixture.machine.session).unwrap();
    assert_eq!(fixture.machine.run(4), RunOutcome::Completed);
    assert!(fixture.machine.session.board.read_register(0, Register::Arm32(arm::Register::R(0))).unwrap() > 0);
}

#[test]
fn reset_observation_preserves_general_registers_reads_and_optional_logical_context() {
    let mut fixture = Fixture::new(false);
    fixture.task(FixtureTask::Primary33);
    fixture.machine.session.board.write_register(0, Register::Arm32(arm::Register::R(12)), 0x1234).unwrap();
    let spec = Spec::parse(&serde_json::to_vec(&json!({"schema":1,"sites":{"0x00001000":"probe"},
        "reads":{"0x00001000":[[0x54760,4]]},"registers":"general","task":"logical", "hit_limit":8,
        "write_ranges":[[0x54760,0x54764]],"watch_limit":64})).unwrap()).unwrap();
    let mut observer = ResetObserver::begin(&mut fixture.machine, &spec).unwrap();
    fixture.machine.traces.stop_labels.insert("probe".into());
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    let hit = fixture.last_hit();
    assert_eq!(hit["task"]["task_id"], 33);
    assert_eq!(hit["registers"]["r12"], "0x1234");
    assert_eq!(hit["memory"], json!(["0c060640"]));
    observer.boundary(&fixture.machine, AttachPhase::Attached).unwrap();
    assert_eq!(serde_json::to_value(observer.finish().unwrap()).unwrap()["schema"], 1);
}

#[test]
fn unavailable_task_memory_does_not_invent_identity_or_abort_the_trace() {
    let mut fixture = Fixture::new(false);
    fixture.task(FixtureTask::Primary33);
    fixture.machine.session.board.cpu(0).unwrap().unmap(0x54000, 0x1000).unwrap();
    fixture.arm(0x1000, TaskCapture::Logical);
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    assert_eq!(fixture.last_hit()["task"], json!({"kind":"unavailable","reason":"unavailable-task-memory"}));
}

#[test]
fn an_identical_foreign_board_cannot_reuse_logical_context_ownership() {
    let mut first = Fixture::new(false);
    let mut second = Fixture::new(false);
    first.task(FixtureTask::Primary33);
    second.task(FixtureTask::Primary33);
    first.arm(0x1000, TaskCapture::Logical);
    std::mem::swap(&mut first.machine.session, &mut second.machine.session);
    assert_eq!(first.machine.run(100), RunOutcome::Failed);
    assert_eq!(first.machine.session.board.read_register(0, Register::Arm32(arm::Register::R(0))).unwrap(), 0);
    assert_eq!(first.machine.session.board.cpu(0).unwrap().instruction_progress(), Some(0));
}

#[test]
fn retained_mutable_points_discard_old_context_and_follow_new_membership() {
    let mut fixture = Fixture::new(false);
    fixture.task(FixtureTask::Primary33);
    fixture.arm(0x1000, TaskCapture::Logical);
    fixture.machine.traces.arm_mutable(&fixture.machine.session, BTreeMap::from([(0x1000, "probe".into())])).unwrap();
    fixture.machine.traces.set_disarmed_points(DisarmedPoints::Retain);
    fixture.machine.session.board.run_window(100, 1, None, None).unwrap();
    fixture.machine.traces.disarm(&fixture.machine.session).unwrap();
    fixture.machine.traces.rearm(&fixture.machine.session).unwrap();
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    assert_eq!(fixture.machine.traces.hits.len(), 1);
    assert_eq!(fixture.last_hit()["task"]["task_id"], 33);
    fixture.machine.traces.replace_sites(&fixture.machine.session, BTreeMap::from([(0x1004, "probe".into())])).unwrap();
    assert_eq!(fixture.machine.run(100), RunOutcome::Stopped);
    assert_eq!(fixture.last_hit()["address"], "0x1004");
    assert_eq!(fixture.last_hit()["task"]["task_id"], 33);
}

#[test]
fn reset_and_ordinary_selectors_reject_unknown_states_at_the_boundary() {
    for value in [json!("invented"), json!(0), Value::Null] {
        assert!(serde_json::from_value::<TaskCapture>(value.clone()).is_err());
        assert!(Spec::parse(&serde_json::to_vec(&json!({"schema":1,"sites":{"0x00001000":"probe"},
            "task":value,"hit_limit":8,"write_ranges":[[0x54760,0x54764]],"watch_limit":64})).unwrap()).is_err());
    }
}
