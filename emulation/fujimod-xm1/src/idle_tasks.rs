use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{CpuDataAttribution, FetchAttribution, MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::{bridge::{read, sha256, write}, guest::Press, machine::{KeyPulse, Machine, RunOutcome}, run::{self, RunDocument, ValidatedUpdate}, trace::Hit};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, num::NonZeroUsize, path::{Path, PathBuf}, process::ExitCode};

#[path = "memory_view.rs"]
mod memory_view;
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};

#[path = "loader_code.rs"]
mod loader_code;
use loader_code::immutable_loader_spans;

const USAGE: &str = "fujimod-xm1-idle-tasks --role retained|owned --run RUN --update DAT --boot-rom ROM --analysis ROOT --linked BIN --symbols SYMBOLS --contract CONTRACT --output NEW_DIR";
const DEADLINE: u64 = 80_000_000;
const HIT_LIMIT: usize = 16_384;
const WATCH_LIMIT: usize = 1_048_576;
const ENTRY: u32 = 0x69cbf0;
const LIMIT: u32 = 0x69cc00;
const IMAGE_BASE: u32 = 0x61c400;
const ORIGINAL_DAT: &str = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490";
const ORIGINAL_IMAGE: &str = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16";
const INITIAL_IMAGE_VIEW_END: u32 = 0x89ea8c;
const LOADER_A_CODE: [(u64, u64); 8] = immutable_loader_spans(0x40000, 0x4dae0);
const LOADER_B_CODE: [(u64, u64); 8] = immutable_loader_spans(0xd0000, 0xddaa0);
const IMAGE_CODE: [(u64, u64); 3] = [(ENTRY as u64, LIMIT as u64), (0x7221a0, 0x7221c8), (0x7221c8, 0x722218)];
const INTERFACES: [(u64, u64); 3] = [(0x96bd20, 0x96bd28), (0x4dae0, 0x4f10c), (0xddaa0, 0xdf0cc)];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct Address(u32);
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct Word(u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct TaskId(u16);
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String")]
struct Digest(String);
impl TryFrom<String> for Digest {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        require(value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)), "invalid SHA256")?;
        Ok(Self(value))
    }
}
fn digest(bytes: &[u8]) -> Digest { Digest(sha256(bytes)) }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Role { Retained, Owned }
impl Role {
    fn parse(value: &str) -> Result<Self, String> { match value { "retained" => Ok(Self::Retained), "owned" => Ok(Self::Owned), _ => Err(USAGE.into()) } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Core { Primary, Secondary }
impl Core {
    fn index(self) -> usize { match self { Self::Primary => 0, Self::Secondary => 1 } }
    fn from_index(index: usize) -> Result<Self, String> { match index { 0 => Ok(Self::Primary), 1 => Ok(Self::Secondary), _ => Err("unexpected core".into()) } }
    fn layout(self) -> Layout {
        match self {
            Self::Primary => Layout { id: TaskId(102), creation: 0x4005beac + 40 * 101, node: 0x4005ff0c + 56 * 101, current: 0x40054760, selected: 0x40054768, stack: 0xa10e00, table: 0x4004dae0 },
            Self::Secondary => Layout { id: TaskId(103), creation: 0x400ebe6c + 40 * 102, node: 0x400efecc + 56 * 102, current: 0x400e4720, selected: 0x400e4728, stack: 0xa10200, table: 0x400ddaa0 },
        }
    }
}
#[derive(Clone, Copy)]
struct Layout { id: TaskId, creation: u32, node: u32, current: u32, selected: u32, stack: u32, table: u32 }
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Qualification { HostAdmittedIdleTask }
#[derive(Deserialize)]
struct ArtifactContract {
    schema: u32, entry: Address, replaced_extent: [Address; 2], linked_bytes: usize,
    linked_sha256: Digest, symbols_sha256: Digest, staged_image4_sha256: Digest,
    scratch_low_from_entry_sp: i32, execution_pcs: Vec<Address>, wfi_pc: Address, resume_pc: Address,
    source_dat_image4_sha256: [Digest; 2],
}
#[derive(Deserialize)]
struct CandidateContract {
    schema: u32, qualification: Qualification, idle_task: ArtifactContract,
    candidate_dat_sha256: Digest, candidate_image4_sha256: Digest,
}
struct Code { name: &'static str, base: u32, bytes: Vec<u8> }
#[derive(Serialize)]
struct SourceIdentity { name: &'static str, base: Address, bytes: usize, sha256: Digest }
#[derive(Serialize)]
struct Identity { run: Digest, update: Digest, boot_rom: Digest, observer: Digest, linked: Digest, symbols: Digest, contract: Digest, sources: Vec<SourceIdentity> }
#[derive(Clone, Debug, Serialize)]
struct Context { core: Core, pc: Address, registers: [Word; 13], sp: Address, lr: Address, cpsr: Word, spsr: Word, counts: [u64; 2], identities: [Word; 2], tcm: [[Word; 2]; 2] }
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Object { CreationRecord, TaskNode, Current, Selected, Pool, Stack, Code, LoadedCode, InterfaceRoots, InterfaceTable }
#[derive(Serialize)]
struct View { object: Object, core: Core, physical: Address, bytes: Vec<u8>, aliases: [[Digest; 2]; 2] }
#[derive(Debug, Serialize)]
struct Kernel {
    core: Core, id: TaskId, creation: Address, node: Address, creation_pointer: Address, creation_id: Word,
    entry: Address, stack_base: Address, stack_bytes: Word, created: Word, saved_sp: Address, resume_pc: Address,
    current: Address, selected: Address, state: u8, effective_priority: u8, pending: u8,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Boundary { Start, Entry, Instruction, IrqEntry, IrqFrame, IrqReturn }
#[derive(Serialize)]
struct Sample { hit: usize, boundary: Boundary, context: Context, code_word: Word, watch_cursor: usize, views: Vec<View>, kernels: [Kernel; 2] }
#[derive(Deserialize, Serialize)]
struct WatchSummary { enabled: bool, violation: Option<String> }
#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Access { CpuFetch, CpuRead, CpuWrite, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Deserialize)]
struct AccessRecord { source: Access, physical: Option<u64>, size: u64, core: Option<usize>, pc: Option<u64>, instructions: Option<u64> }
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome { ObservedMenu, Refused }
#[derive(Serialize)]
struct Receipt<'a> {
    schema: u32, scope: &'static str, role: Role, outcome: Outcome, error: Option<&'a str>, machine_failure: Option<&'a str>,
    identity: Identity, checkpoint_sha256: Digest, checkpoint_reused: bool, instructions: u64, lifecycle_end_counts: Option<[u64; 2]>,
    memory_watch_sha256: Digest, watch: Result<WatchSummary, String>, watch_end_error: Option<String>,
    sites: &'a BTreeMap<u32, String>, hits: &'a [Hit], samples: &'a [Sample], final_menu: Result<Option<[u32; 3]>, String>,
}

fn require(condition: bool, message: &str) -> Result<(), String> { if condition { Ok(()) } else { Err(message.into()) } }
fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes.get(offset..offset + 4).ok_or("word exceeds object")?.try_into().map_err(|_| "invalid word extent")?))
}
fn number(value: &str) -> Result<u32, String> {
    u32::from_str_radix(value.strip_prefix("0x").ok_or("trace word is not hexadecimal")?, 16).map_err(|error| error.to_string())
}
fn register(machine: &Machine, core: Core, register: arm::Register) -> Result<u32, String> {
    u32::try_from(machine.session.board.read_register(core.index(), Register::Arm32(register))?).map_err(|_| "register exceeds32 bits".into())
}
fn context(machine: &Machine, core: Core) -> Result<Context, String> {
    let mut registers = [Word(0); 13];
    for (index, value) in registers.iter_mut().enumerate() { *value = Word(register(machine, core, arm::Register::R(index as u8))?); }
    let mut identities = [Word(0); 2];
    let mut tcm = [[Word(0); 2]; 2];
    for index in 0..2 {
        let cpu = machine.session.board.cpu(index)?;
        identities[index] = Word(u32::try_from(cpu.rprofile_identity(None)?).map_err(|_| "identity exceeds32 bits")?);
        for (bank, opc2) in [0, 1].into_iter().enumerate() {
            tcm[index][bank] = Word(u32::try_from(cpu.coprocessor(arm::Coprocessor { cp: 15, opc1: 0, crn: 9, crm: 1, opc2, secure: 0, wide: false }, None)?).map_err(|_| "TCM register exceeds32 bits")?);
        }
    }
    Ok(Context {
        core, pc: Address(register(machine, core, arm::Register::R(15))?), registers,
        sp: Address(register(machine, core, arm::Register::R(13))?), lr: Address(register(machine, core, arm::Register::R(14))?),
        cpsr: Word(register(machine, core, arm::Register::Cpsr)?), spsr: Word(register(machine, core, arm::Register::Spsr)?),
        counts: machine.session.board.instruction_counts().try_into().map_err(|_| "expected two cores")?, identities, tcm,
    })
}
fn view(machine: &Machine, maps: &[MemoryMap], state: &Context, object: Object, core: Core, address: u32, length: usize) -> Result<View, String> {
    let physical = address & !0x40000000;
    let end = u64::from(physical).checked_add(length as u64).ok_or("resource extent overflow")?;
    require(length != 0 && end <= 0x20000000, "resource is outside admitted DRAM")?;
    let mut bytes = vec![0; length];
    let mut observed = vec![0; length];
    let mut aliases: [[Digest; 2]; 2] = std::array::from_fn(|_| std::array::from_fn(|_| Digest(String::new())));
    for (reader, digests) in aliases.iter_mut().enumerate() {
        for (alias, digest) in digests.iter_mut().enumerate() {
            let guest = u64::from(physical) | if alias == 0 { 0 } else { 0x40000000 };
            let span = static_span(maps, MemoryCore(reader), MemoryAddress(guest)..MemoryAddress(guest + length as u64))?;
            require(span.physical.0 == u64::from(physical), "resource alias translation differs")?;
            for bank in state.tcm[reader] {
                if let Some((low, high)) = tcm_window(TcmRegister(bank.0))? {
                    require(guest + length as u64 <= low || guest >= high, "resource overlaps active TCM")?;
                }
            }
            machine.session.board.read_memory(reader, guest, &mut observed)?;
            if reader == 0 && alias == 0 { bytes.copy_from_slice(&observed); }
            require(observed == bytes, "resource core/alias values differ")?;
            *digest = Digest(sha256(&observed));
        }
    }
    machine.session.board.read_physical(u64::from(physical), &mut observed)?;
    require(observed == bytes, "committed physical view differs")?;
    Ok(View { object, core, physical: Address(physical), bytes, aliases })
}
fn kernel(machine: &Machine, maps: &[MemoryMap], state: &Context, core: Core, views: &mut Vec<View>) -> Result<Kernel, String> {
    let l = core.layout();
    views.push(view(machine, maps, state, Object::Pool, core, l.stack, 1024)?);
    let creation = view(machine, maps, state, Object::CreationRecord, core, l.creation, 40)?;
    let node = view(machine, maps, state, Object::TaskNode, core, l.node, 56)?;
    let current = view(machine, maps, state, Object::Current, core, l.current, 4)?;
    let selected = view(machine, maps, state, Object::Selected, core, l.selected, 4)?;
    let task = Kernel {
        core, id: l.id, creation: Address(l.creation), node: Address(l.node),
        creation_pointer: Address(word(&node.bytes, 8)?), creation_id: Word(word(&creation.bytes, 32)?),
        entry: Address(word(&creation.bytes, 8)?), stack_base: Address(word(&creation.bytes, 20)?),
        stack_bytes: Word(word(&creation.bytes, 16)?), created: Word(word(&creation.bytes, 36)?),
        saved_sp: Address(word(&node.bytes, 24)?), resume_pc: Address(word(&node.bytes, 28)?),
        current: Address(word(&current.bytes, 0)?), selected: Address(word(&selected.bytes, 0)?),
        state: node.bytes[12], effective_priority: node.bytes[13], pending: node.bytes[14],
    };
    require(task.created.0 <= 1, "idle created flag is not Boolean")?;
    if task.created == Word(1) {
        require(task.creation_pointer == task.creation && task.creation_id.0 == u32::from(l.id.0) && task.entry == Address(ENTRY), "idle creation/node identity differs")?;
        require(task.stack_bytes == Word(1024) && task.stack_base.0 & !0x40000000 == l.stack, "idle stack differs from canonical run bounds")?;
        views.push(view(machine, maps, state, Object::Stack, core, task.stack_base.0, 1024)?);
    }
    views.extend([creation, node, current, selected]);
    Ok(task)
}
fn sample(machine: &Machine, maps: &[MemoryMap], sources: &[Code], hit: usize, state: Context, watch_cursor: usize) -> Result<Sample, String> {
    let boundary = boundary(state.pc.0)?;
    let mut views = Vec::with_capacity(17);
    let kernels = [kernel(machine, maps, &state, Core::Primary, &mut views)?, kernel(machine, maps, &state, Core::Secondary, &mut views)?];
    let roots = view(machine, maps, &state, Object::InterfaceRoots, Core::Primary, 0x96bd20, 8)?;
    for core in [Core::Primary, Core::Secondary] {
        require(word(&roots.bytes, 4 * core.index())? == core.layout().table, "installed interface root differs")?;
        views.push(view(machine, maps, &state, Object::InterfaceTable, core, core.layout().table, 0x162c)?);
    }
    views.push(roots);
    views.push(view(machine, maps, &state, Object::Code, Core::Primary, ENTRY, 16)?);
    let code = sources.iter().find(|code| state.pc.0 >= code.base && u64::from(state.pc.0) + 4 <= u64::from(code.base) + code.bytes.len() as u64).ok_or("instruction point leaves canonical source")?;
    let expected = Word(word(&code.bytes, (state.pc.0 - code.base) as usize)?);
    let observed = view(machine, maps, &state, Object::Code, state.core, state.pc.0, 4)?;
    require(word(&observed.bytes, 0)? == expected.0, "instruction word differs from installed source")?;
    views.push(observed);
    Ok(Sample { hit, boundary, context: state, code_word: expected, watch_cursor, views, kernels })
}
fn sources(root: &Path) -> Result<Vec<Code>, String> {
    [("decode/loader-40000.bin", 0x40000, "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"),
     ("decode/loader-d0000.bin", 0xd0000, "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"),
     ("lz/id4-0061c400.bin", IMAGE_BASE, ORIGINAL_IMAGE)].into_iter().map(|(name, base, expected)| {
        let bytes = read(&root.join(name))?;
        require(sha256(&bytes) == expected, "noncanonical source image")?;
        Ok(Code { name, base, bytes })
    }).collect()
}
fn idle_loop(bytes: &[u8]) -> Result<(Address, Address), String> {
    require(bytes.len() == 16, "idle carrier length differs")?;
    let mut waits = bytes.chunks_exact(4).enumerate().filter(|(_, bytes)| *bytes == 0xe320f003u32.to_le_bytes()).map(|(index, _)| ENTRY + 4 * index as u32);
    let wfi = waits.next().ok_or("idle carrier has no WFI")?;
    require(waits.next().is_none(), "idle carrier must contain one WFI")?;
    let resume = wfi.checked_add(4).filter(|resume| *resume < LIMIT).ok_or("idle resume leaves carrier")?;
    let branch = word(bytes, (resume - ENTRY) as usize)?;
    let displacement = (((branch & 0x00ffffff) << 8) as i32 >> 6) as i64;
    require(branch & 0xff000000 == 0xea000000 && i64::from(resume) + 8 + displacement == i64::from(wfi), "idle resume is not an unconditional backedge to WFI")?;
    Ok((Address(wfi), Address(resume)))
}
fn bind_artifact(role: Role, linked: &[u8], symbols: &[u8], contract: &[u8], update: &Digest, sources: &mut [Code]) -> Result<(Address, Address), String> {
    let binding: CandidateContract = serde_json::from_slice(contract).map_err(|error| error.to_string())?;
    let artifact = &binding.idle_task;
    require(binding.schema == 1 && matches!(binding.qualification, Qualification::HostAdmittedIdleTask) && artifact.schema == 1
        && artifact.entry == Address(ENTRY) && artifact.replaced_extent == [Address(ENTRY), Address(LIMIT)]
        && !linked.is_empty() && linked.len() <= 16 && linked.len() % 4 == 0 && artifact.linked_bytes == linked.len()
        && artifact.linked_sha256 == digest(linked) && artifact.symbols_sha256 == digest(symbols)
        && artifact.source_dat_image4_sha256 == [Digest(ORIGINAL_DAT.into()), Digest(ORIGINAL_IMAGE.into())]
        && (-1024..=0).contains(&artifact.scratch_low_from_entry_sp), "idle artifact identity or extent differs")?;
    require(!artifact.execution_pcs.is_empty() && artifact.execution_pcs.iter().all(|pc| pc.0 >= ENTRY && pc.0 < ENTRY + linked.len() as u32 && pc.0 % 4 == 0), "idle execution PCs leave linked extent")?;
    let mut carrier = [0; 16];
    for bytes in carrier.chunks_exact_mut(4) { bytes.copy_from_slice(&0xe320f000u32.to_le_bytes()); }
    carrier[..linked.len()].copy_from_slice(linked);
    let owned_loop = idle_loop(&carrier)?;
    require(owned_loop == (artifact.wfi_pc, artifact.resume_pc), "idle loop contract differs from linked words")?;
    let image = sources.iter_mut().find(|source| source.base == IMAGE_BASE).ok_or("canonical image4 absent")?;
    let offset = (ENTRY - IMAGE_BASE) as usize;
    let retained_loop = idle_loop(image.bytes.get(offset..offset + 16).ok_or("idle span leaves image4")?)?;
    let original: [u8; 16] = image.bytes[offset..offset + 16].try_into().map_err(|_| "idle carrier length differs")?;
    image.bytes[offset..offset + 16].copy_from_slice(&carrier);
    require(digest(&image.bytes) == artifact.staged_image4_sha256 && artifact.staged_image4_sha256 == binding.candidate_image4_sha256, "staged image4 identity differs")?;
    match role {
        Role::Retained => { require(update.0 == ORIGINAL_DAT, "retained role requires canonical original DAT")?; image.bytes[offset..offset + 16].copy_from_slice(&original); Ok(retained_loop) }
        Role::Owned => { require(*update == binding.candidate_dat_sha256, "owned role DAT differs from staging contract")?; Ok(owned_loop) }
    }
}
fn boundary(address: u32) -> Result<Boundary, String> {
    match address {
        0x725dd8 => Ok(Boundary::Start), ENTRY => Ok(Boundary::Entry), 0x69cbf4 | 0x69cbf8 | 0x69cbfc => Ok(Boundary::Instruction),
        0x40204 | 0xd0204 => Ok(Boundary::IrqEntry), 0x40234 | 0xd0234 => Ok(Boundary::IrqFrame),
        0x40364 | 0xd0364 | 0x437d8 | 0xd37d8 => Ok(Boundary::IrqReturn), _ => Err("unknown observation boundary".into()),
    }
}
fn sites() -> BTreeMap<u32, String> {
    [(0x725dd8, "start"), (ENTRY, "entry"), (0x69cbf4, "instruction"), (0x69cbf8, "instruction"), (0x69cbfc, "instruction"),
     (0x40204, "irq-entry"), (0xd0204, "irq-entry"), (0x40234, "irq-frame"), (0xd0234, "irq-frame"),
     (0x40364, "irq-return"), (0xd0364, "irq-return"), (0x437d8, "irq-return"), (0xd37d8, "irq-return")]
        .into_iter().map(|(address, name)| (address, name.into())).collect()
}
fn immutable(physical: u64, size: u64) -> bool {
    LOADER_A_CODE.iter().chain(&LOADER_B_CODE).chain(&IMAGE_CODE).chain(&INTERFACES)
        .any(|&(low, high)| physical < high && low < physical.saturating_add(size))
}
fn watch_ranges() -> Vec<WatchRange> {
    let mut ranges = Vec::with_capacity(42);
    ranges.extend(LOADER_A_CODE.iter().chain(&LOADER_B_CODE).chain(&IMAGE_CODE).chain(&INTERFACES)
        .map(|&(start, end)| WatchRange { start, end, kinds: WatchKinds::WRITES }));
    ranges.extend([
        WatchRange { start: u64::from(ENTRY), end: u64::from(LIMIT), kinds: WatchKinds::CPU_FETCH },
        WatchRange { start: 0x725dd8, end: 0x725ddc, kinds: WatchKinds::CPU_FETCH },
    ]);
    for address in [0x40204, 0xd0204, 0x40234, 0xd0234, 0x40364, 0xd0364, 0x437d8, 0xd37d8] {
        ranges.push(WatchRange { start: address, end: address + 4, kinds: WatchKinds::CPU_FETCH });
    }
    for core in [Core::Primary, Core::Secondary] {
        let l = core.layout();
        for (base, size) in [(l.creation, 40), (l.node, 56), (l.current, 4), (l.selected, 4), (l.stack, 1024)] {
            ranges.push(WatchRange { start: u64::from(base & !0x40000000), end: u64::from((base & !0x40000000) + size), kinds: WatchKinds::WRITES });
        }
    }
    ranges
}
fn fetch_attribution() -> Result<FetchAttribution, String> {
    FetchAttribution::new(CpuDataAttribution::Writes, vec![
        WatchRange { start: 0x40000, end: 0x4dae0, kinds: WatchKinds::CPU_FETCH },
        WatchRange { start: 0xd0000, end: 0xddaa0, kinds: WatchKinds::CPU_FETCH },
        WatchRange { start: u64::from(IMAGE_BASE), end: u64::from(INITIAL_IMAGE_VIEW_END), kinds: WatchKinds::CPU_FETCH },
    ])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { AwaitEntry, AwaitWait, Waiting, IrqEntered, Framed, Returned, Resumed, RepeatPending, Complete }
struct Lifecycle { phase: Phase, wait: Option<Context>, wait_executed: bool, repeated_at: Option<u64> }
struct Producer { wfi: Address, resume: Address, cores: [Lifecycle; 2] }
impl Producer {
    fn new(wfi: Address, resume: Address) -> Self {
        Self { wfi, resume, cores: std::array::from_fn(|_| Lifecycle { phase: Phase::AwaitEntry, wait: None, wait_executed: false, repeated_at: None }) }
    }
    fn complete(&self) -> bool { self.cores.iter().all(|core| core.phase == Phase::Complete) }
    fn fetched(&mut self, core: Core, pc: Address, instructions: u64) {
        let life = &mut self.cores[core.index()];
        if pc == self.wfi {
            if life.phase == Phase::Waiting && life.wait.as_ref().is_some_and(|wait| wait.counts.iter().sum::<u64>() < instructions) { life.wait_executed = true; }
            if life.phase == Phase::RepeatPending && life.repeated_at.is_some_and(|start| start < instructions) { life.phase = Phase::Complete; }
        }
    }
    fn observe(&mut self, sample: &Sample) -> Result<(), String> {
        let state = &sample.context;
        let core = state.core;
        let life = &mut self.cores[core.index()];
        let task = &sample.kernels[core.index()];
        let active = task.created == Word(1) && task.current == task.node;
        let selected = active && task.selected == task.node;
        if (ENTRY..LIMIT).contains(&state.pc.0) {
            require(selected && task.entry == Address(ENTRY), "idle instruction has wrong current/selected task")?;
            require(state.cpsr.0 & 0x0100023f == 0x13 && state.cpsr.0 & 0x80 == 0, "idle instruction has wrong privilege, endian, instruction or IRQ state")?;
            let low = task.stack_base.0 & !0x40000000;
            let sp = state.sp.0 & !0x40000000;
            require(sp >= low && sp <= low + task.stack_bytes.0 && sp % 4 == 0, "idle SP leaves live stack")?;
            if sample.boundary == Boundary::Entry && life.phase == Phase::AwaitEntry {
                require(sp == low + 1024 && state.registers[0] == Word(0), "idle entry argument or stack differs")?;
                life.phase = Phase::AwaitWait;
            }
            if state.pc == self.wfi {
                if life.phase == Phase::AwaitWait {
                    life.wait = Some(state.clone()); life.phase = Phase::Waiting;
                } else if life.phase == Phase::Resumed {
                    let wait = life.wait.as_ref().ok_or("idle first wait absent")?;
                    require(restored(state, wait), "repeated idle wait did not restore task context")?;
                    life.repeated_at = Some(state.counts.iter().sum()); life.phase = Phase::RepeatPending;
                }
            }
            if state.pc == self.resume && life.phase == Phase::Returned {
                let wait = life.wait.as_ref().ok_or("idle first wait absent")?;
                require(restored(state, wait), "idle resume did not restore task context")?;
                life.phase = Phase::Resumed;
            }
        }
        let Some(wait) = life.wait.as_ref() else { return Ok(()) };
        if sample.boundary == Boundary::IrqEntry && life.phase == Phase::Waiting && active && life.wait_executed
            && state.cpsr.0 & 0x1f == 0x12 && state.spsr == wait.cpsr && state.lr.0 == self.resume.0 + 4
            && state.registers == wait.registers {
            life.phase = Phase::IrqEntered;
        }
        if sample.boundary == Boundary::IrqFrame && life.phase == Phase::IrqEntered {
            require(active && state.cpsr.0 & 0xbf == 0x93 && state.sp.0.checked_add(32) == Some(wait.sp.0), "idle IRQ source frame context differs")?;
            require(frame::<8>(sample)? == [wait.cpsr.0, wait.registers[0].0, wait.registers[1].0, wait.registers[2].0, wait.registers[3].0, wait.registers[12].0, wait.lr.0, self.resume.0], "idle IRQ source frame differs")?;
            life.phase = Phase::Framed;
        }
        if sample.boundary == Boundary::IrqReturn && life.phase == Phase::Framed && active && state.sp.0.checked_add(28) == Some(wait.sp.0) {
            require(state.cpsr.0 & 0xbf == 0x93, "idle IRQ return has wrong source control bits")?;
            require(state.spsr == wait.cpsr && frame::<7>(sample)? == [wait.registers[0].0, wait.registers[1].0, wait.registers[2].0, wait.registers[3].0, wait.registers[12].0, wait.lr.0, self.resume.0], "idle IRQ return frame differs")?;
            life.phase = Phase::Returned;
        }
        Ok(())
    }
}
fn restored(state: &Context, wait: &Context) -> bool {
    state.registers == wait.registers && state.sp == wait.sp && state.lr == wait.lr && state.cpsr == wait.cpsr
}
fn frame<const WORDS: usize>(sample: &Sample) -> Result<[u32; WORDS], String> {
    let sp = sample.context.sp.0 & !0x40000000;
    let stack = sample.views.iter().find(|view| matches!(view.object, Object::Stack) && view.core == sample.context.core).ok_or("idle live stack absent at IRQ boundary")?;
    let offset = sp.checked_sub(stack.physical.0).ok_or("IRQ frame below idle stack")? as usize;
    let mut frame = [0; WORDS];
    for (index, value) in frame.iter_mut().enumerate() { *value = word(&stack.bytes, offset + 4 * index)?; }
    Ok(frame)
}

fn inspect_watch(raw: &serde_json::Value, processed: &mut usize, stopped: &[Hit], producer: &mut Producer) -> Result<usize, String> {
    let summary = WatchSummary::deserialize(raw).map_err(|error| error.to_string())?;
    require(summary.enabled && summary.violation.is_none(), "idle memory watch is incomplete")?;
    let rows = raw.get("records").and_then(serde_json::Value::as_array).ok_or("watch records absent")?;
    let new = rows.get(*processed..).ok_or("watch cursor moved backwards")?;
    let events = new.iter().map(|row| AccessRecord::deserialize(row).map_err(|error| error.to_string())).collect::<Result<Vec<_>, _>>()?;
    // Instruction-point hooks precede fetch-watch hooks; stopped fetch rows retain the settled clock.
    let stop_rows = stopped.iter().map(|hit| {
        let pc = u64::from(number(&hit.address)?);
        Ok(events.iter().rposition(|row| row.source == Access::CpuFetch && row.core == Some(hit.core) && row.pc == Some(pc) && row.instructions == Some(hit.instructions)))
    }).collect::<Result<Vec<_>, String>>()?;
    for (index, row) in events.iter().enumerate() {
        if matches!(row.source, Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite) {
            let physical = row.physical.ok_or("write has no physical address")?;
            require(!immutable(physical, row.size), "immutable installed code write after watch start")?;
        }
        if row.source == Access::CpuFetch && !stop_rows.contains(&Some(index)) {
            producer.fetched(Core::from_index(row.core.ok_or("fetch core absent")?)?, Address(u32::try_from(row.pc.ok_or("fetch PC absent")?).map_err(|_| "fetch PC exceeds32 bits")?), row.instructions.ok_or("fetch clock absent")?);
        }
    }
    *processed = rows.len();
    Ok(rows.len())
}
fn menu_route(machine: &mut Machine) -> Result<(), String> {
    machine.run(DEADLINE.saturating_sub(machine.instructions())); machine.require_running()?;
    require(machine.menu()?.is_some_and(|menu| (menu.menu_id, menu.setting_id, menu.cursor) == (69, 52, 12)), "startup did not reach erased-settings menu")?;
    let pulse = KeyPulse { hold: 8_000_000, release: 8_000_000 };
    for key in std::iter::repeat_n("up", 12).chain(["ok", "ok"]) { machine.press_once(Press::parse(key)?, pulse)?; }
    machine.run(40_000_000); machine.require_running()?;
    machine.press_once(Press::parse("menu")?, pulse)?;
    machine.run(90_000_000); machine.require_running()?;
    require(machine.menu()?.is_some_and(|menu| (menu.menu_id, menu.setting_id, menu.cursor) == (1, 5, 2)), "route did not reach shooting menu")
}
fn drive() -> Result<(), String> {
    let mut arguments = BTreeMap::new(); let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag = flag.into_string().map_err(|_| USAGE)?;
        if !["--role", "--run", "--update", "--boot-rom", "--analysis", "--linked", "--symbols", "--contract", "--output"].contains(&flag.as_str())
            || arguments.insert(flag, PathBuf::from(args.next().ok_or(USAGE)?)).is_some() { return Err(USAGE.into()); }
    }
    let mut take = |flag| arguments.remove(flag).ok_or_else(|| USAGE.to_owned());
    let role_path = take("--role")?;
    let role = Role::parse(role_path.to_str().ok_or(USAGE)?)?;
    let run_bytes = read(&take("--run")?)?; let update = read(&take("--update")?)?; let rom = read(&take("--boot-rom")?)?;
    let mut code = sources(&take("--analysis")?)?;
    let linked = read(&take("--linked")?)?; let symbols = read(&take("--symbols")?)?; let contract = read(&take("--contract")?)?;
    let output = take("--output")?;
    let identity = Identity {
        run: digest(&run_bytes), update: digest(&update), boot_rom: digest(&rom),
        observer: digest(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?),
        linked: digest(&linked), symbols: digest(&symbols), contract: digest(&contract),
        sources: code.iter().map(|code| SourceIdentity { name: code.name, base: Address(code.base), bytes: code.bytes.len(), sha256: digest(&code.bytes) }).collect(),
    };
    require(identity.run.0 == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
        && identity.boot_rom.0 == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be", "observer requires canonical run and boot ROM")?;
    let (wfi, resume) = bind_artifact(role, &linked, &symbols, &contract, &identity.update, &mut code)?;
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    fujimod_emulator_exr2_board::register_board_builder(); fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?; let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(update), &rom)?; session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?; machine.disarm()?;
    let reset = machine.session.checkpoint(None)?.export()?; write(&output.join("reset.fujimod"), &reset)?;
    let snapshot = machine.session.import_snapshot(&reset)?; machine.session.restore(&snapshot)?; machine.rearm()?;
    require(machine.instructions() == 0, "reset checkpoint is not preexecution")?;
    let sites = sites(); machine.traces.bound_full_registers(NonZeroUsize::new(HIT_LIMIT).ok_or("invalid hit bound")?);
    machine.traces.stop_labels.extend(sites.values().cloned());
    machine.traces.arm(&machine.session, BTreeMap::from([(0x725dd8, "start".into())]))?;
    let mut watch: Option<Box<MemoryWatch>> = None;
    let mut samples = Vec::new(); let mut processed_hits = 0; let mut processed_rows = 0;
    let mut producer = Producer::new(wfi, resume); let mut lifecycle_end_counts = None;
    let result = (|| -> Result<(), String> {
        loop {
            let stopped = machine.run(DEADLINE.saturating_sub(machine.instructions()));
            let end = machine.traces.hits.len();
            let cursor = if let Some(watch) = &watch {
                inspect_watch(&watch.records.borrow().state(), &mut processed_rows, &machine.traces.hits[processed_hits..end], &mut producer)?
            } else { 0 };
            for index in processed_hits..end {
                let hit = &machine.traces.hits[index]; let address = number(&hit.address)?; let core = Core::from_index(hit.core)?;
                if watch.is_none() && address != 0x725dd8 { continue; }
                let state = context(&machine, core)?;
                require(state.pc == Address(address), "stopped context moved past instruction point")?;
                let mut installed_views = Vec::new();
                if watch.is_none() {
                    require(matches!((core, state.lr.0), (Core::Primary, 0x69ce8c | 0x69cee8) | (Core::Secondary, 0x69cf38 | 0x69cf70)), "first wrapper is outside canonical startup callers")?;
                    for source in &code {
                        let length = if source.base == IMAGE_BASE { (INITIAL_IMAGE_VIEW_END - IMAGE_BASE) as usize } else { source.bytes.len() };
                        let owner = if source.base == 0xd0000 { Core::Secondary } else { Core::Primary };
                        let installed = view(&machine, &maps, &state, Object::LoadedCode, owner, source.base, length)?;
                        require(installed.bytes == source.bytes[..length], "installed source differs at first wrapper")?;
                        installed_views.push(installed);
                    }
                    let lifecycle_sites = sites.iter().filter(|(address, _)| **address != 0x725dd8).map(|(&address, label)| (address, label.clone())).collect();
                    machine.traces.arm(&machine.session, lifecycle_sites)?;
                    watch = Some(machine.session.board.memory_watch_begin_with_fetch_attribution(watch_ranges(), fetch_attribution()?, WATCH_LIMIT, maps.clone(), ViewPolicy::Observe)?);
                }
                let mut observed = sample(&machine, &maps, &code, index, state, cursor)?;
                observed.views.extend(installed_views);
                samples.push(observed);
                producer.observe(samples.last().ok_or("idle sample absent")?)?;
            }
            processed_hits = end;
            machine.require_running()?;
            if producer.complete() {
                lifecycle_end_counts = Some(machine.session.board.instruction_counts().try_into().map_err(|_| "expected two cores")?); break;
            }
            require(stopped == RunOutcome::Stopped, "idle lifecycle incomplete at natural instruction deadline")?;
        }
        Ok(())
    })();
    let raw = watch.as_ref().map(|watch| watch.records.borrow().state());
    let accesses = serde_json::to_vec_pretty(&raw).map_err(|error| error.to_string())?; write(&output.join("memory-watch.json"), &accesses)?;
    let summary = raw.as_ref().ok_or_else(|| "idle watch never began".to_owned()).and_then(|raw| WatchSummary::deserialize(raw).map_err(|error| error.to_string()));
    let watch_end_error = watch.as_mut().and_then(|watch| machine.session.board.memory_watch_end(watch).err());
    let disarm = machine.traces.disarm(&machine.session); machine.traces.stop_labels.clear();
    let outcome = result.and(disarm).and_then(|()| {
        require(summary.as_ref().is_ok_and(|summary| summary.enabled && summary.violation.is_none()) && watch_end_error.is_none(), "idle watch refused")?;
        menu_route(&mut machine)
    });
    let final_menu = machine.menu().map(|menu| menu.map(|menu| [menu.menu_id, menu.setting_id, menu.cursor]));
    let receipt = Receipt {
        schema: 1, scope: "natural-reset-idle-task-lifecycle", role, outcome: if outcome.is_ok() { Outcome::ObservedMenu } else { Outcome::Refused },
        error: outcome.as_ref().err().map(String::as_str), machine_failure: machine.failure.as_deref(), identity,
        checkpoint_sha256: digest(&reset), checkpoint_reused: true, instructions: machine.instructions(), lifecycle_end_counts,
        memory_watch_sha256: digest(&accesses), watch: summary, watch_end_error, sites: &sites, hits: &machine.traces.hits, samples: &samples, final_menu,
    };
    write(&output.join("idle-tasks.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    outcome
}
fn main() -> ExitCode { match drive() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } } }

#[cfg(test)]
mod tests {
    use super::*;

    const WFI: Address = Address(ENTRY + 8);
    const RESUME: Address = Address(ENTRY + 12);

    fn synthetic_kernel(core: Core) -> Kernel {
        let l = core.layout();
        Kernel {
            core, id: l.id, creation: Address(l.creation), node: Address(l.node), creation_pointer: Address(l.creation),
            creation_id: Word(u32::from(l.id.0)), entry: Address(ENTRY), stack_base: Address(l.stack), stack_bytes: Word(1024),
            created: Word(1), saved_sp: Address(l.stack + 1024), resume_pc: Address(ENTRY),
            current: Address(l.node), selected: Address(l.node), state: 1, effective_priority: 14, pending: 0,
        }
    }
    fn synthetic_sample(core: Core, boundary: Boundary, pc: Address, instructions: u64) -> Sample {
        let mut counts = [0; 2]; counts[core.index()] = instructions;
        let state = Context {
            core, pc, registers: std::array::from_fn(|index| Word(if index == 0 { 0 } else { 0x100 + index as u32 })),
            sp: Address(core.layout().stack + 1024), lr: Address(match core { Core::Primary => 0x453f0, Core::Secondary => 0xd53b0 }),
            cpsr: Word(0x53), spsr: Word(0xdeadbeef), counts, identities: [Word(0), Word(1)], tcm: [[Word(0); 2]; 2],
        };
        Sample { hit: 0, boundary, context: state, code_word: Word(0), watch_cursor: 0, views: Vec::new(), kernels: [synthetic_kernel(Core::Primary), synthetic_kernel(Core::Secondary)] }
    }
    fn synthetic_entry_and_wait(core: Core) -> [Sample; 2] {
        let entry = synthetic_sample(core, Boundary::Entry, Address(ENTRY), 10);
        let mut wait = synthetic_sample(core, Boundary::Instruction, WFI, 12);
        wait.context.sp.0 -= 4; wait.context.registers[11] = Word(wait.context.sp.0);
        [entry, wait]
    }
    fn synthetic_irq(core: Core, wait: &Sample, scheduler_return: bool) -> [Sample; 3] {
        let source = match core { Core::Primary => 0x40000, Core::Secondary => 0xd0000 };
        let mut entry = synthetic_sample(core, Boundary::IrqEntry, Address(source + 0x204), 20);
        entry.context.registers = wait.context.registers; entry.context.cpsr = Word(0x1d2);
        entry.context.spsr = wait.context.cpsr; entry.context.lr = Address(RESUME.0 + 4);
        let frame_words = [
            wait.context.cpsr.0, wait.context.registers[0].0, wait.context.registers[1].0,
            wait.context.registers[2].0, wait.context.registers[3].0, wait.context.registers[12].0, wait.context.lr.0, RESUME.0,
        ];
        let mut frame = synthetic_sample(core, Boundary::IrqFrame, Address(source + 0x234), 30);
        frame.context.cpsr = Word(0xd3);
        frame.context.sp = Address(wait.context.sp.0 - 32);
        let mut stack = vec![0; 1024];
        let offset = (frame.context.sp.0 - core.layout().stack) as usize;
        for (index, value) in frame_words.into_iter().enumerate() { stack[offset + 4 * index..offset + 4 * index + 4].copy_from_slice(&value.to_le_bytes()); }
        frame.views.push(View {
            object: Object::Stack, core, physical: Address(core.layout().stack), bytes: stack,
            aliases: std::array::from_fn(|_| std::array::from_fn(|_| Digest("0".repeat(64)))),
        });
        let mut returned = synthetic_sample(core, Boundary::IrqReturn, Address(source + if scheduler_return { 0x37d8 } else { 0x364 }), 40);
        returned.context.cpsr = Word(0xd3);
        returned.context.sp = Address(wait.context.sp.0 - 28); returned.context.spsr = wait.context.cpsr;
        returned.views = frame.views.iter().map(|view| View {
            object: view.object, core: view.core, physical: view.physical, bytes: view.bytes.clone(), aliases: view.aliases.clone(),
        }).collect();
        [entry, frame, returned]
    }
    fn feed_through_repeated_wait(producer: &mut Producer, core: Core, scheduler_return: bool) {
        let [entry, wait] = synthetic_entry_and_wait(core);
        producer.observe(&entry).unwrap(); producer.observe(&wait).unwrap();
        producer.fetched(core, WFI, 13);
        for sample in synthetic_irq(core, &wait, scheduler_return) { producer.observe(&sample).unwrap(); }
        let mut resume = wait;
        resume.context.pc = RESUME; resume.context.counts[core.index()] = 41;
        producer.observe(&resume).unwrap();
        resume.context.pc = WFI; resume.context.counts[core.index()] = 42;
        producer.observe(&resume).unwrap();
    }

