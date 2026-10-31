use fujimod_emulator::board::{bus::{Bus, Mode}, factory::{self, Graph, MemoryMap, Processor, Ram}, memory::BusMapping};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, arm, timeline::Ratio};
use fujimod_emulator_exr2_plan::Resources;
use fujimod_xm1::{machine::Machine, peer_witness::{self, PeerContract}, run::RunDocument};
use fujimod_xm1::provenance::Guest;
use fujimod_xm1::machine::RunOutcome;
use fujimod_xm1::trace::{CaptureCursor, CaptureProtocolError, InstructionCount};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

fn machine(program: &[u32]) -> Machine {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let graph = Graph {
        processors: (0..2).map(|core| Processor {
            model: Model::CortexR5F, rprofile: Some(Default::default()), mprofile: None, released: true,
            registers: vec![(Register::Pc, 0x1000 + core as u64 * 0x1000), (Register::Pstate, 0xd3)],
        }).collect(),
        ram: vec![Ram { base: 0, size: 0x4000 }],
        dma_mappings: vec![BusMapping::ram(0, 0x4000)], decoders: vec![], peripherals: vec![],
        mappings: (0..2).map(|core| MemoryMap { core, address: 0, physical: 0, size: 0x4000, permissions: 7 }).collect(),
        instructions_per_tick: Ratio::new(24, 1).unwrap(),
    };
    let mut board = graph.build(factory::Options { engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20 }, Bus::new(Mode::Strict), None).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    let code: Vec<u8> = program.iter().flat_map(|word| word.to_le_bytes()).collect();
    for address in [0x1000, 0x2000] { board.write_physical(address, &code).unwrap(); }
    let session = Session {
        definition: serde_json::Value::Null, board, controls: None, contact_driver: None,
        resources: Resources::default(), nodes: BTreeMap::new(), contacts: BTreeMap::new(), recording_frames: Default::default(),
    };
    let document = RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap();
    Machine::new(document, session).unwrap()
}
fn context(machine: &Machine) -> Vec<(u64, u64)> {
    (0..2).map(|core| (
        machine.session.board.read_register(core, arm::CORE[0]).unwrap(),
        machine.session.board.read_register(core, Register::Pc).unwrap(),
    )).collect()
}

#[test]
fn one_entry_windows_preserve_the_two_core_quantum_phase() {
    let mut paused = machine(&[0xe2800001, 0xeafffffd]);
    let mut continuous = machine(&[0xe2800001, 0xeafffffd]);
    paused.run(997);
    for expected in [[998, 0], [999, 0], [1000, 0], [1000, 1], [1000, 2], [1000, 3]] {
        let before = paused.instructions();
        paused.run(1);
        paused.require_running().unwrap();
        assert_eq!(paused.instructions(), before + 1);
        assert_eq!(paused.session.board.instruction_counts(), expected);
    }
    paused.run(1048);
    continuous.run(2051);
    assert_eq!(paused.session.board.instruction_counts(), continuous.session.board.instruction_counts());
    assert_eq!(context(&paused), context(&continuous));
    assert!(paused.failure.is_none());
    assert!(continuous.failure.is_none());
}

#[test]
fn removing_another_site_preserves_only_an_unconsumed_entry() {
    let mut machine = machine(&[0xe2800001, 0xeafffffd]);
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "loop".into()), (0x1020, "unused-a".into()), (0x1030, "unused-b".into())])).unwrap();
    machine.traces.stop_labels.insert("loop".into());
    machine.run(10);
    assert_eq!(machine.instructions(), 0);
    assert_eq!(machine.traces.hits.len(), 1);
    machine.traces.remove_site(&machine.session, Guest(0x1020)).unwrap();
    machine.run(1);
    assert_eq!(machine.instructions(), 1);
    assert_eq!(machine.traces.hits.len(), 1);
    machine.run(1);
    assert_eq!(machine.instructions(), 2);
    machine.traces.remove_site(&machine.session, Guest(0x1030)).unwrap();
    machine.run(1);
    assert_eq!(machine.instructions(), 2);
    assert_eq!(machine.traces.hits.len(), 2);
    machine.run(1);
    assert_eq!(machine.instructions(), 3);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 2);
    assert!(machine.failure.is_none());
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
enum Source { CpuWrite, CpuRead, CpuFetch, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Deserialize)]
struct Row {
    source: Source, core: Option<usize>, pc: Option<u64>, instructions: Option<u64>,
    physical: u64, value: Option<u64>, before: Option<Vec<u8>>,
}
#[derive(Deserialize)]
struct Watch { records: Vec<Row>, violation: Option<String> }

