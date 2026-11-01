use super::{Address, Boundary, Code, Digest, require};
use fujimod_xm1::bridge::{read, sha256};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, path::Path};

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
pub(super) struct OwnedProgram { contract: CandidateContract, digest: Digest }
pub(super) enum Program { Stock, Owned(OwnedProgram) }
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(super) enum Summary<'a> {
    Stock,
    Owned { contract_sha256: &'a Digest, linked_sha256: &'a Digest, symbols_sha256: &'a Digest },
}

impl Program {
    pub(super) fn open(contract_path: Option<&Path>, native_path: Option<&Path>, update: &Digest, code: &mut [Code]) -> Result<Self, String> {
        let (Some(contract_path), Some(native_path)) = (contract_path, native_path) else {
            require(contract_path.is_none() && native_path.is_none(), "owned program needs both contract and native image")?;
            require(update.0 == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490", "stock program requires original DAT")?;
            return Ok(Self::Stock);
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
        let image = code.iter_mut().find(|code| code.base == 0x61c400).ok_or("image4 source is absent")?;
        image.bytes.get_mut(0x725dd8 - 0x61c400..0x72612c - 0x61c400).ok_or("owned patch leaves source image")?.copy_from_slice(&native);
        let staged_digest = sha256(&image.bytes);
        require(staged_digest == inner.staged_image4_sha256.0 && inner.staged_image4_sha256.0 == contract.candidate_image4_sha256.0, "staged image4 identity differs")?;
        image.sha256 = Digest(staged_digest);
        Ok(Self::Owned(OwnedProgram { contract, digest: Digest(sha256(&bytes)) }))
    }
    pub(super) fn summary(&self) -> Summary<'_> {
        match self { Self::Stock => Summary::Stock, Self::Owned(owned) => Summary::Owned { contract_sha256: &owned.digest, linked_sha256: &owned.contract.task_submission.linked_sha256, symbols_sha256: &owned.contract.task_submission.symbols_sha256 } }
    }
    pub(super) fn owned(&self) -> bool { matches!(self, Self::Owned(_)) }
    pub(super) fn frame_low(&self, entry: Entry) -> Result<i32, String> {
        match self {
            Self::Stock => Ok(match entry { Entry::Submission => -56, Entry::Release => -32 }),
            Self::Owned(owned) => owned.contract.task_submission.frames.iter().find(|frame| frame.entry == entry)
                .map(|frame| frame.low_from_entry_sp).ok_or_else(|| "admitted native entry frame is absent".into()),
        }
    }
    pub(super) fn call(&self, pc: u32, entry: Entry, incoming_sp: Address, actual_sp: Address) -> Result<Option<&Call>, String> {
        match self {
            Self::Stock => Ok(None),
            Self::Owned(owned) => owned.contract.task_submission.calls.iter().find(|call| call.instruction.0 == pc
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
        match self {
            Self::Stock => for (pc, target) in stock_calls { insert(&mut points, pc, call_boundary(target))?; insert(&mut points, pc + 4, return_boundary(target))?; },
            Self::Owned(owned) => {
                points.retain(|pc, _| !(0x725dd8..0x72612c).contains(pc));
                points.insert(0x725dd8, Boundary::Submission);
                points.insert(0x726098, Boundary::ReleaseEntry);
                for call in &owned.contract.task_submission.calls {
                    insert(&mut points, call.instruction.0, call_boundary(call.target))?;
                    insert(&mut points, call.continuation.0, return_boundary(call.target))?;
                }
                for pc in &owned.contract.task_submission.global_writes {
                    points.entry(pc.0).or_insert(Boundary::NativeWrite);
                }
            }
        }
        Ok(points)
    }
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
