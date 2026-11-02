use fujimod_emulator::board::{bus::{Bus, Mode}, factory::{self, Graph, MemoryMap, Processor, Ram}, memory::BusMapping};
use fujimod_emulator::board::decoder::{Choice, Decoder};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, arm, timeline::Ratio};
use fujimod_emulator_cpu_unicorn::rprofile;
use fujimod_emulator_exr2_plan::Resources;
use fujimod_xm1::{machine::Machine, peer_witness::{self, PeerContract}, run::RunDocument};
use fujimod_xm1::provenance::Guest;
use fujimod_xm1::machine::RunOutcome;
use fujimod_xm1::trace::{
    CaptureCursor, CaptureMemory, CaptureProtocolError, EnteredInstructions, InstructionCount,
    ScopedHitCount, TraceSites, TraceStackScopeConfig, TraceStackSkip, TraceStackViolation,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

fn machine(program: &[u32]) -> Machine {
    machine_with_ram(program, 0x4000, RamViews::Physical)
}

#[derive(Clone, Copy)]
enum RamViews { Physical, AliasedWithDecoder }

fn fixture_graph(ram_bytes: usize, views: RamViews) -> Graph {
    let mut mappings = Vec::new();
    let mut decoder_maps = Vec::new();
    for core in 0..2 {
        match views {
            RamViews::Physical => mappings.push(MemoryMap { core, address: 0, physical: 0, size: ram_bytes, permissions: 7 }),
            RamViews::AliasedWithDecoder => {
                let low = MemoryMap { core, address: 0, physical: 0, size: 0x1000, permissions: 7 };
                mappings.push(low.clone());
                decoder_maps.push(low);
                mappings.push(MemoryMap { core, address: 0x1000, physical: 0x1000, size: ram_bytes - 0x1000, permissions: 7 });
                mappings.push(MemoryMap { core, address: 0x40000000, physical: 0, size: ram_bytes, permissions: 7 });
            }
        }
    }
    let decoders = if decoder_maps.is_empty() { vec![] } else {
        let alternate = decoder_maps.iter().map(|map| MemoryMap { physical: 0x4000, ..map.clone() }).collect();
        vec![Decoder { initial: 0, choices: vec![
            Choice { processors: decoder_maps, dma: vec![] },
            Choice { processors: alternate, dma: vec![] },
        ] }]
    };
    Graph {
        processors: (0..2).map(|core| Processor {
            model: Model::CortexR5F,
            rprofile: Some(match views {
                RamViews::Physical => Default::default(),
                RamViews::AliasedWithDecoder => rprofile::core::Options {
                    tcm: rprofile::tcm::Options {
                        atcm: rprofile::tcm::BankOptions { bytes: 0x1000, reset: 0 },
                        ..Default::default()
                    },
                    ..Default::default()
                },
            }),
            mprofile: None, released: true,
            registers: vec![(Register::Pc, 0x1000 + core as u64 * 0x1000), (Register::Pstate, 0xd3)],
        }).collect(),
        ram: vec![Ram { base: 0, size: ram_bytes }],
        dma_mappings: vec![BusMapping::ram(0, ram_bytes)], decoders, peripherals: vec![],
        mappings,
        instructions_per_tick: Ratio::new(24, 1).unwrap(),
    }
}

fn machine_with_ram(program: &[u32], ram_bytes: usize, views: RamViews) -> Machine {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let mut board = fixture_graph(ram_bytes, views)
        .build(factory::Options { engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20 }, Bus::new(Mode::Strict), None).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    let code: Vec<u8> = program.iter().flat_map(|word| word.to_le_bytes()).collect();
    for address in [0x1000, 0x2000] { board.write_physical(address, &code).unwrap(); }
    let session = Session {
        definition: serde_json::Value::Null, board, controls: None, contact_driver: None,
        resources: Resources::default(), nodes: BTreeMap::new(), contacts: BTreeMap::new(), recording_frames: Default::default(),
    };
    let document = RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap();
    let mut machine = Machine::new(document, session).unwrap();
    let memory = CaptureMemory::from_graph(fixture_graph(ram_bytes, views)).unwrap();
    machine.traces = TraceSites::with_memory(&machine.session, memory);
    machine
}
fn context(machine: &Machine) -> Vec<(u64, u64)> {
    (0..2).map(|core| (
        machine.session.board.read_register(core, arm::CORE[0]).unwrap(),
        machine.session.board.read_register(core, Register::Pc).unwrap(),
    )).collect()
}

fn stack_scope_config() -> TraceStackScopeConfig {
    TraceStackScopeConfig::new([Guest(0x3100), Guest(0x3300)], Guest(0x3500), [Guest(0x3000), Guest(0x3004)]).unwrap()
}

fn begin_stack_scope(machine: &mut Machine) {
    for cell in [0x3000, 0x3004] {
        machine.session.board.write_physical(cell, &0x3600u32.to_le_bytes()).unwrap();
    }
    machine.traces.begin_stack_scope(stack_scope_config()).unwrap();
}

#[test]
fn outside_non_owner_points_are_drained_and_counted_without_spending_full_hits_or_stopping() {
    let mut machine = machine(&[0xe2800001, 0xeafffffd]);
    machine.session.board.write_register(0, arm::CORE[0], 0).unwrap();
    machine.session.board.write_register(0, arm::CORE[13], 0x3800).unwrap();
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "shared".into())])).unwrap();
    machine.traces.stop_labels.insert("shared".into());
    machine.traces.bound_full_registers(std::num::NonZeroUsize::new(1).unwrap());
    begin_stack_scope(&mut machine);
    machine.traces.scope_stack_sites(&machine.session, &[Guest(0x1000)]).unwrap();

    for entered in [6, 12] {
        assert_eq!(machine.run(6), RunOutcome::Completed);
        assert_eq!(machine.instructions(), entered);
        assert!(machine.traces.hits.is_empty());
        let skips: Vec<_> = machine.traces.drain_stack_scope_skips().collect();
        assert_eq!(skips, [entered - 6, entered - 4, entered - 2].map(|instructions| TraceStackSkip {
            core: 0, site: Guest(0x1000), instructions: EnteredInstructions(instructions),
            sp: Guest(0x3800), current: Guest(0x3600),
        }));
        machine.traces.scope_stack_sites(&machine.session, &[Guest(0x1000)]).unwrap();
        let summary = machine.traces.stack_scope_summary().unwrap();
        assert_eq!(summary.sites.len(), 1);
        assert_eq!(summary.sites[0].entered, EnteredInstructions(0));
        assert_eq!(summary.sites[0].outside, [ScopedHitCount(entered / 2), ScopedHitCount(0)]);
        assert_eq!(summary.sites[0].recorded, [ScopedHitCount(0); 2]);
        assert!(summary.violation.is_none());
    }
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 6);
    machine.session.board.write_register(0, arm::CORE[13], 0x3200).unwrap();
    machine.session.board.write_physical(0x3000, &0x3500u32.to_le_bytes()).unwrap();
    assert_eq!(machine.run(10), RunOutcome::Stopped);
    assert_eq!(machine.instructions(), 12);
    assert_eq!(machine.traces.hits.len(), 1);
    assert_eq!(machine.traces.stack_scope_summary().unwrap().sites[0].recorded, [ScopedHitCount(1), ScopedHitCount(0)]);
    machine.session.board.write_register(0, arm::CORE[13], 0x3800).unwrap();
    machine.session.board.write_physical(0x3000, &0x3600u32.to_le_bytes()).unwrap();
    assert_eq!(machine.run(6), RunOutcome::Completed);
    assert_eq!(machine.instructions(), 18);
    assert_eq!(machine.traces.hits.len(), 1);
    assert_eq!(machine.traces.drain_stack_scope_skips().count(), 2);
    assert_eq!(machine.traces.stack_scope_summary().unwrap().sites[0].outside, [ScopedHitCount(8), ScopedHitCount(0)]);
}

#[test]
fn scoped_owner_keeps_full_registers_stops_and_the_original_full_hit_limit() {
    let mut machine = machine(&[0xe2800001, 0xeafffffd]);
    for index in 0..13 {
        machine.session.board.write_register(0, arm::CORE[index], 0x120 + index as u64).unwrap();
    }
    machine.session.board.write_register(0, arm::CORE[13], 0x3200).unwrap();
    machine.session.board.write_register(0, arm::CORE[14], 0x1234).unwrap();
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "shared".into())])).unwrap();
    machine.traces.stop_labels.insert("shared".into());
    machine.traces.bound_full_registers(std::num::NonZeroUsize::new(1).unwrap());
    begin_stack_scope(&mut machine);
    machine.session.board.write_physical(0x3000, &0x3500u32.to_le_bytes()).unwrap();
    machine.traces.scope_stack_sites(&machine.session, &[Guest(0x1000)]).unwrap();

    assert_eq!(machine.run(10), RunOutcome::Stopped);
    assert_eq!(machine.instructions(), 0);
    let hit = &machine.traces.hits[0];
    assert_eq!(hit.registers.len(), 13);
    assert_eq!(hit.registers["r0"], "0x120");
    assert_eq!(hit.registers["r12"], "0x12c");
    assert_eq!(hit.sp, "0x3200");
    assert_eq!(hit.lr, "0x1234");
    assert_eq!(hit.cpsr, 0xd3);
    assert_eq!(machine.run(1), RunOutcome::Completed);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 0x121);
    assert_eq!(machine.run(2), RunOutcome::Failed);
    assert_eq!(machine.traces.hits.len(), 1);
    assert_eq!(machine.traces.stack_scope_summary().unwrap().sites[0].recorded, [ScopedHitCount(1), ScopedHitCount(0)]);
    assert_eq!(machine.traces.drain_stack_scope_skips().count(), 0);
}

#[test]
fn scoped_owner_outside_either_stack_edge_refuses_with_the_exact_omission() {
    for sp in [0x30fc, 0x40003304, 0xc0003200] {
        let mut machine = machine(&[0xe2800001, 0xeafffffd]);
        machine.session.board.write_register(0, arm::CORE[13], sp).unwrap();
        machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "shared".into())])).unwrap();
        begin_stack_scope(&mut machine);
        machine.session.board.write_physical(0x3000, &0x40003500u32.to_le_bytes()).unwrap();
        machine.traces.scope_stack_sites(&machine.session, &[Guest(0x1000)]).unwrap();

        assert_eq!(machine.run(1), RunOutcome::Failed);
        assert_eq!(machine.instructions(), 0);
        assert!(machine.traces.hits.is_empty());
        let hit = TraceStackSkip {
            core: 0, site: Guest(0x1000), instructions: EnteredInstructions(0),
            sp: Guest(sp as u32), current: Guest(0x3500),
        };
        assert_eq!(machine.traces.drain_stack_scope_skips().collect::<Vec<_>>(), [hit]);
        let summary = machine.traces.stack_scope_summary().unwrap();
        assert_eq!(summary.sites[0].outside, [ScopedHitCount(1), ScopedHitCount(0)]);
        assert_eq!(summary.violation, Some(TraceStackViolation::OwnerOutsideStack { hit }));
        assert_eq!(machine.run(1), RunOutcome::Failed);
        assert_eq!(machine.instructions(), 0);
    }
}

#[test]
fn stack_edges_and_known_dram_aliases_remain_in_scope() {
    for sp in [0x3100, 0x40003300] {
        let mut machine = machine(&[0xeafffffe]);
        machine.session.board.write_register(0, arm::CORE[13], sp).unwrap();
        machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "shared".into())])).unwrap();
        machine.traces.stop_labels.insert("shared".into());
        begin_stack_scope(&mut machine);
        machine.session.board.write_physical(0x3000, &0x40003500u32.to_le_bytes()).unwrap();
        machine.traces.scope_stack_sites(&machine.session, &[Guest(0x1000)]).unwrap();
        assert_eq!(machine.run(1), RunOutcome::Stopped);
        assert_eq!(machine.traces.hits.len(), 1);
        assert_eq!(machine.traces.hits[0].sp, format!("{sp:#x}"));
        assert_eq!(machine.traces.drain_stack_scope_skips().count(), 0);
    }
}

#[test]
fn stack_scope_counts_guest_aliases_and_executing_core_slots_separately() {
    let mut machine = machine_with_ram(&[0xeafffffe], 0x5000, RamViews::AliasedWithDecoder);
    machine.session.board.write_register(1, Register::Pc, 0x40001000).unwrap();
    machine.session.board.write_register(0, arm::CORE[13], 0x40003100).unwrap();
    machine.session.board.write_register(1, arm::CORE[13], 0x3800).unwrap();
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "physical".into()), (0x40001000, "alias".into())])).unwrap();
    machine.traces.bound_full_registers(std::num::NonZeroUsize::new(1000).unwrap());
    machine.traces.begin_stack_scope(TraceStackScopeConfig::new(
        [Guest(0x3100), Guest(0x3300)], Guest(0x3500), [Guest(0x3004), Guest(0x3000)],
    ).unwrap()).unwrap();
    machine.session.board.write_physical(0x3004, &0x40003500u32.to_le_bytes()).unwrap();
    machine.session.board.write_physical(0x3000, &0x40003600u32.to_le_bytes()).unwrap();
    machine.traces.scope_stack_sites(&machine.session, &[Guest(0x1000), Guest(0x40001000)]).unwrap();

    assert_eq!(machine.run(1001), RunOutcome::Completed);
    assert_eq!(machine.session.board.instruction_counts(), [1000, 1]);
    assert_eq!(machine.traces.hits.len(), 1000);
    assert!(machine.traces.hits.iter().all(|hit| hit.core == 0 && hit.address == "0x1000"));
    assert_eq!(machine.traces.drain_stack_scope_skips().collect::<Vec<_>>(), [TraceStackSkip {
        core: 1, site: Guest(0x40001000), instructions: EnteredInstructions(1000),
        sp: Guest(0x3800), current: Guest(0x3600),
    }]);
    let summary = machine.traces.stack_scope_summary().unwrap();
    assert_eq!(summary.sites[0].recorded, [ScopedHitCount(1000), ScopedHitCount(0)]);
    assert_eq!(summary.sites[0].outside, [ScopedHitCount(0); 2]);
    assert_eq!(summary.sites[1].recorded, [ScopedHitCount(0); 2]);
    assert_eq!(summary.sites[1].outside, [ScopedHitCount(0), ScopedHitCount(1)]);
    machine.session.board.write_register(1, Register::Pc, 0x1000).unwrap();
    assert_eq!(machine.run(1), RunOutcome::Completed);
    assert_eq!(machine.traces.hits.len(), 1000);
    assert_eq!(machine.traces.drain_stack_scope_skips().collect::<Vec<_>>(), [TraceStackSkip {
        core: 1, site: Guest(0x1000), instructions: EnteredInstructions(1001),
        sp: Guest(0x3800), current: Guest(0x3600),
    }]);
    assert_eq!(machine.traces.stack_scope_summary().unwrap().sites[0].outside, [ScopedHitCount(0), ScopedHitCount(1)]);
}

#[test]
fn dynamic_scope_registration_preserves_ordinary_stops_and_refuses_scoped_site_removal() {
    let mut machine = machine(&[0xe2800001, 0xe2800002, 0xeafffffe]);
    machine.session.board.write_register(0, arm::CORE[0], 0).unwrap();
    machine.session.board.write_register(0, arm::CORE[13], 0x3800).unwrap();
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "entry".into())])).unwrap();
    machine.traces.stop_labels.insert("entry".into());
    assert_eq!(machine.run(10), RunOutcome::Stopped);
    begin_stack_scope(&mut machine);
    machine.traces.scope_stack_sites(&machine.session, &[Guest(0x1000)]).unwrap();
    machine.traces.scope_stack_sites(&machine.session, &[Guest(0x1004)]).unwrap();
    machine.traces.add_sites(&machine.session, BTreeMap::from([(0x1004, "dynamic".into()), (0x1008, "unfiltered".into())])).unwrap();
    assert!(machine.traces.remove_sites(&machine.session, &[Guest(0x1004)]).is_err());
    assert_eq!(machine.run(10), RunOutcome::Stopped);
    assert_eq!(machine.instructions(), 2);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 3);
    assert_eq!(machine.traces.hits.len(), 2);
    assert_eq!(machine.traces.hits[1].address, "0x1008");
    assert_eq!(machine.traces.drain_stack_scope_skips().collect::<Vec<_>>(), [TraceStackSkip {
        core: 0, site: Guest(0x1004), instructions: EnteredInstructions(1),
        sp: Guest(0x3800), current: Guest(0x3600),
    }]);
}

#[test]
fn undrained_stack_omissions_refuse_at_the_fixed_bound_without_truncation() {
    let mut machine = machine(&[0xeafffffe]);
    machine.session.board.write_register(0, arm::CORE[13], 0x3800).unwrap();
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "shared".into())])).unwrap();
    machine.traces.bound_full_registers(std::num::NonZeroUsize::new(1).unwrap());
    begin_stack_scope(&mut machine);
    machine.traces.scope_stack_sites(&machine.session, &[Guest(0x1000)]).unwrap();
    assert_eq!(machine.run(200000), RunOutcome::Failed);
    assert!(machine.traces.hits.is_empty());
    assert_eq!(machine.traces.drain_stack_scope_skips().count(), 65536);
    let summary = machine.traces.stack_scope_summary().unwrap();
    assert_eq!(summary.sites[0].outside, [ScopedHitCount(65536), ScopedHitCount(0)]);
    assert!(matches!(summary.violation, Some(TraceStackViolation::SkipLimit { hit })
        if hit.core == 0 && hit.site == Guest(0x1000) && hit.sp == Guest(0x3800) && hit.current == Guest(0x3600)));
}

#[test]
fn stack_scope_deserialization_keeps_constructor_validation_and_capture_exclusion() {
    let config = stack_scope_config();
    assert_eq!(serde_json::from_value::<TraceStackScopeConfig>(serde_json::to_value(config).unwrap()).unwrap(), config);
    assert_eq!(config.stack(), [Guest(0x3100), Guest(0x3300)]);
    assert_eq!(config.owner(), Guest(0x3500));
    assert_eq!(config.current(), [Guest(0x3000), Guest(0x3004)]);
    for invalid in [
        serde_json::json!({"stack":[0x3300,0x3100],"owner":0x3500,"current":[0x3000,0x3004]}),
        serde_json::json!({"stack":[0x3100,0x40003300],"owner":0x3500,"current":[0x3000,0x3004]}),
        serde_json::json!({"stack":[0x3100,0x3300],"owner":0,"current":[0x3000,0x3004]}),
        serde_json::json!({"stack":[0x3100,0x3300],"owner":0x3500,"current":[0x3000,0x3000]}),
        serde_json::json!({"stack":[0x3100,0x3300],"owner":0x3500,"current":[0x3001,0x3004]}),
    ] {
        assert!(serde_json::from_value::<TraceStackScopeConfig>(invalid).is_err());
    }
    let mut traces = TraceSites::default();
    traces.begin_stack_scope(config).unwrap();
    assert!(traces.begin_stack_scope(config).is_err());
    assert!(traces.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).is_err());
    let mut capture = TraceSites::default();
    capture.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).unwrap();
    assert!(capture.begin_stack_scope(config).is_err());
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
    machine.traces.remove_sites(&machine.session, &[Guest(0x1020)]).unwrap();
    machine.run(1);
    assert_eq!(machine.instructions(), 1);
    assert_eq!(machine.traces.hits.len(), 1);
    machine.run(1);
    assert_eq!(machine.instructions(), 2);
    machine.traces.remove_sites(&machine.session, &[Guest(0x1030)]).unwrap();
    machine.run(1);
    assert_eq!(machine.instructions(), 2);
    assert_eq!(machine.traces.hits.len(), 2);
    machine.run(1);
    assert_eq!(machine.instructions(), 3);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 2);
    assert!(machine.failure.is_none());
}

#[test]
fn a_drained_stop_label_stops_again_when_its_next_site_is_added() {
    let mut machine = machine(&[0xe2800001, 0xe2800002, 0xe2800004, 0xeafffffe]);
    machine.session.board.write_register(0, arm::CORE[0], 0).unwrap();
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "temporary".into())])).unwrap();
    machine.traces.stop_labels.insert("temporary".into());
    machine.run(10);
    assert_eq!(machine.instructions(), 0);
    for (removed, added, entered, value) in [(0x1000, 0x1004, 1, 1), (0x1004, 0x1008, 2, 3)] {
        machine.traces.remove_sites(&machine.session, &[Guest(removed)]).unwrap();
        machine.traces.add_sites(&machine.session, BTreeMap::from([(added, "temporary".into())])).unwrap();
        machine.run(10);
        machine.require_running().unwrap();
        assert_eq!(machine.instructions(), entered);
        assert_eq!(machine.session.board.read_register(0, Register::Pc).unwrap(), u64::from(added));
        assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), value);
    }
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
fn fetch_watch_after_points_uses_rolled_back_stops_and_resumed_entries() {
    use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
    let mut machine = machine(&[0xe2800001, 0xe2800002, 0xeafffffe]);
    machine.session.board.write_register(0, arm::CORE[0], 0).unwrap();
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "entry".into()), (0x1004, "next".into())])).unwrap();
    machine.traces.stop_labels = ["entry".into(), "next".into()].into_iter().collect();
    machine.run(10);
    assert_eq!(machine.instructions(), 0);
    let ranges = || vec![WatchRange { start: 0x1000, end: 0x1008, kinds: WatchKinds::CPU_FETCH }];
    let mut watch = machine.session.board.memory_watch_begin_classified(ranges(), 8, machine.session.board.mappings().to_vec(), ViewPolicy::Observe).unwrap();
    machine.run(10);
    machine.require_running().unwrap();
    let observed: Watch = serde_json::from_value(watch.records.borrow().state()).unwrap();
    machine.session.board.memory_watch_end(&mut watch).unwrap();
    assert!(observed.violation.is_none());
    let fetches: Vec<_> = observed.records.iter().filter(|row| row.source == Source::CpuFetch).map(|row| (row.core, row.pc, row.instructions)).collect();
    assert_eq!(fetches, [(Some(0), Some(0x1000), Some(1)), (Some(0), Some(0x1004), Some(1))]);
    assert_eq!(machine.instructions(), 1);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 1);
    assert_eq!(machine.session.board.read_register(0, Register::Pc).unwrap(), 0x1004);
    let mut watch = machine.session.board.memory_watch_begin_classified(ranges(), 8, machine.session.board.mappings().to_vec(), ViewPolicy::Observe).unwrap();
    machine.run(1);
    machine.require_running().unwrap();
    let resumed: Watch = serde_json::from_value(watch.records.borrow().state()).unwrap();
    machine.session.board.memory_watch_end(&mut watch).unwrap();
    assert!(resumed.violation.is_none());
    let fetches: Vec<_> = resumed.records.iter().filter(|row| row.source == Source::CpuFetch).map(|row| (row.core, row.pc, row.instructions)).collect();
    assert_eq!(fetches, [(Some(0), Some(0x1004), Some(2))]);
    assert_eq!(machine.instructions(), 2);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 3);
}

#[test]
fn reopening_a_fetch_watch_after_dynamic_points_restores_stop_and_resume_clocks() {
    use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
    let mut machine = machine(&[0xe2800001, 0xe2800002, 0xeafffffe]);
    machine.session.board.write_register(0, arm::CORE[0], 0).unwrap();
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "entry".into())])).unwrap();
    machine.traces.stop_labels.insert("entry".into());
    assert_eq!(machine.run(10), RunOutcome::Stopped);
    assert_eq!(machine.instructions(), 0);
    let ranges = || vec![WatchRange { start: 0x1000, end: 0x100c, kinds: WatchKinds::CPU_FETCH }];
    let mut watch = machine.session.board.memory_watch_begin_classified(ranges(), 8, machine.session.board.mappings().to_vec(), ViewPolicy::Observe).unwrap();
    machine.traces.add_sites(&machine.session, BTreeMap::from([(0x1004, "next".into()), (0x1008, "last".into())])).unwrap();
    assert_eq!(machine.run(10), RunOutcome::Stopped);
    machine.require_running().unwrap();
    let observed: Watch = serde_json::from_value(watch.records.borrow().state()).unwrap();
    machine.session.board.memory_watch_end(&mut watch).unwrap();
    assert!(observed.violation.is_none());
    let fetches: Vec<_> = observed.records.iter().filter(|row| row.source == Source::CpuFetch).map(|row| (row.core, row.pc, row.instructions)).collect();
    assert_eq!(fetches, [(Some(0), Some(0x1000), Some(1)), (Some(0), Some(0x1004), Some(2))]);
    assert_eq!(machine.instructions(), 1);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 1);
    let mut watch = machine.session.board.memory_watch_begin_classified(ranges(), 8, machine.session.board.mappings().to_vec(), ViewPolicy::Observe).unwrap();
    assert_eq!(machine.run(10), RunOutcome::Stopped);
    machine.require_running().unwrap();
    let resumed: Watch = serde_json::from_value(watch.records.borrow().state()).unwrap();
    machine.session.board.memory_watch_end(&mut watch).unwrap();
    assert!(resumed.violation.is_none());
    let fetches: Vec<_> = resumed.records.iter().filter(|row| row.source == Source::CpuFetch).map(|row| (row.core, row.pc, row.instructions)).collect();
    assert_eq!(fetches, [(Some(0), Some(0x1004), Some(2)), (Some(0), Some(0x1008), Some(2))]);
    assert_eq!(machine.instructions(), 2);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 3);
    assert_eq!(machine.session.board.read_register(0, Register::Pc).unwrap(), 0x1008);
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

#[test]
fn an_unconsumed_point_at_a_window_endpoint_stops_without_a_new_entry() {
    let mut machine = machine(&[0xe2800001, 0xe2800002, 0xeafffffe]);
    machine.session.board.write_register(0, arm::CORE[0], 0).unwrap();
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1004, "after-window".into())])).unwrap();
    machine.traces.stop_labels.insert("after-window".into());
    assert_eq!(machine.run(1), fujimod_xm1::machine::RunOutcome::Completed);
    assert_eq!(machine.instructions(), 1);
    assert_eq!(machine.session.board.read_register(0, Register::Pc).unwrap(), 0x1004);
    assert_eq!(machine.run(1), fujimod_xm1::machine::RunOutcome::Stopped);
    assert_eq!(machine.instructions(), 1);
    assert_eq!(machine.traces.hits.len(), 1);
    assert_eq!(machine.traces.hits[0].instructions, 1);
    assert_eq!(machine.traces.hits[0].address, "0x1004");
    assert_eq!(machine.run(1), fujimod_xm1::machine::RunOutcome::Completed);
    machine.require_running().unwrap();
    assert_eq!(machine.instructions(), 2);
    assert_eq!(machine.session.board.read_register(0, arm::CORE[0]).unwrap(), 3);
}

#[derive(Deserialize)]
struct CaptureTotals { total: u64 }

#[test]
fn rejected_site_removal_preserves_active_capture_observations() {
    let mut machine = machine(&[0xe2800001, 0xeafffffd]);
    machine.traces.arm(&machine.session, BTreeMap::from([(0x1000, "loop".into())])).unwrap();
    machine.traces.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).unwrap();
    assert!(machine.traces.remove_sites(&machine.session, &[Guest(0x1000)]).is_err());
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
    observed.traces.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).unwrap();
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
    observed.traces.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).unwrap();
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
    observed.traces.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).unwrap();
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
    observed.traces.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).unwrap();
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
    failed.traces.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).unwrap();
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
        let result = peer_witness::run(peer_witness::WitnessMachine::from_unbound(machine), PeerContract::parse(contract.as_bytes()).unwrap(), &output);
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
    let protocol = composed_interleaving::protocol([digest.clone(), digest.clone()]);
    let contract = format!(r#"{{"schema":5,"budget":50000000,"scenario":{{"kind":"natural"}},
        "program":{{"kind":"owned","record_sha256":"{digest}","record_immutable_sha256":"{digest}","loader_sha256":"{digest}",
        "handoff":66592,"entry":66604,"load":66700,"post_load":66704,"loaded_register":0,"poll_period":256,
        "image":[66560,68096],"refusal":null,"forbid_fetch":[[852000,852168],[854444,854480],[854596,854628]],
        "forbid_read":[[852168,852220]]}},
        "startup":{{"kind":"composed","record1_sha256":"{digest}","image_sha256":"{digest}",
        "symbols_sha256":"{digest}","source_sha256":"{digest}","image":[24576,25088],"entry":24576,"handoff":24608,
        "handoff_target":262176,"handoff_argument":6934008,"secondary_entry":66560,"secondary_image_sha256":"{digest}",
        "secondary_symbols_sha256":"{digest}","secondary_source_sha256":"{digest}",{protocol}}}}}"#);
    let output = std::env::temp_dir().join(format!("peer-composed-{}-{}", std::process::id(), OUTPUT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir(&output).unwrap();
    let result = peer_witness::run(peer_witness::WitnessMachine::from_unbound(machine), PeerContract::parse(contract.as_bytes()).unwrap(), &output);
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
    let result = peer_witness::run(peer_witness::WitnessMachine::from_unbound(machine), PeerContract::parse(contract.as_bytes()).unwrap(), &output);
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
    observed.traces.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).unwrap();
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
    bridge.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).unwrap();
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

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ObservedRamStatus { Complete, OutsideRam, ReadFailed, ByteLimit }

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct ObservedHex(String);

#[derive(Debug, Deserialize)]
struct ObservedRamSpan {
    address: Guest,
    requested: usize,
    status: ObservedRamStatus,
    bytes: ObservedHex,
}

#[derive(Deserialize)]
struct ObservedLifecycle {
    r4: u32,
    r5: u32,
    r11: u32,
    tcm: [u32; 2],
}

#[derive(Deserialize)]
struct DiagnosticHit {
    address: Guest,
    sp: Guest,
    registers: [u32; 4],
    ram: Vec<ObservedRamSpan>,
    ram_incomplete: bool,
    lifecycle: Option<ObservedLifecycle>,
}

#[derive(Deserialize)]
struct ObservedLifecycleSnapshot {
    core: usize,
    ram: Vec<ObservedRamSpan>,
    ram_incomplete: bool,
    lifecycle: ObservedLifecycle,
}

#[derive(Deserialize)]
struct DiagnosticPage {
    hits: Vec<DiagnosticHit>,
    incomplete: bool,
    dropped: u64,
}

fn diagnostic_machine(site: Guest, argument: Guest, sp: Guest) -> Machine {
    let mut observed = machine_with_ram(&[0xeafffffe], 0x01800000, RamViews::AliasedWithDecoder);
    // Handwritten A32 self-branch and synthetic RAM, with no firmware bytes.
    observed.session.board.write_physical(u64::from(site.0), &0xeafffffeu32.to_le_bytes()).unwrap();
    observed.session.board.write_register(0, Register::Pc, u64::from(site.0)).unwrap();
    for register in [arm::CORE[0], arm::CORE[3]] {
        observed.session.board.write_register(0, register, u64::from(argument.0)).unwrap();
    }
    observed.session.board.write_register(0, arm::CORE[13], u64::from(sp.0)).unwrap();
    observed.traces.arm_primary(&observed.session, BTreeMap::from([(site.0, "synthetic-diagnostic".into())])).unwrap();
    observed.traces.begin_capture_observation(fujimod_xm1::trace::CaptureScope::Capture).unwrap();
    observed
}

fn diagnostic_page(observed: &mut Machine) -> DiagnosticPage {
    assert_eq!(observed.run(1), RunOutcome::Completed);
    observed.require_running().unwrap();
    let page = observed.traces.capture_observation_page(CaptureCursor::new(0)).unwrap();
    let page: DiagnosticPage = serde_json::from_value(serde_json::to_value(page).unwrap()).unwrap();
    assert_eq!(page.dropped, 0);
    assert_eq!(page.hits.len(), 1);
    for hit in &page.hits {
        assert!(hit.ram.len() <= 24);
        assert!(hit.ram.iter().map(|span| span.bytes.0.len() / 2).sum::<usize>() <= 512);
    }
    page
}

fn synthetic_lifecycle_machine(site: Guest) -> Machine {
    let mut observed = machine_with_ram(&[0xeafffffe], 0x04a00000, RamViews::AliasedWithDecoder);
    observed.session.board.write_physical(u64::from(site.0), &0xeafffffeu32.to_le_bytes()).unwrap();
    observed.session.board.write_register(0, Register::Pc, u64::from(site.0)).unwrap();
    for (address, value) in [
        (0x0157dd3c, 0x04988600u32), (0x015ad53c, u32::MAX), (0x0498b868, 1),
    ] {
        observed.session.board.write_physical(address, &value.to_le_bytes()).unwrap();
    }
    observed.traces.arm_primary(&observed.session, BTreeMap::from([(site.0, "synthetic-lifecycle".into())])).unwrap();
    observed.traces.begin_capture_observation(fujimod_xm1::trace::CaptureScope::SaveLifecycle {
        program: fujimod_xm1::trace::LifecycleProgram::Retained,
    }).unwrap();
    observed
}

fn raw_tcm_registers(observed: &Machine) -> [u32; 2] {
    std::array::from_fn(|bank| u32::try_from(observed.session.board.cpu(0).unwrap().coprocessor(
        arm::Coprocessor { cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank as u32, secure: 0, wide: false },
        None,
    ).unwrap()).unwrap())
}

#[test]
fn lifecycle_allocation_observation_distinguishes_backed_count_from_slot_namespace() {
    let mut backed = synthetic_lifecycle_machine(Guest(0x00e88e18));
    let page = diagnostic_page(&mut backed);
    assert!(!page.incomplete);
    assert!(!page.hits[0].ram_incomplete);
    assert_observed_bytes(&page.hits[0], Guest(0x0498b860), &[0,0,0,0,0,0,0,0,1,0,0,0]);

    let mut unbacked = synthetic_lifecycle_machine(Guest(0x00e88e18));
    unbacked.session.board.write_register(0, arm::CORE[0], 1).unwrap();
    let page = diagnostic_page(&mut unbacked);
    assert!(page.incomplete);
    assert!(page.hits[0].ram_incomplete);
    assert!(page.hits[0].ram.iter().all(|span| span.address.0 < 0x04988600));
}

#[test]
fn unrelated_capture_site_does_not_follow_the_lifecycle_active_frame() {
    let mut observed = synthetic_lifecycle_machine(Guest(0x1000));
    observed.session.board.write_physical(0x015ad53c, &201u32.to_le_bytes()).unwrap();
    let page = diagnostic_page(&mut observed);
    assert!(!page.incomplete);
    assert!(!page.hits[0].ram_incomplete);
    assert_observed_bytes(&page.hits[0], Guest(0x015ad53c), &201u32.to_le_bytes());
    assert!(page.hits[0].ram.iter().all(|span| span.address.0 < 0x04988600));
}

#[test]
fn lifecycle_void_producer_observation_uses_published_output_cells() {
    let mut observed = synthetic_lifecycle_machine(Guest(0x00e80e7c));
    for (register, value) in [(0, 0xdeadbeef), (4, 0x3000), (5, 0x3004), (11, 0x40003500)] {
        observed.session.board.write_register(0, arm::CORE[register], value).unwrap();
    }
    observed.session.board.write_physical(0x3000, &0u32.to_le_bytes()).unwrap();
    observed.session.board.write_physical(0x3004, &0x04988600u32.to_le_bytes()).unwrap();
    let expected_tcm = raw_tcm_registers(&observed);
    let page = diagnostic_page(&mut observed);
    let hit = &page.hits[0];
    assert!(!hit.ram_incomplete);
    assert_eq!(hit.registers[0], 0xdeadbeef);
    assert_observed_bytes(hit, Guest(0x0498b860), &[0,0,0,0,0,0,0,0,1,0,0,0]);
    let context = hit.lifecycle.as_ref().unwrap();
    assert_eq!((context.r4, context.r5, context.r11), (0x3000, 0x3004, 0x40003500));
    assert_eq!(context.tcm, expected_tcm);
}

#[test]
fn lifecycle_stopped_snapshot_retains_callback_and_task_words_without_advancing() {
    let observed = synthetic_lifecycle_machine(Guest(0x00e88e18));
    observed.session.board.write_physical(0x014ddc1c, &0x00a81234u32.to_le_bytes()).unwrap();
    observed.session.board.write_physical(0x0155ccb8, &0x45u32.to_le_bytes()).unwrap();
    let before = (observed.instructions(), context(&observed));
    let expected_tcm = raw_tcm_registers(&observed);
    let snapshot = observed.traces.capture_snapshot(&observed.session).unwrap().unwrap();
    let snapshot: ObservedLifecycleSnapshot = serde_json::from_value(serde_json::to_value(snapshot).unwrap()).unwrap();
    assert_eq!(snapshot.core, 0);
    assert!(!snapshot.ram_incomplete);
    assert_eq!(snapshot.lifecycle.tcm, expected_tcm);
    assert_span_bytes(&snapshot.ram, Guest(0x014ddc1c), &[0x34,0x12,0xa8,0,0,0,0,0,0,0,0,0]);
    assert_span_bytes(&snapshot.ram, Guest(0x0155ccb8), &[0x45,0,0,0,0,0,0,0]);
    assert_eq!((observed.instructions(), context(&observed)), before);
}

fn assert_observed_bytes(hit: &DiagnosticHit, address: Guest, expected: &[u8]) {
    assert_span_bytes(&hit.ram, address, expected);
}

fn assert_span_bytes(spans: &[ObservedRamSpan], address: Guest, expected: &[u8]) {
    let span = spans.iter().find(|span| span.address == address && span.requested == expected.len()).unwrap();
    assert_eq!(span.status, ObservedRamStatus::Complete);
    let expected: String = expected.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(span.bytes.0, expected);
}

#[test]
fn second_filter_descriptor_follows_aliased_nested_pointer_and_retains_selected_row() {
    let mut observed = diagnostic_machine(Guest(0x00da426c), Guest(0x40003000), Guest(0x40003500));
    let mut descriptor = [0x57; 240];
    descriptor[..4].copy_from_slice(&2u32.to_le_bytes());
    descriptor[0xc8..0xcc].copy_from_slice(&0x40003400u32.to_le_bytes());
    let pointee = [0xa3; 16];
    let row = [0x6d; 100];
    observed.session.board.write_physical(0x3000, &descriptor).unwrap();
    observed.session.board.write_physical(0x3400, &pointee).unwrap();
    observed.session.board.write_physical(0x00da5388, &row).unwrap();
    let page = diagnostic_page(&mut observed);
    let hit = &page.hits[0];
    assert_eq!(hit.address, Guest(0x00da426c));
    assert_eq!(hit.registers[0], 0x40003000);
    assert_observed_bytes(hit, Guest(0x3000), &descriptor);
    assert_observed_bytes(hit, Guest(0x3400), &pointee);
    assert_observed_bytes(hit, Guest(0x00da5388), &row);
    assert!(hit.ram_incomplete);
    assert!(page.incomplete);
}

#[test]
fn second_filter_call_and_operand_sites_retain_aliased_stack_and_actual_pointee() {
    for site in [0x00da4300, 0x00689114, 0x00689140, 0x00da4308, 0x00689444, 0x00689468, 0x00689490] {
        let mut observed = diagnostic_machine(Guest(site), Guest(0x40003400), Guest(0x40003500));
        let mut stack = [0xb6; 24];
        stack[4..8].copy_from_slice(&0x40003600u32.to_le_bytes());
        let argument = [0x93; 16];
        let saved_pointee = [0xe2; 16];
        observed.session.board.write_physical(0x3400, &argument).unwrap();
        observed.session.board.write_physical(0x3500, &stack).unwrap();
        observed.session.board.write_physical(0x3600, &saved_pointee).unwrap();
        let page = diagnostic_page(&mut observed);
        let hit = &page.hits[0];
        assert_eq!(hit.address, Guest(site));
        assert_eq!(hit.sp, Guest(0x40003500));
        assert_observed_bytes(hit, Guest(0x3500), &stack);
        match site {
            0x00da4300 | 0x00da4308 => assert_observed_bytes(hit, Guest(0x3400), &argument),
            0x00689444 | 0x00689468 | 0x00689490 => assert_observed_bytes(hit, Guest(0x3600), &saved_pointee),
            _ => {}
        }
        assert!(!hit.ram_incomplete);
        assert!(!page.incomplete);
    }
}

#[test]
fn second_filter_invalid_nested_pointers_report_empty_outside_ram_spans() {
    for pointer in [0xfff30000u32, 0x1ffffff8, 0xfffffff8] {
        for site in [0x00da426c, 0x00689490] {
            let mut observed = diagnostic_machine(Guest(site), Guest(0x40003000), Guest(0x40003500));
            let mut descriptor = [0; 240];
            descriptor[0xc8..0xcc].copy_from_slice(&pointer.to_le_bytes());
            let mut stack = [0; 24];
            stack[4..8].copy_from_slice(&pointer.to_le_bytes());
            observed.session.board.write_physical(0x3000, &descriptor).unwrap();
            observed.session.board.write_physical(0x3500, &stack).unwrap();
            let page = diagnostic_page(&mut observed);
            let hit = &page.hits[0];
            let unavailable = hit.ram.iter().find(|span| span.address == Guest(pointer)).unwrap();
            assert_eq!(unavailable.status, ObservedRamStatus::OutsideRam);
            assert_eq!(unavailable.requested, 16);
            assert!(unavailable.bytes.0.is_empty());
            assert!(hit.ram_incomplete);
            assert!(page.incomplete);
        }
    }
}