#[test]
fn one_entry_store_has_attempt_metadata_and_independent_physical_readback() {
    let mut machine = machine(&[0xe5810000, 0xeafffffe]);
    machine.session.board.write_register(0, arm::CORE[0], 0x22b).unwrap();
    machine.session.board.write_register(0, arm::CORE[1], 0x3000).unwrap();
    let mut watch = machine.session.board.memory_watch_begin(vec![(0x3000, 0x3004)], 32, machine.session.board.mappings().to_vec()).unwrap();
    machine.run(1);
    let mut committed = [0; 4];
    machine.session.board.read_physical(0x3000, &mut committed).unwrap();
    let observed: Watch = serde_json::from_value(watch.records.borrow().state()).unwrap();
    machine.session.board.memory_watch_end(&mut watch).unwrap();
    let attempts: Vec<_> = observed.records.iter().filter(|row| row.source == Source::CpuWrite).collect();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].core, Some(0));
    assert_eq!(attempts[0].pc, Some(0x1000));
    assert_eq!(attempts[0].instructions, Some(1));
    assert_eq!(attempts[0].physical, 0x3000);
    assert_eq!(attempts[0].value, Some(0x22b));
    assert_eq!(attempts[0].before.as_deref(), Some(&[0, 0, 0, 0][..]));
    assert_eq!(committed, [0x2b, 2, 0, 0]);
    assert_eq!(machine.session.board.instruction_counts(), [1, 0]);
    assert_eq!(machine.session.board.read_register(0, Register::Pc).unwrap(), 0x1004);
    assert_eq!(machine.session.board.read_register(0, Register::Pstate).unwrap(), 0xd3);
    assert!(observed.violation.is_none());
    assert!(machine.failure.is_none());
}

#[test]
fn classified_watch_records_only_the_kinds_each_range_lists() {
    use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
    let mut machine = machine(&[0xe5912000, 0xe5810004, 0xeafffffe]);
    machine.session.board.write_register(0, arm::CORE[0], 0x22b).unwrap();
    machine.session.board.write_register(0, arm::CORE[1], 0x3000).unwrap();
    let classes = vec![
        WatchRange { start: 0x1000, end: 0x100c, kinds: WatchKinds::CPU_FETCH },
        WatchRange { start: 0x3000, end: 0x3004, kinds: WatchKinds::CPU_WRITE },
        WatchRange { start: 0x3004, end: 0x3008, kinds: WatchKinds::WRITES },
    ];
    let maps = machine.session.board.mappings().to_vec();
    let mut watch = machine.session.board.memory_watch_begin_classified(classes, 32, maps, ViewPolicy::Fixed).unwrap();
    machine.run(3);
    machine.session.board.write_physical(0x3004, &[1, 2, 3, 4]).unwrap();
    let mut readback = [0; 4];
    machine.session.board.read_physical(0x3004, &mut readback).unwrap();
    let observed: Watch = serde_json::from_value(watch.records.borrow().state()).unwrap();
    machine.session.board.memory_watch_end(&mut watch).unwrap();
    assert!(observed.violation.is_none());
    assert!(observed.records.iter().all(|row| row.source != Source::CpuRead && row.source != Source::HostRead));
    let writes: Vec<_> = observed.records.iter().filter(|row| row.source == Source::CpuWrite).collect();
    assert_eq!(writes.len(), 1);
    assert_eq!((writes[0].core, writes[0].pc, writes[0].physical, writes[0].value), (Some(0), Some(0x1004), 0x3004, Some(0x22b)));
    let fetches: Vec<_> = observed.records.iter().filter(|row| row.source == Source::CpuFetch).collect();
    assert!(!fetches.is_empty() && fetches.iter().all(|row| row.core == Some(0) && (0x1000..0x100c).contains(&row.physical)));
    assert_eq!(observed.records.iter().filter(|row| row.source == Source::HostWrite && row.physical == 0x3004).count(), 1);
    assert!(machine.failure.is_none());
}

#[test]
fn completed_load_and_its_next_unconsumed_point_share_an_entered_count() {
    let mut machine = machine(&[0xe5912000, 0xe3520000, 0xeafffffe]);
    machine.session.board.write_register(0, arm::CORE[1], 0x3000).unwrap();
    machine.session.board.write_physical(0x3000, &0x22bu32.to_le_bytes()).unwrap();
    let mut watch = machine.session.board.memory_watch_begin(vec![(0x3000, 0x3004)], 32, machine.session.board.mappings().to_vec()).unwrap();
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1004, "after-load".into())])).unwrap();
    machine.traces.stop_labels.insert("after-load".into());
    machine.run(10);
    machine.require_running().unwrap();
    let mut committed = [0; 4];
    machine.session.board.read_physical(0x3000, &mut committed).unwrap();
    let observed: Watch = serde_json::from_value(watch.records.borrow().state()).unwrap();
    machine.session.board.memory_watch_end(&mut watch).unwrap();
    let reads: Vec<_> = observed.records.iter().filter(|row| row.source == Source::CpuRead).collect();
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].core, Some(0));
    assert_eq!(reads[0].pc, Some(0x1000));
    assert_eq!(reads[0].instructions, Some(machine.instructions()));
    assert_eq!(reads[0].physical, 0x3000);
    assert_eq!(reads[0].value, Some(0x22b));
    assert_eq!(reads[0].before.as_deref(), Some(&[0x2b, 2, 0, 0][..]));
    assert_eq!(machine.session.board.instruction_counts(), [1, 0]);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[2]).unwrap(), 0x22b);
    assert_eq!(machine.session.board.read_register(0, Register::Pc).unwrap(), 0x1004);
    assert_eq!(machine.session.board.read_register(0, Register::Pstate).unwrap(), 0xd3);
    assert_eq!(committed, [0x2b, 2, 0, 0]);
    assert!(observed.violation.is_none());
    assert!(machine.failure.is_none());
}

