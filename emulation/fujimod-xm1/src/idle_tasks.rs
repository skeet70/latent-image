use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{CpuDataAttribution, FetchAttribution, MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::{bridge::{read, sha256, write}, guest::Press, machine::{KeyPulse, Machine, RunOutcome}, run::{self, RunDocument, ValidatedUpdate}, trace::Hit};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, num::NonZeroUsize, path::{Path, PathBuf}, process::ExitCode};

#[path = "memory_view.rs"]
mod memory_view;
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};

#[path = "loader_code.rs"]
mod loader_code;
use loader_code::immutable_loader_spans;

const USAGE: &str = "fujimod-xm1-idle-tasks [--task idle|root] --role retained|owned --run RUN --update DAT --boot-rom ROM --analysis ROOT --linked BIN --symbols SYMBOLS --contract CONTRACT --output NEW_DIR";
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
enum Object { CreationRecord, TaskNode, Current, Selected, Pool, Stack, Code, LoadedCode, InterfaceRoots, InterfaceTable, TaskDescriptor, CallbackResource, ServiceCode, TaskClasses, ResourceState, ResourceDescriptor }
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
enum Boundary { Start, Entry, Instruction, RootEntry, RootInstruction, ServiceEntry, ServiceReturn, TaskSubmit, PeriodicCallback, IrqEntry, IrqFrame, IrqReturn, PostCleanup }
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
    kernel_at(machine, maps, state, core, core.layout(), TaskScope::Idle, views)
}
fn kernel_at(machine: &Machine, maps: &[MemoryMap], state: &Context, core: Core, l: Layout, scope: TaskScope, views: &mut Vec<View>) -> Result<Kernel, String> {
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
    task.require_layout(l, scope)?;
    if task.created == Word(1) {
        views.push(view(machine, maps, state, Object::Stack, core, task.stack_base.0, 1024)?);
    }
    views.extend([creation, node, current, selected]);
    Ok(task)
}
impl Kernel {
    fn require_layout(&self, layout: Layout, scope: TaskScope) -> Result<(), String> {
        require(self.created.0 <= 1, "idle created flag is not Boolean")?;
        if self.created == Word(1) {
            let (entry, identity_error, stack_error) = match scope {
                TaskScope::Idle => (ENTRY, "idle creation/node identity differs", "idle stack differs from canonical run bounds"),
                TaskScope::Root => (ROOT_ENTRY, "root creation/node identity differs", "root stack differs from canonical run bounds"),
            };
            require(self.creation_pointer == self.creation && self.creation_id.0 == u32::from(layout.id.0) && self.entry == Address(entry), identity_error)?;
            require(self.stack_bytes == Word(1024) && self.stack_base.0 & !0x40000000 == layout.stack, stack_error)?;
        }
        Ok(())
    }
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
const ROOT_ENTRY: u32 = 0x69d160;
const ROOT_LIMIT: u32 = 0x69d2b4;
const ROOT_STACK: u32 = 0xa10a00;
const TASK_STACK_POOL: (u32, u32) = (0xa10200, 0xa5b200);
const TASK_RECORD_POOLS: [(Core, u32, u32); 2] = [
    (Core::Primary, 0x5beac, 0x5ff0c), (Core::Secondary, 0xebe6c, 0xefecc),
];
const TASK_FLAG_POOLS: [(Core, u32); 2] = [(Core::Primary, 0x5d178), (Core::Secondary, 0xed138)];
const ROOT_RESOURCES: [(u32, usize); 7] = [
    (0x96ca4c, 4), (0x828094, 4), (0x8e1f28, 4), (0x9c3500, 0x404),
    (0x89ea8c, 12), (0x5d178 + 12 * 103, 12), (0x624b4 + 16 * 103, 16),
];
#[derive(Clone, Copy)]
enum TaskScope { Idle, Root }
impl TaskScope {
    fn parse(value: &str) -> Result<Self, String> {
        match value { "idle" => Ok(Self::Idle), "root" => Ok(Self::Root), _ => Err(USAGE.into()) }
    }
}
enum ObserverBinding { Idle((Address, Address)), Root(RootBinding) }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum RootCallKind { Call, TailCall }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
struct RootCallSite { pc: Address, target: Address, kind: RootCallKind }
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RootQualification { HostAdmittedRootTask }
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RootAdmissionQualification { FiniteFixedSiteHostAdmission }
#[derive(Deserialize)]
struct RootArtifactContract {
    schema: u32, qualification: RootAdmissionQualification, entry: Address, replaced_extent: [Address; 2],
    linked_bytes: usize, linked_sha256: Digest, symbols_sha256: Digest, staged_image4_sha256: Digest,
    scratch_low_from_entry_sp: i32, execution_pcs: Vec<Address>, call_sites: Vec<RootCallSite>,
    halt_pcs: Vec<Address>, return_pcs: Vec<Address>, scenario_count: usize,
    source_dat_image4_sha256: [Digest; 2],
}
#[derive(Deserialize)]
struct RootCandidateContract {
    schema: u32, qualification: RootQualification, root_task: RootArtifactContract,
    candidate_dat_sha256: Digest, candidate_image4_sha256: Digest,
}
struct RootBinding {
    execution: BTreeSet<u32>, calls: Vec<RootCallSite>, halts: BTreeSet<u32>, returns: BTreeSet<u32>,
    service_words: BTreeSet<u32>, service_literals: BTreeSet<u32>, service_entries: BTreeSet<u32>, indirect_sites: BTreeMap<u32, TransferRegister>,
    service_halts: BTreeSet<u32>, fetch_spans: Vec<(u64, u64)>, scratch_low: i32,
}
fn root_layout() -> Layout {
    Layout { id: TaskId(104), creation: 0x4005beac + 40 * 103, node: 0x4005ff0c + 56 * 103,
        current: 0x40054760, selected: 0x40054768, stack: ROOT_STACK, table: 0x4004dae0 }
}
fn code_word(sources: &[Code], address: u32) -> Result<u32, String> {
    let physical = address & !0x40000000;
    let source = sources.iter().find(|source| physical >= source.base
        && u64::from(physical) + 4 <= u64::from(source.base) + source.bytes.len() as u64).ok_or("required instruction leaves bound canonical sources")?;
    word(&source.bytes, (physical - source.base) as usize)
}
fn direct_target(pc: u32, instruction: u32) -> Result<u32, String> {
    let displacement = (((instruction & 0x00ffffff) << 8) as i32 >> 6) as i64;
    u32::try_from(i64::from(pc) + 8 + displacement).map_err(|_| "branch target exceeds32 bits".into())
}
fn root_call_sites(sources: &[Code], execution: &BTreeSet<u32>) -> Result<Vec<RootCallSite>, String> {
    let mut calls = Vec::new();
    for &pc in execution {
        let instruction = code_word(sources, pc)?;
        if instruction & 0x0e000000 == 0x0a000000 {
            let target = direct_target(pc, instruction)?;
            if !(ROOT_ENTRY..ROOT_LIMIT).contains(&target) {
                calls.push(RootCallSite { pc: Address(pc), target: Address(target),
                    kind: if instruction & 0x01000000 != 0 { RootCallKind::Call } else { RootCallKind::TailCall } });
            }
        }
    }
    Ok(calls)
}
fn returns_to_caller(instruction: u32) -> bool {
    instruction & 0x0fffffff == 0x012fff1e || instruction & 0x0fffffff == 0x01a0f00e
        || instruction & 0x0fff8000 == 0x08bd8000
}
fn switch_bound(sources: &[Code], pc: u32, instruction: u32) -> Result<u32, String> {
    let index = (instruction & 15) as u8;
    let comparison = match instruction >> 28 {
        9 => code_word(sources, pc.checked_sub(4).ok_or("switch comparison underflow")?)?,
        14 => {
            let guard = code_word(sources, pc.checked_sub(4).ok_or("switch guard underflow")?)?;
            require(guard & 0xff000000 == 0x8a000000, "retained switch lacks source unsigned upper-bound guard")?;
            code_word(sources, pc.checked_sub(8).ok_or("switch comparison underflow")?)?
        }
        _ => return Err(format!("retained switch condition is unresolved at0x{pc:x}")),
    };
    require(comparison & 0xfff0f000 == 0xe3500000 && ((comparison >> 16) & 15) as u8 == index,
        "retained switch comparison is unresolved")?;
    let bound = (comparison & 255).rotate_right(2 * ((comparison >> 8) & 15));
    require(bound <= 1024, "retained switch bound exceeds source admission limit")?;
    Ok(bound)
}
#[derive(Clone, Copy)]
#[repr(u8)]
enum TransferRegister { R0, R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12, Sp, Lr, Pc }
impl TransferRegister {
    fn from_instruction(instruction: u32) -> Self {
        match instruction & 15 {
            0 => Self::R0, 1 => Self::R1, 2 => Self::R2, 3 => Self::R3, 4 => Self::R4, 5 => Self::R5, 6 => Self::R6,
            7 => Self::R7, 8 => Self::R8, 9 => Self::R9, 10 => Self::R10, 11 => Self::R11, 12 => Self::R12,
            13 => Self::Sp, 14 => Self::Lr, _ => Self::Pc,
        }
    }
    fn value(self, state: &Context) -> u32 {
        match self { Self::Sp => state.sp.0, Self::Lr => state.lr.0, Self::Pc => state.pc.0.wrapping_add(8), _ => state.registers[self as usize].0 }
    }
}
struct RetainedCode { words: BTreeSet<u32>, literals: BTreeSet<u32>, indirect_sites: BTreeMap<u32, TransferRegister>, halts: BTreeSet<u32> }
fn retained_function_words(sources: &[Code], entries: &BTreeSet<u32>) -> Result<RetainedCode, String> {
    let mut pending: Vec<u32> = entries.iter().copied().collect();
    let mut words = BTreeSet::new();
    let mut literals = BTreeSet::new();
    let mut indirect_sites = BTreeMap::new();
    let mut halts = BTreeSet::new();
    while let Some(pc) = pending.pop() {
        if !words.insert(pc) { continue; }
        require(words.len() <= 262_144, "retained service source graph exceeds admission bound")?;
        let instruction = code_word(sources, pc)?;
        let conditional = instruction >> 28 != 14;
        if instruction & 0x0f7f0000 == 0x051f0000 {
            let base = pc.checked_add(8).ok_or("service literal PC overflow")?;
            let displacement = instruction & 0xfff;
            let address = if instruction & 0x00800000 != 0 { base.checked_add(displacement) } else { base.checked_sub(displacement) }
                .ok_or("service literal address overflow")?;
            code_word(sources, address)?;
            let loader_hole = [0x40000, 0xd0000].into_iter().any(|base| (0..7).any(|index| address == base + 0x3a0 + 16 * index));
            if !loader_hole { literals.insert(address); }
        }
        if instruction & 0x0e000000 == 0x0a000000 {
            require(instruction >> 28 != 15, "retained Thumb transfer requires a source extent")?;
            if instruction == 0xeafffffe { halts.insert(pc); }
            let target = direct_target(pc, instruction)?;
            if !(ROOT_ENTRY..ROOT_LIMIT).contains(&target) { pending.push(target); }
            if instruction & 0x01000000 != 0 || conditional { pending.push(pc + 4); }
        } else if returns_to_caller(instruction) || instruction & 0x0e108000 == 0x08108000 {
            if conditional { pending.push(pc + 4); }
        } else if instruction & 0x0ffffff0 == 0x012fff30 {
            indirect_sites.insert(pc, TransferRegister::from_instruction(instruction));
            pending.push(pc + 4);
        } else if instruction & 0x0ffffff0 == 0x012fff10 || instruction & 0x0ffffff0 == 0x01a0f000 {
            indirect_sites.insert(pc, TransferRegister::from_instruction(instruction));
            if conditional { pending.push(pc + 4); }
        } else if instruction & 0x0ffffff0 == 0x008ff100 {
            let bound = switch_bound(sources, pc, instruction)?;
            for index in 0..=bound {
                let address = pc.checked_add(8 + 4 * index).ok_or("retained switch extent overflow")?;
                require(code_word(sources, address)? & 0xff000000 == 0xea000000, "retained switch table is not canonical A32 branches")?;
                pending.push(address);
            }
            if conditional { pending.push(pc + 4); }
        } else if instruction & 0x0ffffff0 == 0x079ff100 {
            let bound = switch_bound(sources, pc, instruction)?;
            for index in 0..=bound {
                let address = pc.checked_add(8 + 4 * index).ok_or("retained switch extent overflow")?;
                let target = code_word(sources, address)?;
                require(target % 4 == 0, "retained switch target is not A32")?;
                literals.insert(address);
                pending.push(target & !0x40000000);
            }
            if conditional { pending.push(pc + 4); }
        } else if instruction & 0x0c10f000 == 0x0410f000 {
            let base = (instruction >> 16) & 15;
            if base == 15 && instruction & 0x02000000 == 0 {
                let base = pc.checked_add(8).ok_or("retained PC literal overflow")?;
                let displacement = instruction & 0xfff;
                let address = if instruction & 0x00800000 != 0 { base.checked_add(displacement) } else { base.checked_sub(displacement) }
                    .ok_or("retained PC literal address overflow")?;
                let target = code_word(sources, address)?;
                require(target % 4 == 0, "retained literal tail target is not A32")?;
                pending.push(target & !0x40000000);
            } else if base != 13 && ![0x436e8, 0x43758, 0xd36e8, 0xd3758].contains(&pc) {
                return Err(format!("retained indirect memory transfer extent is unresolved at0x{pc:x}"));
            }
            if conditional { pending.push(pc + 4); }
        } else if instruction & 0x0c00f000 == 0x0000f000 && ![8, 9, 10, 11].contains(&((instruction >> 21) & 15)) {
            return Err(format!("retained computed PC extent is unresolved at0x{pc:x}"));
        } else {
            pending.push(pc + 4);
        }
    }
    literals.retain(|pc| !words.contains(pc));
    Ok(RetainedCode { words, literals, indirect_sites, halts })
}
fn bind_root_artifact(role: Role, linked: &[u8], symbols: &[u8], contract: &[u8], update: &Digest, sources: &mut [Code]) -> Result<RootBinding, String> {
    let binding: RootCandidateContract = serde_json::from_slice(contract).map_err(|error| error.to_string())?;
    let artifact = &binding.root_task;
    require(binding.schema == 1 && matches!(binding.qualification, RootQualification::HostAdmittedRootTask)
        && artifact.schema == 1 && matches!(artifact.qualification, RootAdmissionQualification::FiniteFixedSiteHostAdmission)
        && artifact.entry == Address(ROOT_ENTRY) && artifact.replaced_extent == [Address(ROOT_ENTRY), Address(ROOT_LIMIT)]
        && !linked.is_empty() && linked.len() <= (ROOT_LIMIT - ROOT_ENTRY) as usize && linked.len() % 4 == 0
        && artifact.linked_bytes == linked.len() && artifact.linked_sha256 == digest(linked)
        && artifact.symbols_sha256 == digest(symbols)
        && artifact.source_dat_image4_sha256 == [Digest(ORIGINAL_DAT.into()), Digest(ORIGINAL_IMAGE.into())]
        && (-1024..=0).contains(&artifact.scratch_low_from_entry_sp) && artifact.scenario_count != 0,
        "root artifact identity, qualification or extent differs")?;
    let execution: BTreeSet<u32> = artifact.execution_pcs.iter().map(|pc| pc.0).collect();
    require(execution.len() == artifact.execution_pcs.len() && execution.contains(&ROOT_ENTRY)
        && execution.iter().all(|pc| *pc >= ROOT_ENTRY && *pc < ROOT_ENTRY + linked.len() as u32 && pc % 4 == 0),
        "root execution PCs leave linked extent or repeat")?;
    let image = sources.iter_mut().find(|source| source.base == IMAGE_BASE).ok_or("canonical image4 absent")?;
    let offset = (ROOT_ENTRY - IMAGE_BASE) as usize;
    let original = image.bytes.get(offset..offset + (ROOT_LIMIT - ROOT_ENTRY) as usize).ok_or("root carrier leaves image4")?.to_vec();
    for bytes in image.bytes[offset..offset + original.len()].chunks_exact_mut(4) { bytes.copy_from_slice(&0xe320f000u32.to_le_bytes()); }
    image.bytes[offset..offset + linked.len()].copy_from_slice(linked);
    require(digest(&image.bytes) == artifact.staged_image4_sha256 && artifact.staged_image4_sha256 == binding.candidate_image4_sha256,
        "root-only staged image4 identity differs")?;
    require(root_call_sites(sources, &execution)? == artifact.call_sites, "root call sites differ from installed instruction words")?;
    let halts: BTreeSet<u32> = artifact.halt_pcs.iter().map(|pc| pc.0).collect();
    let returns: BTreeSet<u32> = artifact.return_pcs.iter().map(|pc| pc.0).collect();
    for &pc in &halts { require(execution.contains(&pc) && code_word(sources, pc)? == 0xeafffffe, "root halt contract differs")?; }
    for &pc in &returns { require(execution.contains(&pc) && returns_to_caller(code_word(sources, pc)?), "root return contract differs")?; }
    let (execution, calls, halts, returns) = match role {
        Role::Owned => {
            require(*update == binding.candidate_dat_sha256, "owned root DAT differs from staging contract")?;
            (execution, artifact.call_sites.clone(), halts, returns)
        }
        Role::Retained => {
            require(update.0 == ORIGINAL_DAT, "retained root requires canonical original DAT")?;
            sources.iter_mut().find(|source| source.base == IMAGE_BASE).ok_or("canonical image4 absent")?.bytes[offset..offset + original.len()].copy_from_slice(&original);
            let execution = (ROOT_ENTRY..ROOT_LIMIT).step_by(4).collect();
            let calls = root_call_sites(sources, &execution)?;
            (execution, calls, BTreeSet::from([0x69d1d0, 0x69d1e0, 0x69d278]), BTreeSet::from([0x69d2b0]))
        }
    };
    let mut service_entries: BTreeSet<u32> = calls.iter().map(|site| site.target.0).collect();
    service_entries.extend([0x69c9f8, 0x69cc00, 0x69cc18, 0x725b74, 0x72835c, 0x726478,
        0x725dd8, 0x4d434, 0x41140, 0x4d1a4, 0x453f0, 0x43dd0, 0x43688, 0x412bc, 0x4118c, 0x72337c, 0x43728]);
    let services = retained_function_words(sources, &service_entries)?;
    Ok(RootBinding { execution, calls, halts, returns, service_words: services.words, service_literals: services.literals, service_entries,
        indirect_sites: services.indirect_sites, service_halts: services.halts, fetch_spans: Vec::new(),
        scratch_low: if role == Role::Retained { -16 } else { artifact.scratch_low_from_entry_sp } })
}

fn instruction_spans(words: &BTreeSet<u32>) -> Vec<(u64, u64)> {
    let mut spans: Vec<(u64, u64)> = Vec::new();
    for &pc in words {
        if let Some((_, high)) = spans.last_mut().filter(|(_, high)| *high == u64::from(pc)) {
            *high += 4;
        } else { spans.push((u64::from(pc), u64::from(pc) + 4)); }
    }
    spans
}
fn root_sites(binding: &RootBinding) -> BTreeMap<u32, String> {
    let mut sites = BTreeMap::new();
    for pc in (ROOT_ENTRY..ROOT_LIMIT).step_by(4) {
        sites.insert(pc, if pc == ROOT_ENTRY { "root-entry" } else { "root-instruction" }.into());
    }
    for &pc in &binding.service_entries { sites.insert(pc, "service-entry".into()); }
    for &pc in binding.indirect_sites.keys() { sites.insert(pc, "service-entry".into()); }
    for &pc in &binding.service_halts { sites.insert(pc, "service-entry".into()); }
    for site in &binding.calls {
        if site.kind == RootCallKind::Call { sites.insert(site.pc.0 + 4, "service-return".into()); }
    }
    for pc in (0x69cc18..0x69cc6c).step_by(4) { sites.insert(pc, "periodic-callback".into()); }
    sites.insert(0x725dd8, "task-submit".into());
    for pc in [0x724924, 0x725f08, 0x725fa8, 0x72614c, 0x726150, 0x72615c, 0x726160, 0x726198, 0x72619c,
        0x7261a4, 0x7261a8, 0x7261b8, 0x7261bc, 0x7261c4, 0x7261c8, 0x7261cc,
        0x454e8, 0x436e8, 0x453f4] { sites.insert(pc, "service-entry".into()); }
    for pc in [0x725f0c, 0x725fac] { sites.insert(pc, "service-return".into()); }
    for (pc, name) in [(0x40204, "irq-entry"), (0xd0204, "irq-entry"), (0x40234, "irq-frame"),
        (0xd0234, "irq-frame"), (0x40364, "irq-return"), (0xd0364, "irq-return"),
        (0x437d8, "irq-return"), (0xd37d8, "irq-return")] { sites.insert(pc, name.into()); }
    sites
}
fn root_boundary(pc: u32, binding: &RootBinding) -> Result<Boundary, String> {
    let pc = pc & !0x40000000;
    if pc == ROOT_ENTRY { return Ok(Boundary::RootEntry); }
    if binding.calls.iter().any(|site| site.kind == RootCallKind::Call && site.pc.0 + 4 == pc) { return Ok(Boundary::ServiceReturn); }
    if (ROOT_ENTRY..ROOT_LIMIT).contains(&pc) { return Ok(Boundary::RootInstruction); }
    if (0x69cc18..0x69cc6c).contains(&pc) { return Ok(Boundary::PeriodicCallback); }
    if pc == 0x725dd8 { return Ok(Boundary::TaskSubmit); }
    if [0x725f0c, 0x725fac].contains(&pc) { return Ok(Boundary::ServiceReturn); }
    if [0x40204, 0xd0204].contains(&pc) { return Ok(Boundary::IrqEntry); }
    if [0x40234, 0xd0234].contains(&pc) { return Ok(Boundary::IrqFrame); }
    if [0x40364, 0xd0364, 0x437d8, 0xd37d8].contains(&pc) { return Ok(Boundary::IrqReturn); }
    if binding.service_entries.contains(&pc) || binding.indirect_sites.contains_key(&pc) || binding.service_halts.contains(&pc) || (0x72612c..0x72627c).contains(&pc)
        || [0x724924, 0x725f08, 0x725fa8, 0x454e8, 0x436e8, 0x453f4].contains(&pc) { return Ok(Boundary::ServiceEntry); }
    Err("unknown root observation boundary".into())
}
fn callback_task_views(machine: &Machine, maps: &[MemoryMap], state: &Context, current: Address, views: &mut Vec<View>) -> Result<(), String> {
    let (_, creations, nodes) = TASK_RECORD_POOLS[state.core.index()];
    let physical = current.0 & !0x40000000;
    let offset = physical.checked_sub(nodes).ok_or("callback current task leaves source node pool")?;
    require(offset % 56 == 0 && offset / 56 < 120, "callback current task has invalid source node index")?;
    let index = offset / 56;
    let node = view(machine, maps, state, Object::TaskNode, state.core, current.0, 56)?;
    let creation_address = 0x40000000 | (creations + 40 * index);
    require(word(&node.bytes, 8)? == creation_address, "callback current task creation pointer differs")?;
    let creation = view(machine, maps, state, Object::CreationRecord, state.core, creation_address, 40)?;
    require(word(&creation.bytes, 32)? == index + 1 && word(&creation.bytes, 36)? == 1, "callback current task is not a live source creation")?;
    let stack = word(&creation.bytes, 20)? & !0x40000000;
    let size = word(&creation.bytes, 16)?;
    let high = stack.checked_add(size).ok_or("callback stack extent overflow")?;
    let cursor = view(machine, maps, state, Object::ResourceState, state.core, 0x89ea98, 4)?;
    let cursor_address = word(&cursor.bytes, 0)? & !0x40000000;
    require((TASK_STACK_POOL.0..=TASK_STACK_POOL.1).contains(&cursor_address)
        && size != 0 && stack >= TASK_STACK_POOL.0 && high <= cursor_address && size % 4 == 0
        && (stack..=high).contains(&(state.sp.0 & !0x40000000)),
        "callback stack lacks complete allocated task-pool coverage; IRQ/TCM stack requires a source fact")?;
    views.push(view(machine, maps, state, Object::Stack, state.core, stack, size as usize)?);
    views.extend([node, creation, cursor]);
    Ok(())
}
fn callback_frame(sample: &Sample, address: u32) -> Result<[u32; 2], String> {
    let physical = address & !0x40000000;
    let stack = sample.views.iter().find(|view| matches!(view.object, Object::Stack) && view.core == sample.context.core
        && physical >= view.physical.0 && u64::from(physical) + 8 <= u64::from(view.physical.0) + view.bytes.len() as u64)
        .ok_or("callback private frame has no live allocated stack view")?;
    let offset = (physical - stack.physical.0) as usize;
    Ok([word(&stack.bytes, offset)?, word(&stack.bytes, offset + 4)?])
}
fn submission_views(machine: &Machine, maps: &[MemoryMap], state: &Context, task: &SubmittedTask, views: &mut Vec<View>) -> Result<(), String> {
    let index = u32::from(task.id.0) - 1;
    let (_, creations, nodes) = TASK_RECORD_POOLS[task.domain.index()];
    let creation = view(machine, maps, state, Object::CreationRecord, task.domain, creations + 40 * index, 40)?;
    let node = view(machine, maps, state, Object::TaskNode, task.domain, nodes + 56 * index, 56)?;
    let (_, flags) = TASK_FLAG_POOLS[task.domain.index()];
    views.push(view(machine, maps, state, Object::ResourceState, task.domain, flags + 12 * index, 12)?);
    if word(&creation.bytes, 36)? == 1 {
        let low = word(&creation.bytes, 20)? & !0x40000000;
        let length = word(&creation.bytes, 16)?;
        require(length != 0 && low >= TASK_STACK_POOL.0 && low.checked_add(length).is_some_and(|high| high <= TASK_STACK_POOL.1),
            "submitted task stack leaves source allocator pool")?;
        views.push(view(machine, maps, state, Object::Stack, task.domain, low, length as usize)?);
    }
    views.extend([creation, node]);
    Ok(())
}
fn root_sample(machine: &Machine, maps: &[MemoryMap], sources: &[Code],
    hit: usize, state: Context, watch_cursor: usize, boundary: Boundary, callback_context: bool) -> Result<Sample, String> {
    let mut views = Vec::new();
    let kernels = [
        kernel_at(machine, maps, &state, Core::Primary, root_layout(), TaskScope::Root, &mut views)?,
        kernel(machine, maps, &state, Core::Secondary, &mut views)?,
    ];
    if callback_context { callback_task_views(machine, maps, &state, kernels[state.core.index()].current, &mut views)?; }
    let roots = view(machine, maps, &state, Object::InterfaceRoots, Core::Primary, 0x96bd20, 8)?;
    for core in [Core::Primary, Core::Secondary] {
        require(word(&roots.bytes, 4 * core.index())? == core.layout().table, "installed root interface differs")?;
        views.push(view(machine, maps, &state, Object::InterfaceTable, core, core.layout().table, 0x162c)?);
    }
    views.push(roots);
    views.push(view(machine, maps, &state, Object::Code, Core::Primary, ROOT_ENTRY, (ROOT_LIMIT - ROOT_ENTRY) as usize)?);
    views.push(view(machine, maps, &state, Object::TaskClasses, Core::Primary, 0x7cad9c, 104 * 4)?);
    for (address, length) in ROOT_RESOURCES {
        views.push(view(machine, maps, &state, Object::CallbackResource, Core::Primary, address, length)?);
    }
    views.push(view(machine, maps, &state, Object::ResourceState, Core::Primary, 0x93e6f4, 104 * 4)?);
    views.push(view(machine, maps, &state, Object::ResourceState, Core::Primary, 0x89ea98, 4)?);
    if boundary == Boundary::Start {
        views.push(view(machine, maps, &state, Object::Pool, Core::Primary, TASK_STACK_POOL.0, (TASK_STACK_POOL.1 - TASK_STACK_POOL.0) as usize)?);
        for (core, creations, nodes) in TASK_RECORD_POOLS {
            views.push(view(machine, maps, &state, Object::Pool, core, creations, 120 * 40)?);
            views.push(view(machine, maps, &state, Object::Pool, core, nodes, 120 * 56)?);
        }
        for (core, flags) in TASK_FLAG_POOLS { views.push(view(machine, maps, &state, Object::Pool, core, flags, 120 * 12)?); }
    }
    if boundary == Boundary::TaskSubmit {
        views.push(view(machine, maps, &state, Object::TaskDescriptor, state.core, state.registers[0].0, 28)?);
        let caller = state.lr.0.checked_sub(4).ok_or("task-submit LR underflow")?;
        let instruction = code_word(sources, caller)?;
        require(instruction & 0x0f000000 == 0x0b000000 && direct_target(caller & !0x40000000, instruction)? == 0x725dd8,
            "task-submit lacks canonical direct caller")?;
    }
    if state.pc.0 & !0x40000000 == 0x724924 {
        views.push(view(machine, maps, &state, Object::ResourceDescriptor, state.core, state.registers[1].0, 12)?);
    }
    for (pc, length) in [(0x725f08, 8), (0x725fa8, 24)] {
        if state.pc.0 & !0x40000000 == pc {
            views.push(view(machine, maps, &state, Object::ResourceDescriptor, state.core, state.registers[1].0, length)?);
        }
    }
    let expected = code_word(sources, state.pc.0)?;
    let observed = view(machine, maps, &state, Object::Code, state.core, state.pc.0, 4)?;
    require(word(&observed.bytes, 0)? == expected, "root boundary instruction differs from installed source")?;
    views.push(observed);
    Ok(Sample { hit, boundary, context: state, code_word: Word(expected), watch_cursor, views, kernels })
}
fn merged_spans(mut spans: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    spans.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (low, high) in spans {
        if let Some((_, end)) = merged.last_mut().filter(|(_, end)| *end >= low) {
            *end = (*end).max(high);
        } else { merged.push((low, high)); }
    }
    merged
}
fn root_watch_ranges(binding: &mut RootBinding) -> Result<Vec<WatchRange>, String> {
    let mut writes = vec![(0x40000, 0x4dae0), (0xd0000, 0xddaa0), (u64::from(IMAGE_BASE), u64::from(INITIAL_IMAGE_VIEW_END))];
    writes.extend(INTERFACES);
    for core in [Core::Primary, Core::Secondary] {
        let l = if core == Core::Primary { root_layout() } else { core.layout() };
        for (base, size) in [(l.current, 4), (l.selected, 4)] {
            let start = u64::from(base & !0x40000000);
            writes.push((start, start + size));
        }
    }
    writes.push((u64::from(TASK_STACK_POOL.0), u64::from(TASK_STACK_POOL.1)));
    for (_, creations, nodes) in TASK_RECORD_POOLS {
        writes.push((u64::from(creations), u64::from(creations) + 120 * 40));
        writes.push((u64::from(nodes), u64::from(nodes) + 120 * 56));
    }
    for (_, flags) in TASK_FLAG_POOLS { writes.push((u64::from(flags), u64::from(flags) + 120 * 12)); }
    writes.push((0x89ea98, 0x89ea9c));
    writes.extend(ROOT_RESOURCES.into_iter().chain([(0x93e6f4, 104 * 4), (0x7cad9c, 104 * 4)])
        .map(|(address, length)| (u64::from(address), u64::from(address) + length as u64)));
    let writes = merged_spans(writes);
    let mut fetches = vec![
        (u64::from(ROOT_ENTRY), u64::from(ROOT_LIMIT)), (0x725dd8, 0x725ddc),
        (0x69cc18, 0x69cc6c), (0x7261c8, 0x7261d0), (0x436e8, 0x436ec),
        (0x453f0, 0x453f8), (0x43668, 0x43670), (0x43728, 0x43730), (0x437a4, 0x437ac),
    ];
    fetches.extend([0x40204, 0xd0204, 0x40234, 0xd0234, 0x40364, 0xd0364, 0x437d8, 0xd37d8]
        .into_iter().map(|pc| (pc, pc + 4)));
    let fetches = merged_spans(fetches);
    let endpoints: BTreeSet<u64> = writes.iter().chain(&fetches).flat_map(|&(low, high)| [low, high]).collect();
    let endpoints: Vec<u64> = endpoints.into_iter().collect();
    let mut classes = 0;
    let mut previous = None;
    for pair in endpoints.windows(2) {
        let mask = u8::from(writes.iter().any(|&(low, high)| low <= pair[0] && pair[1] <= high))
            | (u8::from(fetches.iter().any(|&(low, high)| low <= pair[0] && pair[1] <= high)) << 1);
        if mask != 0 && previous != Some((pair[0], mask)) { classes += 1; }
        previous = if mask == 0 { None } else { Some((pair[1], mask)) };
    }
    require(classes <= 64, "normalized root watch exceeds64 classes")?;
    let mut ranges: Vec<WatchRange> = writes.into_iter().map(|(start, end)| WatchRange { start, end, kinds: WatchKinds::WRITES }).collect();
    ranges.extend(fetches.iter().map(|&(start, end)| WatchRange { start, end, kinds: WatchKinds::CPU_FETCH }));
    binding.fetch_spans = fetches;
    Ok(ranges)
}
fn root_immutable(binding: &RootBinding, physical: u64, size: u64) -> bool {
    let end = physical.saturating_add(size);
    LOADER_A_CODE.iter().chain(&LOADER_B_CODE).chain(&INTERFACES).any(|&(low, high)| physical < high && low < end)
        || physical < u64::from(ROOT_LIMIT) && u64::from(ROOT_ENTRY) < end
        || physical < 0x7caf3c && 0x7cad9c < end
        || binding.service_words.range((physical.saturating_sub(3) as u32)..).next().is_some_and(|pc| u64::from(*pc) < end)
        || binding.service_literals.range((physical.saturating_sub(3) as u32)..).next().is_some_and(|pc| u64::from(*pc) < end)
}
struct RootService {
    site: RootCallSite, caller: Context, entered: bool, executed: bool, return_executed: bool,
}
struct RootIrq { entry: Context, resume: Address, frame: Option<(Address, Address)>, entry_executed: bool, return_executed: bool, returned: bool }
struct RootCallback { caller: Context, counter: u32, returned: bool, entry_executed: bool, epilogue_executed: bool, after_cleanup: bool }
#[derive(Clone, Copy, Eq, PartialEq)]
enum TaskFlagAttribute { Keep, ClearOnWait }
impl TaskFlagAttribute {
    fn word(self) -> u32 { match self { Self::Keep => 0, Self::ClearOnWait => 4 } }
}
struct SubmittedTask { id: TaskId, name: Address, priority: Word, stack_bytes: Word, entry: Address, argument: Word, flags: TaskFlagAttribute, domain: Core }
impl SubmittedTask {
    fn read(sample: &Sample) -> Result<Self, String> {
        let input = sample.views.iter().find(|view| matches!(view.object, Object::TaskDescriptor)).ok_or("root task descriptor absent")?;
        let id = TaskId(u16::try_from(word(&input.bytes, 0)?).map_err(|_| "submitted task id exceeds16 bits")?);
        require((1..=104).contains(&id.0), "submitted task id leaves source class table")?;
        let classes = sample.views.iter().find(|view| matches!(view.object, Object::TaskClasses)).ok_or("submitted task class table absent")?;
        let domain = match word(&classes.bytes, 4 * (usize::from(id.0) - 1))? {
            1 => Core::Primary, 2 => Core::Secondary, _ => return Err("submitted task domain is not installed".into()),
        };
        Ok(Self { id, name: Address(word(&input.bytes, 4)?), priority: Word(word(&input.bytes, 8)?),
            stack_bytes: Word(word(&input.bytes, 12)?), entry: Address(word(&input.bytes, 16)?), argument: Word(word(&input.bytes, 20)?),
            flags: if word(&input.bytes, 24)? == 0xa5f0a5f0 { TaskFlagAttribute::Keep } else { TaskFlagAttribute::ClearOnWait }, domain })
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum SubmissionStage { Submitted, FlagsEntered, FlagsObserved, ConstructorEntered(Address), Constructed }
struct RootSubmission { caller: Context, executed: bool, task: SubmittedTask, stage: SubmissionStage }
struct RootProducer {
    entry: Option<Context>, entry_executed: bool, sequence: Vec<u32>, action: usize, pending: Option<RootService>,
    resources: u32, submissions: Vec<TaskId>, submission: Option<RootSubmission>, cleanup_step: usize, cleanup_return: bool,
    epilogue: bool, handback: Option<Context>, transfer: Option<(Address, u64)>, transfer_executed: bool,
    irq: Option<RootIrq>, irq_cycles: usize, callback: Option<RootCallback>, post_cleanup_callbacks: usize, complete: bool,
}
impl RootProducer {
    fn new() -> Self {
        let mut sequence = vec![0x649624, 0x7284ec, 0x72872c, 0x725b54];
        sequence.extend(std::iter::repeat_n(0x7247ac, 97));
        sequence.extend([0x69ca24, 0x676054, 0x662e48, 0x665224, 0x649070, 0x6490dc,
            0x6a9354, 0x6ac2b4, 0x69f204, 0x656788, 0x7262cc, 0x724cbc, 0x6a5634, 0x6a59b8, 0x69cfa4]);
        Self { entry: None, entry_executed: false, sequence, action: 0, pending: None, resources: 0, submissions: Vec::new(),
            submission: None, cleanup_step: 0, cleanup_return: false, epilogue: false, handback: None, transfer: None,
            transfer_executed: false, irq: None, irq_cycles: 0, callback: None, post_cleanup_callbacks: 0, complete: false }
    }
    fn deadline_error(&self) -> &'static str {
        if self.entry.is_none() { "root entry was not observed before deadline" }
        else if self.resources != 97 { "root97-loop complete call/result sequence was not observed before deadline" }
        else if self.submissions.len() != 8 || self.submission.is_some() { "root worker submission and return sequence was not observed before deadline" }
        else if !self.transfer_executed && !self.epilogue { "root cleanup did not establish actual termination transfer or returning-63 handback before deadline" }
        else if self.post_cleanup_callbacks == 0 { "executed and restored periodic callback after root cleanup was not observed before deadline" }
        else if self.irq_cycles == 0 { "root IRQ preemption and restoration were not observed before deadline" }
        else { "root concrete post-cleanup task state was not observed before deadline" }
    }
    fn fetched(&mut self, core: Core, pc: u32, clock: u64) {
        let pc = pc & !0x40000000;
        if let Some(callback) = &mut self.callback {
            if core == callback.caller.core && pc == 0x69cc18 { callback.entry_executed = true; }
            if core == callback.caller.core && pc == 0x69cc68 { callback.epilogue_executed = true; }
        }
        if let Some(submission) = &mut self.submission {
            if core == submission.caller.core && pc == 0x725dd8 { submission.executed = true; }
        }
        if core != Core::Primary { return; }
        if pc == ROOT_ENTRY { self.entry_executed = true; }
        if let Some(irq) = &mut self.irq {
            if pc == 0x40204 { irq.entry_executed = true; }
            if irq.returned && [0x40364, 0x437d8].contains(&pc) { irq.return_executed = true; }
        }
        if let Some(service) = &mut self.pending {
            if service.site.pc.0 == pc && clock >= service.caller.counts.iter().sum() { service.executed = true; }
            if service.site.target.0 == 0x72612c && service.entered && pc == 0x7261cc { service.return_executed = true; }
        }
        if self.transfer.is_some_and(|(target, start)| target.0 & !0x40000000 == pc && clock >= start) { self.transfer_executed = true; }
        if self.handback.as_ref().is_some_and(|state| state.pc.0 & !0x40000000 == pc && clock >= state.counts.iter().sum()) {
            self.epilogue = true;
        }
    }
    fn active(&self, sample: &Sample) -> bool {
        sample.context.core == Core::Primary && sample.kernels[0].current == sample.kernels[0].node
    }
    fn observe(&mut self, sample: &mut Sample, binding: &RootBinding) -> Result<(), String> {
        let state = &sample.context;
        let pc = state.pc.0 & !0x40000000;
        let task = &sample.kernels[0];
        let active = self.active(sample);
        require(!binding.service_halts.contains(&pc), &format!("retained source halt reached before execution at0x{pc:x}"))?;
        if let Some(&register) = binding.indirect_sites.get(&pc) {
            if active || self.callback.as_ref().is_some_and(|callback| callback.caller.core == state.core) {
                let target = register.value(state);
                require(target % 4 == 0 && binding.service_words.contains(&(target & !0x40000000)),
                    &format!("retained transfer target lacks source-bound code at0x{pc:x}"))?;
            }
        }
        if !self.submissions.is_empty() {
            let callback = sample.views.iter().find(|view| view.physical == Address(0x8e1f28)).ok_or("executor callback resource absent")?;
            require(word(&callback.bytes, 0)? == 0x69c9f8, "executor callback resource changed")?;
        }
        self.observe_submission(sample)?;
        if self.submission.as_ref().is_some_and(|submission| state.core == submission.caller.core && state.pc == submission.caller.lr) {
            let submission = self.submission.take().ok_or("root task-submit caller absent")?;
            require(submission.executed && submission.stage == SubmissionStage::Constructed && active && state.sp == submission.caller.sp
                && state.registers[4..12] == submission.caller.registers[4..12], "root task-submit return lacks executed constructor or caller context")?;
            require(state.registers[0] == Word(0), "root task-submit returned nonzero")?;
            sample.boundary = Boundary::ServiceReturn;
        }
        if self.pending.as_ref().is_some_and(|service| service.site.kind == RootCallKind::TailCall && service.return_executed
            && pc == (service.caller.lr.0 & !0x40000000)) {
            let service = self.pending.take().ok_or("root tail-call context absent")?;
            require(service.entered && service.executed && state.sp == service.caller.sp
                && state.registers[4..12] == service.caller.registers[4..12], "root tail-call return failed caller restoration")?;
            self.finish_service(service.site.target.0, state.registers[0].0)?;
            sample.boundary = Boundary::ServiceReturn;
        }
        if (ROOT_ENTRY..ROOT_LIMIT).contains(&pc) {
            let cleanup_returning = self.pending.as_ref().is_some_and(|service| service.site.target.0 == 0x72612c && service.return_executed) || self.cleanup_return;
            require(state.core == Core::Primary && (task.created == Word(1) || cleanup_returning) && task.current == task.node && task.selected == task.node,
                "root instruction has wrong task or core")?;
            require(binding.execution.contains(&pc), "root fetch leaves admitted execution PCs")?;
            require(state.cpsr.0 & 0x0100023f == 0x13, "root instruction has wrong execution mode")?;
            let sp = state.sp.0 & !0x40000000;
            require((ROOT_STACK..=ROOT_STACK + 1024).contains(&sp) && sp % 4 == 0, "root instruction stack leaves live bounds")?;
            require(sp >= ROOT_STACK + 1024 - binding.scratch_low.unsigned_abs(), "root SP leaves artifact scratch profile")?;
            require(!binding.halts.contains(&pc), "root reached a source-required halt")?;
            if pc == ROOT_ENTRY {
                require(!self.entry_executed && sp == ROOT_STACK + 1024 && state.registers[0] == Word(0)
                    && state.lr == Address(0x453f0) && state.cpsr == Word(0x53)
                    && task.creation == Address(root_layout().creation) && task.node == Address(root_layout().node)
                    && task.effective_priority == 0, "root entry identity, stack or continuation differs")?;
                self.entry = Some(state.clone());
            } else { require(self.entry.is_some(), "root instruction preceded actual entry")?; }
            if self.pending.as_ref().is_some_and(|service| service.site.kind == RootCallKind::Call && service.site.pc.0 + 4 == pc) {
                let service = self.pending.take().ok_or("root pending service absent")?;
                require(service.entered && service.executed && state.sp == service.caller.sp
                    && state.registers[4..12] == service.caller.registers[4..12] && self.submission.is_none(),
                    "root service return lacks execution or restored caller context")?;
                self.finish_service(service.site.target.0, state.registers[0].0)?;
                sample.boundary = Boundary::ServiceReturn;
            }
            if binding.returns.contains(&pc) {
                require(self.action == self.sequence.len() && self.cleanup_return && self.pending.is_none(), "root epilogue precedes complete cleanup return")?;
                require(state.registers[0] == Word((-63i32) as u32), "root handback status is not-63")?;
            }
            if let Some(&site) = binding.calls.iter().find(|site| site.pc.0 == pc) {
                if let Some(service) = self.pending.as_mut().filter(|service| service.site == site && !service.entered && !service.executed) {
                    require(self.irq.as_ref().is_some_and(|irq| irq.return_executed && irq.resume == state.pc)
                        && restored(state, &service.caller), "root repeated call boundary lacks restored preemption context")?;
                    service.caller = state.clone();
                } else {
                    require(self.pending.is_none() && self.sequence.get(self.action) == Some(&site.target.0), "root service action order differs")?;
                    self.check_arguments(site.target.0, state)?;
                    self.pending = Some(RootService { site, caller: state.clone(), entered: false, executed: false, return_executed: false });
                }
            }
        }
        if let Some(service) = &mut self.pending {
            if pc == service.site.target.0 && active && !service.entered {
                let return_pc = if service.site.kind == RootCallKind::Call { service.site.pc.0 + 4 } else { service.caller.lr.0 };
                let uninterrupted = state.counts[0] == service.caller.counts[0] + 1;
                let restored_irq = self.irq.as_ref().is_some_and(|irq| irq.return_executed && irq.resume == state.pc);
                require(service.executed && (uninterrupted || restored_irq) && state.lr.0 & !0x40000000 == return_pc & !0x40000000
                    && state.sp == service.caller.sp && state.registers == service.caller.registers && state.cpsr == service.caller.cpsr,
                    "root service entry lacks actual attributed call")?;
                service.entered = true; sample.boundary = Boundary::ServiceEntry;
            }
        }
        if pc == 0x725dd8 && active && self.entry.is_some() {
            let service = self.pending.as_ref().ok_or("root task submission is outside a root service interval")?;
            require(service.entered && service.executed && [0x69ca24, 0x69ca44].contains(&service.site.target.0),
                "root task submission has unrelated service context")?;
            self.submit(sample, service.site.target.0)?;
        }
        if pc == 0x724924 && active {
            let service = self.pending.as_ref().ok_or("resource descriptor has no root caller")?;
            if service.site.target.0 == 0x7247ac && service.entered {
                let descriptor = sample.views.iter().find(|view| matches!(view.object, Object::ResourceDescriptor)).ok_or("resource descriptor absent")?;
                require([word(&descriptor.bytes, 0)?, word(&descriptor.bytes, 4)?, word(&descriptor.bytes, 8)?] == [0, 1, 1],
                    "root resource constructor descriptor differs")?;
            }
        }
        self.observe_cleanup(sample)?;
        self.observe_irq(sample)?;
        self.observe_callback(sample)?;
        if self.transfer_executed && task.created == Word(0) && task.state == 0
            && task.current != task.node && task.selected != task.node && state.core == Core::Primary
            && self.irq_cycles != 0 && self.post_cleanup_callbacks != 0 && self.irq.is_none() && self.callback.is_none() {
            require(self.cleanup_step == 8 && self.resources == 97
                && (self.action + 1 == self.sequence.len() || self.cleanup_return && self.action == self.sequence.len() && self.epilogue)
                && self.submissions == [TaskId(88), TaskId(89), TaskId(90), TaskId(91), TaskId(92), TaskId(95), TaskId(60), TaskId(59)],
                "post-cleanup task state precedes required lifecycle facts")?;
            require(task.entry == Address(ROOT_ENTRY) && task.stack_base.0 & !0x40000000 == ROOT_STACK && task.stack_bytes == Word(1024),
                "post-cleanup root descriptor was repurposed")?;
            sample.boundary = Boundary::PostCleanup; self.complete = true;
        }
        if self.cleanup_return && self.epilogue && self.handback.is_some() && self.post_cleanup_callbacks != 0 && state.core == Core::Primary {
            require(self.resources == 97 && self.submissions.len() == 8 && self.submission.is_none()
                && self.action == self.sequence.len() && task.created == Word(0) && task.state == 0
                && self.irq_cycles != 0 && self.irq.is_none() && self.callback.is_none(), "returning root handback lacks complete post-cleanup facts")?;
            sample.boundary = Boundary::PostCleanup; self.complete = true;
        }
        Ok(())
    }
    fn check_arguments(&self, target: u32, state: &Context) -> Result<(), String> {
        let args = state.registers.map(|word| word.0);
        match target {
            0x72872c => require(args[0] == if self.action == 2 { 0x01010000 } else { 0x11010000 }, "root marker argument differs"),
            0x725b54 => require(args[0] == 0x72835c, "root publication callback differs"),
            0x7247ac => require(args[0] == self.resources + 1 && args[1] == 1 && self.resources < 97, "root97-loop argument or count differs"),
            0x6490dc => require(args[0] == 1, "root retained setup argument differs"),
            0x724cbc => require(args[0] == 0x726478, "root error callback differs"),
            0x6a59b8 => require(args[..3] == [34, 0, 0], "root initial configuration arguments differ"),
            0x725424 => require(args[..4] == [30, 0, 0x69cc18, 0x96ca4c], "root periodic registration arguments differ"),
            0x72612c => require(args[0] == 0 && self.resources == 97 && self.submissions.len() == 8, "root cleanup precedes complete actions"),
            _ => Ok(()),
        }
    }
    fn finish_service(&mut self, target: u32, result: u32) -> Result<(), String> {
        if target == 0x7247ac { self.resources += 1; }
        if [0x6a9354, 0x6ac2b4].contains(&target) { require(result == 0, "root checked service returned nonzero")?; }
        if target == 0x69ca24 { require(self.submissions.len() == 5 && self.resources == 97, "root early workers or resource loop incomplete")?; }
        if target == 0x69ca44 { require(self.submissions.len() == 8, "root later task submissions incomplete")?; }
        if target == 0x69cfa4 {
            require(result != 0, "root selected source shutdown arm; no cleanup completion is established")?;
            self.sequence.extend([0x69ca44, 0x725424, 0x72872c, 0x72612c]);
        }
        if target == 0x72612c {
            require(result == (-63i32) as u32 && self.cleanup_step >= 6, "root cleanup return status or lower operations differ")?;
            self.cleanup_return = true;
        }
        self.action += 1;
        Ok(())
    }
    fn submit(&mut self, sample: &Sample, target: u32) -> Result<(), String> {
        let task = SubmittedTask::read(sample)?;
        let (id, priority, bytes, entry, argument) = match self.submissions.len() {
            index @ 0..=4 => (88 + index as u32, 2, 4096, 0x6c6b5c, 0x8e1f28),
            5 => (95, 1, 3072, 0x73bc54, 0),
            6 => (60, 4, 3072, 0x73b3a0, 0x91482c),
            7 => (59, 6, 3072, 0x73e048, 0),
            _ => return Err("root submitted excess tasks".into()),
        };
        require((target == 0x69ca24) == (id >= 88 && id <= 92)
            && [u32::from(task.id.0), task.priority.0, task.stack_bytes.0, task.entry.0, task.argument.0] == [id, priority, bytes, entry, argument],
            "root task submission descriptor or interval differs")?;
        let expected_name = match id { 88..=92 => 0x781a3c, 95 => 0x782078, 60 => 0x782064, 59 => 0x78209c, _ => return Err("root descriptor task id differs".into()) };
        require(task.name == Address(expected_name) && self.submission.is_none(), "root descriptor name or wrapper lifetime differs")?;
        self.submissions.push(task.id);
        self.submission = Some(RootSubmission { caller: sample.context.clone(), executed: false, task, stage: SubmissionStage::Submitted });
        Ok(())
    }
    fn observe_submission(&mut self, sample: &Sample) -> Result<(), String> {
        let Some(submission) = self.submission.as_mut() else { return Ok(()) };
        let state = &sample.context;
        let pc = state.pc.0 & !0x40000000;
        if state.core != submission.caller.core || ![0x725f08, 0x725f0c, 0x725fa8, 0x725fac].contains(&pc) { return Ok(()); }
        require(submission.executed && state.registers[4] == submission.caller.registers[0]
            && state.sp.0.checked_add(56) == Some(submission.caller.sp.0), "task constructor boundary belongs to an unrelated wrapper")?;
        let task = &submission.task;
        let encoded = u32::from(task.id.0) | ((task.domain.index() as u32 + 1) << 16);
        match pc {
            0x725f08 => {
                require(submission.stage == SubmissionStage::Submitted && state.registers[0] == Word(encoded), "task flag construction ordering or domain differs")?;
                let descriptor = sample.views.iter().find(|view| matches!(view.object, Object::ResourceDescriptor)).ok_or("task flag parameter descriptor absent")?;
                require(word(&descriptor.bytes, 0)? == task.flags.word() && word(&descriptor.bytes, 4)? == 0,
                    "task live D+24 sentinel classification was not propagated")?;
                submission.stage = SubmissionStage::FlagsEntered;
            }
            0x725f0c => {
                require(submission.stage == SubmissionStage::FlagsEntered && [0, (-41i32) as u32].contains(&state.registers[0].0), "task flag constructor result or ordering differs")?;
                let (_, flags) = TASK_FLAG_POOLS[task.domain.index()];
                let address = flags + 12 * (u32::from(task.id.0) - 1);
                let object = sample.views.iter().find(|view| view.physical == Address(address)).ok_or("created task flag object absent")?;
                require(word(&object.bytes, 8)? == 1 && (state.registers[0] != Word(0) || word(&object.bytes, 0)? == task.flags.word()),
                    "created task flag attributes differ from consumed parameter")?;
                submission.stage = SubmissionStage::FlagsObserved;
            }
            0x725fa8 => {
                require(submission.stage == SubmissionStage::FlagsObserved && state.registers[0] == Word(encoded), "task constructor ordering or domain differs")?;
                let descriptor = sample.views.iter().find(|view| matches!(view.object, Object::ResourceDescriptor)).ok_or("kernel task constructor descriptor absent")?;
                require([word(&descriptor.bytes, 0)?, word(&descriptor.bytes, 4)?, word(&descriptor.bytes, 8)?,
                    word(&descriptor.bytes, 12)?, word(&descriptor.bytes, 16)?] == [2, task.argument.0, task.entry.0, task.priority.0, task.stack_bytes.0],
                    "kernel task constructor attributes differ")?;
                submission.stage = SubmissionStage::ConstructorEntered(Address(word(&descriptor.bytes, 20)?));
            }
            0x725fac => {
                let SubmissionStage::ConstructorEntered(stack) = submission.stage else { return Err("task creation evidence precedes constructor entry".into()) };
                require(state.registers[0] == Word(0), "kernel task constructor returned nonzero")?;
                let (_, creations, nodes) = TASK_RECORD_POOLS[task.domain.index()];
                let index = u32::from(task.id.0) - 1;
                let creation = sample.views.iter().find(|view| view.physical == Address(creations + 40 * index)).ok_or("constructed task creation record absent")?;
                let node = sample.views.iter().find(|view| view.physical == Address(nodes + 56 * index)).ok_or("constructed task node absent")?;
                require(word(&creation.bytes, 0)? == 2 && word(&creation.bytes, 4)? == task.argument.0
                    && word(&creation.bytes, 8)? == task.entry.0 && word(&creation.bytes, 12)?.checked_add(1) == Some(task.priority.0)
                    && word(&creation.bytes, 16)? == task.stack_bytes.0 && word(&creation.bytes, 20)? == stack.0
                    && word(&creation.bytes, 32)? == u32::from(task.id.0) && word(&creation.bytes, 36)? == 1
                    && word(&node.bytes, 8)? == (0x40000000 | (creations + 40 * index))
                    && u32::from(node.bytes[13]) + 1 == task.priority.0, "live constructed task identity, attributes or stack differs")?;
                let cursor = sample.views.iter().find(|view| view.physical == Address(0x89ea98)).ok_or("task stack cursor absent")?;
                let cursor_address = word(&cursor.bytes, 0)? & !0x40000000;
                require((TASK_STACK_POOL.0..=TASK_STACK_POOL.1).contains(&cursor_address)
                    && (stack.0 & !0x40000000).checked_add(task.stack_bytes.0).is_some_and(|end| end <= cursor_address),
                    "constructed task stack has no committed allocator coverage")?;
                submission.stage = SubmissionStage::Constructed;
            }
            _ => return Err("unknown task constructor boundary".into()),
        }
        Ok(())
    }
    fn observe_cleanup(&mut self, sample: &Sample) -> Result<(), String> {
        let state = &sample.context;
        let pc = state.pc.0 & !0x40000000;
        if pc == 0x453f0 && self.cleanup_return {
            let entry = self.entry.as_ref().ok_or("root original entry absent")?;
            require(state.sp == entry.sp && state.registers[4..12] == entry.registers[4..12]
                && state.registers[0] == Word((-63i32) as u32), "returning root failed original handback context")?;
            self.handback = Some(state.clone());
            return Ok(());
        }
        let cleanup_active = self.pending.as_ref().is_some_and(|service| service.site.target.0 == 0x72612c && service.entered) || self.cleanup_return;
        if !cleanup_active { return Ok(()); }
        if state.core != Core::Primary { return Ok(()); }
        const STEPS: [u32; 8] = [0x726150, 0x726160, 0x72619c, 0x7261a8, 0x7261bc, 0x453f0, 0x454e8, 0x436e8];
        if STEPS.contains(&pc) && self.active(sample) {
            require(STEPS.get(self.cleanup_step) == Some(&pc), "root cleanup lower service ordering differs")?;
            if pc == 0x726150 { require(state.registers[0] == Word(0), "root cleanup selected wrong core root")?; }
            if pc == 0x726160 { require(state.registers[0].0 & 0xffff == 104, "root cleanup queried wrong task")?; }
            if pc == 0x7261bc { require(sample.kernels[0].created == Word(0), "root cleanup failed to clear creation flag")?; }
            self.cleanup_step += 1;
        }
        if pc == 0x436e8 && self.cleanup_step == 7 {
            let task = &sample.kernels[0];
            require(task.created == Word(0) && task.state == 0 && task.current != task.node && task.selected == task.current,
                "root scheduler transfer lacks retired task state")?;
            let node = state.registers[1].0;
            let selected = sample.views.iter().find(|view| matches!(view.object, Object::TaskNode) && view.physical.0 == node & !0x40000000);
            let target = selected.map(|view| word(&view.bytes, 28)).transpose()?.ok_or("post-cleanup selected task node is not captured")?;
            require([0x43668, 0x43728, 0x437a4].contains(&(target & !0x40000000)),
                "selected task savedPC lacks source-written scheduler continuation evidence")?;
            self.transfer = Some((Address(target), state.counts.iter().sum()));
            self.cleanup_step = 8;
        }
        if pc == 0x7261cc {
            require(self.cleanup_step >= 5, "cleanup epilogue preceded lower services")?;
            require(state.registers[0] == Word((-63i32) as u32), "cleanup source return did not produce-63")?;
        }
        Ok(())
    }
    fn observe_irq(&mut self, sample: &Sample) -> Result<(), String> {
        let state = &sample.context;
        if state.core != Core::Primary { return Ok(()); }
        if sample.boundary == Boundary::IrqEntry && self.active(sample) && self.entry.is_some() {
            require(self.irq.is_none() && state.cpsr.0 & 0x1f == 0x12 && state.spsr.0 & 0x1f == 0x13,
                "root IRQ entry has wrong context")?;
            self.irq = Some(RootIrq { entry: state.clone(), resume: Address(state.lr.0.checked_sub(4).ok_or("IRQ LR underflow")?),
                frame: None, entry_executed: false, return_executed: false, returned: false });
        }
        let Some(irq) = &mut self.irq else { return Ok(()) };
        if sample.boundary == Boundary::IrqFrame && sample.kernels[0].current == sample.kernels[0].node {
            require(irq.entry_executed && state.cpsr.0 & 0xbf == 0x93, "root IRQ frame has wrong execution or mode")?;
            let r = irq.entry.registers;
            let saved = frame::<8>(sample)?;
            require(saved[..6] == [irq.entry.spsr.0, r[0].0, r[1].0, r[2].0, r[3].0, r[12].0] && saved[7] == irq.resume.0,
                "root IRQ frame differs from interrupted registers")?;
            irq.frame = Some((state.sp, Address(saved[6])));
        }
        if sample.boundary == Boundary::IrqReturn && sample.kernels[0].current == sample.kernels[0].node {
            let (sp, lr) = irq.frame.ok_or("root IRQ restoration precedes actual frame")?;
            let r = irq.entry.registers;
            let frame = frame::<7>(sample)?;
            require(frame == [r[0].0, r[1].0, r[2].0, r[3].0, r[12].0, lr.0, irq.resume.0] && state.spsr == irq.entry.spsr
                && state.sp.0 == sp.0 + 4, "root IRQ restoration frame differs")?;
            irq.returned = true;
        }
        if state.pc.0 == irq.resume.0 && irq.returned {
            let (sp, lr) = irq.frame.ok_or("root IRQ saved context absent")?;
            require(irq.return_executed && state.registers == irq.entry.registers && state.cpsr == irq.entry.spsr
                && state.sp.0 == sp.0 + 32 && state.lr == lr, "root IRQ resumed without restoring context")?;
            self.irq_cycles += 1; self.irq = None;
        }
        Ok(())
    }
    fn observe_callback(&mut self, sample: &Sample) -> Result<(), String> {
        let state = &sample.context;
        let pc = state.pc.0 & !0x40000000;
        let counter = sample.views.iter().find(|view| view.physical == Address(0x828094)).ok_or("periodic counter absent")?;
        if pc == 0x69cc18 {
            let handle = sample.views.iter().find(|view| view.physical == Address(0x96ca4c)).ok_or("periodic live handle absent")?;
            let table = sample.views.iter().find(|view| view.physical == Address(0x9c3500)).ok_or("periodic live callback table absent")?;
            let handle = word(&handle.bytes, 0)?;
            let index = usize::try_from(state.registers[0].0).map_err(|_| "periodic callback index exceeds host size")?;
            require((1..=64).contains(&index) && state.lr == Address(0x723394)
                && word(&table.bytes, 4 * index)? == 0x69cc18 && word(&table.bytes, 0x100 + 4 * index)? == handle
                && word(&table.bytes, 0x200 + 4 * index)? == 30, "periodic callback is not bound to live root dispatch")?;
            require(self.callback.is_none(), "overlapping root periodic callback invocation")?;
            let value = word(&counter.bytes, 0)?;
            require((1..=104).contains(&value), "periodic callback counter leaves source range")?;
            let after_cleanup = sample.kernels[0].created == Word(0) && sample.kernels[0].state == 0
                && (self.transfer_executed || self.cleanup_return && self.epilogue);
            self.callback = Some(RootCallback { caller: state.clone(), counter: value, returned: false, entry_executed: false, epilogue_executed: false, after_cleanup });
        }
        if pc == 0x69cc3c { return Err("periodic callback reached source error halt".into()); }
        if [0x69cc1c, 0x69cc68].contains(&pc) {
            let callback = self.callback.as_ref().ok_or("periodic private frame has no actual callback entry")?;
            require(callback.entry_executed && state.core == callback.caller.core && state.sp.0.checked_add(8) == Some(callback.caller.sp.0)
                && callback_frame(sample, state.sp.0)? == [callback.caller.registers[11].0, callback.caller.lr.0],
                "periodic callback private r11/LR frame differs from its live caller stack")?;
        }
        if pc == 0x69cc68 {
            let callback = self.callback.as_mut().ok_or("periodic callback has no actual entry")?;
            require(state.core == callback.caller.core && word(&counter.bytes, 0)? == if callback.counter == 104 { 1 } else { callback.counter + 1 },
                "periodic callback counter update differs")?;
            callback.returned = true;
        }
        if self.callback.as_ref().is_some_and(|callback| callback.returned && state.core == callback.caller.core && state.pc == callback.caller.lr) {
            let callback = self.callback.as_ref().ok_or("periodic callback context absent")?;
            require(callback.epilogue_executed && state.sp == callback.caller.sp && state.registers[4..12] == callback.caller.registers[4..12],
                "periodic callback did not execute and restore caller context")?;
            if callback.after_cleanup { self.post_cleanup_callbacks += 1; }
            self.callback = None;
        }
        Ok(())
    }
}

fn root_fixed_sites(binding: &RootBinding, sources: &[Code]) -> Result<BTreeMap<u32, String>, String> {
    let mut sites = root_sites(binding);
    for &pc in &binding.service_words {
        let instruction = code_word(sources, pc)?;
        if instruction & 0x0f000000 == 0x0b000000 && direct_target(pc, instruction)? == 0x725dd8 {
            sites.insert(pc.checked_add(4).ok_or("task wrapper return PC overflow")?, "service-return".into());
        }
    }
    sites.insert(0x723394, "service-return".into());
    for pc in [0x43668, 0x43728, 0x437a4] {
        require(binding.fetch_spans.iter().any(|&(low, high)| low <= u64::from(pc) && u64::from(pc) + 8 <= high),
            "selected task continuation has no complete actual-fetch coverage")?;
        sites.entry(pc).or_insert_with(|| "service-return".into());
        sites.entry(pc + 4).or_insert_with(|| "service-return".into());
    }
    let aliases: Vec<_> = sites.iter().map(|(&pc, label)| (pc | 0x40000000, label.clone())).collect();
    sites.extend(aliases);
    Ok(sites)
}
fn root_resume_sites(sources: &[Code], resume: Address) -> Result<BTreeMap<u32, String>, String> {
    require(resume.0 % 4 == 0, "root IRQ continuation is not A32 aligned")?;
    code_word(sources, resume.0)?;
    let physical = resume.0 & !0x40000000;
    Ok(BTreeMap::from([(physical, "service-return".into()), (physical | 0x40000000, "service-return".into())]))
}
fn root_sample_boundary(producer: &RootProducer, binding: &RootBinding, continuations: &BTreeSet<u32>, pc: u32) -> Result<Boundary, String> {
    let physical = pc & !0x40000000;
    if physical == 0x725dd8 { return Ok(Boundary::TaskSubmit); }
    if producer.irq.as_ref().is_some_and(|irq| irq.resume.0 & !0x40000000 == physical)
        || producer.callback.as_ref().is_some_and(|callback| callback.caller.lr.0 & !0x40000000 == physical)
        || producer.submission.as_ref().is_some_and(|submission| submission.caller.lr.0 & !0x40000000 == physical) {
        return Ok(Boundary::ServiceReturn);
    }
    if producer.transfer.is_some_and(|(target, _)| [target.0 & !0x40000000, (target.0 & !0x40000000) + 4].contains(&physical))
        || producer.handback.as_ref().is_some_and(|state| state.pc.0 + 4 == pc) {
        return Ok(Boundary::PostCleanup);
    }
    if binding.indirect_sites.contains_key(&physical) || binding.service_halts.contains(&physical)
        || binding.service_entries.contains(&physical) { return Ok(Boundary::ServiceEntry); }
    if continuations.contains(&physical) { return Ok(Boundary::ServiceReturn); }
    root_boundary(pc, binding)
}
fn inspect_root_watch(raw: &serde_json::Value, processed: &mut usize, stopped: &[Hit],
    producer: &mut RootProducer, binding: &RootBinding) -> Result<usize, String> {
    let summary = WatchSummary::deserialize(raw).map_err(|error| error.to_string())?;
    require(summary.enabled && summary.violation.is_none(), "root memory watch is incomplete")?;
    let rows = raw.get("records").and_then(serde_json::Value::as_array).ok_or("root watch records absent")?;
    let events = rows.get(*processed..).ok_or("root watch cursor moved backwards")?.iter()
        .map(|row| AccessRecord::deserialize(row).map_err(|error| error.to_string())).collect::<Result<Vec<_>, _>>()?;
    let stop_rows = stopped.iter().map(|hit| {
        let pc = u64::from(number(&hit.address)?);
        Ok(events.iter().rposition(|row| row.source == Access::CpuFetch && row.core == Some(hit.core)
            && row.pc == Some(pc) && row.instructions == Some(hit.instructions)))
    }).collect::<Result<Vec<_>, String>>()?;
    for (index, row) in events.iter().enumerate() {
        if matches!(row.source, Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite) {
            let physical = row.physical.ok_or("root write physical attribution absent")?;
            require(!root_immutable(binding, physical, row.size), "retained root service or carrier write after watch start")?;
            require(row.source == Access::CpuWrite && row.core.is_some() && row.pc.is_some() && row.instructions.is_some(),
                "root live resource write lacks CPU action attribution")?;
            if row.pc.is_some_and(|pc| (u64::from(ROOT_ENTRY)..u64::from(ROOT_LIMIT)).contains(&(pc & !0x40000000))) {
                let low = u64::from(ROOT_STACK + 1024 - binding.scratch_low.unsigned_abs());
                require(physical >= low && physical.checked_add(row.size).is_some_and(|end| end <= u64::from(ROOT_STACK + 1024)),
                    "root carrier write leaves artifact-owned scratch")?;
            }
        }
        if row.source == Access::CpuFetch {
            let pc = u32::try_from(row.pc.ok_or("root fetch PC absent")?).map_err(|_| "root fetch PC exceeds32 bits")?;
            let core = Core::from_index(row.core.ok_or("root fetch core absent")?)?;
            if (ROOT_ENTRY..ROOT_LIMIT).contains(&(pc & !0x40000000)) {
                require(core == Core::Primary && binding.execution.contains(&(pc & !0x40000000)), "actual root fetch has wrong core or admitted PC")?;
                require(row.physical == Some(u64::from(pc & !0x40000000)) && row.size == 4, "actual root fetch has wrong physical attribution")?;
            }
            if !stop_rows.contains(&Some(index)) {
                producer.fetched(core, pc, row.instructions.ok_or("actual root fetch clock absent")?);
            }
        }
    }
    *processed = rows.len();
    Ok(rows.len())
}
fn canonical_interface_target(sources: &[Code], core: Core, offset: usize) -> Result<u32, String> {
    let base = if core == Core::Primary { 0x40000 } else { 0xd0000 };
    if offset == 0x1618 { return Ok(base + 0x12bc); }
    let mut values = [None; 16];
    for pc in ((base + 0x430)..(base + 0x89c)).step_by(4) {
        let instruction = code_word(sources, pc)?;
        let register = ((instruction >> 12) & 15) as usize;
        let immediate = ((instruction >> 4) & 0xf000) | (instruction & 0xfff);
        match instruction & 0x0ff00000 {
            0x03000000 => values[register] = Some(immediate),
            0x03400000 => values[register] = Some((values[register].ok_or("source MOVT lacks MOVW")? & 0xffff) | (immediate << 16)),
            0x03a00000 => values[register] = Some((instruction & 255).rotate_right(2 * ((instruction >> 8) & 15))),
            _ => (),
        }
        if instruction & 0xffff0000 == 0xe5830000 && (instruction & 0xfff) as usize == offset {
            return values[register].ok_or_else(|| "selected-root source service value is unresolved".into());
        }
    }
    Err("selected-root service slot has no canonical installer store".into())
}
fn bind_root_interfaces(machine: &Machine, maps: &[MemoryMap], state: &Context, sources: &[Code], binding: &mut RootBinding) -> Result<Vec<View>, String> {
    let mut views = Vec::new();
    let mut entries = binding.service_entries.clone();
    for core in [Core::Primary, Core::Secondary] {
        let table = view(machine, maps, state, Object::InterfaceTable, core, core.layout().table, 0x162c)?;
        for offset in (0..0x174).step_by(4).chain([0x1618]) {
            let target = word(&table.bytes, offset)?;
            require(target == canonical_interface_target(sources, core, offset)?, "selected-root service differs from canonical installer")?;
            if target == 0 { continue; }
            require(target % 4 == 0, "retained selected-root service target is not A32")?;
            code_word(sources, target)?;
            entries.insert(target & !0x40000000);
        }
        if core == Core::Primary {
            for (offset, expected) in [(4, 0x4d1a4), (0x14, 0x453f0), (0x84, 0x4d434),
                (0x164, 0x41140), (0x168, 0x41164), (0x16c, 0x4118c), (0x1618, 0x412bc)] {
                require(word(&table.bytes, offset)? == expected, "root cleanup selected-root target differs from canonical installer")?;
            }
        }
        views.push(table);
    }
    let services = retained_function_words(sources, &entries)?;
    binding.service_words = services.words;
    binding.service_literals = services.literals;
    binding.indirect_sites = services.indirect_sites;
    binding.service_halts = services.halts;
    for (start, end) in instruction_spans(&binding.service_words).into_iter().chain(instruction_spans(&binding.service_literals)) {
        let installed = view(machine, maps, state, Object::ServiceCode, Core::Primary, start as u32, (end - start) as usize)?;
        for (index, bytes) in installed.bytes.chunks_exact(4).enumerate() {
            require(u32::from_le_bytes(bytes.try_into().map_err(|_| "service instruction extent differs")?)
                == code_word(sources, start as u32 + 4 * index as u32)?, "installed retained service differs from bound source")?;
        }
        views.push(installed);
    }
    Ok(views)
}
fn drive_root(mut machine: Machine, maps: &[MemoryMap], code: &[Code], mut binding: RootBinding, identity: Identity,
    role: Role, output: &Path, reset: &[u8]) -> Result<(), String> {
    let mut producer = RootProducer::new();
    let mut sites = root_sites(&binding);
    machine.traces.bound_full_registers(NonZeroUsize::new(HIT_LIMIT).ok_or("invalid root hit bound")?);
    machine.traces.stop_labels.extend(sites.values().cloned());
    machine.traces.stop_labels.insert("start".into());
    let mut static_sites = BTreeSet::new();
    let mut dynamic_resume: Option<Address> = None;
    let mut continuations = BTreeSet::new();
    machine.traces.arm_mutable(&machine.session, BTreeMap::from([(0x725dd8, "start".into()), (0x40725dd8, "start".into())]))?;
    let mut watch: Option<Box<MemoryWatch>> = None;
    let mut samples = Vec::new();
    let mut processed_hits = 0;
    let mut processed_rows = 0;
    let mut lifecycle_end_counts = None;
    let result = (|| -> Result<(), String> {
        loop {
            let stopped = machine.run(DEADLINE.saturating_sub(machine.instructions()));
            let end = machine.traces.hits.len();
            require(end < HIT_LIMIT, "root lifecycle hit cap reached")?;
            let cursor = if let Some(watch) = &watch {
                inspect_root_watch(&watch.records.borrow().state(), &mut processed_rows,
                    &machine.traces.hits[processed_hits..end], &mut producer, &binding)?
            } else { 0 };
            for index in processed_hits..end {
                let hit = &machine.traces.hits[index];
                let address = number(&hit.address)?;
                let core = Core::from_index(hit.core)?;
                let state = context(&machine, core)?;
                require(state.pc == Address(address), "root stopped context moved past instruction point")?;
                let first = watch.is_none();
                let mut installed_views = Vec::new();
                if first {
                    require(address & !0x40000000 == 0x725dd8
                        && matches!((core, state.lr.0), (Core::Primary, 0x69ce8c | 0x69cee8) | (Core::Secondary, 0x69cf38 | 0x69cf70)),
                        "root watch start is outside canonical startup wrappers")?;
                    for source in code {
                        let length = if source.base == IMAGE_BASE { (INITIAL_IMAGE_VIEW_END - IMAGE_BASE) as usize } else { source.bytes.len() };
                        let owner = if source.base == 0xd0000 { Core::Secondary } else { Core::Primary };
                        let installed = view(&machine, maps, &state, Object::LoadedCode, owner, source.base, length)?;
                        require(installed.bytes == source.bytes[..length], "installed root source differs at watch start")?;
                        installed_views.push(installed);
                    }
                    installed_views.extend(bind_root_interfaces(&machine, maps, &state, code, &mut binding)?);
                    let ranges = root_watch_ranges(&mut binding)?;
                    let base_sites = root_sites(&binding);
                    let fixed = root_fixed_sites(&binding, code)?;
                    continuations.extend(fixed.keys().filter(|pc| !base_sites.contains_key(pc)).map(|pc| *pc & !0x40000000));
                    static_sites.extend(fixed.keys().copied());
                    machine.traces.stop_labels.extend(fixed.values().cloned());
                    machine.traces.replace_sites(&machine.session, fixed.clone())?;
                    sites = fixed;
                    watch = Some(machine.session.board.memory_watch_begin_with_fetch_attribution(ranges, fetch_attribution()?,
                        WATCH_LIMIT, maps.to_vec(), ViewPolicy::Observe)?);
                }
                let boundary = if first { Boundary::Start } else { root_sample_boundary(&producer, &binding, &continuations, address)? };
                let callback_context = (0x69cc18..0x69cc6c).contains(&(address & !0x40000000))
                    || producer.callback.as_ref().is_some_and(|callback| callback.caller.core == core && callback.caller.lr.0 == address);
                let mut observed = root_sample(&machine, maps, code, index, state, cursor, boundary, callback_context)?;
                observed.views.extend(installed_views);
                if [0x725f0c, 0x725fac].contains(&(address & !0x40000000)) {
                    if let Some(submission) = &producer.submission {
                        if submission.caller.core == core { submission_views(&machine, maps, &observed.context, &submission.task, &mut observed.views)?; }
                    }
                }
                if address & !0x40000000 == 0x436e8 {
                    let selected = observed.context.registers[1].0;
                    require(selected != 0, "root cleanup selected null task")?;
                    observed.views.push(view(&machine, maps, &observed.context, Object::TaskNode, Core::Primary, selected, 56)?);
                }
                let observed_result = producer.observe(&mut observed, &binding);
                samples.push(observed);
                observed_result?;
            }
            processed_hits = end;
            machine.require_running()?;
            if producer.complete {
                lifecycle_end_counts = Some(machine.session.board.instruction_counts().try_into().map_err(|_| "expected two cores")?);
                break;
            }
            require(stopped == RunOutcome::Stopped && machine.instructions() < DEADLINE,
                producer.deadline_error())?;
            let next_resume = producer.irq.as_ref().map(|irq| Address(irq.resume.0 & !0x40000000))
                .filter(|resume| !static_sites.contains(&resume.0));
            if next_resume != dynamic_resume {
                if let Some(previous) = dynamic_resume {
                    machine.traces.remove_sites(&machine.session, &[
                        fujimod_xm1::provenance::Guest(previous.0), fujimod_xm1::provenance::Guest(previous.0 | 0x40000000),
                    ])?;
                }
                if let Some(resume) = next_resume {
                    let additions = root_resume_sites(code, resume)?;
                    machine.traces.add_sites(&machine.session, additions.clone())?;
                    continuations.insert(resume.0);
                    sites.extend(additions);
                }
                dynamic_resume = next_resume;
            }
        }
        Ok(())
    })();
    let raw = watch.as_ref().map(|watch| watch.records.borrow().state());
    let accesses = serde_json::to_vec_pretty(&raw).map_err(|error| error.to_string())?;
    write(&output.join("memory-watch.json"), &accesses)?;
    let summary = raw.as_ref().ok_or_else(|| "root watch never began".to_owned())
        .and_then(|raw| WatchSummary::deserialize(raw).map_err(|error| error.to_string()));
    let watch_end_error = watch.as_mut().and_then(|watch| machine.session.board.memory_watch_end(watch).err());
    let disarm = machine.traces.disarm(&machine.session);
    machine.traces.stop_labels.clear();
    let outcome = result.and(disarm).and_then(|()| {
        require(producer.complete && lifecycle_end_counts.is_some()
            && summary.as_ref().is_ok_and(|summary| summary.enabled && summary.violation.is_none())
            && watch_end_error.is_none(), "root lifecycle or watch refused")?;
        menu_route(&mut machine)
    });
    let final_menu = machine.menu().map(|menu| menu.map(|menu| [menu.menu_id, menu.setting_id, menu.cursor]));
    let receipt = Receipt {
        schema: 1, scope: "natural-reset-root-task-lifecycle", role,
        outcome: if outcome.is_ok() { Outcome::ObservedMenu } else { Outcome::Refused },
        error: outcome.as_ref().err().map(String::as_str), machine_failure: machine.failure.as_deref(), identity,
        checkpoint_sha256: digest(reset), checkpoint_reused: true, instructions: machine.instructions(), lifecycle_end_counts,
        memory_watch_sha256: digest(&accesses), watch: summary, watch_end_error, sites: &sites,
        hits: &machine.traces.hits, samples: &samples, final_menu,
    };
    write(&output.join("root-task.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    outcome
}

fn drive() -> Result<(), String> {
    let mut arguments = BTreeMap::new(); let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag = flag.into_string().map_err(|_| USAGE)?;
        if !["--task", "--role", "--run", "--update", "--boot-rom", "--analysis", "--linked", "--symbols", "--contract", "--output"].contains(&flag.as_str())
            || arguments.insert(flag, PathBuf::from(args.next().ok_or(USAGE)?)).is_some() { return Err(USAGE.into()); }
    }
    let task = match arguments.remove("--task") {
        None => TaskScope::Idle,
        Some(value) => TaskScope::parse(value.to_str().ok_or(USAGE)?)?,
    };
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
    let binding = match task {
        TaskScope::Idle => ObserverBinding::Idle(bind_artifact(role, &linked, &symbols, &contract, &identity.update, &mut code)?),
        TaskScope::Root => ObserverBinding::Root(bind_root_artifact(role, &linked, &symbols, &contract, &identity.update, &mut code)?),
    };
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    fujimod_emulator_exr2_board::register_board_builder(); fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?; let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(update), &rom)?; session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?; machine.disarm()?;
    let reset = machine.session.checkpoint(None)?.export()?; write(&output.join("reset.fujimod"), &reset)?;
    let snapshot = machine.session.import_snapshot(&reset)?; machine.session.restore(&snapshot)?; machine.rearm()?;
    require(machine.instructions() == 0, "reset checkpoint is not preexecution")?;
    if let ObserverBinding::Root(binding) = binding {
        return drive_root(machine, &maps, &code, binding, identity, role, &output, &reset);
    }
    let ObserverBinding::Idle((wfi, resume)) = binding else { return Err("idle binding absent".into()) };
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
    #[test]
    fn idle_kernel_refusals_preserve_diagnostics() {
        for core in [Core::Primary, Core::Secondary] {
            let mut wrong_identity = synthetic_kernel(core);
            wrong_identity.creation_id = Word(104);
            assert_eq!(wrong_identity.require_layout(core.layout(), TaskScope::Idle).unwrap_err(), "idle creation/node identity differs");
            let mut wrong_stack = synthetic_kernel(core);
            wrong_stack.stack_base.0 -= 4;
            assert_eq!(wrong_stack.require_layout(core.layout(), TaskScope::Idle).unwrap_err(), "idle stack differs from canonical run bounds");
        }
    }

    fn root_routing_binding() -> RootBinding {
        RootBinding {
            execution: BTreeSet::new(), calls: Vec::new(), halts: BTreeSet::new(), returns: BTreeSet::new(),
            service_words: BTreeSet::new(), service_literals: BTreeSet::new(), service_entries: BTreeSet::from([0x725dd8]),
            indirect_sites: BTreeMap::new(), service_halts: BTreeSet::new(), fetch_spans: Vec::new(), scratch_low: 0,
        }
    }

    #[test]
    fn root_wrapper_entry_is_serialized_as_task_submit_on_both_aliases() {
        let binding = root_routing_binding();
        let producer = RootProducer::new();
        for pc in [0x725dd8, 0x40725dd8] {
            let boundary = root_sample_boundary(&producer, &binding, &BTreeSet::from([0x725dd8]), pc).unwrap();
            assert_eq!(serde_json::to_value(boundary).unwrap(), "task-submit");
        }
    }

    #[test]
    fn fixed_root_points_cover_known_returns_without_opaque_callee_instructions() {
        let mut binding = root_routing_binding();
        binding.service_words.extend([0x6b07f0, 0x6b07f8]);
        let source = Code {
            name: "retained-wrapper-caller", base: 0x6b07f0,
            bytes: [0xeb01d578u32, 0xe3500000, 0xe2800001].into_iter().flat_map(u32::to_le_bytes).collect(),
        };
        root_watch_ranges(&mut binding).unwrap();
        let sites = root_fixed_sites(&binding, &[source]).unwrap();
        for pc in [0x6b07f4, 0x406b07f4] { assert_eq!(sites.get(&pc).map(String::as_str), Some("service-return")); }
        for pc in [0x723394, 0x43668, 0x4366c, 0x43728, 0x4372c, 0x437a4, 0x437a8] {
            assert!(sites.contains_key(&pc) && sites.contains_key(&(pc | 0x40000000)));
        }
        assert!(!sites.contains_key(&0x6b07f8) && !sites.contains_key(&0x406b07f8));
    }
}