#[test]
fn second_filter_unavailable_parent_and_overflowing_row_never_fabricate_context() {
    for (site, argument, sp) in [
        (0x00da426c, 0xfffffff0, 0x40003500),
        (0x00da426c, 0x01800000, 0x40003500),
        (0x00689490, 0x40003000, 0xfffffffc),
        (0x00689490, 0x40003000, 0x01800000),
    ] {
        let mut observed = diagnostic_machine(Guest(site), Guest(argument), Guest(sp));
        let page = diagnostic_page(&mut observed);
        let hit = &page.hits[0];
        assert!(hit.ram_incomplete);
        assert!(page.incomplete);
        assert!(hit.ram.iter().filter(|span| span.status == ObservedRamStatus::Complete)
            .all(|span| span.address.0 >= 0x00800000));
        assert!(hit.ram.iter().any(|span| span.status != ObservedRamStatus::Complete && span.bytes.0.is_empty()));
    }
    for index in [u32::MAX, 42_949_672, 0x01000000] {
        let mut observed = diagnostic_machine(Guest(0x00da426c), Guest(0x40003000), Guest(0x40003500));
        let mut descriptor = [0; 240];
        descriptor[..4].copy_from_slice(&index.to_le_bytes());
        descriptor[0xc8..0xcc].copy_from_slice(&0x40003400u32.to_le_bytes());
        observed.session.board.write_physical(0x3000, &descriptor).unwrap();
        observed.session.board.write_physical(0x3400, &[0x81; 16]).unwrap();
        let page = diagnostic_page(&mut observed);
        let hit = &page.hits[0];
        assert_observed_bytes(hit, Guest(0x3000), &descriptor);
        assert_observed_bytes(hit, Guest(0x3400), &[0x81; 16]);
        assert!(hit.ram_incomplete);
        assert!(page.incomplete);
        assert!(hit.ram.iter().all(|span| span.requested != 100 || span.bytes.0.is_empty()));
    }
}

#[test]
fn second_filter_decoder_window_is_a_gap_without_preempting_proven_stack() {
    let mut observed = diagnostic_machine(Guest(0x00da4308), Guest(0x500), Guest(0x40003500));
    observed.session.board.write_physical(0x500, &[0xf4; 16]).unwrap();
    observed.session.board.write_physical(0x3500, &[0xc7; 24]).unwrap();
    let page = diagnostic_page(&mut observed);
    let hit = &page.hits[0];
    assert_eq!(hit.registers[0], 0x500);
    let gap = hit.ram.iter().find(|span| span.address == Guest(0x500)).unwrap();
    assert_eq!(gap.status, ObservedRamStatus::ReadFailed);
    assert!(gap.bytes.0.is_empty());
    assert_observed_bytes(hit, Guest(0x3500), &[0xc7; 24]);
    assert!(hit.ram_incomplete);
    assert!(page.incomplete);
}

#[test]
fn second_filter_tcm_overlap_is_core_qualified_and_never_returns_underlying_ram() {
    for core in 0..2 {
        let mut observed = diagnostic_machine(Guest(0x00da4308), Guest(0x40003400), Guest(0x40003500));
        observed.session.board.write_physical(0x3400, &[0x9a; 16]).unwrap();
        observed.session.board.write_physical(0x3500, &[0x72; 24]).unwrap();
        observed.session.board.cpu(core).unwrap().coprocessor(arm::Coprocessor {
            cp: 15, opc1: 0, crn: 9, crm: 1, opc2: 1, secure: 0, wide: false,
        }, Some(0x4000300d)).unwrap();
        let page = diagnostic_page(&mut observed);
        let hit = &page.hits[0];
        assert_eq!(hit.registers[0], 0x40003400);
        assert_eq!(hit.sp, Guest(0x40003500));
        if core == 0 {
            for address in [Guest(0x40003400), Guest(0x40003500)] {
                let gap = hit.ram.iter().find(|span| span.address == address).unwrap();
                assert_eq!(gap.status, ObservedRamStatus::ReadFailed);
                assert!(gap.bytes.0.is_empty());
            }
            assert!(hit.ram_incomplete);
            assert!(page.incomplete);
        } else {
            assert_observed_bytes(hit, Guest(0x3400), &[0x9a; 16]);
            assert_observed_bytes(hit, Guest(0x3500), &[0x72; 24]);
            assert!(!hit.ram_incomplete);
            assert!(!page.incomplete);
        }
    }
}

#[cfg(feature = "scoped-watch")]
fn initializer_contract(scratch_bytes: u32) -> String {
    let digest = "ab".repeat(32);
    format!(r#"{{"schema":6,"prefix":{{"schema":4,"budget":50000000,"scenario":{{"kind":"natural"}},
        "program":{{"kind":"owned","record_sha256":"{digest}","record_immutable_sha256":"{digest}","loader_sha256":"{digest}",
        "handoff":66592,"entry":66604,"load":66700,"post_load":66704,"loaded_register":0,"poll_period":256,
        "image":[66560,68096],"refusal":null,"forbid_fetch":[[852000,852168],[854444,854480],[854596,854628]],
        "forbid_read":[[852168,852220]]}}}},
        "initializer":{{"role":"owned","entry":67584,"rust":[67616,67712],"scratch_bytes":{scratch_bytes},
        "image_sha256":"{digest}","symbols_sha256":"{digest}",
        "source_sha256":"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"}}}}"#)
}

