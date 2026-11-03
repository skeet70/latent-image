use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::bridge::{read, sha256, write};
use fujimod_xm1::machine::{Machine, RunOutcome};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, path::{Path, PathBuf}, process::ExitCode};

#[path = "memory_view.rs"]
mod memory_view;
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};

const USAGE: &str = "kernel-install-observe --program retained|owned --scenario natural|invalid-marker --run RUN --update DAT --boot-rom ROM --analysis ROOT --image IMAGE --symbols SYMBOLS --contract CONTRACT --output DIR";
const ENTRY: u32 = 0x725a04;
const END: u32 = 0x725b54;
const RETURN: u32 = 0x69ce38;
const DEADLINE: u64 = 8_000_000;

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct Address(u32);
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct Word(u32);
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
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
#[derive(Clone, Serialize)]
#[serde(transparent)]
struct Bytes(Vec<u8>);
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Program { Retained, Owned }
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Scenario { Natural, InvalidMarker }
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Qualification { HostAdmittedInstallation, ConditionalHostAdmission }
#[derive(Deserialize)]
struct ArtifactContract {
    schema: u32, qualification: Qualification,
    linked_sha256: Digest, symbols_sha256: Digest, linked_bytes: usize,
    patch: [Address; 2], scratch_low_from_installer_sp: i32,
    invalid_marker_loop: Address, execution_pcs: Vec<Address>,
}
#[derive(Deserialize)]
struct CandidateContract {
    schema: u32, qualification: Qualification, kernel_install: ArtifactContract,
    candidate_dat_sha256: Digest,
}
#[derive(Clone, Serialize)]
struct Context {
    pc: Address, registers: [Word; 13], sp: Address, lr: Address, cpsr: Word,
    counts: [u64; 2], tcm: [[Word; 2]; 2], identities: [Word; 2],
}
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Source { CpuRead, CpuWrite, CpuFetch, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ByteOrder { Little, Big }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MemoryEvent {
    source: Source, address: u64, physical: Option<u64>, size: u64,
    value: Option<u64>, before: Option<Bytes>, byte_order: Option<ByteOrder>,
    core: Option<usize>, pc: Option<Address>, instructions: Option<u64>, tick: Option<u64>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WatchClass { range: [u64; 2], kinds: Vec<Source> }
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ViewsPolicy { Observe }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WatchState {
    enabled: bool, ranges: Vec<[u64; 2]>, classes: Vec<WatchClass>, views: ViewsPolicy,
    records: Vec<MemoryEvent>, violation: Option<String>,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Object { Roots, ServiceStorage, TaskStorage, Stack, Code, Cache, Header0, Header1, Marker, Mode, PeerCode, Completion }
#[derive(Serialize)]
struct View { object: Object, core: usize, guest: Address, physical: Address, bytes: Bytes }
#[derive(Serialize)]
struct Mutation { address: Address, before: Word, after: Word }
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome { Returned, Nonreturning, Refused }
#[derive(Serialize)]
struct Identity { run: Digest, update: Digest, boot_rom: Digest, image: Digest, symbols: Digest, contract: Digest, observer: Digest }
#[derive(Default, Serialize)]
struct Observation {
    entry: Option<Context>, cache: Option<Context>, cache_return: Option<Context>,
    after: Option<Context>, mutation: Option<Mutation>,
    before: Vec<View>, committed: Vec<View>, watch: Option<WatchState>, watch_end_error: Option<String>,
}
#[derive(Serialize)]
struct Receipt<'a> {
    schema: u32, program: Program, scenario: Scenario, outcome: Outcome,
    error: Option<&'a str>, machine_failure: Option<&'a str>, identity: Identity,
    checkpoint_sha256: Digest, checkpoint_reused: bool, instructions: u64,
    observation: Observation,
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<u8>::deserialize(deserializer).map(Self)
    }
}
fn require(premise: bool, message: &str) -> Result<(), String> {
    if premise { Ok(()) } else { Err(message.into()) }
}
fn register(machine: &Machine, core: usize, register: arm::Register) -> Result<u32, String> {
    u32::try_from(machine.session.board.read_register(core, Register::Arm32(register))?).map_err(|_| "register exceeds word".into())
}
fn context(machine: &Machine) -> Result<Context, String> {
    let mut registers = [Word(0); 13];
    for (index, word) in registers.iter_mut().enumerate() { *word = Word(register(machine, 0, arm::Register::R(index as u8))?); }
    let mut tcm = [[Word(0); 2]; 2];
    let mut identities = [Word(0); 2];
    for core in 0..2 {
        identities[core] = Word(u32::try_from(machine.session.board.cpu(core)?.rprofile_identity(None)?).map_err(|_| "identity exceeds word")?);
        for (index, opc2) in [1,0].into_iter().enumerate() {
            tcm[core][index] = Word(u32::try_from(machine.session.board.cpu(core)?.coprocessor(arm::Coprocessor {
                cp: 15, opc1: 0, crn: 9, crm: 1, opc2, secure: 0, wide: false,
            }, None)?).map_err(|_| "TCM register exceeds word")?);
        }
    }
    Ok(Context { pc: Address(register(machine, 0, arm::Register::R(15))?), registers,
        sp: Address(register(machine, 0, arm::Register::R(13))?), lr: Address(register(machine, 0, arm::Register::R(14))?),
        cpsr: Word(register(machine, 0, arm::Register::Cpsr)?),
        counts: machine.session.board.instruction_counts().try_into().map_err(|_| "expected two cores")?, tcm, identities })
}
fn read_word(machine: &Machine, address: Address) -> Result<Word, String> {
    let mut bytes = [0; 4];
    machine.session.board.read_memory(0, u64::from(address.0), &mut bytes)?;
    Ok(Word(u32::from_le_bytes(bytes)))
}
fn mapped(machine: &Machine, maps: &[fujimod_emulator::board::factory::MemoryMap], state: &Context,
    core: usize, guest: u32, physical: u32, bytes: &mut [u8]) -> Result<(), String> {
    let end = u64::from(guest) + bytes.len() as u64;
    let span = static_span(maps, MemoryCore(core), MemoryAddress(u64::from(guest))..MemoryAddress(end))?;
    require(span.physical.0 == u64::from(physical), "static alias translation differs")?;
    for bank in state.tcm[core] {
        if let Some((low,high)) = tcm_window(TcmRegister(bank.0))? {
            require(end <= low || u64::from(guest) >= high, "resource aliases active TCM")?;
        }
    }
    machine.session.board.read_memory(core, u64::from(guest), bytes)
}
fn views(machine: &Machine, maps: &[fujimod_emulator::board::factory::MemoryMap], state: &Context) -> Result<Vec<View>, String> {
    let stack = state.sp.0.checked_sub(64).ok_or("stack underflow")? & 0x3fff_ffff;
    let objects = [(Object::Roots,0x96bd10,64),(Object::ServiceStorage,0x9c34f0,0xf64),
        (Object::TaskStorage,0xa0f85c,0x998),(Object::Stack,stack,128),
        (Object::Code,ENTRY,END-ENTRY),(Object::Cache,0x42634,0x74),
        (Object::Header0,0x40010,4),(Object::Header1,0xd0010,4),(Object::Marker,0x7caf3c,4),(Object::Mode,0x61c008,4),
        (Object::PeerCode,0xd0a44,32),(Object::Completion,0xa00c4,4)];
    let mut result = Vec::with_capacity(objects.len()*4);
    for (object,physical,size) in objects {
        for core in 0..2 {
            for alias in [0,0x4000_0000] {
                let guest = physical + alias;
                let mut bytes = vec![0; size as usize];
                mapped(machine,maps,state,core,guest,physical,&mut bytes)?;
                result.push(View { object,core,guest: Address(guest),physical: Address(physical),bytes: Bytes(bytes) });
            }
        }
    }
    Ok(result)
}
fn stop(machine: &mut Machine, address: u32, label: &str, budget: u64) -> Result<(), String> {
    machine.traces.arm(&machine.session, BTreeMap::from([(address,label.into())]))?;
    machine.traces.stop_labels = [label.into()].into_iter().collect();
    require(machine.run(budget) == RunOutcome::Stopped, "natural instruction boundary was not observed")?;
    let hit = machine.traces.hits.last().ok_or("stop hit absent")?;
    require(hit.site == label && hit.core == 0, "stop is not the primary natural caller")
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Flag { Program, Scenario, Run, Update, Rom, Analysis, Image, Symbols, Contract, Output }
fn arguments() -> Result<(Program,Scenario,BTreeMap<Flag,PathBuf>),String> {
    let mut values = BTreeMap::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag = match flag.to_str().ok_or(USAGE)? {
            "--program" => Flag::Program, "--scenario" => Flag::Scenario, "--run" => Flag::Run,
            "--update" => Flag::Update, "--boot-rom" => Flag::Rom, "--analysis" => Flag::Analysis,
            "--image" => Flag::Image, "--symbols" => Flag::Symbols, "--contract" => Flag::Contract,
            "--output" => Flag::Output, _ => return Err(USAGE.into()),
        };
        require(values.insert(flag,PathBuf::from(args.next().ok_or(USAGE)?)).is_none(), "duplicate argument")?;
    }
    let program = match values.remove(&Flag::Program).as_deref().and_then(Path::to_str) {
        Some("retained") => Program::Retained, Some("owned") => Program::Owned, _ => return Err(USAGE.into()),
    };
    let scenario = match values.remove(&Flag::Scenario).as_deref().and_then(Path::to_str) {
        Some("natural") => Scenario::Natural, Some("invalid-marker") => Scenario::InvalidMarker, _ => return Err(USAGE.into()),
    };
    require(values.len() == 8, USAGE)?;
    Ok((program,scenario,values))
}
fn pinned(root: &Path, path: &str, expected: &str) -> Result<Vec<u8>,String> {
    let bytes = read(&root.join(path))?;
    require(sha256(&bytes) == expected, "noncanonical analysis input")?;
    Ok(bytes)
}
fn backed_ranges(document: &RunDocument) -> Result<Vec<WatchRange>,String> {
    let kinds = WatchKinds::CPU_FETCH.union(WatchKinds::CPU_READ).union(WatchKinds::WRITES);
    let mut regions = run::memory_backings(document)?.into_iter().map(|region| {
        let end = region.base.checked_add(region.size as u64).ok_or("backing extent overflow")?;
        require(region.base < end && end <= 1u64 << 32, "backing extent differs")?;
        Ok(WatchRange { start: region.base,end,kinds })
    }).collect::<Result<Vec<_>,String>>()?;
    regions.sort_unstable_by_key(|range| range.start);
    let mut ranges: Vec<WatchRange> = Vec::with_capacity(regions.len());
    for region in regions {
        if let Some(last) = ranges.last_mut().filter(|last| region.start <= last.end) {
            last.end = last.end.max(region.end);
        } else {
            ranges.push(region);
        }
    }
    require(!ranges.is_empty(), "physical backing absent")?;
    Ok(ranges)
}
fn drive() -> Result<(),String> {
    let (program,scenario,mut values) = arguments()?;
    let mut take = |flag| values.remove(&flag).ok_or_else(|| USAGE.to_owned());
    let run_path = take(Flag::Run)?;
    let update_path = take(Flag::Update)?;
    let rom_path = take(Flag::Rom)?;
    let analysis = take(Flag::Analysis)?;
    let image_path = take(Flag::Image)?;
    let symbols_path = take(Flag::Symbols)?;
    let contract_path = take(Flag::Contract)?;
    let output = take(Flag::Output)?;
    let run_bytes = read(&run_path)?;
    let update = read(&update_path)?;
    let rom = read(&rom_path)?;
    let image = read(&image_path)?;
    let symbols = read(&symbols_path)?;
    let contract_bytes = read(&contract_path)?;
    let contract: CandidateContract = serde_json::from_slice(&contract_bytes).map_err(|error| error.to_string())?;
    let artifact = &contract.kernel_install;
    require(contract.schema == 1 && contract.qualification == Qualification::HostAdmittedInstallation
        && artifact.schema == 1 && artifact.qualification == Qualification::ConditionalHostAdmission
        && artifact.patch == [Address(ENTRY),Address(END)] && artifact.linked_bytes == image.len()
        && !image.is_empty() && image.len() <= (END-ENTRY) as usize && image.len()%4 == 0
        && artifact.linked_sha256 == digest(&image) && artifact.symbols_sha256 == digest(&symbols)
        && (-64..=-8).contains(&artifact.scratch_low_from_installer_sp)
        && artifact.execution_pcs.iter().all(|pc| (ENTRY..END).contains(&pc.0) && pc.0%4 == 0)
        && artifact.execution_pcs.contains(&artifact.invalid_marker_loop), "owned artifact contract differs")?;
    require(sha256(&run_bytes) == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
        && sha256(&rom) == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be", "run or ROM differs")?;
    require(match program {
        Program::Retained => sha256(&update) == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490",
        Program::Owned => digest(&update) == contract.candidate_dat_sha256,
    }, "update differs from admitted role")?;
    let mut expected = pinned(&analysis,"lz/id4-0061c400.bin","f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")?;
    let loader_a = pinned(&analysis,"decode/loader-40000.bin","21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")?;
    let loader_b = pinned(&analysis,"decode/loader-d0000.bin","8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")?;
    if program == Program::Owned {
        let offset = (ENTRY-0x61c400) as usize;
        expected[offset..offset+image.len()].copy_from_slice(&image);
        for word in expected[offset+image.len()..offset+(END-ENTRY) as usize].chunks_exact_mut(4) { word.copy_from_slice(&0xe320f000u32.to_le_bytes()); }
    }
    let identity = Identity { run: digest(&run_bytes), update: digest(&update), boot_rom: digest(&rom),
        image: digest(&image), symbols: digest(&symbols), contract: digest(&contract_bytes),
        observer: digest(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?) };
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document,&ValidatedUpdate(update),&rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document,session)?;
    machine.disarm()?;
    let snapshot = machine.session.checkpoint(None)?.export()?;
    write(&output.join("reset.fujimod"),&snapshot)?;
    let restored = machine.session.import_snapshot(&read(&output.join("reset.fujimod"))?)?;
    machine.session.restore(&restored)?;
    machine.rearm()?;
    require(machine.instructions() == 0, "checkpoint is not preexecution reset")?;
    let mut observation = Observation::default();
    let result = (|| -> Result<Outcome,String> {
        stop(&mut machine,ENTRY,"installation-entry",DEADLINE)?;
        let entry = context(&machine)?;
        observation.entry = Some(entry.clone());
        require(entry.pc == Address(ENTRY) && entry.sp == Address(0x40063420) && entry.lr == Address(RETURN)
            && entry.cpsr.0 & 0x07f0ffff == 0xd3 && entry.identities == fujimod_xm1::run::CoreIdentities::Xm1.words().map(Word)
            && entry.registers[0] == Word(0) && read_word(&machine,Address(0x61c008))? == Word(0), "natural phase0 context differs")?;
        let mut buffer = vec![0; expected.len()];
        for core in 0..2 {
            for alias in [0,0x40000000] {
                mapped(&machine,&maps,&entry,core,0x61c400+alias,0x61c400,&mut buffer)?;
                require(buffer == expected, "loaded image4 differs outside or inside installation window")?;
            }
        }
        let mut cache = vec![0;0x74];
        mapped(&machine,&maps,&entry,0,0x42634,0x42634,&mut cache)?;
        require(cache == loader_a[0x2634..0x26a8], "retained cache code differs")?;
        let mut peer = vec![0;32];
        mapped(&machine,&maps,&entry,1,0xd0a44,0xd0a44,&mut peer)?;
        require(peer == loader_b[0xa44..0xa64], "secondary wait code differs")?;
        require(read_word(&machine,Address(0x7caf3c))? == Word(3), "initial marker differs")?;
        if scenario == Scenario::InvalidMarker {
            machine.session.board.write_memory(0,0x7caf3c,&0u32.to_le_bytes())?;
            require(read_word(&machine,Address(0x7caf3c))? == Word(0), "invalid marker mutation did not commit")?;
            observation.mutation = Some(Mutation { address: Address(0x7caf3c),before: Word(3),after: Word(0) });
        }
        observation.before = views(&machine,&maps,&entry)?;
        let mut watch = machine.session.board.memory_watch_begin_classified(
            backed_ranges(&machine.document)?,32768,maps.clone(),ViewPolicy::Observe)?;
        let terminal = if scenario == Scenario::Natural { RETURN } else if program == Program::Retained { 0x725a54 } else { artifact.invalid_marker_loop.0 };
        let body = (|| -> Result<(),String> {
            machine.traces.add_sites(&machine.session,BTreeMap::from([
                (terminal,"installation-terminal".into()),(0x42634,"installation-cache".into()),
            ]))?;
            machine.traces.stop_labels = BTreeSet::from(["installation-terminal".into(),"installation-cache".into()]);
            machine.traces.remove_sites(&machine.session,&[fujimod_xm1::provenance::Guest(ENTRY)])?;
            require(machine.run(100_000) == RunOutcome::Stopped, "cache entry absent")?;
            let cache = context(&machine)?;
            require(cache.pc == Address(0x42634) && cache.registers[0..2] == [Word(0x96bd20),Word(32)], "cache call differs")?;
            let cache_return = cache.lr.0;
            require((ENTRY..END).contains(&cache_return), "cache caller is outside installation")?;
            observation.cache = Some(cache);
            machine.traces.add_sites(&machine.session,BTreeMap::from([(cache_return,"installation-cache-return".into())]))?;
            machine.traces.stop_labels = BTreeSet::from(["installation-terminal".into(),"installation-cache-return".into()]);
            machine.traces.remove_sites(&machine.session,&[fujimod_xm1::provenance::Guest(0x42634)])?;
            require(machine.run(100_000) == RunOutcome::Stopped, "cache return absent")?;
            let returned = context(&machine)?;
            require(returned.pc == Address(cache_return), "cache return PC differs")?;
            observation.cache_return = Some(returned);
            machine.traces.stop_labels = BTreeSet::from(["installation-terminal".into()]);
            machine.traces.remove_sites(&machine.session,&[fujimod_xm1::provenance::Guest(cache_return)])?;
            require(machine.run(100_000) == RunOutcome::Stopped, "installation terminal absent")?;
            let after = context(&machine)?;
            require(after.pc == Address(terminal) && after.tcm == entry.tcm && after.identities == entry.identities, "terminal PC, TCM or identity differs")?;
            observation.after = Some(after);
            observation.committed = views(&machine,&maps,&entry)?;
            Ok(())
        })();
        let observed = serde_json::to_vec_pretty(&watch.records.borrow().state()).map_err(|error| error.to_string());
        let saved = observed.as_ref().map_err(Clone::clone).and_then(|bytes| write(&output.join("memory-watch.json"),bytes));
        let parsed = observed.and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()));
        observation.watch_end_error = machine.session.board.memory_watch_end(&mut watch).err();
        observation.watch = Some(parsed?);
        saved?;
        body?;
        let state = observation.watch.as_ref().ok_or("memory watch absent")?;
        require(state.enabled && state.violation.is_none() && observation.watch_end_error.is_none(), "memory observation refused")?;
        Ok(if scenario == Scenario::Natural { Outcome::Returned } else { Outcome::Nonreturning })
    })();
    let outcome = result.as_ref().copied().unwrap_or(Outcome::Refused);
    let receipt = Receipt { schema: 1,program,scenario,outcome,error: result.as_ref().err().map(String::as_str),
        machine_failure: machine.failure.as_deref(),identity,checkpoint_sha256: digest(&snapshot),checkpoint_reused: true,
        instructions: machine.instructions(),observation };
    write(&output.join("installation.json"),&serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    result.map(|_| ())
}
fn main() -> ExitCode {
    match drive() { Ok(()) => ExitCode::SUCCESS,Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } }
}