    #[test]
    fn both_cores_need_irq_return_resume_and_executed_repeated_wait() {
        let mut producer = Producer::new(WFI, RESUME);
        feed_through_repeated_wait(&mut producer, Core::Primary, false);
        producer.fetched(Core::Primary, WFI, 43);
        assert!(!producer.complete());
        feed_through_repeated_wait(&mut producer, Core::Secondary, true);
        assert!(!producer.complete());
        producer.fetched(Core::Secondary, WFI, 43);
        assert!(producer.complete());
    }

    #[test]
    fn a_parked_first_wait_is_not_completion() {
        let mut producer = Producer::new(WFI, RESUME);
        for core in [Core::Primary, Core::Secondary] {
            for sample in synthetic_entry_and_wait(core) { producer.observe(&sample).unwrap(); }
            producer.fetched(core, WFI, 13);
        }
        assert!(!producer.complete());
    }

    #[test]
    fn unrelated_irq_status_does_not_supply_idle_wakeup() {
        let mut producer = Producer::new(WFI, RESUME);
        let [entry, wait] = synthetic_entry_and_wait(Core::Primary);
        producer.observe(&entry).unwrap(); producer.observe(&wait).unwrap(); producer.fetched(Core::Primary, WFI, 13);
        let [mut irq, frame, returned] = synthetic_irq(Core::Primary, &wait, false);
        irq.context.spsr = Word(0x93);
        for sample in [irq, frame, returned] { producer.observe(&sample).unwrap(); }
        let mut resumed = wait;
        resumed.context.pc = RESUME; producer.observe(&resumed).unwrap();
        resumed.context.pc = WFI; producer.observe(&resumed).unwrap(); producer.fetched(Core::Primary, WFI, 50);
        feed_through_repeated_wait(&mut producer, Core::Secondary, false); producer.fetched(Core::Secondary, WFI, 43);
        assert!(!producer.complete());
    }

