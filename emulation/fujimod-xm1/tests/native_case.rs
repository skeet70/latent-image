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
const DEVICE: u8 = 10;

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
    /// The wire's CP14 identity token of a privileged case.
    identity: Option<String>,
    /// The wire's kernel row of a kernel case.
    kernel: Option<String>,
}

/// A kernel case's wire row; `None` is `n`.
#[derive(Clone, Copy, Default)]
struct Kernel {
    stop: Option<u32>,
    tpidrurw: Option<u32>,
    cpacr: Option<u32>,
    fpexc: Option<u32>,
}

impl Kernel {
    fn row(self) -> String {
        let token = |value: Option<u32>| value.map_or("n".to_owned(), |value| format!("{value:x}"));
        format!("k {} {} {} {}", token(self.stop), token(self.tpidrurw), token(self.cpacr), token(self.fpexc))
    }
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
            identity: None,
            kernel: None,
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

    /// A `None` identity is the wire's `n` token.
    fn privileged(mut self, cpsr: u32, identity: Option<u32>) -> Self {
        self.registers[15] = cpsr;
        self.identity = Some(identity.map_or("n".to_owned(), |value| format!("i {value:x}")));
        self
    }

    /// A kernel case in SVC without a CP14 identity.
    fn kernel(mut self, kernel: Kernel) -> Self {
        self = self.privileged(SVC_ENTRY, None);
        self.kernel = Some(kernel.row());
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
        if let Some(identity) = &self.identity {
            text += &format!("\n{identity}");
        }
        if let Some(kernel) = &self.kernel {
            text += &format!("\n{kernel}");
        }
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

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum BarrierTag {
    Barrier,
}

#[derive(Clone, Copy, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Barrier {
    Dmb,
    Dsb,
    Isb,
}

#[derive(Deserialize, Debug, PartialEq)]
struct BarrierRow {
    pc: String,
    kind: BarrierTag,
    barrier: Barrier,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(untagged)]
enum Row {
    Access(Provenance),
    Barrier(BarrierRow),
    System(SystemRow),
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum SystemTag {
    System,
}

#[derive(Clone, Copy, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum SystemRegister {
    Tpidrurw,
    Fpexc,
}

#[derive(Deserialize, Debug, PartialEq)]
struct SystemRow {
    pc: String,
    kind: SystemTag,
    register: SystemRegister,
    value: String,
}

fn system(pc: u32, register: SystemRegister, value: u32) -> Row {
    Row::System(SystemRow { pc: format!("0x{pc:08x}"), kind: SystemTag::System, register, value: format!("0x{value:08x}") })
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
    StopAddress,
    InvalidCase,
}

/// Final values of a kernel case's declared TPIDRURW and FPEXC.
#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct KernelRegisters {
    tpidrurw: Option<String>,
    fpexc: Option<String>,
    reserved: Option<String>,
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
    memory_provenance: Vec<Row>,
    #[serde(default)]
    kernel: Option<KernelRegisters>,
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