#[cfg(feature = "scoped-watch")]
#[test]
fn initializer_admission_requires_its_natural_source_and_owned_symbol_identity() {
    let contract = initializer_contract(40);
    assert!(PeerContract::parse(contract.as_bytes()).is_ok());
    for invalid in [
        contract.replace(r#""kind":"natural""#, r#""kind":"held""#),
        contract.replace("43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490", &"cd".repeat(32)),
        contract.replace(&format!(r#""symbols_sha256":"{}""#, "ab".repeat(32)), r#""symbols_sha256":null"#),
    ] {
        assert!(PeerContract::parse(invalid.as_bytes()).is_err());
    }
}

#[cfg(feature = "scoped-watch")]
#[test]
fn initializer_admission_allows_word_aligned_leaf_frames_without_assuming_stock_twelve_bytes() {
    for bytes in [8, 12, 40, 1032] {
        assert!(PeerContract::parse(initializer_contract(bytes).as_bytes()).is_ok());
    }
    for bytes in [0, 4, 10, 1036] {
        assert!(PeerContract::parse(initializer_contract(bytes).as_bytes()).is_err());
    }
}

#[cfg(feature = "scoped-watch")]
#[path = "support/composed_nand_fixture.rs"]
mod composed_nand_fixture;

#[cfg(feature = "scoped-watch")]
mod composed_interleaving {
    use super::*;
    use super::composed_nand_fixture::{Timing, Transfer};
    use fujimod_catalog::configuration::ResolvedConfiguration;
    use serde::Serialize;
    use sha2::{Digest, Sha256};

    #[derive(Clone, Copy)]
    enum FirstHandoff { Early, Late, Missing, Invalid, Partial(usize) }

    #[derive(Clone, Copy)]
    enum Scenario {
        Held, Natural, Delayed, NegativeInterleavedTail, NegativeInvalidCorePostLoad,
        NegativeInvalidCoreExit, NegativeInvalidCoreRefusal, NegativePredicateZero,
    }

    impl Scenario {
        fn name(self) -> &'static str {
            match self {
                Self::Held => "held", Self::Natural => "natural", Self::Delayed => "delayed",
                Self::NegativeInterleavedTail => "negative-interleaved-tail",
                Self::NegativeInvalidCorePostLoad | Self::NegativeInvalidCoreExit | Self::NegativeInvalidCoreRefusal => "negative-invalid-core",
                Self::NegativePredicateZero => "negative-predicate-zero",
            }
        }
        fn standalone(self) -> bool {
            matches!(self, Self::NegativeInvalidCorePostLoad | Self::NegativeInvalidCoreExit
                | Self::NegativeInvalidCoreRefusal | Self::NegativePredicateZero)
        }
    }

    #[derive(Clone, Copy)]
    enum Mutation {
        None, BeforeHandoff, AfterInstall, WrongCore, WrongWidth, WrongOrder,
        WrongValue, WrongPreimage, CodeStaging, CodeRestore, OpcodeWrite, ResetWrite,
        BootstrapOpcode, BootstrapReset, BootstrapHandler, CodeHash, SelectorHash,
        ZeroLiteral, ZeroWriter, NandBeforeHandoff, NandAfterInstall, NandProtectedOverlap,
        InterleavedHelper, InterleavedBranch, InterleavedExtra,
    }

    #[derive(Clone, Copy, Serialize)]
    struct Step { site: u32, slot: u32, before: u32, value: u32 }

    const STEPS: [Step; 7] = [
        Step { site: 0x4003c, slot: 0x24, before: 76, value: 0x40100 },
        Step { site: 0x40048, slot: 0x28, before: 100, value: 0x401b4 },
        Step { site: 0x40054, slot: 0x2c, before: 80, value: 0x40128 },
        Step { site: 0x40060, slot: 0x30, before: 88, value: 0x40148 },
        Step { site: 0x4006c, slot: 0x34, before: 96, value: 0x40168 },
        Step { site: 0x40078, slot: 0x38, before: 192, value: 0x40204 },
        Step { site: 0x40084, slot: 0x3c, before: 144, value: 0x40184 },
    ];
    const BOOTSTRAP: [u32; 16] = [
        0xe59ff018, 0xe59ff018, 0xe59ff018, 0xe59ff018,
        0xe59ff018, 0xe59ff018, 0xe59ff018, 0xe59ff018,
        0x6000, 76, 100, 80, 88, 96, 192, 144,
    ];

    pub(super) fn protocol(code: [String; 2]) -> String {
        let bootstrap = serde_json::to_string(&BOOTSTRAP).unwrap();
        let steps = serde_json::to_string(&STEPS).unwrap();
        format!(r#""completion_zero":{{"site":24596,"instruction":3851063296,"literal":24624}},
            "kernel_vector_install":{{"entry":262176,"installed":262280,"bootstrap":{bootstrap},
            "code":[{{"span":[262176,262400],"sha256":"{}"}},{{"span":[272312,272340],"sha256":"{}"}}],
            "steps":{steps}}}"#, code[0], code[1])
    }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    #[serde(rename_all = "kebab-case")]
    enum Label {
        #[serde(rename = "record-1-entry")]
        Record1Entry,
        FirstStageHandoff,
        KernelVectorsInstalled,
        SecondaryEntry,
        #[serde(rename = "record-2-handoff")]
        Record2Handoff,
        PrefixEntry,
        PostLoad,
        HeldGate,
        DelayEntry,
        DelayRelease,
        CompletionStore,
        PrefixExit,
        Refusal,
    }

    #[derive(Deserialize)]
    struct Stop {
        label: Label,
        address: u32,
        core: usize,
        counts: [u64; 2],
        registers: [u32; 13],
        sp: u32,
        lr: u32,
        cpsr: u32,
        completion: u32,
        counter: Option<Counter>,
        watch_records: Option<usize>,
    }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    struct Counter { count: u64, zero_count: u64, nonzero_count: u64 }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    struct Views { r#virtual: Vec<u8>, physical: Vec<u8> }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    struct Decoder { choices: Vec<usize>, pending: Vec<Option<usize>>, failure: Option<String> }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    struct VectorSnapshot { matched: usize, vectors: Views, decoder: Decoder, tcm: [[u32; 2]; 2] }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    struct CodeSnapshot { span: [u32; 2], bytes: Views }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    struct KernelSnapshot {
        #[serde(flatten)]
        vectors: VectorSnapshot,
        code: [CodeSnapshot; 2],
    }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    #[serde(rename_all = "kebab-case")]
    enum InstallState { NotBound, Installing, Installed }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    enum HistoryEpoch { Loaded, Scoped }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    struct HistoryRef { epoch: HistoryEpoch, index: usize }

    #[derive(Deserialize)]
    struct Installation {
        state: InstallState,
        binding: Option<KernelSnapshot>,
        events: Vec<HistoryRef>,
        record2_handoff: Option<VectorSnapshot>,
        installed_stop: Option<usize>,
        installed_readback: Option<KernelSnapshot>,
        terminal_readback: Option<KernelSnapshot>,
    }

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    enum ByteOrder { Little, Big }

    #[derive(Deserialize)]
    struct ProtocolRow {
        source: Source,
        address: Option<u64>,
        byte_order: Option<ByteOrder>,
        core: Option<usize>,
        pc: Option<u64>,
        instructions: Option<u64>,
        physical: u64,
        size: u64,
        value: Option<u64>,
        before: Option<Vec<u8>>,
    }

    #[derive(Deserialize)]
    struct History { records: Vec<ProtocolRow>, violation: Option<String> }

    #[derive(Deserialize)]
    struct CounterSamples {
        count: u64, zero_count: u64, nonzero_count: u64,
        head: Vec<ProtocolRow>, tail: Vec<ProtocolRow>,
        first_nonzero: Option<ProtocolRow>,
        first_nonzero_index: Option<u64>,
    }

    #[derive(Deserialize)]
    struct Coverage { covered_words: u64, first_uncovered: Option<u64> }

    #[derive(Deserialize)]
    struct ScopedHistory {
        #[serde(flatten)]
        history: History,
        counters: Vec<CounterSamples>,
        coverage: Vec<Coverage>,
    }


    #[derive(Deserialize)]
    struct WordViews { r#virtual: u32, physical: u32 }

    #[derive(Deserialize)]
    struct Microstep { before: [u64; 2], after: [u64; 2], cell: u32 }

    #[derive(Deserialize)]
    struct Completion {
        instruction: WordViews,
        before_pc: u32, after_pc: u32,
        before_cpsr: u32, after_cpsr: u32,
        microsteps: Vec<Microstep>,
        readback: WordViews,
        counter: Counter,
        watch_records: usize,
    }

    #[derive(Deserialize)]
    struct ZeroReadback { range: [u64; 2], first_nonzero: Option<u64> }

    #[derive(Deserialize)]
    struct Endpoint {
        frame: Views, vectors: Views, zero: ZeroReadback,
        record_immutable_sha256: String, loader_sha256: String,
    }
    #[derive(Deserialize)]
    struct Receipt {
        schema: u32,
        result: ResultStatus,
        error: Option<String>,
        machine_failure: Option<String>,
        stops: Vec<Stop>,
        initial_record_sha256: Option<String>,
        handoff_record_immutable_sha256: Option<String>,
        loaded_watch: History,
        scoped_watch: Option<ScopedHistory>,
        firststage_zero: Option<HistoryRef>,
        kernel_vector_install: Installation,
        completion: Option<Completion>,
        endpoint: Option<Endpoint>,
    }

    #[derive(Deserialize)]
    struct NegativeEvidence { invalid_core_identity: u32 }

    #[derive(Deserialize)]
    struct StandaloneReceipt {
        schema: u32,
        result: ResultStatus,
        machine_failure: Option<String>,
        cp14_identities: [u32; 2],
        stops: Vec<Stop>,
        scoped_watch: Option<ScopedHistory>,
        negative_evidence: Option<NegativeEvidence>,
        completion: Option<Completion>,
        endpoint: Option<Endpoint>,
    }

    impl Receipt {
        fn row(&self, reference: &HistoryRef) -> &ProtocolRow {
            match reference.epoch {
                HistoryEpoch::Loaded => &self.loaded_watch.records[reference.index],
                HistoryEpoch::Scoped => &self.scoped_watch.as_ref().unwrap().history.records[reference.index],
            }
        }
        fn position(&self, label: Label) -> usize {
            self.stops.iter().position(|stop| stop.label == label).unwrap()
        }
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
    fn observe(order: FirstHandoff, mutation: Mutation) -> (Result<(), String>, Receipt) {
        observe_scenario(order, mutation, Scenario::Held)
    }

    fn observe_scenario(order: FirstHandoff, mutation: Mutation, scenario: Scenario) -> (Result<(), String>, Receipt) {
        let (result, bytes) = observe_bytes(order, mutation, scenario);
        (result, serde_json::from_slice(&bytes).unwrap())
    }

    fn observe_bytes(order: FirstHandoff, mutation: Mutation, scenario: Scenario) -> (Result<(), String>, Vec<u8>) {
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
                (arm::CORE[4], if matches!(order, FirstHandoff::Early) { 1 } else if matches!(scenario, Scenario::Held) { 600 } else { 40_000 }),
                (arm::CORE[5], 0x400a0000),
                (arm::CORE[6], 0x24),
                (arm::CORE[7], 0x40100),
                (arm::CORE[13], 0x70000800),
            ];
        }
        let mut memory = vec![0; 0x61c410];
        for (index, value) in BOOTSTRAP.into_iter().enumerate() {
            put_word(&mut memory, 4 * index, value);
        }
        for (address, value) in [
            (0x4300, 0x40020),
            (0x4304, 0x69cdf8),
            (0x6000, 0xee091f31),
            (0x6004, 0xe2544001),
            (0x6008, 0x1afffffd),
            (0x600c, 0xe59fa01c), // LDR r10,[pc,#0x1c].
            (0x6010, 0xe3a09000),
            (0x6014, 0xe58a9000), // STR r9,[r10].
            (0x6018, branch(0x6018, if matches!(order, FirstHandoff::Missing) { 0x40020 } else { 0x6020 })),
            (0x601c, branch(0x601c, 0x6020)),
            (0x6020, 0xe1a0f002),
            (0x6030, 0x400a00c4),
            (0x10020, 0x10400),
            (0x10400, 0xee091f31),
            (0x10404, branch(0x10404, 0x10420)),
            (0x10420, 0xe1a0f003),
            (0x1042c, 0xe1a00000),
            (0x10430, 0xe59500c4),
            (0x10434, branch(0x10434, 0x10430)),
            (0x4000c, 0x40020),
            (0x40020, 0xe1a05000),
            (0x40024, 0xeb0009e3),
            (0x40028, 0xe3500000),
            (0x4002c, 0x03a06000),
            (0x40030, 0x159f60a4),
            (0x40088, 0xe1a00005),
            (0x4008c, branch(0x4008c, 0x40a00)),
            (0x400dc, 0xffff0000),
            (0x40a00, 0xeafffffe),
            (0x427b8, 0xee121e10),
            (0x427bc, 0xe3a02901),
            (0x427c0, 0xe2822003),
            (0x427c4, 0xe1510002),
            (0x427c8, 0x03a00000),
            (0x427cc, 0x13a00001),
            (0x427d0, 0xe12fff1e),
            (0x61c40c, 0x69cdf8),
        ] {
            put_word(&mut memory, address, value);
        }
        for (index, step) in STEPS.iter().enumerate() {
            let load = step.site - 4;
            let literal = 0x400e0 + 4 * index as u32;
            put_word(&mut memory, (step.site - 8) as usize, 0xe3a03000 | step.slot);
            put_word(&mut memory, load as usize, 0xe59f4000 | (literal - load - 8));
            put_word(&mut memory, step.site as usize, 0xe7834006);
            put_word(&mut memory, literal as usize, step.value);
        }
        if let FirstHandoff::Partial(k) = order {
            // Core 0's thousandth entered instruction is the kth vector store.
            let padding = 980 - 3 * k;
            put_word(&mut memory, 0x6004, branch(0x6004, 0x6100));
            for index in 0..padding {
                put_word(&mut memory, 0x6100 + 4 * index, 0xe1a00000);
            }
            let back = 0x6100 + 4 * padding as u32;
            put_word(&mut memory, back as usize, branch(back, 0x600c));
        }
        match mutation {
            Mutation::None | Mutation::CodeHash | Mutation::SelectorHash | Mutation::NandBeforeHandoff | Mutation::NandAfterInstall
                | Mutation::NandProtectedOverlap | Mutation::InterleavedHelper | Mutation::InterleavedBranch | Mutation::InterleavedExtra => (),
            Mutation::BeforeHandoff => put_word(&mut memory, 0x6018, 0xe5867000),
            Mutation::WrongCore => {
                put_word(&mut memory, 0x10404, 0xe5867000);
                put_word(&mut memory, 0x10408, branch(0x10408, 0x10420));
            }
            Mutation::WrongWidth => put_word(&mut memory, 0x4003c, 0xe7c34006),
            Mutation::WrongOrder => put_word(&mut memory, 0x40034, 0xe3a03028),
            Mutation::WrongValue => put_word(&mut memory, 0x400e0, 0x40104),
            Mutation::WrongPreimage => put_word(&mut memory, 0xa00c4, 1),
            Mutation::BootstrapOpcode => put_word(&mut memory, 0, 0xe59ff01c),
            Mutation::BootstrapReset => put_word(&mut memory, 0x20, 0x6004),
            Mutation::BootstrapHandler => put_word(&mut memory, 0x28, 104),
            Mutation::ZeroLiteral => put_word(&mut memory, 0x6030, 0x400a00c8),
            Mutation::ZeroWriter => put_word(&mut memory, 0x6014, 0xe58a9004),
            Mutation::CodeStaging => {
                put_word(&mut memory, 0x6004, branch(0x6004, 0x43000));
                for (address, value) in [
                    (0x43000, 0xe59f8018),
                    (0x43004, 0xe3a09000),
                    (0x43008, 0xe5889000),
                    (0x4300c, 0xe59f9010),
                    (0x43010, 0xe5889000),
                    (0x43014, branch(0x43014, 0x600c)),
                    (0x43020, 0x427b8),
                    (0x43024, 0xee121e10),
                ] {
                    put_word(&mut memory, address, value);
                }
            }
            Mutation::AfterInstall | Mutation::CodeRestore | Mutation::OpcodeWrite | Mutation::ResetWrite => {
                let (target, value) = match mutation {
                    Mutation::AfterInstall => (0x24, 0x40100),
                    Mutation::CodeRestore => (0x427b8, 0xee121e10),
                    Mutation::OpcodeWrite => (0, 0xe59ff018),
                    Mutation::ResetWrite => (0x20, 0x6000),
                    _ => unreachable!(),
                };
                put_word(&mut memory, 0x4008c, branch(0x4008c, 0x43000));
                for (address, value) in [
                    (0x43000, 0xe59f8010),
                    (0x43004, 0xe59f9010),
                    (0x43008, 0xe5889000),
                    (0x4300c, 0xe5889000),
                    (0x43010, branch(0x43010, 0x40a00)),
                    (0x43018, target),
                    (0x4301c, value),
                ] {
                    put_word(&mut memory, address, value);
                }
            }
        }
        if !matches!(scenario, Scenario::Held) {
            memory[0xddaa0..0xf3420].fill(0xa5);
            for (address, value) in [
                (0x1042c, branch(0x1042c, 0x10440)),
                (0x10430, 0xe59c00c4),
                (0x10434, 0xe3500000),
                (0x10438, branch(0x10438, 0x10600) & 0x0fffffff | 0x10000000),
                (0x1043c, branch(0x1043c, 0x11000)),
                (0x10440, 0xe1a04000),
                (0x10444, 0xe1a05000),
                (0x10448, 0xe59f1090),
                (0x1044c, 0xe59f2090),
                (0x10450, 0xe3a03000),
                (0x10454, 0xe4813004),
                (0x10458, 0xe1510002),
                (0x1045c, 0x1afffffc),
                (0x10460, 0xe59fc080),
                (0x10464, branch(0x10464, 0x10430)),
                (0x104e0, 0x400ddaa0),
                (0x104e4, 0x400f3420),
                (0x104e8, 0x400a0000),
                (0x10600, 0xe59f10d8),
                (0x10604, 0xe59f60d8),
                (0x10608, 0xe3a03007),
                (0x1060c, 0xe4912004),
                (0x10610, 0xe4862004),
                (0x10614, 0xe2533001),
                (0x10618, 0x1afffffb),
                (0x1061c, 0xe59f10c4),
                (0x10620, 0xe59f20c4),
                (0x10624, 0xe5812000),
                (0x10628, 0xe5814004),
                (0x1062c, 0xe581b008),
                (0x10630, 0xe59f20b8),
                (0x10634, 0xe581200c),
                (0x10638, 0xe281b00c),
                (0x1063c, 0xe1a0d001),
                (0x10640, 0xe59f60ac),
                (0x10644, 0xe59fe0ac),
                (0x10648, branch(0x10648, 0xd0aac)),
                (0x106e0, 0x10700),
                (0x106e4, 0xffff0024),
                (0x106e8, 0x400f3410),
                (0x106ec, 0x400f3420),
                (0x106f0, 0xd00c0),
                (0x106f4, 0xffff0000),
                (0x106f8, 0xd09bc),
                (0x40a00, if matches!(scenario, Scenario::Delayed) { branch(0x40a00, 0x180) | 0x01000000 } else { 0xe3a05001 }),
                (0x40a04, 0xe59f30f0),
                (0x40a08, 0xe58350c4),
                (0x40a0c, 0xeafffffe),
                (0x40afc, 0x400a0000),
            ] {
                put_word(&mut memory, address, value);
            }
            for (index, word) in [0xd0100, 0xd01b4, 0xd0128, 0xd0148, 0xd0168, 0xd0204, 0xd0184].into_iter().enumerate() {
                put_word(&mut memory, 0x10700 + 4 * index, word);
            }
            for index in 0..251 {
                put_word(&mut memory, 0x11000 + 4 * index, 0xe1a00000);
            }
            put_word(&mut memory, 0x113ec, branch(0x113ec, 0x10430));
            if matches!(scenario, Scenario::Delayed) {
                for (index, word) in [0xe10f3000, 0xe3a05801, 0xe2555001, 0x1afffffd, 0xe128f003, 0xe3a05001, 0xe12fff1e].into_iter().enumerate() {
                    put_word(&mut memory, 0x180 + 4 * index, word);
                }
            }
        }
        if scenario.standalone() {
            put_word(&mut memory, 0x6000, branch(0x6000, 0x1800));
            put_word(&mut memory, 0x1800, 0xee091f31);
            put_word(&mut memory, 0x1804, branch(0x1804, 0x6004));
            put_word(&mut memory, 0x10464, branch(0x10464, 0x10500));
            put_word(&mut memory, 0x10500, 0xee121e10);
            let continuation = match scenario {
                Scenario::NegativeInvalidCoreExit => 0xd0aac,
                Scenario::NegativeInvalidCoreRefusal => 0x10510,
                _ => 0x10430,
            };
            put_word(&mut memory, 0x10504, branch(0x10504, continuation));
            put_word(&mut memory, 0x10510, 0xeafffffe);
            if matches!(scenario, Scenario::NegativePredicateZero) {
                put_word(&mut memory, 0x10438, branch(0x10438, 0x10600));
            }
        }
        let canonical_installer = digest(&memory[0x40020..0x40100]);
        let hook = if matches!(scenario, Scenario::NegativeInterleavedTail) {
            let instruction = branch(0x40040, 0x6040);
            put_word(&mut memory, 0x40040, instruction);
            for (index, word) in [0xe3a03024, 0xe59f4008, 0xe7834006, 0xe3a03028, 0xe59ff000, 0xdeadbe00, 0x40044].into_iter().enumerate() {
                put_word(&mut memory, 0x6040 + 4 * index, word);
            }
            match mutation {
                Mutation::InterleavedHelper => put_word(&mut memory, 0x6054, 0xdeadbe04),
                Mutation::InterleavedBranch => put_word(&mut memory, 0x40040, branch(0x40040, 0x6044)),
                Mutation::InterleavedExtra => put_word(&mut memory, 0x400a0, 0xe1a00000),
                _ => (),
            }
            format!(r#""interleaved_tail":{{"site":262208,"instruction":{instruction},"target":24640}},"#)
        } else { String::new() };
        let bus = match mutation {
            Mutation::NandBeforeHandoff | Mutation::NandAfterInstall | Mutation::NandProtectedOverlap => {
                let timing = if matches!(mutation, Mutation::NandAfterInstall) {
                    Timing::AfterInstall
                } else {
                    Timing::BeforeHandoff
                };
                let transfer = if matches!(mutation, Mutation::NandProtectedOverlap) {
                    Transfer::CompletionGuards
                } else {
                    Transfer::Installer
                };
                super::composed_nand_fixture::build_bus(&mut memory, timing, transfer)
            }
            _ => Bus::new(Mode::Strict),
        };
        let mut board = graph.build(
            factory::Options { engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20 },
            bus, None,
        ).unwrap();
        board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
        let mut immutable = Sha256::new();
        immutable.update(&memory[0x10000..0x14c60]);
        immutable.update(&memory[0x14c6c..0x14c80]);
        let immutable = format!("{:x}", immutable.finalize());
        let record1 = digest(&memory[..0x10000]);
        let first_image = digest(&memory[0x6000..0x7100]);
        let record2 = digest(&memory[0x10000..0x14c80]);
        let image_end = if matches!(scenario, Scenario::Held) { 0x10500 } else { 0x11400 };
        let second_image = digest(&memory[0x10400..image_end]);
        let loader = digest(&memory[0xd0000..0xddaa0]);
        let code = [
            if matches!(mutation, Mutation::CodeHash) { "ab".repeat(32) }
                else if matches!(scenario, Scenario::NegativeInterleavedTail) { canonical_installer }
                else { digest(&memory[0x40020..0x40100]) },
            if matches!(mutation, Mutation::SelectorHash) { "ab".repeat(32) } else { digest(&memory[0x427b8..0x427d4]) },
        ];
        let protocol = protocol(code);
        board.write_physical(0, &memory).unwrap();
        let session = Session {
            definition: serde_json::Value::Null, board, controls: None, contact_driver: None,
            resources: Resources::default(), nodes: BTreeMap::new(), contacts: BTreeMap::new(), recording_frames: Default::default(),
        };
        let machine = Machine::new(document, session).unwrap();
        let identity = "ab".repeat(32);
        let scenario_name = scenario.name();
        let contract = format!(r#"{{"schema":5,"budget":50000000,"scenario":{{"kind":"{scenario_name}"}},
            "program":{{"kind":"owned","record_sha256":"{record2}","record_immutable_sha256":"{immutable}",
            "loader_sha256":"{loader}","handoff":66592,"entry":66604,"load":66608,"post_load":66612,
            "loaded_register":0,"poll_period":256,"image":[66560,{image_end}],"refusal":null,
            "forbid_fetch":[[852000,852168],[854444,854480],[854596,854628]],"forbid_read":[[852168,852220]]}},
            "startup":{{"kind":"composed","record1_sha256":"{record1}","image_sha256":"{first_image}",
            "symbols_sha256":"{identity}","source_sha256":"{identity}","image":[24576,28928],"entry":24576,
            "handoff":24608,"handoff_target":262176,"handoff_argument":6934008,"secondary_entry":66560,
            "secondary_image_sha256":"{second_image}","secondary_symbols_sha256":"{identity}",
            "secondary_source_sha256":"{identity}",{hook}{protocol}}}}}"#);
        let contract = if scenario.standalone() {
            let period = if matches!(scenario, Scenario::NegativePredicateZero) { "null" } else { "256" };
            format!(r#"{{"schema":4,"budget":50000000,"scenario":{{"kind":"{scenario_name}"}},
                "program":{{"kind":"owned","record_sha256":"{record2}","record_immutable_sha256":"{immutable}",
                "loader_sha256":"{loader}","handoff":66592,"entry":66604,"load":66608,"post_load":66612,
                "loaded_register":0,"poll_period":{period},"image":[66560,{image_end}],"refusal":66832,
                "forbid_fetch":[[852000,852168],[854444,854480],[854596,854628]],"forbid_read":[[852168,852220]]}}}}"#)
        } else { contract };
        let output = std::env::temp_dir().join(format!(
            "peer-composed-order-{}-{}", std::process::id(), OUTPUT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&output).unwrap();
        let result = peer_witness::run(peer_witness::WitnessMachine::from_unbound(machine), PeerContract::parse(contract.as_bytes()).unwrap(), &output);
        let receipt = std::fs::read(output.join("peer-witness.json")).unwrap();
        std::fs::remove_dir_all(output).unwrap();
        (result, receipt)
    }

    #[test]
    fn invalid_core_injection_executes_owned_instructions_before_refusing_each_continuation() {
        for (scenario, boundary) in [
            (Scenario::NegativeInvalidCorePostLoad, Label::PostLoad),
            (Scenario::NegativeInvalidCoreExit, Label::PrefixExit),
            (Scenario::NegativeInvalidCoreRefusal, Label::Refusal),
        ] {
            let (result, bytes) = observe_bytes(FirstHandoff::Late, Mutation::None, scenario);
            let receipt: StandaloneReceipt = serde_json::from_slice(&bytes).unwrap();
            assert!(result.is_err());
            assert_eq!(receipt.schema, 4);
            assert_eq!(receipt.result, ResultStatus::Failed);
            assert!(receipt.machine_failure.is_none());
            assert_eq!(receipt.cp14_identities, [0x4003, 0x5003]);
            assert_eq!(receipt.negative_evidence.unwrap().invalid_core_identity, 0x6003);
            let entry = receipt.stops.iter().find(|stop| stop.label == Label::PrefixEntry).unwrap();
            let refused = receipt.stops.iter().find(|stop| stop.label == boundary).unwrap();
            assert!(refused.counts[1] > entry.counts[1] + 1);
            assert_eq!(refused.registers[1], 0x6003);
            assert!(receipt.completion.is_none());
            assert!(receipt.endpoint.is_none());
        }
    }

    #[test]
    fn zero_accepting_predicate_reaches_the_unchanged_uncommitted_prefix_exit_refusal() {
        let (result, bytes) = observe_bytes(FirstHandoff::Late, Mutation::None, Scenario::NegativePredicateZero);
        let receipt: StandaloneReceipt = serde_json::from_slice(&bytes).unwrap();
        assert!(result.is_err());
        assert_eq!(receipt.schema, 4);
        assert_eq!(receipt.result, ResultStatus::Failed);
        assert!(receipt.machine_failure.is_none());
        assert!(receipt.negative_evidence.is_none());
        let post = receipt.stops.iter().find(|stop| stop.label == Label::PostLoad).unwrap();
        assert_eq!(post.registers[0], 0);
        let exit = receipt.stops.iter().find(|stop| stop.label == Label::PrefixExit).unwrap();
        assert_eq!(exit.completion, 0);
        assert_eq!(receipt.scoped_watch.unwrap().counters[0].nonzero_count, 0);
        assert!(receipt.completion.is_none());
        assert!(receipt.endpoint.is_none());
    }

    fn assert_vectors(snapshot: &VectorSnapshot, matched: usize) {
        assert_eq!(snapshot.matched, matched);
        let mut expected = BOOTSTRAP;
        for step in STEPS.iter().take(matched) {
            expected[step.slot as usize / 4] = step.value;
        }
        let mut bytes = [0; 64];
        for (word, bytes) in expected.into_iter().zip(bytes.chunks_exact_mut(4)) {
            bytes.copy_from_slice(&word.to_le_bytes());
        }
        assert_eq!(snapshot.vectors.physical, bytes);
        assert_eq!(snapshot.vectors.r#virtual, bytes);
        assert_eq!(snapshot.decoder.choices, [1]);
        assert_eq!(snapshot.decoder.pending, [None]);
        assert!(snapshot.decoder.failure.is_none());
    }

    fn assert_installation(receipt: &Receipt, at_handoff: usize, continuation: Label) {
        assert_eq!(receipt.schema, 5);
        assert_eq!(receipt.result, ResultStatus::Passed);
        assert!(receipt.error.is_none());
        assert!(receipt.machine_failure.is_none());
        assert!(receipt.initial_record_sha256.is_some());
        assert!(receipt.handoff_record_immutable_sha256.is_some());
        let installation = &receipt.kernel_vector_install;
        assert_eq!(installation.state, InstallState::Installed);
        assert_eq!(installation.events.len(), 7);
        assert_vectors(&installation.binding.as_ref().unwrap().vectors, 0);
        assert_vectors(installation.record2_handoff.as_ref().unwrap(), at_handoff);
        let installed = installation.installed_readback.as_ref().unwrap();
        assert_vectors(&installed.vectors, 7);
        let terminal = installation.terminal_readback.as_ref().unwrap();
        assert_vectors(&terminal.vectors, 7);
        assert_eq!(terminal.code, installed.code);
        assert_eq!(installed.code, installation.binding.as_ref().unwrap().code);
        assert_eq!(installed.code[0].span, [0x40020, 0x40100]);
        assert_eq!(installed.code[1].span, [0x427b8, 0x427d4]);
        assert_eq!(terminal.vectors.tcm, installation.record2_handoff.as_ref().unwrap().tcm);
        let stop = &receipt.stops[installation.installed_stop.unwrap()];
        assert_eq!(stop.label, Label::KernelVectorsInstalled);
        assert_eq!(stop.address, 0x40088);
        assert_eq!(stop.core, 0);
        assert!(receipt.position(Label::FirstStageHandoff) < installation.installed_stop.unwrap());
        assert!(installation.installed_stop.unwrap() < receipt.position(continuation));
        for code in &installed.code {
            assert_eq!(code.bytes.r#virtual, code.bytes.physical);
            assert_eq!(code.bytes.physical.len(), (code.span[1] - code.span[0]) as usize);
        }
        let zero = receipt.row(receipt.firststage_zero.as_ref().unwrap());
        assert_eq!(zero.source, Source::CpuWrite);
        assert_eq!(zero.core, Some(0));
        assert_eq!(zero.pc, Some(0x6014));
        assert_eq!(zero.physical, 0xa00c4);
        assert_eq!(zero.size, 4);
        assert_eq!(zero.value, Some(0));
        assert_eq!(zero.before.as_deref(), Some(&[0, 0, 0, 0][..]));
        for (reference, step) in installation.events.iter().zip(STEPS) {
            let row = receipt.row(reference);
            assert_eq!(row.source, Source::CpuWrite);
            assert_eq!(row.core, Some(0));
            assert_eq!(row.pc, Some(u64::from(step.site)));
            assert_eq!(row.physical, u64::from(step.slot));
            assert_eq!(row.size, 4);
            assert_eq!(row.value, Some(u64::from(step.value)));
            assert_eq!(row.before.as_deref(), Some(step.before.to_le_bytes().as_slice()));
        }
        for pair in installation.events.windows(2) {
            assert!(receipt.row(&pair[0]).instructions.unwrap() < receipt.row(&pair[1]).instructions.unwrap());
        }
        assert!(zero.instructions.unwrap() < receipt.row(&installation.events[0]).instructions.unwrap());
        assert!(!installation.events.contains(receipt.firststage_zero.as_ref().unwrap()));
    }

    fn assert_accepted(receipt: &Receipt, at_handoff: usize) {
        assert_installation(receipt, at_handoff, Label::HeldGate);
        let record1 = receipt.position(Label::Record1Entry);
        let record2 = receipt.position(Label::Record2Handoff);
        assert!(receipt.stops[record1].watch_records.is_none());
        let loaded_stops = &receipt.stops[record1 + 1..=record2];
        for stop in loaded_stops {
            assert!(stop.counter.is_none());
            let cursor = stop.watch_records.unwrap();
            assert!(cursor <= receipt.loaded_watch.records.len());
            let through: u64 = stop.counts.iter().sum();
            assert!(receipt.loaded_watch.records[..cursor].iter().filter_map(|row| row.instructions).all(|instructions| instructions <= through));
            assert!(receipt.loaded_watch.records[cursor..].iter().filter_map(|row| row.instructions).all(|instructions| instructions > through));
        }
        for pair in loaded_stops.windows(2) {
            assert!(pair[0].watch_records.unwrap() <= pair[1].watch_records.unwrap());
        }
        assert_eq!(receipt.stops[record2].watch_records, Some(receipt.loaded_watch.records.len()));
        assert_eq!(receipt.stops[receipt.position(Label::PrefixEntry)].watch_records, Some(0));
        let scoped = receipt.scoped_watch.as_ref().unwrap();
        assert!(receipt.loaded_watch.violation.is_none());
        assert!(scoped.history.violation.is_none());
        let [counter] = scoped.counters.as_slice() else { panic!("one completion counter required") };
        assert!(counter.count >= 4);
        assert_eq!(counter.zero_count, counter.count);
        assert_eq!(counter.nonzero_count, 0);
        assert_eq!(counter.head.len(), counter.count.min(8) as usize);
        assert_eq!(counter.tail.len(), counter.count.min(2) as usize);
        for row in counter.head.iter().chain(&counter.tail) {
            assert_eq!(row.core, Some(1));
            assert_eq!(row.physical, 0xa00c4);
            assert_eq!(row.value, Some(0));
        }
        let [coverage] = scoped.coverage.as_slice() else { panic!("one zero coverage rule required") };
        assert_eq!(coverage.covered_words, 0);
        assert_eq!(coverage.first_uncovered, Some(0xddaa0));
        let scoped_stops = receipt.stops.iter().filter(|stop| stop.counter.is_some());
        for (before, after) in scoped_stops.clone().zip(scoped_stops.clone().skip(1)) {
            assert!(before.counter.as_ref().unwrap().count <= after.counter.as_ref().unwrap().count);
            assert!(before.watch_records.unwrap() <= after.watch_records.unwrap());
        }
        let last = scoped_stops.last().unwrap();
        assert_eq!(last.counter.as_ref().unwrap().count, counter.count);
        assert_eq!(last.counter.as_ref().unwrap().zero_count, counter.count);
        assert_eq!(last.counter.as_ref().unwrap().nonzero_count, 0);
        assert_eq!(last.watch_records, Some(scoped.history.records.len()));
    }

    fn assert_completed(receipt: &Receipt, scenario: Scenario) {
        assert_installation(receipt, 0, Label::CompletionStore);
        let installation = &receipt.kernel_vector_install;
        assert_eq!(receipt.firststage_zero.as_ref().unwrap().epoch, HistoryEpoch::Scoped);
        assert!(installation.events.iter().all(|reference| reference.epoch == HistoryEpoch::Scoped));
        assert!(receipt.loaded_watch.records.is_empty());
        assert!(receipt.loaded_watch.violation.is_none());
        let scoped = receipt.scoped_watch.as_ref().unwrap();
        assert!(scoped.history.violation.is_none());
        let pre = &receipt.stops[receipt.position(Label::CompletionStore)];
        assert_eq!((pre.core, pre.address, pre.completion), (0, 0x40a08, 0));
        assert_eq!((pre.registers[3], pre.registers[5]), (0x400a0000, 1));
        let cursor = pre.watch_records.unwrap();
        assert!(cursor > 0);
        assert_eq!(cursor, installation.events.last().unwrap().index + 1);
        assert_eq!(scoped.history.records.len(), cursor + 1);
        let committed = receipt.completion.as_ref().unwrap();
        assert_eq!(committed.watch_records, cursor + 1);
        assert_eq!((committed.instruction.r#virtual, committed.instruction.physical), (0xe58350c4, 0xe58350c4));
        assert_eq!((committed.before_pc, committed.after_pc), (0x40a08, 0x40a0c));
        assert_eq!(committed.before_cpsr, pre.cpsr);
        assert_eq!(committed.after_cpsr, pre.cpsr);
        assert_eq!((committed.readback.r#virtual, committed.readback.physical), (1, 1));
        assert!(!committed.microsteps.is_empty());
        assert_eq!(committed.microsteps[0].before, pre.counts);
        for pair in committed.microsteps.windows(2) {
            assert_eq!(pair[0].after, pair[1].before);
        }
        for step in &committed.microsteps {
            assert!(step.after == [step.before[0] + 1, step.before[1]]
                || step.after == [step.before[0], step.before[1] + 1]);
        }
        let last = committed.microsteps.last().unwrap();
        assert_eq!(last.after, [last.before[0] + 1, last.before[1]]);
        assert_eq!(last.cell, 1);
        assert!(committed.microsteps[..committed.microsteps.len() - 1].iter().all(|step| step.cell == 0));
        let row = &scoped.history.records[cursor];
        assert_eq!(row.source, Source::CpuWrite);
        assert_eq!((row.core, row.pc, row.instructions), (Some(0), Some(0x40a08), Some(last.after.iter().sum())));
        assert_eq!((row.physical, row.size, row.value), (0xa00c4, 4, Some(1)));
        assert_eq!(row.before.as_deref(), Some(&[0, 0, 0, 0][..]));
        assert_eq!(row.address, Some(0x400a00c4));
        assert_eq!(row.byte_order, Some(ByteOrder::Little));
        let [counter] = scoped.counters.as_slice() else { panic!("one completion counter required") };
        assert!(counter.zero_count >= 4);
        assert_eq!(counter.nonzero_count, 1);
        assert_eq!(counter.count, counter.zero_count + counter.nonzero_count);
        assert_eq!(counter.head.len(), counter.count.min(8) as usize);
        assert_eq!(counter.tail.len(), counter.count.min(2) as usize);
        for sample in counter.head.iter().chain(&counter.tail) {
            assert_eq!(sample.source, Source::CpuRead);
            assert_eq!((sample.core, sample.pc, sample.physical, sample.size), (Some(1), Some(0x10430), 0xa00c4, 4));
            assert!(matches!(sample.value, Some(0 | 1)));
        }
        let first = counter.first_nonzero.as_ref().unwrap();
        assert_eq!(counter.first_nonzero_index, Some(counter.count - 1));
        assert_eq!(first.source, Source::CpuRead);
        assert_eq!((first.core, first.pc, first.physical, first.size, first.value), (Some(1), Some(0x10430), 0xa00c4, 4, Some(1)));
        assert_eq!(first.before.as_deref(), Some(&[1, 0, 0, 0][..]));
        assert!(first.instructions.unwrap() > row.instructions.unwrap());
        assert_eq!(counter.tail.last().unwrap().value, Some(1));
        assert_eq!(committed.counter.nonzero_count, 0);
        assert_eq!(committed.counter.count, committed.counter.zero_count);
        assert!(committed.counter.count >= pre.counter.as_ref().unwrap().count);
        assert!(counter.zero_count >= committed.counter.count);
        let [coverage] = scoped.coverage.as_slice() else { panic!("one zero coverage rule required") };
        assert_eq!(coverage.covered_words, (0xf3420 - 0xddaa0) / 4);
        assert!(coverage.first_uncovered.is_none());
        let posts: Vec<_> = receipt.stops.iter().filter(|stop| stop.label == Label::PostLoad).collect();
        assert_eq!(posts.len(), 4);
        for pair in posts.windows(2) {
            assert_eq!(pair[1].counts[1] - pair[0].counts[1], 256);
            assert_eq!(pair[1].counter.as_ref().unwrap().count, pair[0].counter.as_ref().unwrap().count + 1);
        }
        for stop in &posts {
            assert_eq!(stop.registers[0], 0);
            assert_eq!(stop.counter.as_ref().unwrap().nonzero_count, 0);
        }
        for stop in receipt.stops.iter().filter(|stop| stop.counter.is_some()) {
            let cursor = stop.watch_records.unwrap();
            let through = stop.counts.iter().sum::<u64>();
            assert!(scoped.history.records[..cursor].iter().all(|row| row.instructions.unwrap() <= through));
            assert!(scoped.history.records[cursor..].iter().all(|row| row.instructions.unwrap() > through));
        }
        let exit = &receipt.stops[receipt.position(Label::PrefixExit)];
        let handoff = &receipt.stops[receipt.position(Label::Record2Handoff)];
        assert_eq!((exit.core, exit.address, exit.sp, exit.lr), (1, 0xd0aac, 0x400f3410, 0xd09bc));
        assert!(exit.counts[0] >= last.after[0]);
        assert!(exit.counts[1] > last.after[1]);
        assert_eq!(&exit.registers[4..7], &[0x69cdf8, 0x69cdf8, 0xffff0000]);
        assert_eq!(&exit.registers[7..11], &handoff.registers[7..11]);
        assert_eq!(exit.registers[11], 0x400f341c);
        assert_eq!(exit.cpsr & !0xf0000000, handoff.cpsr & !0xf0000000);
        assert_eq!(exit.watch_records, Some(scoped.history.records.len()));
        assert_eq!(exit.counter.as_ref().unwrap(), &Counter { count: counter.count, zero_count: counter.zero_count, nonzero_count: counter.nonzero_count });
        let endpoint = receipt.endpoint.as_ref().unwrap();
        let frame: Vec<_> = [0x400f3420u32, 0x69cdf8, handoff.registers[11], 0xd00c0].into_iter().flat_map(u32::to_le_bytes).collect();
        let vectors: Vec<_> = [0xd0100u32, 0xd01b4, 0xd0128, 0xd0148, 0xd0168, 0xd0204, 0xd0184].into_iter().flat_map(u32::to_le_bytes).collect();
        assert_eq!(endpoint.frame.r#virtual, frame);
        assert_eq!(endpoint.frame.physical, frame);
        assert_eq!(endpoint.vectors.r#virtual, vectors);
        assert_eq!(endpoint.vectors.physical, vectors);
        assert_eq!(endpoint.zero.range, [0xddaa0, 0xf33ec]);
        assert!(endpoint.zero.first_nonzero.is_none());
        assert_eq!(Some(&endpoint.record_immutable_sha256), receipt.handoff_record_immutable_sha256.as_ref());
        assert_eq!(endpoint.loader_sha256, digest(&vec![0; 0xddaa0 - 0xd0000]));
        if matches!(scenario, Scenario::Delayed) {
            let entry = &receipt.stops[receipt.position(Label::DelayEntry)];
            let release = &receipt.stops[receipt.position(Label::DelayRelease)];
            assert_eq!((entry.address, entry.core, entry.lr), (0x180, 0, 0x40a04));
            assert_eq!((release.address, release.core), (0x194, 0));
            assert_eq!((release.sp, release.registers[11]), (entry.sp, entry.registers[11]));
            assert_eq!(release.cpsr & 0xf8000000, entry.cpsr & 0xf8000000);
            assert!(pre.counter.as_ref().unwrap().count >= entry.counter.as_ref().unwrap().count + 4);
            assert!(receipt.position(Label::DelayEntry) < receipt.position(Label::DelayRelease));
            assert!(receipt.position(Label::DelayRelease) < receipt.position(Label::CompletionStore));
        } else {
            assert!(!receipt.stops.iter().any(|stop| matches!(stop.label, Label::DelayEntry | Label::DelayRelease)));
        }
    }

    #[test]
    fn natural_and_delayed_composed_producers_commit_after_nonempty_scoped_protocol_history() {
        for scenario in [Scenario::Natural, Scenario::Delayed] {
            let (result, receipt) = observe_scenario(FirstHandoff::Late, Mutation::None, scenario);
            assert!(result.is_ok(), "{result:?}");
            assert_completed(&receipt, scenario);
        }
    }

    fn assert_nand_row(row: &ProtocolRow, transfer: Transfer) {
        assert_eq!(row.source, Source::ModelBusWrite);
        let size = match transfer { Transfer::Installer => 512, Transfer::CompletionGuards => 3072 };
        assert_eq!((row.physical, row.size), (0x40000, size));
        assert_eq!((row.core, row.pc, row.instructions, row.value), (None, None, None, None));
        assert_eq!(row.address, Some(0x40000));
        assert!(row.byte_order.is_none());
        assert!(row.before.is_none());
    }

    #[test]
    fn real_nand_staging_write_is_accepted_before_binding_and_refused_after_installation() {
        let (result, receipt) = observe(FirstHandoff::Early, Mutation::NandBeforeHandoff);
        assert!(result.is_ok(), "{result:?}");
        assert_accepted(&receipt, 7);
        let rows = &receipt.loaded_watch.records;
        let transfers: Vec<_> = rows.iter().enumerate().filter(|(_, row)| row.source == Source::ModelBusWrite).collect();
        let [(index, transfer)] = transfers.as_slice() else { panic!("one NAND staging transfer required") };
        assert_nand_row(transfer, Transfer::Installer);
        let first = &receipt.stops[receipt.position(Label::FirstStageHandoff)];
        assert!(*index < first.watch_records.unwrap());
        assert_eq!(receipt.firststage_zero.as_ref().unwrap().epoch, HistoryEpoch::Loaded);
        assert!(receipt.firststage_zero.as_ref().unwrap().index < *index);
        assert!(receipt.scoped_watch.as_ref().unwrap().history.records.is_empty());

        let (result, receipt) = observe(FirstHandoff::Early, Mutation::NandAfterInstall);
        assert_refused(result, &receipt);
        assert!(receipt.machine_failure.is_none());
        let installation = &receipt.kernel_vector_install;
        assert_eq!(installation.state, InstallState::Installed);
        assert_eq!(installation.events.len(), 7);
        let installed_stop = &receipt.stops[installation.installed_stop.unwrap()];
        assert_eq!((installed_stop.label == Label::KernelVectorsInstalled, installed_stop.core, installed_stop.address), (true, 0, 0x40088));
        let installed = installation.installed_readback.as_ref().unwrap();
        assert_vectors(&installed.vectors, 7);
        assert_eq!(installed.code, installation.binding.as_ref().unwrap().code);
        assert!(installation.terminal_readback.is_none());
        assert!(receipt.completion.is_none());
        assert!(receipt.endpoint.is_none());
        assert!(receipt.loaded_watch.violation.is_none());
        let scoped = receipt.scoped_watch.as_ref().map(|watch| &watch.history);
        assert!(scoped.is_none_or(|history| history.violation.is_none()));
        let transfers: Vec<_> = receipt.loaded_watch.records.iter().chain(scoped.into_iter().flat_map(|history| &history.records))
            .filter(|row| row.source == Source::ModelBusWrite).collect();
        let [transfer] = transfers.as_slice() else { panic!("one refused NAND transfer required") };
        assert_nand_row(transfer, Transfer::Installer);
        assert!(installation.events.iter().all(|reference| reference.epoch == HistoryEpoch::Loaded));
        let transfer_index = receipt.loaded_watch.records.iter().position(|row| row.source == Source::ModelBusWrite).unwrap();
        assert!(transfer_index >= installed_stop.watch_records.unwrap());
        assert!(installation.events.iter().all(|reference| reference.index < transfer_index));
        for (reference, step) in installation.events.iter().zip(STEPS) {
            let row = receipt.row(reference);
            assert_eq!(row.pc, Some(u64::from(step.site)));
            assert_eq!(row.value, Some(u64::from(step.value)));
        }
    }

    #[test]
    fn real_nand_staging_transfer_must_not_overlap_scoped_completion_code_or_literal() {
        let (result, accepted) = observe(FirstHandoff::Late, Mutation::NandBeforeHandoff);
        assert!(result.is_ok(), "{result:?}");
        assert_accepted(&accepted, 0);
        assert!(accepted.loaded_watch.records.is_empty());
        let accepted_history = &accepted.scoped_watch.as_ref().unwrap().history;
        let accepted_transfers: Vec<_> = accepted_history.records.iter().enumerate()
            .filter(|(_, row)| row.source == Source::ModelBusWrite).collect();
        let [(accepted_index, accepted_transfer)] = accepted_transfers.as_slice() else { panic!("one accepted scoped NAND transfer required") };
        assert_nand_row(accepted_transfer, Transfer::Installer);
        let handoff = &accepted.stops[accepted.position(Label::FirstStageHandoff)];
        assert!(*accepted_index < handoff.watch_records.unwrap());
        let zero_ref = accepted.firststage_zero.as_ref().unwrap();
        assert_eq!(zero_ref.epoch, HistoryEpoch::Scoped);
        assert!(zero_ref.index < *accepted_index);
        assert!(accepted.kernel_vector_install.events.iter().all(|reference|
            reference.epoch == HistoryEpoch::Scoped && reference.index > *accepted_index));
        let installed = accepted.kernel_vector_install.installed_readback.as_ref().unwrap();
        assert_eq!(installed.code, accepted.kernel_vector_install.binding.as_ref().unwrap().code);

        let (result, refused) = observe(FirstHandoff::Late, Mutation::NandProtectedOverlap);
        assert_refused(result, &refused);
        assert!(refused.machine_failure.is_none());
        assert_eq!(refused.initial_record_sha256, accepted.initial_record_sha256);
        assert_eq!(refused.handoff_record_immutable_sha256, accepted.handoff_record_immutable_sha256);
        assert!(refused.loaded_watch.records.is_empty());
        assert!(refused.loaded_watch.violation.is_none());
        let history = &refused.scoped_watch.as_ref().unwrap().history;
        assert!(history.violation.is_none());
        let transfers: Vec<_> = history.records.iter().enumerate()
            .filter(|(_, row)| row.source == Source::ModelBusWrite).collect();
        let [(index, transfer)] = transfers.as_slice() else { panic!("one refused compound NAND transfer required") };
        assert_nand_row(transfer, Transfer::CompletionGuards);
        assert_eq!(transfer.physical + transfer.size, 0x40c00);
        let refused_zero_ref = refused.firststage_zero.as_ref().unwrap();
        assert_eq!(refused_zero_ref.epoch, HistoryEpoch::Scoped);
        assert!(refused_zero_ref.index < *index);
        let accepted_zero = accepted.row(zero_ref);
        let refused_zero = refused.row(refused_zero_ref);
        assert_eq!(refused_zero.source, Source::CpuWrite);
        assert_eq!((refused_zero.core, refused_zero.pc, refused_zero.physical, refused_zero.size, refused_zero.value),
            (accepted_zero.core, accepted_zero.pc, accepted_zero.physical, accepted_zero.size, accepted_zero.value));
        assert_eq!(refused_zero.before, accepted_zero.before);
        assert_eq!(history.records.len(), 2);
        assert!(!refused.stops.iter().any(|stop| stop.label == Label::FirstStageHandoff));
        assert!(refused.stops.iter().any(|stop| stop.label == Label::PrefixEntry));
        let installation = &refused.kernel_vector_install;
        assert_eq!(installation.state, InstallState::NotBound);
        assert!(installation.binding.is_none());
        assert!(installation.events.is_empty());
        assert!(installation.installed_stop.is_none());
        assert!(installation.installed_readback.is_none());
        assert!(installation.terminal_readback.is_none());
        assert!(refused.completion.is_none());
        assert!(refused.endpoint.is_none());
    }

    fn assert_refused(result: Result<(), String>, receipt: &Receipt) {
        let error = result.unwrap_err();
        assert_eq!(receipt.result, ResultStatus::Failed);
        assert_eq!(receipt.error.as_deref(), Some(error.as_str()));
        assert!(!receipt.stops.iter().any(|stop| stop.label == Label::HeldGate));
    }

    #[test]
    fn composed_observation_accepts_both_cross_core_handoff_orders() {
        for order in [FirstHandoff::Early, FirstHandoff::Late] {
            let (result, receipt) = observe(order, Mutation::None);
            assert!(result.is_ok(), "{result:?}");
            assert_accepted(&receipt, if matches!(order, FirstHandoff::Early) { 7 } else { 0 });
            let first = receipt.position(Label::FirstStageHandoff);
            let second = receipt.position(Label::Record2Handoff);
            assert!(receipt.position(Label::Record1Entry) < first);
            assert!(receipt.position(Label::SecondaryEntry) < second);
            assert!(second < receipt.position(Label::PrefixEntry));
            assert_eq!(first < second, matches!(order, FirstHandoff::Early));
            assert_eq!(receipt.stops[first].core, 0);
            if matches!(order, FirstHandoff::Late) {
                assert!(receipt.position(Label::PrefixEntry) < first);
                assert!(receipt.stops[first].counter.as_ref().is_some_and(|counter| counter.count > 0 && counter.nonzero_count == 0));
                assert!(receipt.stops[first].watch_records.is_some());
                assert_eq!(receipt.firststage_zero.as_ref().unwrap().epoch, HistoryEpoch::Scoped);
            } else {
                assert!(receipt.stops[first].counter.is_none());
                assert!(receipt.stops[first].watch_records.is_some());
                assert_eq!(receipt.firststage_zero.as_ref().unwrap().epoch, HistoryEpoch::Loaded);
            }
        }
    }

    #[test]
    fn every_partial_installation_survives_the_record_two_observer_handoff() {
        for k in 1..7 {
            let (result, receipt) = observe(FirstHandoff::Partial(k), Mutation::None);
            assert!(result.is_ok(), "prefix {k}: {result:?}");
            assert_accepted(&receipt, k);
            let handoff = &receipt.stops[receipt.position(Label::Record2Handoff)];
            assert_eq!(handoff.counts[0], 1000);
            assert_eq!(receipt.kernel_vector_install.events.iter().filter(|event| event.epoch == HistoryEpoch::Loaded).count(), k);
            assert_eq!(receipt.kernel_vector_install.events.iter().filter(|event| event.epoch == HistoryEpoch::Scoped).count(), 7 - k);
            assert!(receipt.stops[receipt.position(Label::PrefixEntry)].watch_records.is_some_and(|cursor| cursor == 0));
            assert!(receipt.kernel_vector_install.installed_readback.as_ref().unwrap().vectors.tcm
                == receipt.kernel_vector_install.record2_handoff.as_ref().unwrap().tcm);
        }
    }

    #[test]
    fn composed_observation_refuses_missing_or_invalid_first_stage_handoff() {
        for order in [FirstHandoff::Missing, FirstHandoff::Invalid] {
            let (result, receipt) = observe(order, Mutation::None);
            assert_refused(result, &receipt);
            assert!(receipt.machine_failure.is_none());
            assert_eq!(receipt.kernel_vector_install.state, InstallState::NotBound);
            assert!(receipt.kernel_vector_install.events.is_empty());
            assert!(receipt.kernel_vector_install.installed_stop.is_none());
            assert!(receipt.stops.iter().any(|stop| stop.label == Label::Record2Handoff));
            assert!(receipt.stops.iter().any(|stop| stop.label == Label::PrefixEntry));
            assert_eq!(
                receipt.stops.iter().any(|stop| stop.label == Label::FirstStageHandoff),
                matches!(order, FirstHandoff::Invalid),
            );
            assert!(receipt.loaded_watch.violation.is_none());
            assert!(receipt.scoped_watch.as_ref().unwrap().history.violation.is_none());
        }
    }

    #[test]
    fn code_staging_restore_is_bound_before_installation() {
        let (result, receipt) = observe(FirstHandoff::Early, Mutation::CodeStaging);
        assert!(result.is_ok(), "{result:?}");
        assert_accepted(&receipt, 7);
        let mut staged = receipt.loaded_watch.records.iter().filter(|row| row.physical == 0x427b8);
        assert_eq!(staged.next().unwrap().value, Some(0));
        assert_eq!(staged.next().unwrap().value, Some(0xee121e10));
        assert!(staged.next().is_none());
    }

    #[test]
    fn full_bootstrap_code_and_zero_literal_proofs_are_bound_to_public_source_facts() {
        for mutation in [
            Mutation::BootstrapOpcode, Mutation::BootstrapReset, Mutation::BootstrapHandler,
            Mutation::CodeHash, Mutation::SelectorHash, Mutation::ZeroLiteral, Mutation::ZeroWriter,
        ] {
            let (result, receipt) = observe(FirstHandoff::Early, mutation);
            assert_refused(result, &receipt);
            assert!(receipt.machine_failure.is_none());
            assert!(receipt.kernel_vector_install.events.is_empty());
            assert!(receipt.kernel_vector_install.installed_stop.is_none());
        }
    }

    #[test]
    fn declared_interleaved_tail_records_the_owned_illegal_store_after_the_first_canonical_store() {
        let (result, receipt) = observe_scenario(FirstHandoff::Early, Mutation::None, Scenario::NegativeInterleavedTail);
        assert_refused(result, &receipt);
        assert!(receipt.machine_failure.is_none());
        assert_eq!(receipt.kernel_vector_install.state, InstallState::Installing);
        assert_eq!(receipt.kernel_vector_install.events.len(), 1);
        let first = receipt.row(&receipt.kernel_vector_install.events[0]);
        assert_eq!((first.pc, first.physical, first.value), (Some(0x4003c), 0x24, Some(0x40100)));
        let illegal = receipt.loaded_watch.records.iter().chain(
            receipt.scoped_watch.iter().flat_map(|watch| &watch.history.records),
        ).find(|row| row.pc == Some(0x6048)).unwrap();
        assert_eq!(illegal.source, Source::CpuWrite);
        assert_eq!((illegal.core, illegal.address, illegal.physical, illegal.size), (Some(0), Some(0x24), 0x24, 4));
        assert_eq!(illegal.value, Some(0xdeadbe00));
        assert_eq!(illegal.before, Some(0x40100_u32.to_le_bytes().to_vec()));
        assert!(receipt.kernel_vector_install.installed_stop.is_none());
        assert!(receipt.completion.is_none());
        assert!(receipt.endpoint.is_none());
    }

    #[test]
    fn declared_interleaved_tail_preserves_helper_branch_and_full_canonical_code_identity() {
        for mutation in [Mutation::InterleavedHelper, Mutation::InterleavedBranch, Mutation::InterleavedExtra] {
            let (result, receipt) = observe_scenario(FirstHandoff::Early, mutation, Scenario::NegativeInterleavedTail);
            assert_refused(result, &receipt);
            assert!(receipt.machine_failure.is_none());
            assert_eq!(receipt.kernel_vector_install.state, InstallState::NotBound);
            assert!(receipt.kernel_vector_install.events.is_empty());
            assert!(receipt.kernel_vector_install.installed_stop.is_none());
            assert!(receipt.loaded_watch.records.iter().all(|row| row.pc != Some(0x6048)));
        }
    }

    #[test]
    fn malformed_bound_install_code_and_zero_preimage_are_refused() {
        for mutation in [Mutation::WrongWidth, Mutation::WrongOrder, Mutation::WrongValue, Mutation::WrongPreimage] {
            let (result, receipt) = observe(FirstHandoff::Early, mutation);
            assert_refused(result, &receipt);
            assert!(receipt.machine_failure.is_none());
            assert!(receipt.kernel_vector_install.installed_stop.is_none());
            assert!(receipt.kernel_vector_install.events.is_empty());
        }
    }

    #[test]
    fn tail_writes_before_handoff_and_from_the_other_core_are_refused_at_a_boundary() {
        for mutation in [Mutation::BeforeHandoff, Mutation::WrongCore] {
            let (result, receipt) = observe(FirstHandoff::Late, mutation);
            assert_refused(result, &receipt);
            assert!(receipt.machine_failure.is_none());
            assert!(receipt.kernel_vector_install.installed_stop.is_none());
            let rows = receipt.loaded_watch.records.iter().chain(
                receipt.scoped_watch.iter().flat_map(|watch| &watch.history.records),
            );
            assert!(rows.into_iter().any(|row| row.source == Source::CpuWrite && row.physical == 0x24));
        }
    }

    #[test]
    fn post_install_tail_and_code_restore_writes_preserve_the_first_terminal_error() {
        for mutation in [Mutation::AfterInstall, Mutation::CodeRestore] {
            let (result, receipt) = observe(FirstHandoff::Late, mutation);
            assert_refused(result, &receipt);
            assert!(receipt.machine_failure.is_none());
            assert_eq!(receipt.kernel_vector_install.state, InstallState::Installed);
            assert_eq!(receipt.kernel_vector_install.events.len(), 7);
            assert!(receipt.kernel_vector_install.installed_stop.is_some());
            let installed = receipt.kernel_vector_install.installed_readback.as_ref().unwrap();
            assert_vectors(&installed.vectors, 7);
            assert!(receipt.kernel_vector_install.terminal_readback.is_none());
            let history = &receipt.scoped_watch.as_ref().unwrap().history;
            assert!(history.violation.is_none());
            let target = if matches!(mutation, Mutation::AfterInstall) { 0x24 } else { 0x427b8 };
            assert!(history.records.iter().any(|row| row.source == Source::CpuWrite && row.pc == Some(0x43008) && row.physical == target));
        }
    }

    #[test]
    fn vector_opcodes_and_reset_literal_remain_engine_confined() {
        for mutation in [Mutation::OpcodeWrite, Mutation::ResetWrite] {
            let (result, receipt) = observe(FirstHandoff::Late, mutation);
            assert_refused(result, &receipt);
            assert_eq!(receipt.kernel_vector_install.events.len(), 7);
            assert_vectors(&receipt.kernel_vector_install.installed_readback.as_ref().unwrap().vectors, 7);
            assert!(receipt.scoped_watch.as_ref().unwrap().history.violation.is_some());
        }
    }
}

#[test]
fn lifecycle_save_return_observes_its_status_output_cell() {
    let mut observed = synthetic_lifecycle_machine(Guest(0x00e99e90));
    observed.session.board.write_register(0, arm::CORE[11], 0x40003500).unwrap();
    observed.session.board.write_physical(0x34e0, &2u32.to_le_bytes()).unwrap();
    let page = diagnostic_page(&mut observed);
    assert!(!page.hits[0].ram_incomplete);
    assert_observed_bytes(&page.hits[0], Guest(0x34e0), &2u32.to_le_bytes());
}

#[test]
fn lifecycle_save_return_refuses_a_status_cell_underflow_without_substitution() {
    let mut observed = synthetic_lifecycle_machine(Guest(0x00e99e90));
    observed.session.board.write_register(0, arm::CORE[11], 0x1f).unwrap();
    let page = diagnostic_page(&mut observed);
    assert!(page.hits[0].ram_incomplete);
    assert!(page.hits[0].ram.iter().all(|span| span.address.0 != 0));
}

#[test]
fn lifecycle_invalid_release_observes_the_stock_no_frame_branch() {
    let mut observed = synthetic_lifecycle_machine(Guest(0x00e87d80));
    observed.session.board.write_register(0, arm::CORE[0], 201).unwrap();
    let page = diagnostic_page(&mut observed);
    assert!(!page.hits[0].ram_incomplete);
    assert_eq!(page.hits[0].registers[0], 201);
    assert_observed_bytes(&page.hits[0], Guest(0x00e87d80), &0xeafffffeu32.to_le_bytes());
    assert!(page.hits[0].ram.iter().all(|span| span.address.0 < 0x04988600));
}
