//! `FUJI_NATIVE_V1` wires judged by the observation `native_case::run` prints.

use fujimod_xm1::native_case;
use serde::Deserialize;
use std::collections::BTreeMap;

const CODE: u32 = 0x1000;
const EXIT: u32 = 0x10f0;
const STACK: u32 = 0x3000;
const SP: u32 = 0x3100;
const STUB: u32 = 0x2000;
const READ: u8 = 1;
const READ_WRITE: u8 = 3;
const READ_EXECUTE: u8 = 5;

const BX_LR: u32 = 0xe12f_ff1e;
const PUSH_LR: u32 = 0xe92d_4000;
const POP_PC: u32 = 0xe8bd_8000;
const ADD_R0_1: u32 = 0xe280_0001;
const STR_R0_R1_POST_4: u32 = 0xe481_0004;
const LDR_R0_R1: u32 = 0xe591_0000;

#[derive(Clone)]
struct Region {
    address: u32,
    size: u32,
    permission: u8,
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct Stub {
    address: u32,
    inputs: Vec<(u8, u32)>,
    outputs: Vec<(u8, u32)>,
}

/// One case: code at `CODE`, the exit inside the code region, a stack below `SP`.
#[derive(Clone)]
struct Case {
    entry: u32,
    exit: u32,
    registers: [u32; 16],
    instructions: u32,
    wall_ms: u32,
    regions: Vec<Region>,
    functions: Vec<(u32, u32)>,
    stubs: Vec<Stub>,
}

fn words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

impl Case {
    fn new(code: &[u32]) -> Self {
        let mut bytes = vec![0u8; 0x100];
        bytes[..4 * code.len()].copy_from_slice(&words(code));
        let mut registers = [0u32; 16];
        registers[13] = SP;
        registers[14] = EXIT;
        registers[15] = 0x10;
        Self {
            entry: CODE,
            exit: EXIT,
            registers,
            instructions: 1000,
            wall_ms: 1000,
            regions: vec![
                Region { address: CODE, size: 0x100, permission: READ_EXECUTE, bytes },
                Region { address: STACK, size: 0x100, permission: READ_WRITE, bytes: vec![0; 0x100] },
            ],
            functions: vec![(CODE, 4 * code.len() as u32)],
            stubs: Vec::new(),
        }
    }

    fn code(mut self, address: u32, code: &[u32]) -> Self {
        let offset = (address - CODE) as usize;
        self.regions[0].bytes[offset..offset + 4 * code.len()].copy_from_slice(&words(code));
        self
    }

    fn function(mut self, address: u32, size: u32) -> Self {
        self.functions.push((address, size));
        self
    }

    fn region(mut self, address: u32, permission: u8, bytes: &[u8]) -> Self {
        self.regions.push(Region { address, size: bytes.len() as u32, permission, bytes: bytes.to_vec() });
        self
    }

    fn register(mut self, index: usize, value: u32) -> Self {
        self.registers[index] = value;
        self
    }

    fn stub(mut self, address: u32, inputs: &[(u8, u32)], outputs: &[(u8, u32)]) -> Self {
        self.stubs.push(Stub { address, inputs: inputs.to_vec(), outputs: outputs.to_vec() });
        self
    }

    /// Four regions sharing page 0x4000: r, rw, r, an unmapped gap, then rw.
    fn shared_page(self) -> Self {
        self.region(0x4000, READ, &[0x44, 0x33, 0x22, 0x11])
            .region(0x4004, READ_WRITE, &[0; 4])
            .region(0x4008, READ, &[0xdd, 0xcc, 0xbb, 0xaa])
            .region(0x4010, READ_WRITE, &[1, 2, 3, 4])
    }

    fn wire(&self) -> String {
        let mut text = format!(
            "FUJI_NATIVE_V1\ncortex-r5f-arm-le\n{:x} {:x} {} {}\n",
            self.entry, self.exit, self.instructions, self.wall_ms
        );
        let registers: Vec<String> = self.registers.iter().map(|value| format!("{value:x}")).collect();
        text += &registers.join(" ");
        text += &format!("\n{} {} {}\n", self.regions.len(), self.functions.len(), self.stubs.len());
        for region in &self.regions {
            let hex: String = region.bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            text += &format!("{:x} {:x} {} {hex}\n", region.address, region.size, region.permission);
        }
        for (address, size) in &self.functions {
            text += &format!("{address:x} {size:x}\n");
        }
        for stub in &self.stubs {
            text += &format!("{:x} {} {}\n", stub.address, stub.inputs.len(), stub.outputs.len());
            for (register, value) in stub.inputs.iter().chain(&stub.outputs) {
                text += &format!("{register} {value:x}\n");
            }
        }
        text
    }