    fn kernel(&self) -> &KernelRegisters {
        self.observation.kernel.as_ref().expect("a kernel observation")
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
    let entry = |pc: &str, kind: Kind, address: &str, value: &str| Row::Access(Provenance {
        pc: pc.into(),
        kind,
        address: address.into(),
        size: 4,
        value: value.into(),
    });
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
fn post_indexed_doubleword_stores_refuse_the_whole_transfer_before_writeback() {
    for word in [0xe0c2_00f8, 0xe082_00f3] {
        let observed = Case::new(&[word, BX_LR])
            .shared_page()
            .register(0, 0x1122_3344)
            .register(1, 0xaabb_ccdd)
            .register(2, 0x4004)
            .register(3, 8)
            .run();
        observed.assert_stopped(Status::MemoryGuard, CODE);
        assert_eq!(observed.bytes(0x4000), "44332211", "{word:#010x}");
        assert_eq!(observed.bytes(0x4004), "00000000", "{word:#010x}");
        assert_eq!(observed.bytes(0x4008), "ddccbbaa", "{word:#010x}");
        assert_eq!(observed.bytes(0x4010), "01020304", "{word:#010x}");
        assert_eq!(observed.register("r0"), 0x1122_3344, "{word:#010x}");
        assert_eq!(observed.register("r1"), 0xaabb_ccdd, "{word:#010x}");
        assert_eq!(observed.register("r2"), 0x4004, "{word:#010x}");
        assert_eq!(observed.register("r3"), 8, "{word:#010x}");
        assert_eq!(observed.observation.memory_provenance_total, "0", "{word:#010x}");
    }
}

#[test]
fn post_indexed_doubleword_loads_refuse_the_whole_transfer_before_writeback() {
    for word in [0xe0c2_00d8, 0xe082_00d3] {
        let observed = Case::new(&[word, BX_LR])
            .shared_page()
            .register(0, 0x77)
            .register(1, 0x88)
            .register(2, 0x4008)
            .register(3, 8)
            .run();
        observed.assert_stopped(Status::MemoryGuard, CODE);
        assert_eq!(observed.bytes(0x4000), "44332211", "{word:#010x}");
        assert_eq!(observed.bytes(0x4004), "00000000", "{word:#010x}");
        assert_eq!(observed.bytes(0x4008), "ddccbbaa", "{word:#010x}");
        assert_eq!(observed.bytes(0x4010), "01020304", "{word:#010x}");
        assert_eq!(observed.register("r0"), 0x77, "{word:#010x}");
        assert_eq!(observed.register("r1"), 0x88, "{word:#010x}");
        assert_eq!(observed.register("r2"), 0x4008, "{word:#010x}");
        assert_eq!(observed.register("r3"), 8, "{word:#010x}");
        assert_eq!(observed.observation.memory_provenance_total, "0", "{word:#010x}");
    }
}

#[test]
fn post_indexed_doubleword_stores_write_both_words_then_update_the_base() {
    for (word, base) in [(0xe0c2_00f8, 0x400c), (0xe082_00f3, 0x400c), (0xe082_00f2, 0x8008)] {
        let observed = Case::new(&[word, BX_LR])
            .region(0x4004, READ_WRITE, &[0; 8])
            .register(0, 0x1122_3344)
            .register(1, 0xaabb_ccdd)
            .register(2, 0x4004)
            .register(3, 8)
            .run();
        observed.assert_returned();
        assert_eq!(observed.bytes(0x4004), "44332211ddccbbaa", "{word:#010x}");
        assert_eq!(observed.register("r0"), 0x1122_3344, "{word:#010x}");
        assert_eq!(observed.register("r1"), 0xaabb_ccdd, "{word:#010x}");
        assert_eq!(observed.register("r2"), base, "{word:#010x}");
        assert_eq!(observed.register("r3"), 8, "{word:#010x}");
        assert_eq!(observed.observation.memory_provenance_total, "2", "{word:#010x}");
    }
}

#[test]
fn post_indexed_doubleword_loads_read_both_words_then_update_the_base() {
    for (word, base) in [(0xe0c2_00d8, 0x400c), (0xe082_00d3, 0x400c), (0xe082_00d2, 0x8008)] {
        let observed = Case::new(&[word, BX_LR])
            .region(0x4004, READ, &[0x44, 0x33, 0x22, 0x11, 0xdd, 0xcc, 0xbb, 0xaa])
            .register(0, 0x77)
            .register(1, 0x88)
            .register(2, 0x4004)
            .register(3, 8)
            .run();
        observed.assert_returned();
        assert_eq!(observed.bytes(0x4004), "44332211ddccbbaa", "{word:#010x}");
        assert_eq!(observed.register("r0"), 0x1122_3344, "{word:#010x}");
        assert_eq!(observed.register("r1"), 0xaabb_ccdd, "{word:#010x}");
        assert_eq!(observed.register("r2"), base, "{word:#010x}");
        assert_eq!(observed.register("r3"), 8, "{word:#010x}");
        assert_eq!(observed.observation.memory_provenance_total, "2", "{word:#010x}");
    }
}

#[test]
fn unpredictable_post_indexed_doublewords_are_refused_without_effects() {
    for word in [
        0xe0c0_00d8, 0xe0c1_00d8, 0xe0c0_00f8, 0xe0c1_00f8,
        0xe080_00d3, 0xe081_00d3, 0xe080_00f3, 0xe081_00f3,
        0xe082_00d0, 0xe082_00d1,
        0xe0e2_00d8, 0xe0e2_00f8, 0xe0a2_00d3, 0xe0a2_00f3,
    ] {
        let observed = Case::new(&[word, BX_LR])
            .region(0x4004, READ_WRITE, &[0; 16])
            .register(0, 0x4004)
            .register(1, 0x4004)
            .register(2, 0x4004)
            .register(3, 8)
            .run();
        observed.assert_stopped(Status::UnsupportedInstruction, CODE);
        assert_eq!(observed.bytes(0x4004), "00000000000000000000000000000000", "{word:#010x}");
        assert_eq!(observed.register("r0"), 0x4004, "{word:#010x}");
        assert_eq!(observed.register("r1"), 0x4004, "{word:#010x}");
        assert_eq!(observed.register("r2"), 0x4004, "{word:#010x}");
        assert_eq!(observed.register("r3"), 8, "{word:#010x}");
        assert_eq!(observed.observation.memory_provenance_total, "0", "{word:#010x}");
    }
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
fn register_apsr_writes_change_only_the_selected_nzcvq_and_ge_bits() {
    for (instruction, source, expected) in [
        (0xe128_f00c, 0xffff_ffff, 0xf800_0010),
        (0xe12c_f00c, 0xffff_ffff, 0xf80f_0010),
        (0xe128_f00c, 0x000f_ffff, 0x0000_0010),
        (0xe12c_f00c, 0x000a_ffff, 0x000a_0010),
    ] {
        let observed = Case::new(&[instruction, BX_LR]).register(12, source).run();
        observed.assert_returned();
        assert_eq!(observed.register("cpsr"), expected, "{instruction:#010x}");
        assert_eq!(observed.register("r12"), source);
        assert_eq!(observed.observation.memory_provenance_total, "0");
    }
}

#[test]
fn nzcvq_writes_preserve_ge_and_nzcvqg_writes_can_clear_every_application_flag() {
    let preserved = Case::new(&[0xe12c_f001, 0xe128_f00c, BX_LR])
        .register(1, 0xa80a_ffff)
        .register(12, 0x5005_ffff)
        .run();
    preserved.assert_returned();
    assert_eq!(preserved.register("cpsr"), 0x500a_0010);
    let cleared = Case::new(&[0xe12c_f001, 0xe12c_f00c, BX_LR])
        .register(1, 0xffff_ffff)
        .register(12, 0)
        .run();
    cleared.assert_returned();
    assert_eq!(cleared.register("cpsr"), 0x10);
}

#[test]
fn condition_false_apsr_writes_preserve_the_application_flags() {
    for instruction in [0x0128_f00c, 0x012c_f00c] {
        let observed = Case::new(&[instruction, BX_LR]).register(12, 0xffff_ffff).run();
        observed.assert_returned();
        assert_eq!(observed.register("cpsr"), 0x10);
    }
}

fn assert_status_write_has_no_effect(case: &Case, observed: &Observed) {
    observed.assert_stopped(Status::UnsupportedInstruction, CODE);
    for (index, label) in ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12", "sp", "lr", "cpsr"].iter().enumerate() {
        assert_eq!(observed.register(label), case.registers[index], "{label}");
    }
    for region in &case.regions {
        let expected: String = region.bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(observed.bytes(region.address), expected);
    }
    assert_eq!(observed.observation.memory_provenance_total, "0");
}

#[test]
fn other_register_status_masks_and_spsr_targets_are_refused_without_effects() {
    for saved_status in [0, 1 << 22] {
        for mask in 0..16 {
            if saved_status == 0 && matches!(mask, 8 | 12) {
                continue;
            }
            let instruction = 0xe120_f00c | saved_status | (mask << 16);
            let case = Case::new(&[instruction, STR_R0_R1_POST_4, BX_LR])
                .region(0x4000, READ_WRITE, &[0x44, 0x33, 0x22, 0x11])
                .register(0, 9)
                .register(1, 0x4000)
                .register(12, 0xffff_ffff)
                .register(15, 0xa000_0010);
            let observed = case.run();
            assert_status_write_has_no_effect(&case, &observed);
        }
    }
}

#[test]
fn immediate_status_writes_pc_sources_and_condition_false_control_writes_still_refuse() {
    for instruction in [
        0xe321_f0ff, 0xe322_f0ff, 0xe324_f0ff, 0xe329_f0ff, 0xe32f_f0ff,
        0xe328_f20f, 0xe368_f20f, 0xe128_f00f, 0xe12c_f00f, 0x0121_f00c,
    ] {
        let case = Case::new(&[instruction, BX_LR]).register(12, 0xffff_ffff);
        let observed = case.run();
        assert_status_write_has_no_effect(&case, &observed);
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

const SVC_ENTRY: u32 = 0x6000_00d3;
const SYS_ENTRY: u32 = 0x0801_001f;
const MRS_R1_CPSR: u32 = 0xe10f_1000;
const MSR_CPSR_C_R1: u32 = 0xe121_f001;
const MSR_CPSR_FC_R1: u32 = 0xe129_f001;
const ORR_R1_I: u32 = 0xe381_1080;
const BIC_R1_I: u32 = 0xe3c1_1080;
const MRC_P14_R0_IDENTITY: u32 = 0xee12_0e10;

#[test]
fn privileged_status_writes_set_and_clear_i_in_svc_and_sys() {
    for (entry, change, expected) in [
        (0x4000_0013, ORR_R1_I, 0x4000_0093),
        (SVC_ENTRY, BIC_R1_I, 0x6000_0053),
        (SYS_ENTRY, ORR_R1_I, 0x0801_009f),
        (0x0801_00df, BIC_R1_I, 0x0801_005f),
    ] {
        for write in [MSR_CPSR_C_R1, MSR_CPSR_FC_R1] {
            let observed = Case::new(&[MRS_R1_CPSR, change, write, BX_LR]).privileged(entry, None).run();
            observed.assert_returned();
            assert_eq!(observed.register("cpsr"), expected, "{entry:#010x} {write:#010x}");
            assert_eq!(observed.register("r1"), expected, "{entry:#010x} {write:#010x}");
            assert_eq!(observed.register("sp"), SP);
            assert_eq!(observed.register("lr"), EXIT);
        }
    }
}

#[test]
fn privileged_cpsid_and_cpsie_change_only_i() {
    // cpsid i; cpsie i
    for (entry, word, expected) in [(0x4000_0013, 0xf10c_0080, 0x4000_0093), (SVC_ENTRY, 0xf108_0080, 0x6000_0053), (SYS_ENTRY, 0xf10c_0080, 0x0801_009f)] {
        let observed = Case::new(&[word, BX_LR]).privileged(entry, None).run();
        observed.assert_returned();
        assert_eq!(observed.register("cpsr"), expected, "{word:#010x}");
    }
}

#[test]
fn privileged_status_writes_that_change_the_mode_or_set_t_are_refused_without_effects() {
    for (entry, source) in [(SVC_ENTRY, 0x6000_00df), (SVC_ENTRY, 0x6000_00d0), (SVC_ENTRY, 0x6000_00f3), (SYS_ENTRY, 0x0801_0013), (SYS_ENTRY, 0x0801_003f)] {
        // msr CPSR_c, r12; msr CPSR_fc, r12
        for word in [0xe121_f00c, 0xe129_f00c] {
            let case = Case::new(&[word, BX_LR]).privileged(entry, None).register(12, source);
            assert_status_write_has_no_effect(&case, &case.run());
        }
    }
}

#[test]
fn conditional_privileged_status_writes_check_the_value_only_when_the_condition_passes() {
    // msreq CPSR_c, r12 with Z set and r12 naming SYS
    let taken = Case::new(&[0x0121_f00c, BX_LR]).privileged(SVC_ENTRY, None).register(12, 0x6000_00df);
    assert_status_write_has_no_effect(&taken, &taken.run());
    // msrne CPSR_c, r12 with Z set
    let skipped = Case::new(&[0x1121_f00c, BX_LR]).privileged(SVC_ENTRY, None).register(12, 0x6000_00df).run();
    skipped.assert_returned();
    assert_eq!(skipped.register("cpsr"), SVC_ENTRY);
}

#[test]
fn privileged_cases_refuse_spsr_mode_changes_exception_returns_and_other_coprocessor_words() {
    for word in [
        0xe14f_0000, // mrs r0, spsr
        0x114f_0000, // mrsne r0, spsr
        0xe169_f00c, // msr SPSR_fc, r12
        0xe361_f0d3, // msr SPSR_c, #0xd3
        0xe321_f0d3, // msr CPSR_c, #0xd3
        0xe125_f00c, // msr CPSR_sc, r12
        0xe123_f00c, // msr CPSR_xc, r12
        0xe121_f00f, // msr CPSR_c, pc
        0xe100_0200, // mrs r0, r8_usr
        0xe120_f20c, // msr r8_usr, r12
        0xf102_0013, // cps #0x13
        0xf10e_0093, // cpsid i, #0x13
        0xf10c_00c0, // cpsid if
        0xf10c_0100, // cpsid a
        0xef00_0000, // svc #0
        0xe1b0_f00e, // movs pc, lr
        0xe25e_f004, // subs pc, lr, #4
        0xe8fd_8000, // ldm sp!, {pc}^
        0xe8c2_0001, // stmia r2, {r0}^
        0xf8bd_0a00, // rfeia sp!
        0xe160_006e, // eret
        0xee12_0e30, // mrc p14, 0, r0, c2, c0, 1
        0xee10_0e10, // mrc p14, 0, r0, c0, c0, 0
        0xee32_0e10, // mrc p14, 1, r0, c2, c0, 0
        0xee12_fe10, // mrc p14, 0, pc, c2, c0, 0
        0xee02_0e10, // mcr p14, 0, r0, c2, c0, 0
        0xfe12_0e10, // mrc2 p14, 0, r0, c2, c0, 0
        0xee10_0fb0, // mrc p15, 0, r0, c0, c0, 5
        0xec50_0e00, // mrrc p14, 0, r0, r0, c0
        0xec40_0e00, // mcrr p14, 0, r0, r0, c0
        0xed92_0e00, // ldc p14, c0, [r2]
        0xed82_0e00, // stc p14, c0, [r2]
        0xee12_0e00, // cdp p14, 1, c0, c2, c0, 0
        0xe320_f003, // wfi
    ] {
        for entry in [SVC_ENTRY, SYS_ENTRY] {
            let case = Case::new(&[word, BX_LR])
                .privileged(entry, Some(0x4003))
                .register(0, 9)
                .register(2, STACK)
                .register(12, entry);
            let observed = case.run();
            assert_eq!(observed.status(), Status::UnsupportedInstruction, "{word:#010x}");
            assert_status_write_has_no_effect(&case, &observed);
        }
    }
}

#[test]
fn the_cp14_identity_read_returns_the_case_identity() {
    for identity in [0x4003, 0x8003, 0] {
        let observed = Case::new(&[MRC_P14_R0_IDENTITY, BX_LR]).privileged(SVC_ENTRY, Some(identity)).register(0, 9).run();
        observed.assert_returned();
        assert_eq!(observed.register("r0"), identity);
        assert_eq!(observed.instructions(), 2);
    }
    // mrc p14, 0, r7, c2, c0, 0 in SYS
    let observed = Case::new(&[0xee12_7e10, BX_LR]).privileged(SYS_ENTRY, Some(0x14003)).run();
    observed.assert_returned();
    assert_eq!(observed.register("r7"), 0x14003);
}

#[test]
fn a_condition_false_cp14_read_leaves_its_destination_and_continues() {
    // mrcne / mrceq p14, 0, r0, c2, c0, 0 with Z set
    for (word, expected) in [(0x1e12_0e10, 9), (0x0e12_0e10, 0x4003)] {
        let observed = Case::new(&[word, ADD_R0_1, BX_LR]).privileged(0x4000_0013, Some(0x4003)).register(0, 9).run();
        observed.assert_returned();
        assert_eq!(observed.register("r0"), expected + 1, "{word:#010x}");
        assert_eq!(observed.instructions(), 3);
    }
}

#[test]
fn a_cp14_read_without_a_case_identity_or_in_user_mode_is_refused() {
    for case in [
        Case::new(&[MRC_P14_R0_IDENTITY, BX_LR]).privileged(SVC_ENTRY, None).register(0, 9),
        Case::new(&[MRC_P14_R0_IDENTITY, BX_LR]).register(0, 9),
    ] {
        let observed = case.run();
        observed.assert_stopped(Status::UnsupportedInstruction, CODE);
        assert_eq!(observed.register("r0"), 9);
    }
}

const HELPER: u32 = 0x1040;

/// push {r11, lr}; bl HELPER; pop {r11, pc}.
fn identity_probe(identity: u32) -> Observed {
    // mrc p14, 0, r1, c2, c0, 0; movw r2, #0x4003; cmp r1, r2; moveq r0, #0; movne r0, #1; bx lr
    let helper = [MRC_P14_R0_IDENTITY | 1 << 12, 0xe304_2003, 0xe151_0002, 0x03a0_0000, 0x13a0_0001, BX_LR];
    let mut case = Case::new(&[PUSH_LR | 1 << 11, branch(CODE + 4, HELPER, true), POP_PC | 1 << 11])
        .code(HELPER, &helper)
        .function(HELPER, 4 * helper.len() as u32);
    for index in 0..13 {
        case.registers[index] = 0x5a5a_5a5a;
    }
    case.privileged(SVC_ENTRY, Some(identity)).run()
}

#[test]
fn an_identity_probe_returns_zero_only_for_identity_4003() {
    for (identity, expected) in [(0x4003, 0), (0x4002, 1)] {
        let observed = identity_probe(identity);
        observed.assert_returned();
        assert_eq!(observed.register("r0"), expected, "{identity:#x}");
        assert_eq!(observed.register("r1"), identity);
        assert_eq!(observed.register("r11"), 0x5a5a_5a5a);
        assert_eq!(observed.register("sp"), SP);
        assert_eq!(observed.register("cpsr") & 0xff, SVC_ENTRY & 0xff);
    }
}

#[test]
fn malformed_privileged_entries_are_invalid_cases() {
    let valid = Case::new(&[BX_LR]).privileged(SVC_ENTRY, Some(0x4003));
    assert_eq!(valid.run().status(), Status::Returned);
    let mut cases: Vec<Case> = [0x33, 0x113, 0x12, 0x17, 0x1b, 0x11, 0x0100_0013, 0x0000_0213]
        .into_iter()
        .map(|cpsr| valid.clone().privileged(cpsr, Some(0x4003)))
        .collect();
    let mut missing = valid.clone();
    missing.identity = None;
    let mut unknown = valid.clone();
    unknown.identity = Some("x".to_owned());
    let mut user = Case::new(&[BX_LR]);
    user.identity = Some("n".to_owned());
    cases.extend([missing, unknown, user]);
    for case in cases {
        let observed = case.run();
        assert_eq!(observed.status(), Status::InvalidCase, "{}", case.wire());
    }
}

const DATA: u32 = 0x4000;
const STOP: u32 = 0x10c0;
const BANK: u32 = 0xfffb_30cc;
const NOP: u32 = 0xe320_f000;
const LDREX_R1_R2: u32 = 0xe192_1f9f;
const STREX_R3_R1_R2: u32 = 0xe182_3f91;
const CLREX: u32 = 0xf57f_f01f;
const ADD_R1_1: u32 = 0xe281_1001;
const STR_R0_R2: u32 = 0xe582_0000;
const DMB_SY: u32 = 0xf57f_f05f;
const DSB_SY: u32 = 0xf57f_f04f;
const ISB_SY: u32 = 0xf57f_f06f;
/// `mcr p15, 0, r5, c7, c10, 5` and `, 4`.
const CP15_DMB_R5: u32 = 0xee07_5fba;
const CP15_DSB_R5: u32 = 0xee07_5f9a;
const TPIDRURW_TO_R4: u32 = 0xee1d_4f50;
const TPIDRURW_FROM_R6: u32 = 0xee0d_6f50;
const TPIDRURW_TO_R7: u32 = 0xee1d_7f50;
const CPACR_TO_R5: u32 = 0xee11_5f50;
const FPEXC_TO_R8: u32 = 0xeef8_8a10;
const FPEXC_FROM_R6: u32 = 0xeee8_6a10;
const FPEXC_EN: u32 = 1 << 30;

fn branch_ne(from: u32, to: u32) -> u32 {
    branch(from, to, false) & 0x0fff_ffff | 0x1000_0000
}

fn access(pc: u32, kind: Kind, address: u32, value: u32) -> Row {
    Row::Access(Provenance {
        pc: format!("0x{pc:08x}"),
        kind,
        address: format!("0x{address:08x}"),
        size: 4,
        value: format!("0x{value:016x}"),
    })
}

fn barrier(pc: u32, barrier: Barrier) -> Row {
    Row::Barrier(BarrierRow { pc: format!("0x{pc:08x}"), kind: BarrierTag::Barrier, barrier })
}

#[test]
fn an_exclusive_pair_updates_the_word_and_reports_success() {
    let observed = Case::new(&[LDREX_R1_R2, ADD_R1_1, STREX_R3_R1_R2, BX_LR])
        .kernel(Kernel::default())
        .region(DATA, READ_WRITE, &words(&[5]))
        .register(2, DATA)
        .register(3, 9)
        .run();
    observed.assert_returned();
    assert_eq!(observed.register("r1"), 6);
    assert_eq!(observed.register("r3"), 0);
    assert_eq!(observed.bytes(DATA), "06000000");
    assert_eq!(observed.observation.memory_provenance, [access(0x1000, Kind::Read, DATA, 5), access(0x1008, Kind::Write, DATA, 6)]);
}

#[test]
fn a_store_exclusive_without_a_reservation_fails_without_a_store() {
    // strex alone; ldrex, clrex, strex
    for code in [vec![STREX_R3_R1_R2, BX_LR], vec![LDREX_R1_R2, CLREX, STREX_R3_R1_R2, BX_LR]] {
        let observed = Case::new(&code)
            .kernel(Kernel::default())
            .region(DATA, READ_WRITE, &words(&[5]))
            .register(1, 7)
            .register(2, DATA)
            .register(3, 9)
            .run();
        observed.assert_returned();
        assert_eq!(observed.register("r3"), 1, "{code:x?}");
        assert_eq!(observed.bytes(DATA), "05000000", "{code:x?}");
        assert!(observed.observation.memory_provenance.iter().all(|row| !matches!(row, Row::Access(Provenance { kind: Kind::Write, .. }))));
    }
    // ldrex; strexne with Z set is skipped and keeps the reservation; strex then stores
    let observed = Case::new(&[LDREX_R1_R2, STREX_R3_R1_R2 & 0x0fff_ffff | 0x1000_0000, 0xe182_4f91, BX_LR])
        .kernel(Kernel::default())
        .region(DATA, READ_WRITE, &words(&[5]))
        .register(2, DATA)
        .register(3, 9)
        .register(4, 9)
        .run();
    observed.assert_returned();
    assert_eq!(observed.register("r3"), 9);
    assert_eq!(observed.register("r4"), 0);
    assert_eq!(observed.bytes(DATA), "05000000");
}

/// Two lock acquisitions over the word at r2, the second preceded by a `nop`.
fn acquisitions() -> [Vec<u32>; 2] {
    let acquire = |at: u32| {
        vec![
            LDREX_R1_R2,
            0xe351_0000, // cmp r1, #0
            branch_ne(at + 8, at),
            0xe3a0_1001, // mov r1, #1
            STREX_R3_R1_R2,
            0xe353_0000, // cmp r3, #0
            branch_ne(at + 24, at),
            BX_LR,
        ]
    };
    [acquire(CODE), [vec![NOP], acquire(CODE + 4)].concat()]
}

#[test]
fn a_spin_on_a_held_word_ends_on_the_budget_in_every_shape() {
    for code in acquisitions() {
        let mut case = Case::new(&code).kernel(Kernel::default()).region(DATA, READ_WRITE, &words(&[1])).register(2, DATA);
        case.instructions = 100;
        let observed = case.run();
        assert_eq!(observed.status(), Status::InstructionBudget, "{code:x?}");
        assert!(!observed.returned);
        assert_eq!(observed.instructions(), 100, "{code:x?}");
        assert_eq!(observed.bytes(DATA), "01000000");
        assert_eq!(observed.kernel().reserved, Some(format!("0x{DATA:08x}")), "{code:x?}");
    }
}

#[test]
fn the_last_ldrex_word_stays_reported_after_strex_and_clrex() {
    // ldrex then clrex; ldrex, add, strex; ldrex r1, [r2] then ldrex r1, [r4]
    for (code, reserved) in [
        (vec![LDREX_R1_R2, CLREX, BX_LR], Some(DATA)),
        (vec![LDREX_R1_R2, ADD_R1_1, STREX_R3_R1_R2, BX_LR], Some(DATA)),
        (vec![LDREX_R1_R2, 0xe194_1f9f, BX_LR], Some(DATA + 4)),
        (vec![CLREX, BX_LR], None),
    ] {
        let observed = Case::new(&code)
            .kernel(Kernel::default())
            .region(DATA, READ_WRITE, &words(&[0, 0]))
            .register(2, DATA)
            .register(4, DATA + 4)
            .run();
        observed.assert_returned();
        assert_eq!(observed.kernel().reserved, reserved.map(|address| format!("0x{address:08x}")), "{code:x?}");
    }
}

#[test]
fn a_free_word_is_acquired_in_every_shape() {
    for code in acquisitions() {
        let observed = Case::new(&code).kernel(Kernel::default()).region(DATA, READ_WRITE, &words(&[0])).register(2, DATA).run();
        observed.assert_returned();
        assert_eq!(observed.register("r3"), 0, "{code:x?}");
        assert_eq!(observed.bytes(DATA), "01000000", "{code:x?}");
    }
}

#[test]
fn stores_over_a_reserved_word_and_a_store_exclusive_elsewhere_are_refused_without_effect() {
    // ldrex r1, [r2] then: str r0, [r2]; stmia r4, {r0, r3} over it; strex r3, r1, [r4]
    for (second, r2) in [(STR_R0_R2, DATA), (0xe884_0009, DATA + 4), (0xe184_3f91, DATA)] {
        let observed = Case::new(&[LDREX_R1_R2, second, BX_LR])
            .kernel(Kernel::default())
            .region(DATA, READ_WRITE, &words(&[0x11, 0x22]))
            .register(0, 0xdead_beef)
            .register(2, r2)
            .register(3, 0x3333_3333)
            .register(4, if second == 0xe884_0009 { DATA } else { DATA + 4 })
            .run();
        observed.assert_stopped(Status::UnsupportedInstruction, CODE + 4);
        assert_eq!(observed.bytes(DATA), "1100000022000000", "{second:#010x}");
        assert_eq!(observed.register("r3"), 0x3333_3333, "{second:#010x}");
        assert_eq!(observed.observation.memory_provenance_total, "1", "{second:#010x}");
    }
}

#[test]
fn other_exclusive_forms_are_refused_before_they_execute() {
    for word in [
        0xe1d2_0f9f, // ldrexb r0, [r2]
        0xe1b2_0f9f, // ldrexd r0, r1, [r2]
        0xe1f2_0f9f, // ldrexh r0, [r2]
        0xe1e2_3f91, // strexh r3, r1, [r2]
        0xe192_ff9f, // ldrex pc, [r2]
        0xe19f_1f9f, // ldrex r1, [pc]
        0xe182_2f91, // strex r2, r1, [r2]
        0xe182_3f93, // strex r3, r3, [r2]
        0xe182_3f9f, // strex r3, pc, [r2]
        0xe102_0091, // swp r0, r1, [r2]
    ] {
        let case = Case::new(&[word, BX_LR])
            .kernel(Kernel::default())
            .region(DATA, READ_WRITE, &words(&[5, 6]))
            .register(0, 9)
            .register(1, 7)
            .register(2, DATA)
            .register(3, 4);
        assert_status_write_has_no_effect(&case, &case.run());
    }
}

#[test]
fn exclusives_outside_writable_aligned_ordinary_words_are_memory_guards() {
    let device = |case: Case| case.region(BANK, DEVICE, &[0; 4]);
    for (code, address, region) in [
        (vec![LDREX_R1_R2, BX_LR], DATA + 2, READ_WRITE),
        (vec![LDREX_R1_R2, BX_LR], DATA + 8, READ_WRITE),
        (vec![LDREX_R1_R2, STREX_R3_R1_R2, BX_LR], DATA, READ),
        (vec![STREX_R3_R1_R2, BX_LR], DATA, READ),
        (vec![LDREX_R1_R2, BX_LR], BANK, READ_WRITE),
        (vec![STREX_R3_R1_R2, BX_LR], BANK, READ_WRITE),
    ] {
        let observed = device(Case::new(&code).kernel(Kernel::default()))
            .region(DATA, region, &words(&[5, 6]))
            .register(1, 7)
            .register(2, address)
            .register(3, 9)
            .run();
        let at = if code.len() == 3 { CODE + 4 } else { CODE };
        observed.assert_stopped(Status::MemoryGuard, at);
        assert_eq!(observed.register("r3"), 9, "{code:x?} {address:#x}");
        assert_eq!(observed.bytes(DATA), "0500000006000000");
        assert_eq!(observed.bytes(BANK), "00000000");
    }
}

#[test]
fn barriers_are_ordered_markers_among_the_stores() {
    let observed = Case::new(&[STR_R0_R2, DMB_SY, 0xe582_1004, DSB_SY, ISB_SY, CP15_DMB_R5, CP15_DSB_R5, 0xf57f_f05b, BX_LR])
        .kernel(Kernel::default())
        .region(DATA, READ_WRITE, &[0; 8])
        .register(0, 0x11)
        .register(1, 0x22)
        .register(2, DATA)
        .register(5, 0x5555_5555)
        .run();
    observed.assert_returned();
    assert_eq!(observed.bytes(DATA), "1100000022000000");
    assert_eq!(observed.register("r5"), 0x5555_5555);
    assert_eq!(
        observed.observation.memory_provenance,
        [
            access(0x1000, Kind::Write, DATA, 0x11),
            barrier(0x1004, Barrier::Dmb),
            access(0x1008, Kind::Write, DATA + 4, 0x22),
            barrier(0x100c, Barrier::Dsb),
            barrier(0x1010, Barrier::Isb),
            barrier(0x1014, Barrier::Dmb),
            barrier(0x1018, Barrier::Dsb),
            barrier(0x101c, Barrier::Dmb),
        ]
    );
    assert_eq!(observed.observation.memory_provenance_total, "8");
}

#[test]
fn a_condition_false_cp15_barrier_leaves_no_marker() {
    // mcrne p15, 0, r5, c7, c10, 5 with Z set
    let observed = Case::new(&[CP15_DMB_R5 & 0x0fff_ffff | 0x1000_0000, BX_LR]).kernel(Kernel::default()).run();
    observed.assert_returned();
    assert!(observed.observation.memory_provenance.is_empty());
}

#[test]
fn reserved_barrier_options_and_other_cp15_c7_words_are_refused() {
    for word in [
        0xf57f_f050, // dmb #0
        0xf57f_f041, // dsb #1
        0xf57f_f06b, // isb #11
        0xee07_5f95, // mcr p15, 0, r5, c7, c5, 4
        0xee07_ff9a, // mcr p15, 0, pc, c7, c10, 4
        0xee07_5fda, // mcr p15, 0, r5, c7, c10, 6
        0xee27_5fba, // mcr p15, 1, r5, c7, c10, 5
    ] {
        let case = Case::new(&[word, BX_LR]).kernel(Kernel::default()).register(5, 3);
        assert_status_write_has_no_effect(&case, &case.run());
    }
}

#[test]
fn nop_is_the_only_admitted_hint() {
    let observed = Case::new(&[NOP, ADD_R0_1, BX_LR]).kernel(Kernel::default()).run();
    observed.assert_returned();
    assert_eq!(observed.register("r0"), 1);
    assert_eq!(observed.instructions(), 3);
    // yield, wfe, wfi, sev, dbg #0, nopne
    for word in [0xe320_f001, 0xe320_f002, 0xe320_f003, 0xe320_f004, 0xe320_f0f0, 0x1320_f000] {
        let case = Case::new(&[word, BX_LR]).kernel(Kernel::default());
        assert_status_write_has_no_effect(&case, &case.run());
    }
}

#[test]
fn kernel_words_are_refused_in_a_privileged_case_without_a_kernel_row() {
    for word in [NOP, LDREX_R1_R2, STREX_R3_R1_R2, CLREX, DMB_SY, DSB_SY, ISB_SY, CP15_DMB_R5, TPIDRURW_TO_R4, CPACR_TO_R5, FPEXC_TO_R8] {
        let case = Case::new(&[word, BX_LR]).privileged(SVC_ENTRY, None).region(DATA, READ_WRITE, &[0; 4]).register(2, DATA);
        let observed = case.run();
        assert_status_write_has_no_effect(&case, &observed);
        assert!(observed.observation.kernel.is_none());
    }
}

#[test]
fn tpidrurw_serves_the_case_value_and_keeps_the_last_write() {
    let observed = Case::new(&[TPIDRURW_TO_R4, TPIDRURW_FROM_R6, TPIDRURW_TO_R7, BX_LR])
        .kernel(Kernel { tpidrurw: Some(0xabc), ..Kernel::default() })
        .register(6, 0x1234)
        .run();
    observed.assert_returned();
    assert_eq!(observed.register("r4"), 0xabc);
    assert_eq!(observed.register("r7"), 0x1234);
    assert_eq!(observed.kernel(), &KernelRegisters { tpidrurw: Some("0x00001234".into()), fpexc: None, reserved: None });
    assert_eq!(observed.observation.memory_provenance, [system(CODE + 4, SystemRegister::Tpidrurw, 0x1234)]);
}

#[test]
fn cpacr_reads_serve_the_case_value() {
    let observed = Case::new(&[CPACR_TO_R5, BX_LR]).kernel(Kernel { cpacr: Some(0x00f0_0000), ..Kernel::default() }).run();
    observed.assert_returned();
    assert_eq!(observed.register("r5"), 0x00f0_0000);
    assert_eq!(observed.kernel(), &KernelRegisters { tpidrurw: None, fpexc: None, reserved: None });
}

#[test]
fn fpexc_serves_the_case_value_and_keeps_an_en_write() {
    for (initial, written) in [(FPEXC_EN, 0), (0, FPEXC_EN)] {
        for cpacr in [0x00f0_0000, 0x0050_0000] {
            let observed = Case::new(&[FPEXC_TO_R8, FPEXC_FROM_R6, BX_LR])
                .kernel(Kernel { cpacr: Some(cpacr), fpexc: Some(initial), ..Kernel::default() })
                .register(6, written)
                .run();
            observed.assert_returned();
            assert_eq!(observed.register("r8"), initial);
            assert_eq!(observed.kernel().fpexc, Some(format!("0x{written:08x}")));
            assert_eq!(observed.observation.memory_provenance, [system(CODE + 4, SystemRegister::Fpexc, written)]);
        }
    }
}

#[test]
fn fpexc_is_refused_unless_cpacr_grants_cp10_and_cp11_alike() {
    // no access; cp10 only; cp10 privileged with cp11 full; the reserved 0b10 in both
    for cpacr in [0, 0x0010_0000, 0x00d0_0000, 0x00a0_0000] {
        for word in [FPEXC_TO_R8, FPEXC_FROM_R6] {
            let case = Case::new(&[word, BX_LR]).kernel(Kernel { cpacr: Some(cpacr), fpexc: Some(FPEXC_EN), ..Kernel::default() });
            let observed = case.run();
            assert_status_write_has_no_effect(&case, &observed);
            assert_eq!(observed.kernel().fpexc, Some(format!("0x{FPEXC_EN:08x}")), "{cpacr:#x} {word:#010x}");
        }
    }
}

#[test]
fn mov_pc_lr_is_a_tracked_return_in_a_kernel_case_only() {
    // mov pc, lr; movle pc, lr with Z set
    for word in [0xe1a0_f00e, 0xd1a0_f00e] {
        Case::new(&[word]).kernel(Kernel::default()).run().assert_returned();
        let case = Case::new(&[word, BX_LR]).privileged(SVC_ENTRY, None);
        assert_status_write_has_no_effect(&case, &case.run());
    }
    // movne pc, lr with Z set falls through to bx lr
    Case::new(&[0x11a0_f00e, BX_LR]).kernel(Kernel::default()).run().assert_returned();
    // mov lr, #0; mov pc, lr leaves the tracked frame
    let observed = Case::new(&[0xe3a0_e000, 0xe1a0_f00e]).kernel(Kernel::default()).run();
    observed.assert_stopped(Status::UnknownCall, CODE + 4);
    // a nested callee returns with mov pc, lr
    let observed = Case::new(&[PUSH_LR, branch(CODE + 4, 0x1040, true), ADD_R0_1, POP_PC])
        .code(0x1040, &[ADD_R0_1, 0xe1a0_f00e])
        .function(0x1040, 8)
        .kernel(Kernel::default())
        .run();
    observed.assert_returned();
    assert_eq!(observed.register("r0"), 2);
    // movs pc, lr stays refused
    let case = Case::new(&[0xe1b0_f00e, BX_LR]).kernel(Kernel::default());
    assert_status_write_has_no_effect(&case, &case.run());
}

#[test]
fn undeclared_and_other_system_register_words_are_refused() {
    let declared = Kernel { tpidrurw: Some(1), cpacr: Some(2), fpexc: Some(FPEXC_EN), ..Kernel::default() };
    for (kernel, word) in [
        (Kernel::default(), TPIDRURW_TO_R4),
        (Kernel::default(), TPIDRURW_FROM_R6),
        (Kernel::default(), CPACR_TO_R5),
        (Kernel::default(), FPEXC_TO_R8),
        (Kernel::default(), FPEXC_FROM_R6),
        (declared, 0xee01_5f50), // mcr p15, 0, r5, c1, c0, 2
        (declared, 0xee1d_4f70), // mrc p15, 0, r4, c13, c0, 3
        (declared, 0xee1d_ff50), // mrc p15, 0, pc, c13, c0, 2
        (declared, 0xeef1_4a10), // vmrs r4, fpscr
        (declared, 0xeee1_6a10), // vmsr fpscr, r6
        (declared, FPEXC_FROM_R6), // r6 sets EX
        (declared, 0xfe1d_4f50), // mrc2 p15, 0, r4, c13, c0, 2
    ] {
        let case = Case::new(&[word, BX_LR]).kernel(kernel).register(6, 0x8000_0000);
        let observed = case.run();
        assert_status_write_has_no_effect(&case, &observed);
        assert_eq!(observed.kernel().fpexc, kernel.fpexc.map(|value| format!("0x{value:08x}")), "{word:#010x}");
        assert_eq!(observed.kernel().tpidrurw, kernel.tpidrurw.map(|value| format!("0x{value:08x}")), "{word:#010x}");
    }
}

#[test]
fn a_kernel_case_ends_at_its_stop_address_by_call_branch_return_or_load() {
    let stop = Kernel { stop: Some(STOP), ..Kernel::default() };
    let target = words(&[0, 0, 0, 0, 0, 0, 0, 0, 0, STOP]);
    for (code, lr, instructions) in [
        (vec![branch(CODE, STOP, true)], CODE + 4, 1),
        (vec![branch(CODE, STOP, false)], EXIT, 1),
        (vec![0xe1a0_e005, BX_LR], STOP, 2), // mov lr, r5; bx lr
        (vec![0xe594_f024, BX_LR], EXIT, 1), // ldr pc, [r4, #0x24]
    ] {
        let observed = Case::new(&code)
            .kernel(stop)
            .region(DATA, READ, &target)
            .register(4, DATA)
            .register(5, STOP)
            .register(9, 0x99)
            .run();
        observed.assert_stopped(Status::StopAddress, STOP);
        assert_eq!(observed.register("lr"), lr, "{code:x?}");
        assert_eq!(observed.register("r9"), 0x99);
        assert_eq!(observed.instructions(), instructions, "{code:x?}");
    }
    let returned = Case::new(&[BX_LR]).kernel(stop).run();
    returned.assert_returned();
}

#[test]
fn a_pc_load_of_anything_but_the_stop_address_is_refused() {
    // ldr pc, [r4, #0x24] loading CODE + 4; with no stop address; ldr pc, [r4, #0x24]!
    for (word, kernel, loaded) in [
        (0xe594_f024, Kernel { stop: Some(STOP), ..Kernel::default() }, CODE + 4),
        (0xe594_f024, Kernel::default(), STOP),
        (0xe5b4_f024, Kernel { stop: Some(STOP), ..Kernel::default() }, STOP),
    ] {
        let case = Case::new(&[word, BX_LR])
            .kernel(kernel)
            .region(DATA, READ, &words(&[0, 0, 0, 0, 0, 0, 0, 0, 0, loaded]))
            .register(4, DATA);
        assert_status_write_has_no_effect(&case, &case.run());
    }
}

#[test]
fn the_bank_words_take_word_stores_in_order_after_a_barrier() {
    for bank in [0xfffb_30cc, 0xfffb_40cc, 0xfffb_30ec, 0xfffb_40ec] {
        let observed = Case::new(&[DSB_SY, STR_R0_R2, BX_LR])
            .kernel(Kernel::default())
            .region(bank, DEVICE, &[0; 4])
            .register(0, 0x0100_0000)
            .register(2, bank)
            .run();
        observed.assert_returned();
        assert_eq!(observed.bytes(bank), "00000001");
        assert_eq!(observed.observation.memory_provenance, [barrier(CODE, Barrier::Dsb), access(CODE + 4, Kind::Write, bank, 0x0100_0000)]);
    }
}

#[test]
fn bank_word_loads_and_narrow_stores_are_memory_guards() {
    // ldr r0, [r2]; strb r0, [r2]; strh r0, [r2]; strd r0, r1, [r2]
    for word in [0xe592_0000, 0xe5c2_0000, 0xe1c2_00b0, 0xe1c2_00f0] {
        let observed = Case::new(&[word, BX_LR])
            .kernel(Kernel::default())
            .region(BANK, DEVICE, &[0; 4])
            .register(0, 0x77)
            .register(2, BANK)
            .run();
        observed.assert_stopped(Status::MemoryGuard, CODE);
        assert_eq!(observed.register("r0"), 0x77, "{word:#010x}");
        assert_eq!(observed.bytes(BANK), "00000000", "{word:#010x}");
        assert_eq!(observed.observation.memory_provenance_total, "0");
    }
}

#[test]
fn malformed_kernel_cases_are_invalid() {
    let valid = Case::new(&[BX_LR]).kernel(Kernel { stop: Some(STOP), ..Kernel::default() }).region(BANK, DEVICE, &[0; 4]);
    valid.run().assert_returned();
    let stop = |address| valid.clone().kernel(Kernel { stop: Some(address), ..Kernel::default() });
    let mut user = valid.clone();
    user.registers[15] = 0x10;
    user.identity = None;
    let mut privileged = valid.clone();
    privileged.kernel = None;
    let stubbed = valid.clone().stub(STOP, &[], &[(0, 1)]);
    let mut cases = vec![
        stop(CODE),
        stop(EXIT),
        stop(STOP + 2),
        stop(STACK),
        stubbed,
        valid.clone().kernel(Kernel { cpacr: Some(0x00f0_0000), fpexc: Some(0x8000_0000), ..Kernel::default() }),
        valid.clone().kernel(Kernel { fpexc: Some(FPEXC_EN), ..Kernel::default() }),
        valid.clone().kernel(Kernel { fpexc: Some(FPEXC_EN | 1), ..Kernel::default() }),
        user,
        privileged,
        valid.clone().region(0xfffb_30d0, DEVICE, &[0; 4]),
    ];
    let mut wide = valid.clone();
    wide.regions[2] = Region { address: BANK, size: 8, permission: DEVICE, bytes: vec![0; 8] };
    let mut malformed = valid.clone();
    malformed.kernel = Some("k n n n".to_owned());
    cases.extend([wide, malformed]);
    for case in cases {
        let observed = case.run();
        assert_eq!(observed.status(), Status::InvalidCase, "{}", case.wire());
    }
}