    #[test]
    fn idle_execution_with_another_current_task_refuses() {
        let mut producer = Producer::new(WFI, RESUME);
        let [mut entry, _] = synthetic_entry_and_wait(Core::Primary);
        entry.kernels[0].current = Address(entry.kernels[0].node.0 + 56);
        assert!(producer.observe(&entry).is_err());
    }

    #[test]
    fn corrupt_source_frame_refuses_before_return() {
        let mut producer = Producer::new(WFI, RESUME);
        let [entry, wait] = synthetic_entry_and_wait(Core::Primary);
        producer.observe(&entry).unwrap(); producer.observe(&wait).unwrap(); producer.fetched(Core::Primary, WFI, 13);
        let [irq, mut frame, _] = synthetic_irq(Core::Primary, &wait, false);
        producer.observe(&irq).unwrap();
        let offset = (frame.context.sp.0 - Core::Primary.layout().stack) as usize;
        frame.views[0].bytes[offset] ^= 1;
        assert!(producer.observe(&frame).is_err());
    }

    #[test]
    fn irq_frame_and_return_require_masked_svc_control() {
        for core in [Core::Primary, Core::Secondary] {
            for scheduler_return in [false, true] {
                for unmasked_boundary in [Boundary::IrqFrame, Boundary::IrqReturn] {
                    let mut producer = Producer::new(WFI, RESUME);
                    let [entry, wait] = synthetic_entry_and_wait(core);
                    producer.observe(&entry).unwrap();
                    producer.observe(&wait).unwrap();
                    producer.fetched(core, WFI, 13);
                    let [irq, mut frame, mut returned] = synthetic_irq(core, &wait, scheduler_return);
                    producer.observe(&irq).unwrap();
                    if unmasked_boundary == Boundary::IrqFrame {
                        frame.context.cpsr.0 &= !0x80;
                        assert!(producer.observe(&frame).is_err());
                    } else {
                        producer.observe(&frame).unwrap();
                        returned.context.cpsr.0 &= !0x80;
                        assert!(producer.observe(&returned).is_err());
                    }
                    assert!(!producer.complete());
                }
            }
        }
    }

    #[test]
    fn stopped_only_repeated_wait_fetch_does_not_finish() {
        let mut producer = Producer::new(WFI, RESUME);
        feed_through_repeated_wait(&mut producer, Core::Primary, false); producer.fetched(Core::Primary, WFI, 43);
        feed_through_repeated_wait(&mut producer, Core::Secondary, false);
        let raw = serde_json::json!({"enabled":true,"violation":null,"records":[
            {"source":"cpu-fetch","physical":WFI.0,"size":4,"core":1,"pc":WFI.0,"instructions":42}
        ]});
        let hit = Hit {
            site: "instruction".into(), address: format!("{:#x}", WFI.0), core: 1, instructions: 42,
            registers: BTreeMap::new(), sp: "0x0".into(), lr: "0x0".into(), cpsr: 0x53, code_word: None,
        };
        let mut cursor = 0;
        assert_eq!(inspect_watch(&raw, &mut cursor, &[hit], &mut producer).unwrap(), 1);
        assert!(!producer.complete());
        let executed = serde_json::json!({"enabled":true,"violation":null,"records":[
            {"source":"cpu-fetch","physical":WFI.0,"size":4,"core":1,"pc":WFI.0,"instructions":42},
            {"source":"cpu-fetch","physical":WFI.0,"size":4,"core":1,"pc":WFI.0,"instructions":43}
        ]});
        inspect_watch(&executed, &mut cursor, &[], &mut producer).unwrap();
        assert!(producer.complete());
    }

    #[test]
    fn journal_overflow_and_immutable_writes_refuse() {
        let mut producer = Producer::new(WFI, RESUME);
        let overflow = serde_json::json!({"enabled":true,"violation":"record limit","records":[]});
        assert!(inspect_watch(&overflow, &mut 0, &[], &mut producer).is_err());
        for source in ["cpu-write", "host-write", "model-bus-write"] {
            let write = serde_json::json!({"enabled":true,"violation":null,"records":[
                {"source":source,"physical":ENTRY,"size":4,"core":null,"pc":null,"instructions":null}
            ]});
            assert!(inspect_watch(&write, &mut 0, &[], &mut producer).is_err());
        }
        assert!(!producer.complete());
    }

    #[test]
    fn only_source_code_and_interfaces_are_immutable() {
        let mut producer = Producer::new(WFI, RESUME);
        for physical in [0x820f24, 0x8297f0, 0x8297f4, 0x828580] {
            let write = serde_json::json!({"enabled":true,"violation":null,"records":[
                {"source":"cpu-write","physical":physical,"size":4,"core":0,"pc":0x662e60,"instructions":6502046}
            ]});
            assert!(inspect_watch(&write, &mut 0, &[], &mut producer).is_ok());
        }
        for base in [0x40000, 0xd0000] {
            for offset in loader_code::MUTABLE_TABLE_OFFSETS {
                let mutable = serde_json::json!({"enabled":true,"violation":null,"records":[
                    {"source":"cpu-write","physical":base+offset,"size":4,"core":0,"pc":0x662e60,"instructions":6502046}
                ]});
                assert!(inspect_watch(&mutable, &mut 0, &[], &mut producer).is_ok());
                for adjacent in [base+offset-1, base+offset+4] {
                    let code_write = serde_json::json!({"enabled":true,"violation":null,"records":[
                        {"source":"cpu-write","physical":adjacent,"size":1,"core":0,"pc":0x662e60,"instructions":6502046}
                    ]});
                    assert!(inspect_watch(&code_write, &mut 0, &[], &mut producer).is_err());
                }
            }
        }
        for physical in [0x69cbf0, 0x69cbfc, 0x7221a0, 0x7221c4, 0x7221c8, 0x722214, 0x96bd20, 0x4dae0, 0xddaa0] {
            let write = serde_json::json!({"enabled":true,"violation":null,"records":[
                {"source":"cpu-write","physical":physical,"size":4,"core":0,"pc":0x662e60,"instructions":6502046}
            ]});
            assert!(inspect_watch(&write, &mut 0, &[], &mut producer).is_err());
        }
    }
}
