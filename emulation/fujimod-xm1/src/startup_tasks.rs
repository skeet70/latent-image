use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::{bridge::{read, sha256, write}, guest::Press, machine::{KeyPulse, Machine, RunOutcome}, run::{self, RunDocument, ValidatedUpdate}, trace::Hit};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, num::NonZeroUsize, path::{Path, PathBuf}, process::ExitCode};

#[path = "memory_view.rs"]
mod memory_view;
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};

const USAGE: &str = "startup-tasks --run RUN --update ORIGINAL_DAT --boot-rom ROM --analysis ROOT --output NEW_DIR";
const DEADLINE: u64 = 80_000_000;
const HIT_LIMIT: usize = 4096;
const WATCH_LIMIT: usize = 1_048_576;
const COUNT: u32 = 120;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct Address(u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct Word(u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct TaskId(u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Core { Primary, Secondary }
impl Core {
    fn index(self) -> usize { match self { Self::Primary => 0, Self::Secondary => 1 } }
    fn from_index(index: usize) -> Result<Self, String> { match index { 0 => Ok(Self::Primary), 1 => Ok(Self::Secondary), _ => Err("unexpected core".into()) } }
    fn layout(self) -> Layout {
        match self {
            Self::Primary => Layout { creation: 0x4005beac, tasks: 0x4005ff0c, ids: 0x400622d4, auxiliary: 0x4005d178, auxiliary_nodes: 0x400624b4, domains: 0x400545cc, control: 0x400545a4, selected: 0x40054768, current: 0x40054760, domain_pointer: 0x400545c8 },
            Self::Secondary => Layout { creation: 0x400ebe6c, tasks: 0x400efecc, ids: 0x400f2294, auxiliary: 0x400ed138, auxiliary_nodes: 0x400f2474, domains: 0x400e458c, control: 0x400e4564, selected: 0x400e4728, current: 0x400e4720, domain_pointer: 0x400e4588 },
        }
    }
}
#[derive(Clone, Copy)]
struct Layout { creation: u32, tasks: u32, ids: u32, auxiliary: u32, auxiliary_nodes: u32, domains: u32, control: u32, selected: u32, current: u32, domain_pointer: u32 }
#[derive(Serialize)]
#[serde(transparent)]
struct Digest(String);
#[derive(Serialize)]
struct Identity { run: Digest, update: Digest, boot_rom: Digest, observer: Digest, sources: Vec<SourceIdentity> }
#[derive(Serialize)]
struct SourceIdentity { name: &'static str, base: Address, bytes: usize, sha256: Digest }
struct Code { name: &'static str, base: u32, bytes: Vec<u8> }
#[derive(Serialize)]
struct Context { core: Core, pc: Address, registers: [Word; 13], sp: Address, lr: Address, cpsr: Word, counts: [u64; 2], identities: [Word; 2], tcm: [[Word; 2]; 2] }
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Object { CreationRecords, TaskNodes, IdMap, AuxiliaryRecords, AuxiliaryNodes, Domains, Control, QueueHeads, Bitmap, Selected, Current, Descriptor, Wrapper, Storage, Stack, EntryCode, SharedDomains, InterfaceRoots, InterfaceTable }
#[derive(Serialize)]
struct View { object: Object, physical: Address, bytes: Vec<u8>, aliases: [[Digest; 2]; 2] }
#[derive(Serialize)]
struct TaskState { id: TaskId, creation: Address, creation_pointer: Address, creation_id: Word, node: Address, flags: Word, entry: Address, argument: Word, priority: Word, stack_bytes: Word, stack_base: Address, created: Word, state: u8, effective_priority: u8, pending: u8, saved_sp: Address, resume_pc: Address }
#[derive(Serialize)]
struct Kernel { core: Core, views: Vec<View>, tasks: Vec<TaskState>, domain: Address, selected: Address, current: Address }
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Boundary { Submission, AuxiliaryReturn, CreationCall, CreationReturn, ActivationCall, ActivationReturn, WrapperReturn, AutoReadyEntry, AutoReadyReturn, Prepare, Prepared, Scheduler, Selected, StackTransfer, Trampoline, EntryTransfer, TaskEntry, EnteredInstruction }
#[derive(Serialize)]
struct Sample { hit: usize, boundary: Boundary, context: Context, code_word: Word, watch_cursor: usize, kernels: Vec<Kernel>, local: Vec<View> }
#[derive(Serialize)]
struct Submission { id: TaskId, core: Core, descriptor: Address, caller_sp: Address, return_pc: Address, entry_hit: usize, return_hit: Option<usize>, status: Option<Word> }
#[derive(Serialize)]
struct FirstTask { core: Core, id: TaskId, node: Address, creation: Address, stack_top: Address, entry: Address, selected_hit: usize, transfer_hit: Option<usize>, entry_hit: Option<usize>, entry_counts: Option<[u64; 2]>, entered_instruction_hit: Option<usize> }
#[derive(Deserialize, Serialize)]
struct WatchSummary { enabled: bool, violation: Option<String> }
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome { ObservedMenu, Refused }
#[derive(Serialize)]
struct Receipt<'a> { schema: u32, scope: &'static str, outcome: Outcome, error: Option<&'a str>, machine_failure: Option<&'a str>, identity: Identity, checkpoint_sha256: Digest, checkpoint_reused: bool, instructions: u64, startup_end_counts: Option<[u64; 2]>, memory_watch_sha256: Digest, watch: Result<WatchSummary, String>, watch_end_error: Option<String>, sites: &'a BTreeMap<u32, String>, hits: &'a [Hit], submissions: &'a [Submission], first_tasks: &'a [FirstTask], samples: &'a [Sample], final_menu: Result<Option<[u32; 3]>, String> }

fn require(value: bool, message: &str) -> Result<(), String> { if value { Ok(()) } else { Err(message.into()) } }
fn number(value: &str) -> Result<u32, String> { u32::from_str_radix(value.strip_prefix("0x").ok_or("trace word is not hexadecimal")?, 16).map_err(|error| error.to_string()) }
fn register(machine: &Machine, core: Core, register: arm::Register) -> Result<u32, String> { u32::try_from(machine.session.board.read_register(core.index(), Register::Arm32(register))?).map_err(|_| "register exceeds32 bits".into()) }
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
    Ok(Context { core, pc: Address(register(machine, core, arm::Register::R(15))?), registers, sp: Address(register(machine, core, arm::Register::R(13))?), lr: Address(register(machine, core, arm::Register::R(14))?), cpsr: Word(register(machine, core, arm::Register::Cpsr)?), counts: machine.session.board.instruction_counts().try_into().map_err(|_| "expected two cores")?, identities, tcm })
}
fn view(machine: &Machine, maps: &[MemoryMap], state: &Context, object: Object, address: u32, length: usize) -> Result<View, String> {
    let physical = address & !0x40000000;
    let end = u64::from(physical).checked_add(length as u64).ok_or("resource extent overflow")?;
    require(length != 0 && end <= 0x20000000, "resource is outside admitted DRAM")?;
    let mut bytes = vec![0; length];
    let mut observed = vec![0; length];
    let mut aliases: [[Digest; 2]; 2] = std::array::from_fn(|_| std::array::from_fn(|_| Digest(String::new())));
    for (core, digests) in aliases.iter_mut().enumerate() {
        for (alias, digest) in digests.iter_mut().enumerate() {
            let guest = u64::from(physical) | if alias == 0 { 0 } else { 0x40000000 };
            let span = static_span(maps, MemoryCore(core), MemoryAddress(guest)..MemoryAddress(guest + length as u64))?;
            require(span.physical.0 == u64::from(physical), "resource alias translation differs")?;
            for bank in state.tcm[core] {
                if let Some((low, high)) = tcm_window(TcmRegister(bank.0))? { require(guest + length as u64 <= low || guest >= high, "resource overlaps active TCM")?; }
            }
            machine.session.board.read_memory(core, guest, &mut observed)?;
            if core == 0 && alias == 0 { bytes.copy_from_slice(&observed); }
            require(observed == bytes, "resource core/alias values differ")?;
            *digest = Digest(sha256(&observed));
        }
    }
    machine.session.board.read_physical(u64::from(physical), &mut observed)?;
    require(observed == bytes, "committed physical view differs")?;
    Ok(View { object, physical: Address(physical), bytes, aliases })
}
fn word(bytes: &[u8], offset: usize) -> Result<u32, String> { Ok(u32::from_le_bytes(bytes.get(offset..offset + 4).ok_or("word exceeds object")?.try_into().map_err(|_| "invalid word extent")?)) }
fn pointer(machine: &Machine, core: Core, address: u32) -> Result<u32, String> {
    let mut bytes = [0; 4]; machine.session.board.read_memory(core.index(), u64::from(address), &mut bytes)?; Ok(u32::from_le_bytes(bytes))
}
fn kernel(machine: &Machine, maps: &[MemoryMap], state: &Context, core: Core) -> Result<Kernel, String> {
    let l = core.layout();
    let mut views = Vec::with_capacity(15);
    for (kind, base, bytes) in [(Object::CreationRecords, l.creation, 40 * COUNT), (Object::TaskNodes, l.tasks, 56 * COUNT), (Object::IdMap, l.ids, 4 * COUNT), (Object::AuxiliaryRecords, l.auxiliary, 12 * COUNT), (Object::AuxiliaryNodes, l.auxiliary_nodes, 16 * COUNT), (Object::Domains, l.domains, 264), (Object::Control, l.control, 0x1c8)] { views.push(view(machine, maps, state, kind, base, bytes as usize)?); }
    let creation = &views[0].bytes;
    let nodes = &views[1].bytes;
    let tasks = (0..COUNT).map(|i| {
        let c = (40 * i) as usize; let t = (56 * i) as usize;
        Ok(TaskState { id: TaskId((i + 1) as u16), creation: Address(l.creation + 40 * i), creation_pointer: Address(word(nodes, t + 8)?), creation_id: Word(word(creation, c + 32)?), node: Address(l.tasks + 56 * i), flags: Word(word(creation, c)?), argument: Word(word(creation, c + 4)?), entry: Address(word(creation, c + 8)?), priority: Word(word(creation, c + 12)?), stack_bytes: Word(word(creation, c + 16)?), stack_base: Address(word(creation, c + 20)?), created: Word(word(creation, c + 36)?), state: nodes[t + 12], effective_priority: nodes[t + 13], pending: nodes[t + 14], saved_sp: Address(word(nodes, t + 24)?), resume_pc: Address(word(nodes, t + 28)?) })
    }).collect::<Result<Vec<_>, String>>()?;
    let domain = pointer(machine, core, l.domain_pointer)?;
    require(domain == l.domains || domain == l.domains + 132, "current domain is outside canonical two-domain array")?;
    for offset in [0u32, 132] {
        let d = l.domains + offset;
        for (kind, field, bytes) in [(Object::QueueHeads, 0x20, 128), (Object::Bitmap, 0x24, 4), (Object::Selected, 0x2c, 4), (Object::Current, 0x28, 4)] {
            let target = pointer(machine, core, d + field)?;
            require(target != 0, "domain resource pointer is null")?;
            views.push(view(machine, maps, state, kind, target, bytes)?);
        }
    }
    Ok(Kernel { core, views, tasks, domain: Address(domain), selected: Address(pointer(machine, core, l.selected)?), current: Address(pointer(machine, core, l.current)?) })
}
fn code_word(machine: &Machine, maps: &[MemoryMap], state: &Context, sources: &[Code]) -> Result<Word, String> {
    let code = sources.iter().find(|code| u64::from(state.pc.0) >= u64::from(code.base) && u64::from(state.pc.0) + 4 <= u64::from(code.base) + code.bytes.len() as u64).ok_or("instruction point is outside canonical code")?;
    let expected = word(&code.bytes, (state.pc.0 - code.base) as usize)?;
    let actual = view(machine, maps, state, Object::EntryCode, state.pc.0, 4)?;
    require(word(&actual.bytes, 0)? == expected, "instruction point differs from canonical code")?;
    Ok(Word(expected))
}
fn sources(root: &Path) -> Result<Vec<Code>, String> {
    [("decode/loader-40000.bin", 0x40000, "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"), ("decode/loader-d0000.bin", 0xd0000, "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"), ("lz/id4-0061c400.bin", 0x61c400, "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")].into_iter().map(|(name, base, expected)| {
        let bytes = read(&root.join(name))?; require(sha256(&bytes) == expected, "noncanonical source image")?; Ok(Code { name, base, bytes })
    }).collect()
}
fn sites() -> BTreeMap<u32, String> {
    [(0x725dd8, "submission"), (0x725f0c, "auxiliary-return"), (0x725fa8, "creation-call"), (0x725fac, "creation-return"), (0x726024, "activation-call"), (0x726028, "activation-return"), (0x69ce8c, "wrapper-return"), (0x69cee8, "wrapper-return"), (0x69cf38, "wrapper-return"), (0x69cf70, "wrapper-return"), (0x43f9c, "auto-ready-entry"), (0xd3f58, "auto-ready-entry"), (0x40a74, "auto-ready-return"), (0xd0a74, "auto-ready-return"), (0x43f70, "prepare"), (0xd3f2c, "prepare"), (0x43f98, "prepared"), (0xd3f54, "prepared"), (0x43688, "scheduler"), (0xd3688, "scheduler"), (0x436bc, "selected"), (0xd36bc, "selected"), (0x436e8, "stack-transfer"), (0xd36e8, "stack-transfer"), (0x43728, "trampoline"), (0xd3728, "trampoline"), (0x43758, "entry-transfer"), (0xd3758, "entry-transfer"), (0x69d160, "task-entry"), (0x69cbf0, "task-entry"), (0x69cd58, "task-entry"), (0x69d164, "entered-instruction"), (0x69cbf4, "entered-instruction"), (0x69cd5c, "entered-instruction")].into_iter().map(|(address, label)| (address, label.into())).collect()
}
fn boundary(address: u32) -> Result<Boundary, String> {
    Ok(match address {
        0x725dd8 => Boundary::Submission, 0x725f0c => Boundary::AuxiliaryReturn, 0x725fa8 => Boundary::CreationCall, 0x725fac => Boundary::CreationReturn, 0x726024 => Boundary::ActivationCall, 0x726028 => Boundary::ActivationReturn,
        0x69ce8c | 0x69cee8 | 0x69cf38 | 0x69cf70 => Boundary::WrapperReturn,
        0x43f9c | 0xd3f58 => Boundary::AutoReadyEntry, 0x40a74 | 0xd0a74 => Boundary::AutoReadyReturn, 0x43f70 | 0xd3f2c => Boundary::Prepare, 0x43f98 | 0xd3f54 => Boundary::Prepared,
        0x43688 | 0xd3688 => Boundary::Scheduler, 0x436bc | 0xd36bc => Boundary::Selected, 0x436e8 | 0xd36e8 => Boundary::StackTransfer, 0x43728 | 0xd3728 => Boundary::Trampoline, 0x43758 | 0xd3758 => Boundary::EntryTransfer,
        0x69d160 | 0x69cbf0 | 0x69cd58 => Boundary::TaskEntry, 0x69d164 | 0x69cbf4 | 0x69cd5c => Boundary::EnteredInstruction,
        _ => return Err("unknown observation boundary".into()),
    })
}
fn caller(core: Core, lr: u32) -> Option<TaskId> { match (core, lr) { (Core::Primary, 0x69ce8c) => Some(TaskId(104)), (Core::Primary, 0x69cee8) => Some(TaskId(102)), (Core::Secondary, 0x69cf38) => Some(TaskId(103)), (Core::Secondary, 0x69cf70) => Some(TaskId(93)), _ => None } }
fn watch_ranges(sources: &[Code]) -> Vec<WatchRange> {
    let mut ranges: Vec<_> = [(0x4dae0, 0x4f10c), (0xddaa0, 0xdf0cc), (0x545a4, 0x63460), (0xe4564, 0xf3410), (0xc0800, 0xc0900), (0x96bd20, 0x96bd28), (0xa0fc44, 0xa101e4), (0xa5b204, 0xa5b5c4), (0x89ea94, 0x89ea9c), (0x9c280c, 0x9c2810), (0xa10200, 0xa5b200)].into_iter().map(|(start, end)| WatchRange { start, end, kinds: WatchKinds::WRITES }).collect();
    ranges.extend(sources.iter().map(|code| WatchRange { start: u64::from(code.base), end: u64::from(code.base) + code.bytes.len() as u64, kinds: WatchKinds::CPU_FETCH.union(WatchKinds::WRITES) }));
    ranges
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
        if !["--run", "--update", "--boot-rom", "--analysis", "--output"].contains(&flag.as_str()) || arguments.insert(flag, PathBuf::from(args.next().ok_or(USAGE)?)).is_some() { return Err(USAGE.into()); }
    }
    let mut take = |flag| arguments.remove(flag).ok_or_else(|| USAGE.to_owned());
    let run_bytes = read(&take("--run")?)?; let update = read(&take("--update")?)?; let rom = read(&take("--boot-rom")?)?; let code = sources(&take("--analysis")?)?; let output = take("--output")?;
    let identity = Identity { run: Digest(sha256(&run_bytes)), update: Digest(sha256(&update)), boot_rom: Digest(sha256(&rom)), observer: Digest(sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?)), sources: code.iter().map(|code| SourceIdentity { name: code.name, base: Address(code.base), bytes: code.bytes.len(), sha256: Digest(sha256(&code.bytes)) }).collect() };
    require(identity.run.0 == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef" && identity.update.0 == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490" && identity.boot_rom.0 == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be", "observer requires canonical stock inputs")?;
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    fujimod_emulator_exr2_board::register_board_builder(); fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?; let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(update), &rom)?; session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?; machine.disarm()?;
    let reset = machine.session.checkpoint(None)?.export()?; write(&output.join("reset.fujimod"), &reset)?;
    let snapshot = machine.session.import_snapshot(&reset)?; machine.session.restore(&snapshot)?; machine.rearm()?;
    require(machine.instructions() == 0, "reset checkpoint is not preexecution")?;
    let sites = sites(); machine.traces.bound_full_registers(NonZeroUsize::new(HIT_LIMIT).ok_or("invalid hit bound")?);
    machine.traces.stop_labels.extend(sites.values().cloned()); machine.traces.arm(&machine.session, sites.clone())?;
    let mut watch = None; let mut samples = Vec::new(); let mut submissions = Vec::<Submission>::new(); let mut first_tasks = Vec::<FirstTask>::new(); let mut processed = 0; let mut startup_end_counts = None;
    let result = (|| -> Result<(), String> {
        loop {
            let stopped = machine.run(DEADLINE.saturating_sub(machine.instructions())); machine.require_running()?;
            require(stopped == RunOutcome::Stopped, "startup observation deadline")?;
            let end = machine.traces.hits.len();
            for index in processed..end {
                let hit = &machine.traces.hits[index]; let address = number(&hit.address)?; let core = Core::from_index(hit.core)?;
                if watch.is_none() && address != 0x725dd8 { continue; }
                let state = context(&machine, core)?; require(state.pc.0 == address, "stopped context moved past instruction point")?;
                let kind = boundary(address)?;
                if watch.is_none() {
                    require(caller(core, state.lr.0).is_some(), "first wrapper is outside the four admitted phase callers")?;
                    for source in &code { let observed = view(&machine, &maps, &state, Object::EntryCode, source.base, source.bytes.len())?; require(observed.bytes == source.bytes, "loaded canonical image differs before startup watch")?; }
                    watch = Some(machine.session.board.memory_watch_begin_classified(watch_ranges(&code), WATCH_LIMIT, maps.clone(), ViewPolicy::Observe)?);
                }
                let mut local = Vec::new();
                local.push(view(&machine, &maps, &state, Object::SharedDomains, 0x400c0800, 256)?);
                let roots = view(&machine, &maps, &state, Object::InterfaceRoots, 0x96bd20, 8)?;
                for (offset, expected) in [(0, 0x4004dae0), (4, 0x400ddaa0)] {
                    let root = word(&roots.bytes, offset)?;
                    require(root == expected, "installed interface root differs")?;
                    local.push(view(&machine, &maps, &state, Object::InterfaceTable, root, 0x162c)?);
                }
                local.push(roots);
                if address == 0x725dd8 {
                    if let Some(id) = caller(core, state.lr.0) {
                        require(!submissions.iter().any(|item| item.id == id), "startup task submitted twice")?;
                        let descriptor = state.registers[0].0;
                        let observed = view(&machine, &maps, &state, Object::Descriptor, descriptor, 28)?;
                        require(word(&observed.bytes, 0)? == u32::from(id.0), "startup descriptor id differs from caller")?;
                        submissions.push(Submission { id, core, descriptor: Address(descriptor), caller_sp: state.sp, return_pc: state.lr, entry_hit: index, return_hit: None, status: None }); local.push(observed);
                    }
                }
                if let Some(item) = submissions.iter_mut().find(|item| item.core == core && item.return_pc.0 == address && item.return_hit.is_none()) { item.return_hit = Some(index); item.status = Some(state.registers[0]); }
                if let Some(item) = submissions.iter().rev().find(|item| item.core == core && (item.return_hit.is_none() || item.return_hit == Some(index))) {
                    local.push(view(&machine, &maps, &state, Object::Descriptor, item.descriptor.0, 28)?);
                    local.push(view(&machine, &maps, &state, Object::Stack, item.caller_sp.0.checked_sub(56).ok_or("wrapper stack underflow")?, 88)?);
                    let i = u32::from(item.id.0) - 1;
                    let wrapper = view(&machine, &maps, &state, Object::Wrapper, 0x40a5b204 + 8 * i, 8)?;
                    let base = word(&wrapper.bytes, 0)?; let bytes = word(&wrapper.bytes, 4)?;
                    if base != 0 {
                        let physical = base & !0x40000000;
                        require(bytes == 1024 && physical >= 0xa10200 && physical.checked_add(bytes).is_some_and(|end| end <= 0xa5b200), "startup storage leaves admitted pool")?;
                        local.push(view(&machine, &maps, &state, Object::Stack, base, bytes as usize)?);
                    }
                    local.push(wrapper);
                    local.push(view(&machine, &maps, &state, Object::Storage, 0x40a0fc44 + 12 * i, 12)?);
                }
                let kernels = [Core::Primary, Core::Secondary].into_iter().map(|core| kernel(&machine, &maps, &state, core)).collect::<Result<Vec<_>, _>>()?;
                if matches!(kind, Boundary::Selected) && !first_tasks.iter().any(|item| item.core == core) {
                    let l = core.layout(); let node = kernels[core.index()].current.0;
                    require(node >= l.tasks && (node - l.tasks) % 56 == 0 && (node - l.tasks) / 56 < COUNT, "first selected task is not a canonical node")?;
                    let task = &kernels[core.index()].tasks[((node - l.tasks) / 56) as usize];
                    require(task.creation_pointer == task.creation && task.creation_id.0 == u32::from(task.id.0) && kernels[core.index()].selected.0 == node, "first selected creation/current/runnable identity differs")?;
                    require([0x69d160, 0x69cbf0, 0x69cd58].contains(&task.entry.0), "first selected entry lacks an admitted sequential stock prologue")?;
                    require(task.saved_sp.0 >= task.stack_base.0 && task.saved_sp.0 == task.stack_base.0.checked_add(task.stack_bytes.0).ok_or("task stack wraps")?, "first selected stack is not creation stack-top")?;
                    first_tasks.push(FirstTask { core, id: task.id, node: Address(node), creation: task.creation, stack_top: task.saved_sp, entry: task.entry, selected_hit: index, transfer_hit: None, entry_hit: None, entry_counts: None, entered_instruction_hit: None });
                }
                if let Some(first) = first_tasks.iter_mut().find(|item| item.core == core) {
                    if matches!(kind, Boundary::EntryTransfer) { first.transfer_hit.get_or_insert(index); }
                    if address == first.entry.0 && first.entry_hit.is_none() {
                        require(state.sp == first.stack_top && state.registers[0] == Word(0), "first task argument or stack differs")?;
                        first.entry_hit = Some(index);
                        require(state.registers[1].0 == first.node.0 && state.registers[2].0 == first.creation.0 && state.lr.0 == match core { Core::Primary => 0x453f0, Core::Secondary => 0xd53b0 } && state.cpsr.0 & 0xff0000ff == 0x53, "first task trampoline context differs")?;
                        first.entry_counts = Some(state.counts);
                        local.push(view(&machine, &maps, &state, Object::Stack, state.sp.0.checked_sub(64).ok_or("entry stack underflow")?, 96)?);
                    }
                    if address == first.entry.0 + 4 && first.entry_hit.is_some() && first.entered_instruction_hit.is_none() {
                        let frame = match first.entry.0 { 0x69d160 => 16, 0x69cbf0 => 4, 0x69cd58 => 20, _ => return Err("unadmitted task prologue".into()) };
                        let counts = first.entry_counts.ok_or("first entry counts absent")?;
                        require(state.counts[core.index()] == counts[core.index()] + 1 && state.sp.0.checked_add(frame) == Some(first.stack_top.0), "first prologue did not execute exactly one instruction")?;
                        first.entered_instruction_hit = Some(index);
                        local.push(view(&machine, &maps, &state, Object::Stack, first.stack_top.0.checked_sub(64).ok_or("entry stack underflow")?, 96)?);
                    }
                }
                let cursor = watch.as_ref().ok_or("startup watch absent")?.records.borrow().state().get("records").and_then(serde_json::Value::as_array).ok_or("watch records absent")?.len();
                let instruction = code_word(&machine, &maps, &state, &code)?;
                samples.push(Sample { hit: index, boundary: kind, context: state, code_word: instruction, watch_cursor: cursor, kernels, local });
            }
            processed = end;
            if first_tasks.len() == 2 && first_tasks.iter().all(|item| item.entered_instruction_hit.is_some()) {
                require(submissions.len() == 4 && submissions.iter().all(|item| item.return_hit.is_some()), "first entries precede complete startup submissions")?;
                startup_end_counts = Some(machine.session.board.instruction_counts().try_into().map_err(|_| "expected two cores")?); break;
            }
        }
        Ok(())
    })();
    let raw = watch.as_ref().map(|watch| watch.records.borrow().state());
    let accesses = serde_json::to_vec_pretty(&raw).map_err(|error| error.to_string())?; write(&output.join("memory-watch.json"), &accesses)?;
    let summary = raw.as_ref().ok_or_else(|| "startup watch never began".to_owned()).and_then(|raw| serde_json::from_value::<WatchSummary>(raw.clone()).map_err(|error| error.to_string()));
    let watch_end_error = watch.as_mut().and_then(|watch| machine.session.board.memory_watch_end(watch).err());
    machine.traces.disarm(&machine.session)?; machine.traces.stop_labels.clear();
    let outcome = result.and_then(|()| { require(summary.as_ref().is_ok_and(|summary| summary.enabled && summary.violation.is_none()) && watch_end_error.is_none(), "startup watch refused")?; menu_route(&mut machine) });
    let final_menu = machine.menu().map(|menu| menu.map(|menu| [menu.menu_id, menu.setting_id, menu.cursor]));
    let receipt = Receipt { schema: 1, scope: "stock-natural-reset-startup-task-objects-through-both-first-prologue-instructions;unwatched-continuation-to-menu", outcome: if outcome.is_ok() { Outcome::ObservedMenu } else { Outcome::Refused }, error: outcome.as_ref().err().map(String::as_str), machine_failure: machine.failure.as_deref(), identity, checkpoint_sha256: Digest(sha256(&reset)), checkpoint_reused: true, instructions: machine.instructions(), startup_end_counts, memory_watch_sha256: Digest(sha256(&accesses)), watch: summary, watch_end_error, sites: &sites, hits: &machine.traces.hits, submissions: &submissions, first_tasks: &first_tasks, samples: &samples, final_menu };
    write(&output.join("startup-tasks.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    outcome
}
fn main() -> ExitCode { match drive() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } } }
