use fujimod_emulator::board::{bus::{Bus, Mode}, factory::{self, Graph, MemoryMap, Processor, Ram}, memory::BusMapping};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, arm, timeline::Ratio};
use fujimod_emulator_exr2_plan::Resources;
use fujimod_xm1::{machine::Machine, peer_witness::{self, PeerContract}, run::RunDocument};
use fujimod_xm1::provenance::Guest;
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
