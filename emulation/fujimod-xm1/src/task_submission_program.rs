use super::{Address, Boundary, Code, Digest, require};
use fujimod_xm1::bridge::{read, sha256};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, path::{Path, PathBuf}};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub(super) enum Entry { Submission, Release }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub(super) enum Target {
    #[serde(rename = "FixedService CriticalEnter")] CriticalEnter,
    #[serde(rename = "FixedService CriticalExit")] CriticalExit,
    #[serde(rename = "FixedService Reservation")] Reservation,
    #[serde(rename = "FixedService Fill")] Fill,
    #[serde(rename = "FixedService Cache")] Cache,
    #[serde(rename = "FixedService Log")] Log,
    #[serde(rename = "FixedService ErrorCallback")] ErrorCallback,
    #[serde(rename = "FixedService TryLock")] TryLock,
    #[serde(rename = "FixedService Unlock")] Unlock,
    AuxiliaryService, CreationService, ActivationService, CurrentCoreService, SaveMaskService, RestoreMaskService,
}
#[derive(Deserialize)]
struct Frame { entry: Entry, low_from_entry_sp: i32 }
#[derive(Deserialize)]
pub(super) struct Call {
    pub(super) entry: Entry,
    pub(super) instruction: Address,
    pub(super) continuation: Address,
    pub(super) target: Target,
    pub(super) target_register: Option<u8>,
    actual_sp_from_entry: i32,
    caller_live_words: Vec<i32>,
}
#[derive(Deserialize)]
struct NativeContract {
    schema: u32,
    qualification: NativeClaim,
    source_dat_image4_loader_a_loader_b_sha256: [Digest; 4],
    linked_sha256: Digest,
    symbols_sha256: Digest,
    linked_bytes: usize,
    patch: [Address; 2],
    staged_image4_sha256: Digest,
    frames: Vec<Frame>,
    calls: Vec<Call>,
    execution_pcs: Vec<Address>,
    global_writes: Vec<Address>,
}
#[derive(Deserialize)]
enum NativeClaim { #[serde(rename = "conditional-owned-task-submission-admission")] Conditional }
#[derive(Deserialize)]
enum CandidateClaim { #[serde(rename = "host-admitted-owned-task-submission")] Admitted }
#[derive(Deserialize)]
struct CandidateContract {
    schema: u32,
    qualification: CandidateClaim,
    task_submission: NativeContract,
    candidate_dat_sha256: Digest,
    candidate_image4_sha256: Digest,
}
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum Role { #[default] Default, CompositionReference }
impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value { "task-composition" => Ok(Self::CompositionReference), _ => Err("unknown task observation scope".into()) }
    }
}
pub(super) struct JoinedInputs {
    pub(super) critical_native: PathBuf,
    pub(super) storage_native: PathBuf,
    pub(super) loader_a: PathBuf,
    pub(super) loader_b: PathBuf,
    pub(super) record2: PathBuf,
    pub(super) image4: PathBuf,
}
#[derive(Deserialize)]
enum JoinedClaim { #[serde(rename = "host-admitted-owned-task-composition")] Admitted }
#[derive(Deserialize)]
enum CompositionClaim { #[serde(rename = "conditional-owned-task-composition")] Conditional }
#[derive(Deserialize)]
struct ComponentLengths { wrapper: usize, critical: usize, storage: usize }
#[derive(Deserialize)]
struct CriticalContract {
    linked_image_sha256: Digest, symbols_sha256: Digest,
    entry_adapter: Address, exit_adapter: Address,
    entry_frame_bytes: i32, exit_frame_bytes: i32,
}
#[derive(Deserialize)]
struct StoragePatch { start: Address, end: Address }
#[derive(Deserialize)]
struct StorageContract { linked_sha256: Digest, symbols_sha256: Digest, rust: [Address; 2], patch: StoragePatch }
#[derive(Deserialize)]
struct Execution {
    wrapper_pcs: Vec<Address>, wrapper_global_store_pcs: Vec<Address>,
    critical_pcs: Vec<Address>, critical_counter_store_pcs: Vec<Address>, critical_private_store_pcs: Vec<Address>,
    storage_pcs: Vec<Address>, storage_guest_store_pcs: Vec<Address>,
}
#[derive(Deserialize)]
struct JoinedCall {
    entry: Entry, instruction: Address, continuation: Address, target: Target,
    stack_from_entry: i32, caller_live_words: Vec<i32>, callee_low: i32, nested_low: i32,
}
#[derive(Deserialize)]
struct JoinedStack {
    submission_low: i32, release_low: i32, critical_enter_low: i32, critical_exit_low: i32,
    reservation_low: i32, calls: Vec<JoinedCall>,
}
#[derive(Deserialize)]
struct SourceDigests { loader_a_sha256: Digest, loader_b_sha256: Digest, record2_sha256: Digest, image4_sha256: Digest }
#[derive(Deserialize)]
struct Composition {
    schema: u32, qualification: CompositionClaim, task_submission: NativeContract,
    critical_pair: CriticalContract, task_storage: StorageContract,
    component_linked_bytes: ComponentLengths, execution: Execution, owned_stack: JoinedStack, sources: SourceDigests,
}
#[derive(Deserialize)]
struct JoinedContract {
    schema: u32, qualification: JoinedClaim, composition: Composition,
    candidate_dat_sha256: Digest, candidate_image4_sha256: Digest,
}
pub(super) struct JoinedProgram { contract: JoinedContract, digest: Digest }
pub(super) struct OwnedProgram { contract: CandidateContract, digest: Digest }
pub(super) enum Program { Stock, CompositionReference, Owned(OwnedProgram), Joined(JoinedProgram) }
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(super) enum Summary<'a> {
    Stock,
    Owned { contract_sha256: &'a Digest, linked_sha256: &'a Digest, symbols_sha256: &'a Digest },
    Joined {
        contract_sha256: &'a Digest, linked_sha256: &'a Digest, symbols_sha256: &'a Digest,
        critical_linked_sha256: &'a Digest, critical_symbols_sha256: &'a Digest,
        storage_linked_sha256: &'a Digest, storage_symbols_sha256: &'a Digest,
    },
}

impl Program {
    fn open_joined(contract_path: &Path, native_path: &Path, inputs: &JoinedInputs, update: &Digest, code: &mut Vec<Code>) -> Result<Self, String> {
        let bytes = read(contract_path)?;
        let contract: JoinedContract = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        let c = &contract.composition;
        let _ = (&contract.qualification, &c.qualification);
        require(contract.schema == 1 && c.schema == 1 && update.0 == contract.candidate_dat_sha256.0, "joined candidate identity or schema differs")?;
        let task = read(native_path)?;
        let critical = read(&inputs.critical_native)?;
        let storage = read(&inputs.storage_native)?;
        require(c.task_submission.schema == 1 && c.task_submission.patch == [Address(0x725dd8), Address(0x72612c)]
            && task.len() == 852 && task.len() == c.task_submission.linked_bytes && task.len() == c.component_linked_bytes.wrapper
            && critical.len() == c.component_linked_bytes.critical && !critical.is_empty() && critical.len() <= 0x1400
            && storage.len() == c.component_linked_bytes.storage && storage.len() >= 24 && storage.len() <= 228
            && task.len() % 4 == 0 && critical.len() % 4 == 0 && storage.len() % 4 == 0, "joined native extents differ")?;
        require(sha256(&task) == c.task_submission.linked_sha256.0 && sha256(&critical) == c.critical_pair.linked_image_sha256.0
            && sha256(&storage) == c.task_storage.linked_sha256.0, "joined native identity differs")?;
        validate_task_metadata(&c.task_submission)?;
        require(c.task_storage.patch.start == Address(0x725cc4) && c.task_storage.patch.end == Address(0x725da8)
            && c.task_storage.rust[0].0 >= 0x725cdc && c.task_storage.rust[0].0 < c.task_storage.rust[1].0
            && u64::from(c.task_storage.rust[1].0) <= 0x725cc4 + storage.len() as u64, "joined storage boundary differs")?;
        let sources = [
            source("decode/loader-40000.bin", 0x40000, &inputs.loader_a, &c.sources.loader_a_sha256)?,
            source("decode/loader-d0000.bin", 0xd0000, &inputs.loader_b, &c.sources.loader_b_sha256)?,
            source("record2/critical-pair.bin", 0x10000, &inputs.record2, &c.sources.record2_sha256)?,
            source("lz/id4-0061c400.bin", 0x61c400, &inputs.image4, &c.sources.image4_sha256)?,
        ];
        require(sources[0].bytes.len() == 0xdae0 && sources[1].bytes.len() == 0xdaa0 && sources[2].bytes.len() == 0x10000
            && sources[3].bytes.len() == code.iter().find(|source| source.base == 0x61c400).ok_or("original image4 source absent")?.bytes.len()
            && c.sources.image4_sha256.0 == contract.candidate_image4_sha256.0, "joined staged source extent or identity differs")?;
        require(sources[2].bytes.get(0x400..0x400 + critical.len()) == Some(critical.as_slice())
            && sources[3].bytes.get(0x725dd8 - 0x61c400..0x72612c - 0x61c400) == Some(task.as_slice())
            && sources[3].bytes.get(0x725cc4 - 0x61c400..0x725cc4 - 0x61c400 + storage.len()) == Some(storage.as_slice()),
            "joined staged component bytes differ")?;
        for (site, target) in [(0x7235d8, c.critical_pair.entry_adapter), (0x7236b0, c.critical_pair.exit_adapter)] {
            require(target.0 >= 0x10400 && u64::from(target.0) < 0x10400 + critical.len() as u64 && target.0 & 3 == 0,
                "joined critical adapter leaves linked image")?;
            let displacement = i64::from(target.0) - i64::from(site) - 8;
            let expected = 0xea000000 | ((displacement >> 2) as u32 & 0xffffff);
            require(super::word(&sources[3].bytes, (site - 0x61c400) as usize)? == expected, "joined critical entry branch differs")?;
        }
        validate_composition(c)?;
        code.clear();
        code.extend(sources);
        Ok(Self::Joined(JoinedProgram { contract, digest: Digest(sha256(&bytes)) }))
    }
    pub(super) fn open(role: Role, contract_path: Option<&Path>, native_path: Option<&Path>, joined: Option<&JoinedInputs>, update: &Digest, code: &mut Vec<Code>) -> Result<Self, String> {
        require(role != Role::CompositionReference || (contract_path.is_none() && native_path.is_none() && joined.is_none()),
            "retained composition scope excludes owned inputs")?;
        if let Some(inputs) = joined {
            return Self::open_joined(contract_path.ok_or("joined contract is absent")?, native_path.ok_or("joined task native is absent")?, inputs, update, code);
        }
        let (Some(contract_path), Some(native_path)) = (contract_path, native_path) else {
            require(contract_path.is_none() && native_path.is_none(), "owned program needs both contract and native image")?;
            require(update.0 == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490", "stock program requires original DAT")?;
            return Ok(if role == Role::CompositionReference { Self::CompositionReference } else { Self::Stock });
        };
        let bytes = read(contract_path)?;
        let contract: CandidateContract = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        let native = read(native_path)?;
        let inner = &contract.task_submission;
        let _ = (&contract.qualification, &inner.qualification);
        require(contract.schema == 1 && inner.schema == 1 && native.len() == 852 && inner.linked_bytes == 852
            && inner.patch == [Address(0x725dd8), Address(0x72612c)], "native contract boundary differs")?;
        require(sha256(&native) == inner.linked_sha256.0 && update.0 == contract.candidate_dat_sha256.0, "native or candidate identity differs")?;
        let expected = ["43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490", "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16", "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882", "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"];
        require(inner.source_dat_image4_loader_a_loader_b_sha256.iter().zip(expected).all(|(actual, expected)| actual.0 == expected), "native source identities differ")?;
        validate_task_metadata(inner)?;
        let image = code.iter_mut().find(|code| code.base == 0x61c400).ok_or("image4 source is absent")?;
        image.bytes.get_mut(0x725dd8 - 0x61c400..0x72612c - 0x61c400).ok_or("owned patch leaves source image")?.copy_from_slice(&native);
        let staged_digest = sha256(&image.bytes);
        require(staged_digest == inner.staged_image4_sha256.0 && inner.staged_image4_sha256.0 == contract.candidate_image4_sha256.0, "staged image4 identity differs")?;
        image.sha256 = Digest(staged_digest);
        Ok(Self::Owned(OwnedProgram { contract, digest: Digest(sha256(&bytes)) }))
    }
    pub(super) fn summary(&self) -> Summary<'_> {
        match self {
            Self::Stock | Self::CompositionReference => Summary::Stock,
            Self::Owned(owned) => Summary::Owned { contract_sha256: &owned.digest, linked_sha256: &owned.contract.task_submission.linked_sha256, symbols_sha256: &owned.contract.task_submission.symbols_sha256 },
            Self::Joined(joined) => {
                let c = &joined.contract.composition;
                Summary::Joined { contract_sha256: &joined.digest, linked_sha256: &c.task_submission.linked_sha256, symbols_sha256: &c.task_submission.symbols_sha256,
                    critical_linked_sha256: &c.critical_pair.linked_image_sha256, critical_symbols_sha256: &c.critical_pair.symbols_sha256,
                    storage_linked_sha256: &c.task_storage.linked_sha256, storage_symbols_sha256: &c.task_storage.symbols_sha256 }
            }
        }
    }
    pub(super) fn owned(&self) -> bool { matches!(self, Self::Owned(_) | Self::Joined(_)) }
    pub(super) fn joined(&self) -> bool { matches!(self, Self::Joined(_)) }
    pub(super) fn composition(&self) -> bool { matches!(self, Self::CompositionReference | Self::Joined(_)) }
    fn task(&self) -> Option<&NativeContract> {
        match self { Self::Stock | Self::CompositionReference => None, Self::Owned(owned) => Some(&owned.contract.task_submission), Self::Joined(joined) => Some(&joined.contract.composition.task_submission) }
    }
    pub(super) fn frame_low(&self, entry: Entry) -> Result<i32, String> {
        match self {
            Self::Stock | Self::CompositionReference => Ok(match entry { Entry::Submission => -56, Entry::Release => -32 }),
            Self::Owned(owned) => owned.contract.task_submission.frames.iter().find(|frame| frame.entry == entry)
                .map(|frame| frame.low_from_entry_sp).ok_or_else(|| "admitted native entry frame is absent".into()),
            Self::Joined(joined) => Ok(match entry {
                Entry::Submission => joined.contract.composition.owned_stack.submission_low,
                Entry::Release => joined.contract.composition.owned_stack.release_low,
            }),
        }
    }
    pub(super) fn admits_execution(&self, pc: u32) -> bool {
        let Self::Joined(joined) = self else { return true; };
        let execution = &joined.contract.composition.execution;
        let pcs = if (0x10000..0x20000).contains(&pc) { &execution.critical_pcs }
            else if (0x725cc4..0x725da8).contains(&pc) { &execution.storage_pcs }
            else if (0x725dd8..0x72612c).contains(&pc) { &execution.wrapper_pcs }
            else { return true; };
        pcs.contains(&Address(pc))
    }
    pub(super) fn call(&self, pc: u32, entry: Entry, incoming_sp: Address, actual_sp: Address) -> Result<Option<&Call>, String> {
        match self.task() {
            None => Ok(None),
            Some(task) => task.calls.iter().find(|call| call.instruction.0 == pc
                && call.entry == entry && incoming_sp.0.checked_add_signed(call.actual_sp_from_entry) == Some(actual_sp.0))
                .map(Some).ok_or_else(|| "actual call SP or entry has no emitted provenance alternative".into()),
        }
    }
    pub(super) fn points(&self) -> Result<BTreeMap<u32, Boundary>, String> {
        let mut points = super::sites().into_keys().map(|pc| Ok((pc, super::boundary(pc)?))).collect::<Result<BTreeMap<_, _>, String>>()?;
        points.insert(0x72612c, Boundary::DeletionEntry);
        points.insert(0x726098, Boundary::ReleaseEntry);
        points.insert(0x726240, Boundary::ReleaseReturn);
        let stock_calls = [
            (0x725dfc, Target::CriticalEnter), (0x725e84, Target::CriticalExit), (0x725fb4, Target::CriticalExit), (0x726064, Target::CriticalExit),
            (0x725e74, Target::Reservation), (0x725f68, Target::Fill), (0x725f78, Target::Cache), (0x725f08, Target::AuxiliaryService),
            (0x725fa8, Target::CreationService), (0x726024, Target::ActivationService), (0x726000, Target::CurrentCoreService),
            (0x725e94, Target::Log), (0x725fcc, Target::Log), (0x726040, Target::Log), (0x726074, Target::Log),
            (0x725ea8, Target::ErrorCallback), (0x725fe0, Target::ErrorCallback), (0x726054, Target::ErrorCallback), (0x726088, Target::ErrorCallback),
            (0x7260b8, Target::CurrentCoreService), (0x7260c8, Target::SaveMaskService), (0x7260e4, Target::TryLock), (0x726104, Target::Unlock), (0x72611c, Target::RestoreMaskService),
        ];
        match self.task() {
            None => for (pc, target) in stock_calls { insert(&mut points, pc, call_boundary(target))?; insert(&mut points, pc + 4, return_boundary(target))?; },
            Some(task) => {
                points.retain(|pc, _| !(0x725dd8..0x72612c).contains(pc));
                points.insert(0x725dd8, Boundary::Submission);
                points.insert(0x726098, Boundary::ReleaseEntry);
                for call in &task.calls {
                    insert(&mut points, call.instruction.0, call_boundary(call.target))?;
                    insert(&mut points, call.continuation.0, return_boundary(call.target))?;
                }
                for pc in &task.global_writes {
                    points.entry(pc.0).or_insert(Boundary::NativeWrite);
                }
            }
        }
        if let Self::Joined(joined) = self {
            for pc in &joined.contract.composition.execution.critical_counter_store_pcs {
                points.entry(pc.0).or_insert(Boundary::NativeWrite);
            }
            for pc in &joined.contract.composition.execution.storage_guest_store_pcs {
                points.entry(pc.0).or_insert(Boundary::NativeWrite);
            }
        }
        Ok(points)
    }
}
fn validate_task_metadata(inner: &NativeContract) -> Result<(), String> {
        require(inner.frames.len() == 2 && [Entry::Submission, Entry::Release].into_iter().all(|entry| inner.frames.iter().filter(|frame| frame.entry == entry).count() == 1)
            && inner.frames.iter().all(|frame| (-65536..=0).contains(&frame.low_from_entry_sp)), "native frame metadata differs")?;
        let execution: BTreeSet<_> = inner.execution_pcs.iter().map(|pc| pc.0).collect();
        require(!execution.is_empty() && execution.len() == inner.execution_pcs.len() && execution.len() <= 213
            && execution.iter().all(|pc| (0x725dd8..0x726128).contains(pc) && pc & 3 == 0), "native execution set differs")?;
        require(inner.calls.len() <= 1024 && inner.calls.iter().all(|call| execution.contains(&call.instruction.0)
            && call.instruction.0.checked_add(4) == Some(call.continuation.0)
            && call.target_register.is_none_or(|register| register <= 15)
            && (-65536..=0).contains(&call.actual_sp_from_entry) && call.actual_sp_from_entry % 8 == 0
            && call.caller_live_words.iter().all(|offset| *offset >= call.actual_sp_from_entry && *offset < 0 && offset % 4 == 0)), "native call metadata differs")?;
        let writes = inner.global_writes.iter().map(|pc| pc.0).collect::<BTreeSet<_>>();
        require(writes.len() == inner.global_writes.len() && writes.len() <= execution.len()
            && writes.iter().all(|pc| execution.contains(pc)), "native write checkpoint set differs")?;
    Ok(())
}
fn source(name: &'static str, base: u32, path: &Path, expected: &Digest) -> Result<Code, String> {
    let bytes = read(path)?;
    let digest = sha256(&bytes);
    require(digest == expected.0, "joined staged source digest differs")?;
    Ok(Code { name, base, bytes, sha256: Digest(digest) })
}
fn pc_set(pcs: &[Address], start: u32, end: u32) -> Result<BTreeSet<u32>, String> {
    let set = pcs.iter().map(|pc| pc.0).collect::<BTreeSet<_>>();
    require(!set.is_empty() && set.len() == pcs.len() && set.iter().all(|pc| *pc >= start && *pc < end && pc & 3 == 0),
        "joined execution metadata differs")?;
    Ok(set)
}
fn store_set(stores: &[Address], execution: &BTreeSet<u32>) -> Result<(), String> {
    let set = stores.iter().map(|pc| pc.0).collect::<BTreeSet<_>>();
    require(set.len() == stores.len() && set.iter().all(|pc| execution.contains(pc)), "joined store metadata differs")
}
fn validate_composition(c: &Composition) -> Result<(), String> {
    let execution = &c.execution;
    require(execution.wrapper_pcs == c.task_submission.execution_pcs
        && execution.wrapper_global_store_pcs == c.task_submission.global_writes, "joined wrapper metadata differs")?;
    let critical = pc_set(&execution.critical_pcs, 0x10400, 0x10400 + c.component_linked_bytes.critical as u32)?;
    let storage = pc_set(&execution.storage_pcs, 0x725cc4, 0x725cc4 + c.component_linked_bytes.storage as u32)?;
    require(critical.contains(&c.critical_pair.entry_adapter.0) && critical.contains(&c.critical_pair.exit_adapter.0)
        && storage.contains(&0x725cc4) && storage.contains(&c.task_storage.rust[0].0), "joined component entry metadata differs")?;
    store_set(&execution.critical_counter_store_pcs, &critical)?;
    store_set(&execution.critical_private_store_pcs, &critical)?;
    store_set(&execution.storage_guest_store_pcs, &storage)?;
    let stack = &c.owned_stack;
    require([stack.submission_low, stack.release_low, stack.critical_enter_low, stack.critical_exit_low, stack.reservation_low]
        .into_iter().all(|low| (-65536..=0).contains(&low))
        && c.critical_pair.entry_frame_bytes.checked_neg() == Some(stack.critical_enter_low)
        && c.critical_pair.exit_frame_bytes.checked_neg() == Some(stack.critical_exit_low)
        && stack.reservation_low <= -40, "joined component stack bounds differ")?;
    let owned_call = |call: &&Call| matches!(call.target, Target::CriticalEnter | Target::CriticalExit | Target::Reservation);
    require(stack.calls.len() == c.task_submission.calls.iter().filter(owned_call).count(), "joined call coverage differs")?;
    for call in c.task_submission.calls.iter().filter(owned_call) {
        let mut alternatives = stack.calls.iter().filter(|joined| joined.entry == call.entry && joined.instruction == call.instruction
            && joined.continuation == call.continuation && joined.target == call.target
            && joined.stack_from_entry == call.actual_sp_from_entry && joined.caller_live_words == call.caller_live_words);
        let joined = alternatives.next().ok_or("joined call provenance is absent")?;
        require(alternatives.next().is_none(), "joined call provenance differs")?;
        let bound = match call.entry { Entry::Submission => stack.submission_low, Entry::Release => stack.release_low };
        require((-65536..=0).contains(&joined.callee_low) && joined.stack_from_entry.checked_add(joined.callee_low) == Some(joined.nested_low)
            && bound <= joined.nested_low && joined.caller_live_words.iter().all(|word| *word >= joined.stack_from_entry),
            "joined actual call stack bound differs")?;
        let expected = match call.target {
            Target::CriticalEnter => Some(stack.critical_enter_low), Target::CriticalExit => Some(stack.critical_exit_low),
            Target::Reservation => Some(stack.reservation_low), _ => None,
        };
        require(expected.is_none_or(|low| joined.callee_low == low), "joined callee demand differs")?;
    }
    for frame in &c.task_submission.frames {
        require(match frame.entry { Entry::Submission => stack.submission_low, Entry::Release => stack.release_low } <= frame.low_from_entry_sp,
            "joined stack excludes wrapper frame")?;
    }
    Ok(())
}
fn insert(points: &mut BTreeMap<u32, Boundary>, pc: u32, boundary: Boundary) -> Result<(), String> {
    if let Some(previous) = points.insert(pc, boundary) { require(previous == boundary, "semantic instruction point collision")?; }
    Ok(())
}
pub(super) fn call_boundary(target: Target) -> Boundary {
    match target {
        Target::CriticalEnter => Boundary::CriticalEnterCall, Target::CriticalExit => Boundary::CriticalExitCall,
        Target::Reservation => Boundary::ReservationCall, Target::Fill => Boundary::FillCall, Target::Cache => Boundary::CacheCall,
        Target::Log => Boundary::LogCall, Target::ErrorCallback => Boundary::ErrorCallbackCall, Target::TryLock => Boundary::TryLockCall,
        Target::Unlock => Boundary::UnlockCall, Target::AuxiliaryService => Boundary::AuxiliaryCall, Target::CreationService => Boundary::CreationCall,
        Target::ActivationService => Boundary::ActivationCall, Target::CurrentCoreService => Boundary::IdentityCall,
        Target::SaveMaskService => Boundary::SaveMaskCall, Target::RestoreMaskService => Boundary::RestoreMaskCall,
    }
}
fn return_boundary(target: Target) -> Boundary {
    match target {
        Target::CriticalEnter => Boundary::CriticalEnterReturn, Target::CriticalExit => Boundary::CriticalExitReturn,
        Target::Reservation => Boundary::ReservationReturn, Target::Fill => Boundary::FillReturn, Target::Cache => Boundary::CacheReturn,
        Target::Log => Boundary::LogReturn, Target::ErrorCallback => Boundary::ErrorCallbackReturn, Target::TryLock => Boundary::TryLockReturn,
        Target::Unlock => Boundary::UnlockReturn, Target::AuxiliaryService => Boundary::AuxiliaryReturn, Target::CreationService => Boundary::CreationReturn,
        Target::ActivationService => Boundary::ActivationReturn, Target::CurrentCoreService => Boundary::IdentityReturn,
        Target::SaveMaskService => Boundary::SaveMaskReturn, Target::RestoreMaskService => Boundary::RestoreMaskReturn,
    }
}
pub(super) fn label(boundary: Boundary) -> &'static str {
    match boundary {
        Boundary::Submission => "submission", Boundary::AuxiliaryCall => "auxiliary-call", Boundary::AuxiliaryReturn => "auxiliary-return",
        Boundary::CreationCall => "creation-call", Boundary::CreationReturn => "creation-return", Boundary::ActivationCall => "activation-call", Boundary::ActivationReturn => "activation-return", Boundary::WrapperReturn => "wrapper-return",
        Boundary::AutoReadyEntry => "auto-ready-entry", Boundary::AutoReadyReturn => "auto-ready-return", Boundary::Prepare => "prepare", Boundary::Prepared => "prepared", Boundary::Scheduler => "scheduler", Boundary::Selected => "selected", Boundary::StackTransfer => "stack-transfer", Boundary::Trampoline => "trampoline", Boundary::EntryTransfer => "entry-transfer", Boundary::TaskEntry => "task-entry", Boundary::EnteredInstruction => "entered-instruction",
        Boundary::CriticalEnterCall => "critical-enter-call", Boundary::CriticalEnterReturn => "critical-enter-return", Boundary::CriticalExitCall => "critical-exit-call", Boundary::CriticalExitReturn => "critical-exit-return",
        Boundary::ReservationCall => "reservation-call", Boundary::ReservationReturn => "reservation-return", Boundary::FillCall => "fill-call", Boundary::FillReturn => "fill-return", Boundary::CacheCall => "cache-call", Boundary::CacheReturn => "cache-return",
        Boundary::LogCall => "log-call", Boundary::LogReturn => "log-return", Boundary::ErrorCallbackCall => "error-callback-call", Boundary::ErrorCallbackReturn => "error-callback-return", Boundary::IdentityCall => "identity-call", Boundary::IdentityReturn => "identity-return",
        Boundary::SaveMaskCall => "save-mask-call", Boundary::SaveMaskReturn => "save-mask-return", Boundary::RestoreMaskCall => "restore-mask-call", Boundary::RestoreMaskReturn => "restore-mask-return", Boundary::TryLockCall => "try-lock-call", Boundary::TryLockReturn => "try-lock-return", Boundary::UnlockCall => "unlock-call", Boundary::UnlockReturn => "unlock-return",
        Boundary::ReleaseEntry => "release-entry", Boundary::ReleaseReturn => "release-return", Boundary::DeletionEntry => "deletion-entry",
        Boundary::NativeWrite => "native-write",
    }
}
