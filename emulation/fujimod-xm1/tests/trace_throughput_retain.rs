//! Retained persistent point throughput. Run with `cargo test --release --test trace_throughput_retain -- --ignored --nocapture`.
use fujimod_emulator::board::{bus::{Bus, Mode}, factory::{self, Graph, MemoryMap, Processor, Ram}, memory::BusMapping};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, timeline::Ratio};
use fujimod_emulator_exr2_plan::Resources;
use fujimod_xm1::machine::{Machine, RunOutcome};
use fujimod_xm1::run::RunDocument;
use fujimod_xm1::trace::DisarmedPoints;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Instant;

const INSTRUCTIONS: u64 = 20_000_000;
const REPETITIONS: usize = 5;
/// `add r0, r0, #1; b 0x1000` at 0x1000.
const LOOP: [u32; 2] = [0xe2800001, 0xeafffffd];
/// Mapped but outside the loop.
const SITES: [u32; 2] = [0x1800, 0x1900];
const LABEL: &str = "never";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Workload {
    MutableDisarmedRetained,
}

fn machine() -> Machine {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let graph = Graph {
        processors: (0..2).map(|core| Processor {
            model: Model::CortexR5F, rprofile: Some(Default::default()), mprofile: None, released: core == 0,
            registers: vec![(Register::Pc, 0x1000), (Register::Pstate, 0xd3)],
        }).collect(),
        ram: vec![Ram { base: 0, size: 0x4000 }],
        dma_mappings: vec![BusMapping::ram(0, 0x4000)], decoders: vec![], peripherals: vec![],
        mappings: (0..2).map(|core| MemoryMap { core, address: 0, physical: 0, size: 0x4000, permissions: 7 }).collect(),
        instructions_per_tick: Ratio::new(24, 1).unwrap(),
    };
    let mut board = graph
        .build(factory::Options { engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20 }, Bus::new(Mode::Strict), None)
        .unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    let code: Vec<u8> = LOOP.iter().flat_map(|word| word.to_le_bytes()).collect();
    board.write_physical(0x1000, &code).unwrap();
    let session = Session {
        definition: serde_json::Value::Null, board, controls: None, contact_driver: None,
        resources: Resources::default(), nodes: BTreeMap::new(), contacts: BTreeMap::new(), recording_frames: Default::default(),
    };
    Machine::new(RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap(), session).unwrap()
}

fn sites() -> BTreeMap<u32, String> {
    SITES.iter().map(|&address| (address, LABEL.to_owned())).collect()
}

fn prepare(machine: &mut Machine, workload: Workload) {
    match workload {
        Workload::MutableDisarmedRetained => {
            machine.traces.set_disarmed_points(DisarmedPoints::Retain);
            machine.traces.arm_mutable(&machine.session, sites()).unwrap();
            machine.traces.disarm(&machine.session).unwrap();
        }
    }
}

fn measure(workload: Workload) {
    let mut machine = machine();
    machine.traces.stop_labels.insert(LABEL.into());
    prepare(&mut machine, workload);
    let mut rates = Vec::with_capacity(REPETITIONS);
    for _ in 0..REPETITIONS {
        let start = machine.instructions();
        let began = Instant::now();
        assert_eq!(machine.run(INSTRUCTIONS), RunOutcome::Completed);
        let seconds = began.elapsed().as_secs_f64();
        assert_eq!(machine.instructions() - start, INSTRUCTIONS);
        rates.push(INSTRUCTIONS as f64 / seconds);
    }
    assert!(machine.traces.hits.is_empty());
    rates.sort_by(f64::total_cmp);
    println!("{}", serde_json::json!({
        "mode": workload, "instructions": INSTRUCTIONS,
        "median_ips": rates[REPETITIONS / 2], "best_ips": rates[REPETITIONS - 1],
    }));
}

#[test]
#[ignore]
fn mutable_disarmed_retained() {
    measure(Workload::MutableDisarmedRetained);
}