#[derive(Deserialize)]
struct CaptureTotals { total: u64 }

#[test]
fn rejected_site_removal_preserves_active_capture_observations() {
    let mut machine = machine(&[0xe2800001, 0xeafffffd]);
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "loop".into())])).unwrap();
    machine.traces.begin_capture_observation().unwrap();
    assert!(machine.traces.remove_site(&machine.session, Guest(0x1000)).is_err());
    machine.run(3);
    machine.require_running().unwrap();
    let page = machine.traces.capture_observation_page(CaptureCursor::new(0)).unwrap();
    let totals: CaptureTotals = serde_json::from_value(serde_json::to_value(page).unwrap()).unwrap();
    assert_eq!(totals.total, 2);
    assert_eq!(machine.instructions(), 3);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 2);
    assert!(machine.traces.capture_observation_active());
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
struct StreamHit {
    sequence: CaptureCursor,
    core: usize,
    instructions: u64,
    registers: [u32; 4],
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
struct StreamSite {
    total: u64,
    captured: u64,
    pending: u64,
    dropped: u64,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
struct StreamPage {
    from: CaptureCursor,
    next: CaptureCursor,
    acknowledged: CaptureCursor,
    total: u64,
    captured: u64,
    pending: usize,
    dropped: u64,
    through_instructions: u64,
    boundary_complete: bool,
    hits: Vec<StreamHit>,
    sites: Vec<StreamSite>,
}

fn stream_page(machine: &mut Machine, cursor: CaptureCursor) -> StreamPage {
    let page = machine.traces.capture_observation_page(cursor).unwrap();
    serde_json::from_value(serde_json::to_value(page).unwrap()).unwrap()
}

fn drain_stream(machine: &mut Machine, cursor: &mut CaptureCursor, output: &mut Vec<StreamHit>) {
    loop {
        let instructions = InstructionCount::from_board(&machine.session).unwrap();
        machine.traces.collect(&machine.session, instructions).unwrap();
        let page = stream_page(machine, *cursor);
        assert_eq!(page.from, *cursor);
        assert_eq!(page.acknowledged, *cursor);
        assert_eq!(page.dropped, 0);
        assert!(page.sites.iter().all(|site| site.total == site.captured + site.dropped));
        assert!(page.sites.iter().all(|site| site.pending <= 64));
        assert!(page.pending <= 2048);
        assert!(page.hits.len() <= 64);
        if page.hits.is_empty() {
            assert!(page.boundary_complete);
            assert_eq!(page.through_instructions, machine.instructions());
            assert!(machine.traces.capture_drained());
            return;
        }
        output.extend(page.hits);
        machine.traces.acknowledge_capture_observation(page.next).unwrap();
        *cursor = page.next;
    }
}

#[test]
fn stream_pages_survive_wrong_and_duplicate_acknowledgements_without_reexecution() {
    let mut observed = machine(&[0xe2800001, 0xeafffffd]);
    observed.traces.arm(&observed.session, BTreeMap::from([(0x1000, "loop".into())])).unwrap();
    observed.traces.begin_capture_observation().unwrap();
    assert!(matches!(observed.traces.acknowledge_capture_observation(CaptureCursor::new(0)), Err(CaptureProtocolError::NoPublishedPage)));
    assert_eq!(observed.run(1000), RunOutcome::DrainRequired);
    let before = (observed.instructions(), context(&observed));
    let page = stream_page(&mut observed, CaptureCursor::new(0));
    assert_eq!(page.pending, 64);
    assert_eq!(page.captured, 64);
    assert!(!page.boundary_complete);
    assert_eq!(page.hits.iter().map(|hit| hit.sequence).collect::<Vec<_>>(),
        (0..64).map(CaptureCursor::new).collect::<Vec<_>>());
    assert!(matches!(observed.traces.acknowledge_capture_observation(CaptureCursor::new(63)), Err(CaptureProtocolError::WrongAcknowledgement)));
    assert!(matches!(observed.traces.capture_observation_page(CaptureCursor::new(1)), Err(CaptureProtocolError::WrongCursor)));
    assert_eq!(observed.run(1000), RunOutcome::DrainRequired);
    assert_eq!(stream_page(&mut observed, CaptureCursor::new(0)), page);
    assert_eq!((observed.instructions(), context(&observed)), before);
    observed.traces.acknowledge_capture_observation(page.next).unwrap();
    assert!(matches!(observed.traces.acknowledge_capture_observation(page.next), Err(CaptureProtocolError::NoPublishedPage)));
    assert!(!observed.traces.capture_drained());
    let mut events = page.hits;
    let mut cursor = page.next;
    drain_stream(&mut observed, &mut cursor, &mut events);
    assert_eq!(events.len(), 65);
    assert_eq!(events.last().unwrap().sequence, CaptureCursor::new(64));
    assert_eq!((observed.instructions(), context(&observed)), before);
}

#[test]
fn multicore_backpressure_preserves_order_counts_and_guest_state_beyond_sixty_four_hits() {
    let program = [0xe2800001, 0xeafffffd];
    let mut observed = machine(&program);
    let mut uninterrupted = machine(&program);
    let sites = BTreeMap::from([(0x1000, "primary".into()), (0x2000, "secondary".into())]);
    observed.traces.arm(&observed.session, sites.clone()).unwrap();
    uninterrupted.traces.arm(&uninterrupted.session, sites).unwrap();
    observed.traces.begin_capture_observation().unwrap();
    let endpoint = 5001;
    uninterrupted.run(endpoint);
    uninterrupted.require_running().unwrap();
    let mut events = Vec::new();
    let mut cursor = CaptureCursor::new(0);
    while observed.instructions() < endpoint {
        let outcome = observed.run(endpoint - observed.instructions());
        assert!(matches!(outcome, RunOutcome::Completed | RunOutcome::DrainRequired));
        drain_stream(&mut observed, &mut cursor, &mut events);
    }
    assert_eq!(observed.session.board.instruction_counts(), uninterrupted.session.board.instruction_counts());
    assert_eq!(context(&observed), context(&uninterrupted));
    assert!(events.len() > 64);
    assert!(events.iter().any(|hit| hit.core == 0));
    assert!(events.iter().any(|hit| hit.core == 1));
    assert_eq!(events.len(), uninterrupted.traces.hits.len());
    for (index, (event, expected)) in events.iter().zip(&uninterrupted.traces.hits).enumerate() {
        assert_eq!(event.sequence, CaptureCursor::new(index as u64));
        assert_eq!((event.core, event.instructions), (expected.core, expected.instructions));
        assert_eq!(event.registers[0], u32::from_str_radix(expected.registers["r0"].trim_start_matches("0x"), 16).unwrap());
    }
}

#[test]
fn aggregate_arena_yields_at_2048_details_and_reuses_only_acknowledged_storage() {
    let mut program = vec![0xe2800001; 33];
    program.push(0xeaffffdd);
    let mut observed = machine(&program);
    let sites = [0x1000, 0x2000].into_iter().flat_map(|base| {
        (0..33).map(move |index| (base + index * 4, format!("entry-{base:x}-{index}")))
    }).collect();
    observed.traces.arm(&observed.session, sites).unwrap();
    observed.traces.begin_capture_observation().unwrap();
    assert_eq!(observed.run(10_000), RunOutcome::DrainRequired);
    let page = stream_page(&mut observed, CaptureCursor::new(0));
    assert_eq!(page.pending, 2048);
    assert_eq!(page.hits.len(), 64);
    assert!(page.sites.iter().all(|site| site.pending < 64));
    let before = (observed.instructions(), context(&observed));
    observed.traces.acknowledge_capture_observation(page.next).unwrap();
    let mut events = page.hits;
    let mut cursor = page.next;
    drain_stream(&mut observed, &mut cursor, &mut events);
    assert!(events.len() > 2048);
    assert_eq!((observed.instructions(), context(&observed)), before);
    assert_eq!(events.iter().map(|hit| hit.sequence).collect::<Vec<_>>(),
        (0..events.len() as u64).map(CaptureCursor::new).collect::<Vec<_>>());
}

#[test]
fn finite_entry_endpoints_and_terminal_tail_collection_do_not_advance_the_guest() {
    let program = [0xe2800001, 0xeafffffd];
    let mut observed = machine(&program);
    let mut uninterrupted = machine(&program);
    observed.traces.arm(&observed.session, BTreeMap::from([(0x1000, "add".into()), (0x1004, "branch".into())])).unwrap();
    observed.traces.begin_capture_observation().unwrap();
    let mut events = Vec::new();
    let mut cursor = CaptureCursor::new(0);
    for endpoint in 1..=260 {
        observed.run(endpoint - observed.instructions());
        drain_stream(&mut observed, &mut cursor, &mut events);
        assert_eq!(observed.instructions(), endpoint);
    }
    uninterrupted.run(260);
    assert_eq!(context(&observed), context(&uninterrupted));
    assert_eq!(observed.session.board.instruction_counts(), uninterrupted.session.board.instruction_counts());
    assert_eq!(events.iter().map(|event| event.sequence).collect::<Vec<_>>(),
        (0..events.len() as u64).map(CaptureCursor::new).collect::<Vec<_>>());

    let mut failed = machine(&[0xe3a01902, 0xe5910000]);
    failed.traces.arm(&failed.session, BTreeMap::from([(0x1000, "prepare".into()), (0x1004, "unmapped-load".into())])).unwrap();
    failed.traces.begin_capture_observation().unwrap();
    assert_eq!(failed.run(100), RunOutcome::Failed);
    let failure = failed.failure.clone();
    let boundary = (failed.instructions(), context(&failed));
    let mut cursor = CaptureCursor::new(0);
    let mut tail = Vec::new();
    drain_stream(&mut failed, &mut cursor, &mut tail);
    assert_eq!(tail.iter().map(|hit| hit.registers[1]).collect::<Vec<_>>(), [0, 0x8000]);
    assert_eq!(failed.run(100), RunOutcome::Failed);
    assert_eq!(failed.failure, failure);
    assert_eq!((failed.instructions(), context(&failed)), boundary);
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
enum Control { HeldSentinel, DelayedRelease }
#[derive(Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum ResultStatus { Passed, Failed }
#[derive(Deserialize)]
struct FailedReceipt {
    schema: u32, control: Control, result: ResultStatus, error: Option<String>,
    instructions: u64, clear_readback: Option<()>,
}
static OUTPUT: AtomicU64 = AtomicU64::new(0);

#[test]
fn control_configuration_failure_still_writes_a_schema_two_receipt() {
    for (name, expected) in [("held-sentinel", Control::HeldSentinel), ("delayed-release", Control::DelayedRelease)] {
        let mut machine = machine(&[0xeafffffe]);
        machine.document.execution.quantum = 999;
        let contract = format!(r#"{{"schema":2,"control":"{name}","budget":10000000}}"#);
        let output = std::env::temp_dir().join(format!("peer-control-{}-{}", std::process::id(), OUTPUT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&output).unwrap();
        let result = peer_witness::run(machine, PeerContract::parse(contract.as_bytes()).unwrap(), &output);
        let receipt: FailedReceipt = serde_json::from_slice(&std::fs::read(output.join("peer-witness.json")).unwrap()).unwrap();
        std::fs::remove_dir_all(&output).unwrap();
        assert!(result.is_err());
        assert_eq!(receipt.schema, 2);
        assert_eq!(receipt.control, expected);
        assert_eq!(receipt.result, ResultStatus::Failed);
        assert!(receipt.error.is_some());
        assert_eq!(receipt.instructions, 0);
        assert!(receipt.clear_readback.is_none());
    }
}

#[cfg(feature = "scoped-watch")]
#[derive(Deserialize)]
struct ComposedFailure {
    schema: u32,
    result: ResultStatus,
    instructions: u64,
    initial_record_sha256: ObservedDigest,
    handoff_record_immutable_sha256: ObservedDigest,
    startup: ComposedBinding,
}

#[cfg(feature = "scoped-watch")]
#[derive(Deserialize)]
#[serde(untagged)]
enum ObservedDigest { Present(String), Absent(()) }

#[cfg(feature = "scoped-watch")]
impl ObservedDigest {
    fn assert_unobserved(self) {
        match self {
            Self::Absent(()) => (),
            Self::Present(digest) => panic!("configuration refusal unexpectedly observed identity {digest}"),
        }
    }
}

#[cfg(feature = "scoped-watch")]
#[derive(Deserialize)]
struct ComposedBinding {
    kind: ComposedKind,
    entry: u32,
    secondary_entry: u32,
    record1_sha256: String,
}

#[cfg(feature = "scoped-watch")]
#[derive(Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ComposedKind { Composed }

#[cfg(feature = "scoped-watch")]
#[test]
fn composed_configuration_refusal_keeps_its_schema_and_selected_startup_identity() {
    let mut machine = machine(&[0xeafffffe]);
    machine.document.execution.quantum = 999;
    let digest = "ab".repeat(32);
    let contract = format!(r#"{{"schema":5,"budget":50000000,"scenario":{{"kind":"natural"}},
        "program":{{"kind":"owned","record_sha256":"{digest}","record_immutable_sha256":"{digest}","loader_sha256":"{digest}",
        "handoff":66592,"entry":66604,"load":66700,"post_load":66704,"loaded_register":0,"poll_period":256,
        "image":[66560,68096],"refusal":null,"forbid_fetch":[[852000,852168],[854444,854480],[854596,854628]],
        "forbid_read":[[852168,852220]]}},
        "startup":{{"kind":"composed","record1_sha256":"{digest}","image_sha256":"{digest}",
        "symbols_sha256":"{digest}","source_sha256":"{digest}","image":[24576,25088],"entry":24576,"handoff":24608,
        "handoff_target":262176,"handoff_argument":6934008,"secondary_entry":66560,"secondary_image_sha256":"{digest}",
        "secondary_symbols_sha256":"{digest}","secondary_source_sha256":"{digest}"}}}}"#);
    let output = std::env::temp_dir().join(format!("peer-composed-{}-{}", std::process::id(), OUTPUT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir(&output).unwrap();
    let result = peer_witness::run(machine, PeerContract::parse(contract.as_bytes()).unwrap(), &output);
    let receipt: ComposedFailure = serde_json::from_slice(&std::fs::read(output.join("peer-witness.json")).unwrap()).unwrap();
    std::fs::remove_dir_all(&output).unwrap();
    assert!(result.is_err());
    assert_eq!(receipt.schema, 5);
    assert_eq!(receipt.result, ResultStatus::Failed);
    assert_eq!(receipt.instructions, 0);
    receipt.initial_record_sha256.assert_unobserved();
    receipt.handoff_record_immutable_sha256.assert_unobserved();
    assert_eq!(receipt.startup.kind, ComposedKind::Composed);
    assert_eq!(receipt.startup.entry, 0x6000);
    assert_eq!(receipt.startup.secondary_entry, 0x10400);
    assert_eq!(receipt.startup.record1_sha256, digest);
}

#[cfg(feature = "scoped-watch")]
#[derive(Deserialize)]
struct LoaderIdentityFailure {
    schema: u32,
    result: ResultStatus,
    instructions: u64,
    initial_record_sha256: ObservedDigest,
    handoff_record_immutable_sha256: ObservedDigest,
}

#[cfg(feature = "scoped-watch")]
#[test]
fn standalone_configuration_refusal_keeps_initial_and_handoff_identity_unobserved() {
    let mut machine = machine(&[0xeafffffe]);
    machine.document.execution.quantum = 999;
    let digest = "ab".repeat(32);
    let contract = format!(r#"{{"schema":4,"budget":50000000,"scenario":{{"kind":"natural"}},
        "program":{{"kind":"owned","record_sha256":"{digest}","record_immutable_sha256":"{digest}","loader_sha256":"{digest}",
        "handoff":66592,"entry":66604,"load":66700,"post_load":66704,"loaded_register":0,"poll_period":256,
        "image":[66560,68096],"refusal":null,"forbid_fetch":[[852000,852168],[854444,854480],[854596,854628]],
        "forbid_read":[[852168,852220]]}}}}"#);
    let output = std::env::temp_dir().join(format!("peer-loader-identity-{}-{}", std::process::id(), OUTPUT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir(&output).unwrap();
    let result = peer_witness::run(machine, PeerContract::parse(contract.as_bytes()).unwrap(), &output);
    let receipt: LoaderIdentityFailure = serde_json::from_slice(&std::fs::read(output.join("peer-witness.json")).unwrap()).unwrap();
    std::fs::remove_dir_all(&output).unwrap();
    assert!(result.is_err());
    assert_eq!(receipt.schema, 4);
    assert_eq!(receipt.result, ResultStatus::Failed);
    assert_eq!(receipt.instructions, 0);
    receipt.initial_record_sha256.assert_unobserved();
    receipt.handoff_record_immutable_sha256.assert_unobserved();
}

#[test]
fn strict_stop_pending_hook_is_collected_read_only_before_claiming_a_complete_boundary() {
    let mut observed = machine(&[0xe3a01006, 0xe3a02003, 0xe710f211]);
    observed.traces.arm(&observed.session, BTreeMap::from([(0x1008, "strict-stop".into())])).unwrap();
    observed.traces.begin_capture_observation().unwrap();
    assert_eq!(observed.run(100), RunOutcome::Failed);
    let failure = observed.failure.clone();
    let boundary = (observed.instructions(), context(&observed));
    let before = stream_page(&mut observed, CaptureCursor::new(0));
    assert!(before.hits.is_empty());
    assert!(!before.boundary_complete);
    let mut events = Vec::new();
    let mut cursor = CaptureCursor::new(0);
    drain_stream(&mut observed, &mut cursor, &mut events);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].registers[1..3], [6, 3]);
    assert_eq!(events[0].instructions, boundary.0);
    assert_eq!(observed.failure, failure);
    assert_eq!((observed.instructions(), context(&observed)), boundary);
}

#[test]
fn ff80_advance_returns_backpressure_and_reaches_the_original_endpoint_after_drain() {
    use fujimod_xm1::ff80::{Ff80, RendezvousInputs};
    let mut observed = machine(&[0xe2800001, 0xeafffffd]);
    observed.traces.arm(&observed.session, BTreeMap::from([(0x1000, "loop".into())])).unwrap();
    let mut bridge = Ff80::new(observed, RendezvousInputs::MenuUp);
    bridge.begin_capture_observation().unwrap();
    assert_eq!(bridge.advance(301).unwrap(), RunOutcome::DrainRequired);
    let mut cursor = CaptureCursor::new(0);
    let mut events = Vec::new();
    loop {
        drain_stream(&mut bridge.machine, &mut cursor, &mut events);
        let remaining = 301 - bridge.machine.instructions();
        if bridge.advance(remaining).unwrap() == RunOutcome::Completed { break; }
    }
    drain_stream(&mut bridge.machine, &mut cursor, &mut events);
    assert_eq!(bridge.machine.instructions(), 301);
    assert_eq!(bridge.machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 151);
    assert_eq!(events.len(), 151);
    assert!(bridge.machine.failure.is_none());
}

#[cfg(feature = "scoped-watch")]
mod composed_interleaving {
    use super::*;
    use fujimod_catalog::configuration::ResolvedConfiguration;
    use sha2::{Digest, Sha256};

    #[derive(Clone, Copy)]
    enum FirstHandoff { Early, Late, Missing, Invalid }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    #[serde(rename_all = "kebab-case")]
    enum Label {
        #[serde(rename = "record-1-entry")]
        Record1Entry,
        FirstStageHandoff,
        SecondaryEntry,
        #[serde(rename = "record-2-handoff")]
        Record2Handoff,
        PrefixEntry,
        PostLoad,
        HeldGate,
    }

    #[derive(Deserialize)]
    struct Stop {
        label: Label,
        core: usize,
        counter: Option<Counter>,
        watch_records: Option<usize>,
    }

    #[derive(Deserialize)]
    struct Counter { count: u64, nonzero_count: u64 }

    #[derive(Deserialize)]
    struct Receipt {
        result: ResultStatus,
        machine_failure: Option<String>,
        stops: Vec<Stop>,
        initial_record_sha256: Option<String>,
        handoff_record_immutable_sha256: Option<String>,
        scoped_watch: Option<Watch>,
    }

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn branch(from: u32, to: u32) -> u32 {
        0xea00_0000 | ((to.wrapping_sub(from + 8) >> 2) & 0x00ff_ffff)
    }

    fn put_word(bytes: &mut [u8], address: usize, value: u32) {
        bytes[address..address + 4].copy_from_slice(&value.to_le_bytes());
    }

    // The fixture executes handwritten ARM instructions, not recovered firmware.
    fn observe(order: FirstHandoff) -> (Result<(), String>, Receipt) {
        fujimod_emulator_exr2_board::register_board_builder();
        fujimod_emulator_models::register();
        let document = RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap();
        let resolved = ResolvedConfiguration::with_overlays(&document.catalog_configuration.0, &[]).unwrap();
        let mut graph = fujimod_emulator::boot::graph::assemble_with_starts(
            &resolved.reference, document.execution.instructions_per_tick, None, &[],
        ).unwrap();
        let decoder = &mut graph.decoders[0];
        let initial = &decoder.choices[decoder.initial];
        let ram = &decoder.choices[1];
        graph.mappings.retain(|map| !initial.processors.contains(map));
        graph.mappings.extend(ram.processors.iter().cloned());
        graph.dma_mappings.retain(|map| !initial.dma.contains(map));
        graph.dma_mappings.extend(ram.dma.iter().copied());
        decoder.initial = 1;
        for (core, processor) in graph.processors.iter_mut().enumerate() {
            processor.released = true;
            processor.registers = vec![
                (Register::Pc, if core == 0 { 0x6000 } else { 0x10400 }),
                (Register::Pstate, 0xd3),
                (arm::CORE[0], 0x69cdf8),
                (arm::CORE[1], 0x70000011),
                (arm::CORE[2], if matches!(order, FirstHandoff::Invalid) { 0x40024 } else { 0x40020 }),
                (arm::CORE[3], 0x1042c),
                (arm::CORE[4], if matches!(order, FirstHandoff::Early) { 1 } else { 600 }),
                (arm::CORE[5], 0x400a0000),
                (arm::CORE[13], 0x70000800),
            ];
        }
        let mut board = graph.build(
            factory::Options { engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20 },
            Bus::new(Mode::Strict), None,
        ).unwrap();
        board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
        let mut memory = vec![0; 0x61c410];
        for (address, value) in [
            (0x20, 0x6000),
            (0x4300, 0x40020),
            (0x4304, 0x69cdf8),
            (0x6000, 0xee091f31), // MCR p15,0,r1,c9,c1,1.
            (0x6004, 0xe2544001), // SUBS r4,r4,#1.
            (0x6008, 0x1afffffd), // BNE 0x6004.
            (0x600c, branch(0x600c, if matches!(order, FirstHandoff::Missing) { 0x40020 } else { 0x6020 })),
            (0x6020, 0xe1a0f002),
            (0x10020, 0x10400),
            (0x10400, 0xee091f31),
            (0x10404, branch(0x10404, 0x10420)),
            (0x10420, 0xe1a0f003),
            (0x1042c, 0xe59500c4), // LDR r0,[r5,#0xc4].
            (0x10430, branch(0x10430, 0x1042c)),
            (0x4000c, 0x40020),
            (0x40020, branch(0x40020, 0x40a00)),
            (0x40a00, 0xeafffffe),
            (0x61c40c, 0x69cdf8),
        ] {
            put_word(&mut memory, address, value);
        }
        let mut immutable = Sha256::new();
        immutable.update(&memory[0x10000..0x14c60]);
        immutable.update(&memory[0x14c6c..0x14c80]);
        let immutable = format!("{:x}", immutable.finalize());
        let record1 = digest(&memory[..0x10000]);
        let first_image = digest(&memory[0x6000..0x6040]);
        let record2 = digest(&memory[0x10000..0x14c80]);
        let second_image = digest(&memory[0x10400..0x10500]);
        let loader = digest(&memory[0xd0000..0xddaa0]);
        board.write_physical(0, &memory).unwrap();
        let session = Session {
            definition: serde_json::Value::Null, board, controls: None, contact_driver: None,
            resources: Resources::default(), nodes: BTreeMap::new(), contacts: BTreeMap::new(), recording_frames: Default::default(),
        };
        let machine = Machine::new(document, session).unwrap();
        let identity = "ab".repeat(32);
        let contract = format!(r#"{{"schema":5,"budget":50000000,"scenario":{{"kind":"held"}},
            "program":{{"kind":"owned","record_sha256":"{record2}","record_immutable_sha256":"{immutable}",
            "loader_sha256":"{loader}","handoff":66592,"entry":66604,"load":66604,"post_load":66608,
            "loaded_register":0,"poll_period":256,"image":[66560,66816],"refusal":null,
            "forbid_fetch":[[852000,852168],[854444,854480],[854596,854628]],"forbid_read":[[852168,852220]]}},
            "startup":{{"kind":"composed","record1_sha256":"{record1}","image_sha256":"{first_image}",
            "symbols_sha256":"{identity}","source_sha256":"{identity}","image":[24576,24640],"entry":24576,
            "handoff":24608,"handoff_target":262176,"handoff_argument":6934008,"secondary_entry":66560,
            "secondary_image_sha256":"{second_image}","secondary_symbols_sha256":"{identity}",
            "secondary_source_sha256":"{identity}"}}}}"#);
        let output = std::env::temp_dir().join(format!(
            "peer-composed-order-{}-{}", std::process::id(), OUTPUT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&output).unwrap();
        let result = peer_witness::run(machine, PeerContract::parse(contract.as_bytes()).unwrap(), &output);
        let receipt = serde_json::from_slice(&std::fs::read(output.join("peer-witness.json")).unwrap()).unwrap();
        std::fs::remove_dir_all(output).unwrap();
        (result, receipt)
    }

    #[test]
    fn composed_observation_accepts_both_cross_core_handoff_orders() {
        for order in [FirstHandoff::Early, FirstHandoff::Late] {
            let (result, receipt) = observe(order);
            assert!(result.is_ok(), "{result:?}");
            assert_eq!(receipt.result, ResultStatus::Passed);
            assert!(receipt.machine_failure.is_none());
            assert!(receipt.initial_record_sha256.is_some());
            assert!(receipt.handoff_record_immutable_sha256.is_some());
            let position = |label| receipt.stops.iter().position(|stop| stop.label == label).unwrap();
            let first = position(Label::FirstStageHandoff);
            let second = position(Label::Record2Handoff);
            assert!(position(Label::Record1Entry) < first);
            assert!(position(Label::SecondaryEntry) < second);
            assert!(second < position(Label::PrefixEntry));
            assert!(first < position(Label::HeldGate));
            assert_eq!(first < second, matches!(order, FirstHandoff::Early));
            assert_eq!(receipt.stops[first].core, 0);
            if matches!(order, FirstHandoff::Late) {
                assert!(position(Label::PrefixEntry) < first);
                assert!(receipt.stops[first].counter.as_ref().is_some_and(|counter| counter.count > 0 && counter.nonzero_count == 0));
                assert!(receipt.stops[first].watch_records.is_some());
            } else {
                assert!(receipt.stops[first].counter.is_none());
            }
            assert!(receipt.scoped_watch.unwrap().violation.is_none());
        }
    }

    #[test]
    fn composed_observation_refuses_missing_or_invalid_first_stage_handoff() {
        for order in [FirstHandoff::Missing, FirstHandoff::Invalid] {
            let (result, receipt) = observe(order);
            assert!(result.is_err());
            assert_eq!(receipt.result, ResultStatus::Failed);
            assert!(receipt.machine_failure.is_none());
            assert!(receipt.stops.iter().any(|stop| stop.label == Label::Record2Handoff));
            assert!(receipt.stops.iter().any(|stop| stop.label == Label::PrefixEntry));
            assert_eq!(
                receipt.stops.iter().any(|stop| stop.label == Label::FirstStageHandoff),
                matches!(order, FirstHandoff::Invalid),
            );
            assert!(receipt.scoped_watch.unwrap().violation.is_none());
        }
    }
}