    fn run(&self) -> Observed {
        observe(self.wire().as_bytes())
    }
}

#[derive(Deserialize)]
struct Memory {
    address: String,
    bytes: Option<String>,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Event {
    address: String,
    inputs: BTreeMap<String, String>,
    outputs: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Trace {
    pc: String,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Read,
    Write,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Provenance {
    pc: String,
    kind: Kind,
    address: String,
    size: u32,
    value: String,
}

#[derive(Clone, Copy, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Status {
    Returned,
    InstructionBudget,
    WallBudget,
    MemoryGuard,
    FetchGuard,
    UnknownCall,
    UnsupportedInstruction,
    StubContract,
    EngineError,
    InvalidCase,
}

#[derive(Deserialize)]
struct Observation {
    wire_version: u32,
    status: Status,
    instructions: String,
    registers: BTreeMap<String, String>,
    memory: Vec<Memory>,
    events: Vec<Event>,
    trace: Vec<Trace>,
    memory_provenance_total: String,
    memory_provenance: Vec<Provenance>,
}

struct Observed {
    returned: bool,
    observation: Observation,
}

fn observe(wire: &[u8]) -> Observed {
    let outcome = native_case::run(wire);
    let text = String::from_utf8(outcome.json).unwrap();
    let observation: Observation = serde_json::from_str(&text).unwrap_or_else(|error| panic!("{error}: {text}"));
    assert_eq!(observation.wire_version, 1, "{text}");
    Observed { returned: outcome.returned, observation }
}

fn word(text: &str) -> u32 {
    u32::from_str_radix(text.strip_prefix("0x").unwrap(), 16).unwrap()
}

impl Observed {
    fn status(&self) -> Status {
        self.observation.status
    }

    fn register(&self, name: &str) -> u32 {
        word(&self.observation.registers[name])
    }

    fn bytes(&self, address: u32) -> String {
        let label = format!("0x{address:08x}");
        let region = self.observation.memory.iter().find(|region| region.address == label).unwrap();
        region.bytes.clone().unwrap()
    }

    fn instructions(&self) -> u64 {
        self.observation.instructions.parse().unwrap()
    }

    fn assert_stopped(&self, status: Status, pc: u32) {
        assert_eq!(self.status(), status);
        assert!(!self.returned);
        assert_eq!(self.register("pc"), pc);
    }

    fn assert_returned(&self) {
        assert_eq!(self.status(), Status::Returned);
        assert!(self.returned);
        assert_eq!(self.register("pc"), EXIT);
    }
}

fn branch(from: u32, to: u32, link: bool) -> u32 {
    let offset = (to.wrapping_sub(from).wrapping_sub(8) >> 2) & 0x00ff_ffff;
    (if link { 0xeb00_0000 } else { 0xea00_0000 }) | offset
}

fn bindings(pairs: &[(&str, u32)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(name, value)| (name.to_string(), format!("0x{value:08x}"))).collect()
}

#[test]
fn smlabb_multiplies_signed_bottom_halves_and_returns_the_abi_result() {
    // smlabb r0, r0, r3, r1
    for (r0, r3, r1, product) in [(0x1234_fffe, 0xabcd_0003, 100, 94), (3, 0x1d0, 0x0159_6800, 0x0159_6d70)] {
        let observed = Case::new(&[0xe100_1380, BX_LR]).register(0, r0).register(1, r1).register(3, r3).run();
        observed.assert_returned();
        assert_eq!(observed.register("r0"), product);
        assert_eq!(observed.register("lr"), EXIT);
        assert_eq!(observed.instructions(), 2);
    }
}

#[test]
fn clz_counts_leading_zeros_including_a_zero_operand() {
    // clz r0, r1
    for (operand, zeros) in [(0x0001_0000, 15), (0, 32), (0x8000_0000, 0), (1, 31)] {
        let observed = Case::new(&[0xe16f_0f11, BX_LR]).register(1, operand).run();
        observed.assert_returned();
        assert_eq!(observed.register("r0"), zeros);
    }
}

#[test]
fn post_indexed_transfers_update_the_base_memory_and_provenance() {
    // str r0, [r1], #4; ldr r3, [r1], #4; ldr r2, [r1, #-8]
    let observed = Case::new(&[STR_R0_R1_POST_4, 0xe491_3004, 0xe511_2008, BX_LR])
        .region(0x4000, READ_WRITE, &[0, 0, 0, 0, 0x88, 0x77, 0x66, 0x55])
        .register(0, 0xdead_beef)
        .register(1, 0x4000)
        .run();
    observed.assert_returned();
    assert_eq!(observed.register("r1"), 0x4008);
    assert_eq!(observed.register("r2"), 0xdead_beef);
    assert_eq!(observed.register("r3"), 0x5566_7788);
    assert_eq!(observed.bytes(0x4000), "efbeadde88776655");
    let entry = |pc: &str, kind: Kind, address: &str, value: &str| Provenance {
        pc: pc.into(),
        kind,
        address: address.into(),
        size: 4,
        value: value.into(),
    };
    assert_eq!(
        observed.observation.memory_provenance,
        [
            entry("0x00001000", Kind::Write, "0x00004000", "0x00000000deadbeef"),
            entry("0x00001004", Kind::Read, "0x00004004", "0x0000000055667788"),
            entry("0x00001008", Kind::Read, "0x00004000", "0x00000000deadbeef"),
        ]
    );
    assert_eq!(observed.observation.memory_provenance_total, "3");
}

#[test]
fn writable_and_read_only_subranges_of_one_page_keep_their_own_permissions() {
    let store = Case::new(&[STR_R0_R1_POST_4, BX_LR]).shared_page().register(0, 0xdead_beef);
    let accepted = store.clone().register(1, 0x4004).run();
    accepted.assert_returned();
    assert_eq!(accepted.bytes(0x4004), "efbeadde");
    assert_eq!(accepted.register("r1"), 0x4008);

    let load = Case::new(&[LDR_R0_R1, BX_LR]).shared_page().register(1, 0x4008).run();
    load.assert_returned();
    assert_eq!(load.register("r0"), 0xaabb_ccdd);

    for refused_at in [0x4000, 0x4008, 0x400c] {
        let refused = store.clone().register(1, refused_at).run();
        refused.assert_stopped(Status::MemoryGuard, CODE);
        assert_eq!(refused.instructions(), 1);
        assert_eq!(refused.register("r1"), refused_at, "the post-indexed base is not written back");
        assert_eq!(refused.register("r0"), 0xdead_beef);
        assert_eq!(refused.bytes(0x4000), "44332211");
        assert_eq!(refused.bytes(0x4004), "00000000");
        assert_eq!(refused.bytes(0x4008), "ddccbbaa");
        assert_eq!(refused.bytes(0x4010), "01020304");
        assert_eq!(refused.observation.memory_provenance_total, "0");
    }
}

#[test]
fn a_load_from_a_gap_inside_a_mapped_page_leaves_its_destination() {
    let observed = Case::new(&[LDR_R0_R1, BX_LR]).shared_page().register(0, 0x77).register(1, 0x400c).run();
    observed.assert_stopped(Status::MemoryGuard, CODE);
    assert_eq!(observed.register("r0"), 0x77);
}

#[test]
fn a_store_multiple_reaching_a_read_only_subrange_has_no_effect() {
    // stmia r2!, {r0, r1}
    let observed = Case::new(&[0xe8a2_0003, BX_LR])
        .shared_page()
        .register(0, 0x1111_1111)
        .register(1, 0x2222_2222)
        .register(2, 0x4004)
        .run();
    observed.assert_stopped(Status::MemoryGuard, CODE);
    assert_eq!(observed.bytes(0x4004), "00000000");
    assert_eq!(observed.bytes(0x4008), "ddccbbaa");
    assert_eq!(observed.register("r2"), 0x4004);
    assert_eq!(observed.observation.memory_provenance_total, "0");
}

#[test]
fn a_load_multiple_reaching_a_gap_leaves_every_destination() {
    // ldmia r2, {r0, r1}
    let observed = Case::new(&[0xe892_0003, BX_LR])
        .shared_page()
        .register(0, 0x77)
        .register(1, 0x88)
        .register(2, 0x4008)
        .run();
    observed.assert_stopped(Status::MemoryGuard, CODE);
    assert_eq!(observed.register("r0"), 0x77);
    assert_eq!(observed.register("r1"), 0x88);
}

#[test]
fn stores_to_read_only_and_unmapped_pages_are_refused_without_effects() {
    for (pointer, regions) in [(0x6000, true), (0x5000, false)] {
        let mut case = Case::new(&[STR_R0_R1_POST_4, BX_LR]).register(0, 0xdead_beef).register(1, pointer);
        if regions {
            case = case.region(0x6000, READ, &[9, 9, 9, 9]);
        }
        let observed = case.run();
        observed.assert_stopped(Status::MemoryGuard, CODE);
        assert_eq!(observed.register("r1"), pointer);
        if regions {
            assert_eq!(observed.bytes(0x6000), "09090909");
        }
    }
}

#[test]
fn a_doubleword_store_reaching_a_read_only_subrange_has_no_effect() {
    // strd r0, r1, [r2]
    let observed = Case::new(&[0xe1c2_00f0, BX_LR])
        .shared_page()
        .register(0, 0x1111_1111)
        .register(1, 0x2222_2222)
        .register(2, 0x4004)
        .run();
    observed.assert_stopped(Status::MemoryGuard, CODE);
    assert_eq!(observed.bytes(0x4004), "00000000");
    assert_eq!(observed.bytes(0x4008), "ddccbbaa");
    assert_eq!(observed.observation.memory_provenance_total, "0");
}

#[test]
fn register_offset_doublewords_admit_four_byte_aligned_transfers() {
    // ldrd r0, r1, [r2, r3]; strd r0, r1, [r2, r3]
    let loaded = Case::new(&[0xe182_00d3, BX_LR])
        .region(0x4004, READ, &[0x44, 0x33, 0x22, 0x11, 0xdd, 0xcc, 0xbb, 0xaa])
        .register(2, 0x4000)
        .register(3, 4)
        .run();
    loaded.assert_returned();
    assert_eq!(loaded.register("r0"), 0x1122_3344);
    assert_eq!(loaded.register("r1"), 0xaabb_ccdd);
    assert_eq!(loaded.register("r2"), 0x4000);
    assert_eq!(loaded.register("r3"), 4);

    let stored = Case::new(&[0xe182_00f3, BX_LR])
        .region(0x4004, READ_WRITE, &[0; 8])
        .register(0, 0x1122_3344)
        .register(1, 0xaabb_ccdd)
        .register(2, 0x4000)
        .register(3, 4)
        .run();
    stored.assert_returned();
    assert_eq!(stored.bytes(0x4004), "44332211ddccbbaa");
    assert_eq!(stored.register("r2"), 0x4000);
    assert_eq!(stored.register("r3"), 4);
}

#[test]
fn a_register_doubleword_store_reaching_read_only_memory_has_no_first_store() {
    let observed = Case::new(&[0xe182_00f3, BX_LR])
        .shared_page()
        .register(0, 0x1122_3344)
        .register(1, 0xaabb_ccdd)
        .register(2, 0x4000)
        .register(3, 4)
        .run();
    observed.assert_stopped(Status::MemoryGuard, CODE);
    assert_eq!(observed.bytes(0x4004), "00000000");
    assert_eq!(observed.bytes(0x4008), "ddccbbaa");
    assert_eq!(observed.register("r2"), 0x4000);
    assert_eq!(observed.observation.memory_provenance_total, "0");
}

#[test]
fn a_register_doubleword_load_reaching_a_gap_leaves_both_destinations() {
    let observed = Case::new(&[0xe182_00d3, BX_LR])
        .region(0x4004, READ, &[0x44, 0x33, 0x22, 0x11])
        .register(0, 0x77)
        .register(1, 0x88)
        .register(2, 0x4000)
        .register(3, 4)
        .run();
    observed.assert_stopped(Status::MemoryGuard, CODE);
    assert_eq!(observed.register("r0"), 0x77);
    assert_eq!(observed.register("r1"), 0x88);
    assert_eq!(observed.register("r2"), 0x4000);
    assert_eq!(observed.observation.memory_provenance_total, "0");
}

#[test]
fn a_load_from_an_unmapped_page_leaves_its_destination_and_base() {
    // ldr r0, [r1], #4
    let observed = Case::new(&[0xe491_0004, BX_LR]).register(0, 0x77).register(1, 0x5000).run();
    observed.assert_stopped(Status::MemoryGuard, CODE);
    assert_eq!(observed.register("r0"), 0x77);
    assert_eq!(observed.register("r1"), 0x5000);
}

fn stub_call(r0: u32) -> Observed {
    Case::new(&[PUSH_LR, branch(0x1004, STUB, true), ADD_R0_1, POP_PC])
        .stub(STUB, &[(0, 5)], &[(0, 0x40), (1, 7)])
        .register(0, r0)
        .run()
}

#[test]
fn a_called_stub_applies_its_contract_and_returns_after_the_call() {
    let observed = stub_call(5);
    observed.assert_returned();
    assert_eq!(observed.register("r0"), 0x41);
    assert_eq!(observed.register("r1"), 7);
    assert_eq!(observed.register("lr"), 0x1008);
    assert_eq!(observed.register("sp"), SP);
    assert_eq!(observed.instructions(), 4);
    assert_eq!(
        observed.observation.events,
        [Event {
            address: "0x00002000".into(),
            inputs: bindings(&[("r0", 5)]),
            outputs: bindings(&[("r0", 0x40), ("r1", 7)]),
        }]
    );
    assert!(observed.bytes(STACK).ends_with("f0100000"));
}

#[test]
fn a_called_stub_with_a_mismatched_input_is_a_contract_failure() {
    let observed = stub_call(6);
    observed.assert_stopped(Status::StubContract, 0x1004);
    assert!(observed.observation.events.is_empty());
    assert_eq!(observed.register("r0"), 6);
}

#[test]
fn an_entry_tail_stub_returns_to_the_exit_with_lr_preserved() {
    // bx r12
    let tail = Case::new(&[0xe12f_ff1c]).stub(STUB, &[(1, 7)], &[(0, 0x55)]).register(12, STUB);
    let observed = tail.clone().register(1, 7).run();
    observed.assert_returned();
    assert_eq!(observed.register("r0"), 0x55);
    assert_eq!(observed.register("lr"), EXIT);
    assert_eq!(observed.instructions(), 1);
    assert_eq!(observed.observation.events.len(), 1);

    let mismatch = tail.register(1, 8).run();
    mismatch.assert_stopped(Status::StubContract, CODE);
    assert!(mismatch.observation.events.is_empty());
}

fn nested_tail(inner: &[u32], r1: u32) -> Observed {
    Case::new(&[PUSH_LR, branch(0x1004, 0x1040, true), ADD_R0_1, POP_PC])
        .code(0x1040, inner)
        .function(0x1040, 4 * inner.len() as u32)
        .stub(STUB, &[(1, 7)], &[(0, 0x55)])
        .register(1, r1)
        .run()
}

#[test]
fn a_nested_tail_stub_returns_to_the_tracked_caller() {
    let observed = nested_tail(&[branch(0x1040, STUB, false)], 7);
    observed.assert_returned();
    assert_eq!(observed.register("r0"), 0x56);
    assert_eq!(observed.register("lr"), 0x1008);
    assert_eq!(observed.instructions(), 5);
    assert_eq!(observed.observation.events.len(), 1);

    let mismatch = nested_tail(&[branch(0x1040, STUB, false)], 8);
    mismatch.assert_stopped(Status::StubContract, 0x1040);
    assert_eq!(mismatch.register("lr"), 0x1008);
    assert!(mismatch.observation.events.is_empty());
}

#[test]
fn a_tail_stub_with_a_link_outside_the_tracked_frame_is_an_unknown_call() {
    // mov lr, #0; b STUB
    let observed = nested_tail(&[0xe3a0_e000, branch(0x1044, STUB, false)], 7);
    observed.assert_stopped(Status::UnknownCall, 0x1044);
    assert!(observed.observation.events.is_empty());
}

#[test]
fn a_call_to_an_undeclared_target_is_an_unknown_call() {
    let observed = Case::new(&[branch(CODE, 0x1080, true), BX_LR]).run();
    observed.assert_stopped(Status::UnknownCall, CODE);
    assert_eq!(observed.instructions(), 1);
}

#[test]
fn falling_past_a_retired_function_body_is_an_unknown_call() {
    // mov r0, #1, then executable bytes outside the declared function
    let mut case = Case::new(&[0xe3a0_0001, BX_LR]);
    case.functions = vec![(CODE, 4)];
    let observed = case.run();
    observed.assert_stopped(Status::UnknownCall, CODE + 4);
    assert_eq!(observed.register("r0"), 1);
    assert_eq!(observed.instructions(), 1);
}

#[test]
fn fetches_outside_executable_regions_are_fetch_guards() {
    let other_page = Case::new(&[branch(CODE, 0x4000, false)]).region(0x4000, READ_WRITE, &words(&[BX_LR])).run();
    other_page.assert_stopped(Status::FetchGuard, 0x4000);
    assert_eq!(other_page.instructions(), 1);

    let same_page = Case::new(&[branch(CODE, 0x1100, false)]).region(0x1100, READ_WRITE, &words(&[BX_LR])).run();
    same_page.assert_stopped(Status::FetchGuard, 0x1100);
    assert_eq!(same_page.instructions(), 1);
}

#[test]
fn the_instruction_budget_stops_before_the_next_entry() {
    // b .
    let mut case = Case::new(&[0xeaff_fffe]);
    case.instructions = 5;
    let observed = case.run();
    observed.assert_stopped(Status::InstructionBudget, CODE);
    assert_eq!(observed.instructions(), 5);
    assert_eq!(observed.observation.trace.len(), 5);
    assert!(observed.observation.trace.iter().all(|entry| entry.pc == "0x00001000"));
}

#[test]
fn the_wall_budget_stops_a_long_loop() {
    let mut case = Case::new(&[0xeaff_fffe]);
    case.instructions = 4_000_000;
    case.wall_ms = 1;
    let observed = case.run();
    assert_eq!(observed.status(), Status::WallBudget);
    assert!(!observed.returned);
    assert!(observed.instructions() < 4_000_000);
}

#[test]
fn cortex_r4f_exclusions_are_refused_before_they_execute() {
    // sdiv r0, r0, r1; udiv r0, r0, r1; sdiveq r0, r0, r1 with Z clear; mrc p15, 0, r0, c0, c0, 5
    for word in [0xe710_f110, 0xe730_f110, 0x0710_f110, 0xee10_0fb0] {
        let observed = Case::new(&[word, BX_LR]).register(0, 9).register(1, 3).run();
        observed.assert_stopped(Status::UnsupportedInstruction, CODE);
        assert_eq!(observed.register("r0"), 9, "{word:#010x}");
        assert_eq!(observed.instructions(), 1);
    }
}

#[test]
fn an_undefined_instruction_is_unsupported() {
    // udf #0
    let observed = Case::new(&[0xe7f0_00f0, BX_LR]).register(0, 9).run();
    observed.assert_stopped(Status::UnsupportedInstruction, CODE);
    assert_eq!(observed.register("r0"), 9);
}

#[test]
fn hints_status_writes_and_synchronization_primitives_are_refused_before_they_execute() {
    // nop; yield; wfi; msr cpsr_f, #0xf0000000; msr cpsr_fc, r0; swp r0, r1, [r2]; ldrex r0, [r2]; strex r0, r1, [r2]
    for word in [0xe320_f000, 0xe320_f001, 0xe320_f003, 0xe328_f20f, 0xe129_f000, 0xe102_0091, 0xe192_0f9f, 0xe182_0f91] {
        let observed = Case::new(&[word, BX_LR])
            .region(0x4000, READ_WRITE, &[0x44, 0x33, 0x22, 0x11])
            .register(0, 9)
            .register(1, 3)
            .register(2, 0x4000)
            .run();
        observed.assert_stopped(Status::UnsupportedInstruction, CODE);
        assert_eq!(observed.register("r0"), 9, "{word:#010x}");
        assert_eq!(observed.bytes(0x4000), "44332211", "{word:#010x}");
        assert_eq!(observed.instructions(), 1);
    }
}

#[test]
fn mrs_reads_the_declared_user_status_and_refuses_a_pc_destination() {
    // mrs r0, cpsr
    let read = Case::new(&[0xe10f_0000, BX_LR]).register(15, 0x6000_0010).run();
    read.assert_returned();
    assert_eq!(read.register("r0"), 0x6000_0010);
    // mrs pc, cpsr
    let into_pc = Case::new(&[0xe10f_f000, BX_LR]).run();
    into_pc.assert_stopped(Status::UnsupportedInstruction, CODE);
}

#[test]
fn unpredictable_transfer_and_multiply_forms_are_refused_before_they_execute() {
    // ldr r2, [r2, r3]!; strh r2, [r2, #2]!; ldrd r4, r5, [r4, #8]!; ldrd r4, r5, [r2, r4];
    // smull r1, r1, r2, r3; ldrb r0, [r2, pc]; strb pc, [r2, r3]; stmdb sp!, {r0, sp}
    for word in [0xe7b2_2003, 0xe1e2_20b2, 0xe1e4_40d8, 0xe182_40d4, 0xe0c1_1392, 0xe7d2_000f, 0xe7c2_f003, 0xe92d_2001] {
        let observed = Case::new(&[word, BX_LR])
            .region(0x4000, READ_WRITE, &[0; 16])
            .register(0, 9)
            .register(1, 7)
            .register(2, 0x4000)
            .register(3, 4)
            .register(4, 0x4000)
            .run();
        observed.assert_stopped(Status::UnsupportedInstruction, CODE);
        assert_eq!(observed.register("r1"), 7, "{word:#010x}");
        assert_eq!(observed.register("r2"), 0x4000, "{word:#010x}");
        assert_eq!(observed.bytes(0x4000), "00000000000000000000000000000000", "{word:#010x}");
    }
}

#[test]
fn pc_destinations_outside_the_call_contract_are_refused() {
    // mov pc, r1; movw pc, #0; uxtb pc, r1; ubfx pc, r1, #4, #12; smulbb pc, r1, r2; clz pc, r1
    for word in [0xe1a0_f001, 0xe300_f000, 0xe6ef_f071, 0xe7eb_f251, 0xe16f_0281, 0xe16f_ff11] {
        let observed = Case::new(&[word, BX_LR]).register(1, CODE + 4).register(2, 1).run();
        observed.assert_stopped(Status::UnsupportedInstruction, CODE);
        assert_eq!(observed.instructions(), 1, "{word:#010x}");
    }
}

#[test]
fn malformed_and_out_of_contract_wires_are_invalid_cases() {
    let valid = Case::new(&[PUSH_LR, branch(0x1004, STUB, true), ADD_R0_1, POP_PC]).register(0, 5).stub(STUB, &[(0, 5)], &[(0, 0x40)]);
    let text = valid.wire();
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let nine_digit_entry = {
        lines[2] = lines[2].replacen("1000", "000001000", 1);
        lines.join("\n")
    };
    let structural = |change: fn(&mut Case)| {
        let mut case = valid.clone();
        change(&mut case);
        case.wire()
    };
    let wires = [
        String::new(),
        text.replacen("FUJI_NATIVE_V1", "FUJI_NATIVE_V2", 1),
        text.trim_end().rsplit_once(' ').unwrap().0.to_owned(),
        format!("{text} 0\n"),
        text.replacen("00402de9", "00402DE9", 1),
        nine_digit_entry,
        structural(|case| case.instructions = 0),
        structural(|case| case.instructions = 4_000_001),
        structural(|case| case.regions[1].permission = 2),
        structural(|case| case.regions.push(Region { address: 0x30f0, size: 0x20, permission: READ_WRITE, bytes: vec![0; 0x20] })),
        structural(|case| case.regions.push(Region { address: 0xffff_f000, size: 0x2000, permission: READ, bytes: vec![0; 0x2000] })),
        structural(|case| case.registers[14] = CODE),
        structural(|case| case.registers[15] = 0x30),
        structural(|case| case.entry = CODE + 4),
        structural(|case| case.stubs[0].address = CODE + 4),
        structural(|case| case.stubs[0].inputs = vec![(0, 5), (0, 5)]),
        structural(|case| case.stubs[0].inputs = vec![(13, 0)]),
    ];
    for wire in wires {
        let observed = observe(wire.as_bytes());
        assert_eq!(observed.status(), Status::InvalidCase, "{wire}");
        assert!(!observed.returned);
    }
    let mut largest = valid.clone();
    largest.instructions = 4_000_000;
    let observed = largest.run();
    observed.assert_returned();
    assert_eq!(observed.register("r0"), 0x41);
}
