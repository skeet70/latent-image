use super::{Access, CallKind, Catalog, Core, Event, Guest, Report, SourceKind};
use super::continuation_view::{Digest, EventId, FileBinding, RangeSnapshot, Words};
use super::loader_continuations_check::{artifact, event, guarded, in_class, mapping, words};
use super::image5_program::{CARRIER, FEED_CALLBACK, FILL_CALLBACK, FILL_FLAG, FILL_REQUEST, Program};
use super::image5_runtime::{BLOCK, BSS, CALLER, CALLER_RETURN, DECODER_GLOBALS, DESTINATION, HALO, IMAGE5_ENTRY, PAYLOAD_SHA256, SEGMENT_WORDS, STACK_GUARD, STAGING, TASK1_ENTRY, record};
use super::image5_wire::{Evidence, Limit, Region, Role, SampleKind};
use super::plain_check::{ByteOrder, Watch};
use fujimod_xm1::bridge::read;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

const SEGMENT: [u32; SEGMENT_WORDS] = [5, DESTINATION[0], DESTINATION[1] - DESTINATION[0], BSS[0], BSS[1] - BSS[0], 0x2c0000, STAGING[1] - STAGING[0], 0x20, 1];
const START_BLOCK: u32 = 0x2c0000 >> 17;
const BLOCKS: u32 = (STAGING[1] - STAGING[0]) / BLOCK;
const STORAGE_READ: Guest = Guest(0x6a8f1c);
const SEGMENT_LOOKUP: Guest = Guest(0x657114);
const MODE_QUERY: Guest = Guest(0x657184);
const MARKER: Guest = Guest(0x42188);
const DECODER_INIT: Guest = Guest(0x6ae360);
const DECODER_FINISH: Guest = Guest(0x6ae3f4);
const DECODER_RESOURCE: u32 = 0x48;
const FLASHLOADER_RESOURCE: u32 = 0x4c;
const REQUEST_CALLBACK: Guest = Guest(0x73c56c);
const STORAGE_PROGRESS_LR: Guest = Guest(0x6f1a1c);
const FILL_STORE_PC: Guest = Guest(0x6a8c00);
const IMAGE5_OUTCOME: u32 = 0x11010100;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub(super) enum MarkerId { MainStart, LoadStart, LoadEnd, DecodeStart, DecodeEnd, MainEnd }
impl MarkerId {
    fn string(self) -> Guest { Guest(0x7819a0 + 8 * self as u32) }
    fn text(self) -> &'static [u8; 8] { match self { Self::MainStart => b"MAIN_S\0\0", Self::LoadStart => b"LMAIN_S\0", Self::LoadEnd => b"LMAIN_E\0", Self::DecodeStart => b"DMAIN_S\0", Self::DecodeEnd => b"DMAIN_E\0", Self::MainEnd => b"MAIN_E\0\0" } }
}
#[derive(Clone, Copy, Debug)]
enum Step { Segment, Marker(MarkerId), Fill, Mode, DecoderInit, StorageRead, DecoderFinish }
/// The read-mode, accepted-fill path of the source contract.
const NATURAL: [Step; 12] = [Step::Segment, Step::Marker(MarkerId::MainStart), Step::Fill, Step::Mode, Step::Marker(MarkerId::LoadStart), Step::DecoderInit, Step::StorageRead, Step::Marker(MarkerId::LoadEnd), Step::Marker(MarkerId::DecodeStart), Step::DecoderFinish, Step::Marker(MarkerId::DecodeEnd), Step::Marker(MarkerId::MainEnd)];
impl Step {
    fn target(self) -> Guest { match self { Self::Segment => SEGMENT_LOOKUP, Self::Marker(_) => MARKER, Self::Fill => FILL_REQUEST, Self::Mode => MODE_QUERY, Self::DecoderInit => DECODER_INIT, Self::StorageRead => STORAGE_READ, Self::DecoderFinish => DECODER_FINISH } }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum MissingEvidence { Image5Witness, CallerBefore, ProducerEntry, CallerReturn, Operation, Marker, Progress, FreshBlock, StagingWrites, DecoderFinishView, ProducerReturnView, Image5EntryView, DecoderResource, FillCompletion, FlagClear, Publication, Image5Entry, Task1, Frame, CodeGuard, InputReads, WriterAttribution }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Boundary { SourceIdentity, CommittedData, Mapping, WriteConfinement, SourceOperation }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum CallbackRole { FillDone, StorageFeed }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum PublicationBoundary { FlashloaderDone, Image5Fetch, ReturnBeforeFlag, RequestCallback }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum FlagStep { Clear, WaitExit }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "refusal", rename_all = "kebab-case")]
pub(super) enum Refusal {
    Missing { evidence: MissingEvidence },
    WrongImageIdentity,
    WrongSegment,
    WrongDestination,
    WrongStaging,
    WrongEntry,
    WrongCallback { callback: CallbackRole },
    WrongFifthArgument,
    SyntheticSkipMode,
    CpuFillNotCredited,
    StorageReadFailed { status: u32 },
    PrematurePublication { boundary: PublicationBoundary },
    FlagOrderDiffers { step: FlagStep },
    FieldReadsDiffer,
    BssNotZero { address: u32 },
    DecodedByteMismatch { address: u32 },
    StagingByteMismatch { address: u32 },
    OutOfRangeWrite { address: u32 },
    FlagWriterDiffers { address: u32 },
    DecoderCompletionOrder,
    InitializerDiffers,
    Task1Differs,
    BadFrame,
    BadStatus,
    SourceOperationDiffers,
    CoverageDiffers,
    Abi { boundary: Boundary },
    Artifact { boundary: Boundary, detail: String },
}
type Result<T, E = Refusal> = std::result::Result<T, E>;
fn require(ok: bool, error: Refusal) -> Result<()> { if ok { Ok(()) } else { Err(error) } }
fn missing(evidence: MissingEvidence) -> Refusal { Refusal::Missing { evidence } }
fn source_error(detail: String) -> Refusal { Refusal::Artifact { boundary: Boundary::SourceIdentity, detail } }
fn projection_error(detail: String) -> Refusal { Refusal::Artifact { boundary: Boundary::CommittedData, detail } }
fn boundary(report: &Report, id: EventId) -> Result<&Event> { event(report, id).map_err(|_| Refusal::CoverageDiffers) }
fn word(view: &Words, address: u32) -> Option<u32> { let offset = address.checked_sub(view.mapping.guest.0)?; if offset % 4 != 0 { return None; } view.values.get((offset / 4) as usize).copied() }
fn overlaps(low: u64, high: u64, start: u32, end: u32) -> bool { low < u64::from(end) && u64::from(start) < high }
fn inside(low: u64, high: u64, start: u32, end: u32) -> bool { u64::from(start) <= low && high <= u64::from(end) }
fn carrier(pc: Guest) -> bool { (CARRIER[0]..CARRIER[1]).contains(&(pc.0 & !0x40000000)) }
fn between(observed: &Event, low: &Event, high: &Event) -> bool { low.id.0 < observed.id.0 && observed.id.0 < high.id.0 }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct ContentDigest(String);
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "identity", rename_all = "kebab-case")]
enum ViewBoundary { FreshBlock(u32), Destination(SampleKind), Bss(SampleKind) }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ViewEffect { kind: ViewBoundary, start: Guest, bytes: u32, digest: ContentDigest }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct Status(u32);
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct ByteCount(u32);
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum SkipPolicy { SkipBadBlocks }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum LoadMode { Read }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum BssPolicy { Requested }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "operation", rename_all = "kebab-case")]
enum Operation {
    Segment { pointer: Guest, record: [u32; SEGMENT_WORDS] },
    Marker { id: MarkerId },
    FillRequest { base: Guest, words: ByteCount, value: u32, callback: Guest, policy: BssPolicy },
    Mode { mode: LoadMode },
    DecoderInit { destination: Guest, staging: Guest },
    StorageRead { first: u32, blocks: u32, staging: Guest, callback: Guest, skip: SkipPolicy, status: Status },
    DecoderFinish,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum Field { Destination, BssBase, BssLength, FlashOffset, FlashLength }
impl Field { fn offset(self) -> u32 { match self { Self::Destination => 0x4, Self::BssBase => 0xc, Self::BssLength => 0x10, Self::FlashOffset => 0x14, Self::FlashLength => 0x18 } } }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum ReadPhase { BeforeFirstMarker, BeforeFillRequest, BeforeDecoderInit }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct FieldRead { field: Field, phase: ReadPhase, value: u32 }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "pc", rename_all = "kebab-case")]
enum Writer { Retained(Guest) }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct OperationIndex(u32);
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct FlagPosition { after: Option<OperationIndex>, before: Option<OperationIndex> }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct FlagEffect { clear: FlagPosition, completed_by: Writer, completion_callback: Guest, wait: FlagPosition }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct FrameEffect { status: Status, restored: bool, link: Guest }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "step", rename_all = "kebab-case")]
enum PublicationStep { ProducerReturn, DoneStore { cell: Guest }, Release { resource: u32 }, RequestCallback { target: Guest, failed: bool }, Image5Transfer { descriptor: Guest }, Image5Entry }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Image5Effect { core: Core, entry: Guest, initializer: Vec<Guest>, outcome: u32 }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Task1Effect { core: Core, entry: Guest, priority: u32, stack: u32, argument: u32, status: Status }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Effects { reads: Vec<FieldRead>, operations: Vec<Operation>, progress: Vec<ByteCount>, views: Vec<ViewEffect>, flag: FlagEffect, frame: FrameEffect, publication: Vec<PublicationStep>, image5: Image5Effect, task1: Task1Effect }
/// Admitted per receipt; not a comparison obligation.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct DecoderObservation { completions: usize, takes: usize, completion_gives: usize }
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Scope { NaturalReadSuccessObservedBoundaries }
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum UnqualifiedBoundary { SkipMode, CpuFillPath, StorageFailure, DecoderFailure, MissingSegmentNonreturn, InteriorCpuWritesBetweenViews, HaloCpuWritesRecordedNotRefused, RetainedDecoderOwnership, PhysicalCacheCoherence, TransientRemapExclusion, PostConsumerAndWholeMachineLifetime }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Admission { schema: u32, role: Role, scope: Scope, limits: Vec<Limit>, unqualified: [UnqualifiedBoundary; 11], decoder: DecoderObservation, effects: Effects }

fn range_mapping(view: &RangeSnapshot, start: u32, end: u32) -> Result<()> {
    mapping(&view.mapping).map_err(projection_error)?; mapping(&view.alias_mapping).map_err(projection_error)?;
    require(view.mapping.guest == Guest(start) && view.alias_mapping.guest == Guest(start | 0x40000000) && view.mapping.physical == Guest(start) && view.alias_mapping.physical == Guest(start) && view.mapping.bytes.0 == end - start && view.alias_mapping.bytes.0 == end - start && view.mapping.tcm == view.alias_mapping.tcm && std::mem::discriminant(&view.mapping.decoder) == std::mem::discriminant(&view.alias_mapping.decoder), Refusal::Abi { boundary: Boundary::Mapping })
}
/// Streams the five views, requires guest/physical agreement and returns the physical digest with its first difference from `expected` (zero when absent).
fn checked_view(root: &Path, view: &RangeSnapshot, start: u32, end: u32, expected: Option<&[u8]>) -> Result<(ContentDigest, Option<u32>)> {
    range_mapping(view, start, end)?;
    let length = (end - start) as usize;
    require(expected.is_none_or(|bytes| bytes.len() == length), Refusal::CoverageDiffers)?;
    let mut digests = Vec::with_capacity(5); let mut difference = None;
    for (index, binding) in view.files.iter().enumerate() {
        let mut file = File::open(artifact(root, binding).map_err(projection_error)?).map_err(|error| projection_error(error.to_string()))?;
        require(file.metadata().map_err(|error| projection_error(error.to_string()))?.len() == length as u64, Refusal::CoverageDiffers)?;
        let mut hasher = Sha256::new(); let mut offset = 0usize; let mut bytes = [0; 4096];
        loop {
            let count = file.read(&mut bytes).map_err(|error| projection_error(error.to_string()))?; if count == 0 { break; }
            let high = offset.checked_add(count).ok_or(Refusal::CoverageDiffers)?; require(high <= length, Refusal::CoverageDiffers)?;
            hasher.update(&bytes[..count]);
            if index == 4 && difference.is_none() { difference = bytes[..count].iter().enumerate().position(|(inside, observed)| *observed != expected.map_or(0, |expected| expected[offset + inside])).map(|inside| start + (offset + inside) as u32); }
            offset = high;
        }
        require(offset == length, Refusal::CoverageDiffers)?;
        let digest = format!("{:x}", hasher.finalize()); require(digest == binding.sha256.as_str(), Refusal::CoverageDiffers)?; digests.push(digest);
    }
    require(view.guest_equal == [true; 4] && digests.iter().all(|digest| digest == &digests[4]), Refusal::Abi { boundary: Boundary::Mapping })?;
    require(view.expected_equal == difference.is_none() && view.first_difference == difference.map(Guest), Refusal::CoverageDiffers)?;
    Ok((ContentDigest(digests.pop().ok_or(Refusal::CoverageDiffers)?), difference))
}
fn untyped_view(root: &Path, view: &RangeSnapshot, event: EventId, start: u32, end: u32) -> Result<Vec<u8>> {
    range_mapping(view, start, end)?;
    require(view.event.0 == event.0 && view.guest_equal == [true; 4], Refusal::BadFrame)?;
    let mut contents: Option<Vec<u8>> = None;
    for binding in &view.files {
        let bytes = read(&artifact(root, binding).map_err(projection_error)?).map_err(projection_error)?;
        require(bytes.len() == (end - start) as usize && Digest::of(&bytes).as_str() == binding.sha256.as_str() && contents.as_ref().is_none_or(|first| *first == bytes), Refusal::BadFrame)?;
        contents.get_or_insert(bytes);
    }
    contents.ok_or(Refusal::BadFrame)
}

fn identity(report: &Report, catalog: &Catalog, program: &Program) -> Result<()> {
    require(report.identity.update.as_str() == program.update(), Refusal::WrongImageIdentity)?;
    for (kind, digest) in [(SourceKind::Image5, "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"), (SourceKind::Image4, program.image4())] {
        let image = catalog.0.iter().find(|image| image.binding.kind == kind).ok_or(Refusal::WrongImageIdentity)?;
        require(report.identity.sources.iter().filter(|source| source.kind == kind && source.sha256.as_str() == digest && source.start == image.binding.start && source.bytes.0 == image.binding.bytes.0).count() == 1, Refusal::WrongImageIdentity)?;
    }
    let image5 = catalog.0.iter().find(|image| image.binding.kind == SourceKind::Image5).ok_or(Refusal::WrongImageIdentity)?;
    require(image5.binding.start == Guest(DESTINATION[0]) && image5.bytes.len() == (DESTINATION[1] - DESTINATION[0]) as usize, Refusal::WrongImageIdentity)
}
fn limits(evidence: &Evidence) -> Result<()> {
    let required = [Limit::PausedMappingSamplesNotTransientRemapExclusion, Limit::FrameSamplesNotContinuousStackConfinement, Limit::FiniteDestinationHaloNotWholeMachineWriteConfinement, Limit::RetainedServiceAndCallerLifetimeOnly];
    require(evidence.limits.len() == required.len() && required.iter().all(|limit| evidence.limits.iter().filter(|observed| *observed == limit).count() == 1), Refusal::CoverageDiffers)
}

struct Interval<'a> { entry: &'a Event, returned: &'a Event }
struct Call<'a> { step: Step, entered: &'a Event, finished: &'a Event }

pub(super) fn admit(root: &Path, report: &Report, catalog: &Catalog, program: &Program) -> Result<Admission> {
    let evidence = report.image5.as_ref().ok_or(missing(MissingEvidence::Image5Witness))?;
    identity(report, catalog, program)?; limits(evidence)?;
    let payload = read(&artifact(root, &evidence.payload).map_err(source_error)?).map_err(source_error)?;
    require(evidence.payload.sha256.as_str() == PAYLOAD_SHA256 && Digest::of(&payload).as_str() == PAYLOAD_SHA256, Refusal::WrongImageIdentity)?;
    let staged = record(&payload).map_err(|_| Refusal::WrongImageIdentity)?;
    let before = boundary(report, evidence.caller_before.ok_or(missing(MissingEvidence::CallerBefore))?)?;
    let entry = boundary(report, evidence.entry.ok_or(missing(MissingEvidence::ProducerEntry))?)?;
    let returned = boundary(report, evidence.caller_return.ok_or(missing(MissingEvidence::CallerReturn))?)?;
    require(before.pc == CALLER && entry.pc == Guest(CARRIER[0]) && returned.pc == CALLER_RETURN && before.core == entry.core && entry.core == returned.core && entry.core == Core::Primary && before.instructions < entry.instructions && entry.instructions < returned.instructions, Refusal::WrongEntry)?;
    require(report.calls.iter().filter(|call| call.kind == CallKind::Image5Producer && call.before.is_some_and(|id| id.0 == before.id.0) && call.entry.is_some_and(|id| id.0 == entry.id.0) && call.returned.is_some_and(|id| id.0 == returned.id.0)).count() == 1, Refusal::CoverageDiffers)?;
    require(entry.registers.lr == CALLER_RETURN && entry.registers.sp == before.registers.sp && entry.registers.sp == Guest(0xa1c1e0) && returned.registers.sp == entry.registers.sp && returned.registers.r[4..12] == entry.registers.r[4..12] && returned.registers.lr == program.return_lr() && returned.registers.cpsr & 0xff == entry.registers.cpsr & 0xff, Refusal::BadFrame)?;
    let interval = Interval { entry, returned };
    let (operations, calls) = operations(report, &interval)?;
    let status = calls.iter().find(|call| matches!(call.step, Step::StorageRead)).map(|call| call.finished.registers.r[0]).ok_or(missing(MissingEvidence::Operation))?;
    require(returned.registers.r[0] == status && status == 0, Refusal::BadStatus)?;
    let mut views = progress(root, report, evidence, &interval, &calls, staged)?;
    frames(report, evidence, &interval, program)?;
    let (reads, flag) = guards(root, report, catalog, evidence, &interval, &calls, program)?;
    samples(root, report, catalog, evidence, &interval, &calls, &mut views)?;
    let decoder = decoder(report, &interval, &calls)?;
    let publication = publication(report, &interval)?;
    let image5 = image5_effect(report)?;
    let task1 = task1(report)?;
    Ok(Admission { schema: 1, role: program.role(), scope: Scope::NaturalReadSuccessObservedBoundaries, limits: evidence.limits.clone(), unqualified: [UnqualifiedBoundary::SkipMode, UnqualifiedBoundary::CpuFillPath, UnqualifiedBoundary::StorageFailure, UnqualifiedBoundary::DecoderFailure, UnqualifiedBoundary::MissingSegmentNonreturn, UnqualifiedBoundary::InteriorCpuWritesBetweenViews, UnqualifiedBoundary::HaloCpuWritesRecordedNotRefused, UnqualifiedBoundary::RetainedDecoderOwnership, UnqualifiedBoundary::PhysicalCacheCoherence, UnqualifiedBoundary::TransientRemapExclusion, UnqualifiedBoundary::PostConsumerAndWholeMachineLifetime], decoder, effects: Effects { reads, operations, progress: evidence.progress.iter().map(|progress| ByteCount(progress.cumulative.0)).collect(), views, flag, frame: FrameEffect { status: Status(status), restored: true, link: returned.registers.lr }, publication, image5, task1 } })
}

fn operations<'a>(report: &'a Report, interval: &Interval<'_>) -> Result<(Vec<Operation>, Vec<Call<'a>>)> {
    let services = report.calls.iter().filter(|call| call.kind == CallKind::Image5ProducerService && call.core == interval.entry.core && call.before.is_some_and(|id| interval.entry.id.0 < id.0 && id.0 < interval.returned.id.0)).collect::<Vec<_>>();
    let mut operations = Vec::with_capacity(NATURAL.len()); let mut calls = Vec::with_capacity(NATURAL.len());
    for (index, call) in services.iter().enumerate() {
        let step = *NATURAL.get(index).ok_or(Refusal::SourceOperationDiffers)?;
        require(call.target == step.target(), Refusal::SourceOperationDiffers)?;
        let entered = boundary(report, call.entry.ok_or(missing(MissingEvidence::Operation))?)?;
        let finished = boundary(report, call.returned.ok_or(missing(MissingEvidence::Operation))?)?;
        let r = &entered.registers.r;
        let operation = match step {
            Step::Segment => {
                require(r[0] == 5, Refusal::SourceOperationDiffers)?;
                let pointer = finished.registers.r[0];
                require(pointer != 0 && pointer % 4 == 0, Refusal::WrongSegment)?;
                let view = finished.state.iter().find(|view| view.mapping.guest == Guest(pointer) && view.values.len() == SEGMENT_WORDS).ok_or(missing(MissingEvidence::InputReads))?;
                words(view).map_err(projection_error)?;
                require(view.values == SEGMENT, Refusal::WrongSegment)?;
                Operation::Segment { pointer: Guest(pointer), record: SEGMENT }
            }
            Step::Marker(id) => {
                require(r[0] == id.string().0, Refusal::SourceOperationDiffers)?;
                let text = entered.state.iter().find(|view| view.mapping.guest == id.string() && view.values.len() == 2).ok_or(missing(MissingEvidence::Marker))?;
                words(text).map_err(projection_error)?;
                let bytes: Vec<u8> = text.values.iter().flat_map(|value| value.to_le_bytes()).collect();
                require(bytes.as_slice() == id.text(), Refusal::SourceOperationDiffers)?;
                Operation::Marker { id }
            }
            Step::Fill => {
                require(r[3] == FILL_CALLBACK.0, Refusal::WrongCallback { callback: CallbackRole::FillDone })?;
                require(r[0] == BSS[0] && r[1] == (BSS[1] - BSS[0]) >> 2 && r[2] == 0, Refusal::SourceOperationDiffers)?;
                require(finished.registers.r[0] == 0, Refusal::CpuFillNotCredited)?;
                Operation::FillRequest { base: Guest(BSS[0]), words: ByteCount((BSS[1] - BSS[0]) >> 2), value: 0, callback: FILL_CALLBACK, policy: BssPolicy::Requested }
            }
            Step::Mode => { require(finished.registers.r[0] == 0, Refusal::SyntheticSkipMode)?; Operation::Mode { mode: LoadMode::Read } }
            Step::DecoderInit => {
                require(r[0] == DESTINATION[0], Refusal::WrongDestination)?;
                require(r[1] == STAGING[0] | 0x40000000, Refusal::WrongStaging)?;
                Operation::DecoderInit { destination: Guest(DESTINATION[0]), staging: Guest(STAGING[0] | 0x40000000) }
            }
            Step::StorageRead => {
                require(r[2] == STAGING[0] | 0x40000000, Refusal::WrongStaging)?;
                require(r[3] == FEED_CALLBACK.0, Refusal::WrongCallback { callback: CallbackRole::StorageFeed })?;
                require(r[0] == START_BLOCK && r[1] == BLOCKS, Refusal::SourceOperationDiffers)?;
                let frame = entered.frame.as_ref().map_err(|_| missing(MissingEvidence::Frame))?; words(frame).map_err(projection_error)?;
                require(word(frame, entered.registers.sp.0) == Some(1), Refusal::WrongFifthArgument)?;
                require(finished.registers.r[0] == 0, Refusal::StorageReadFailed { status: finished.registers.r[0] })?;
                Operation::StorageRead { first: START_BLOCK, blocks: BLOCKS, staging: Guest(STAGING[0] | 0x40000000), callback: FEED_CALLBACK, skip: SkipPolicy::SkipBadBlocks, status: Status(0) }
            }
            Step::DecoderFinish => Operation::DecoderFinish,
        };
        operations.push(operation); calls.push(Call { step, entered, finished });
    }
    require(services.len() == NATURAL.len(), Refusal::SourceOperationDiffers)?;
    Ok((operations, calls))
}
fn call<'c, 'a>(calls: &'c [Call<'a>], wanted: fn(Step) -> bool) -> Result<&'c Call<'a>> { calls.iter().find(|call| wanted(call.step)).ok_or(missing(MissingEvidence::Operation)) }

fn progress(root: &Path, report: &Report, evidence: &Evidence, interval: &Interval<'_>, calls: &[Call<'_>], staged: &[u8]) -> Result<Vec<ViewEffect>> {
    let read = call(calls, |step| matches!(step, Step::StorageRead))?;
    let observed = report.events.iter().filter(|observed| observed.pc == FEED_CALLBACK && observed.core == interval.entry.core && between(observed, interval.entry, interval.returned)).collect::<Vec<_>>();
    require(evidence.progress.len() == observed.len() && observed.len() == BLOCKS as usize, missing(MissingEvidence::Progress))?;
    let mut views = Vec::with_capacity(BLOCKS as usize);
    for (index, (progress, observed)) in evidence.progress.iter().zip(observed).enumerate() {
        let index = index as u32;
        require(progress.event.0 == observed.id.0 && progress.cumulative.0 == observed.registers.r[0] && progress.cumulative.0 == BLOCK * (index + 1), Refusal::CoverageDiffers)?;
        require(between(observed, read.entered, read.finished) && observed.registers.lr == STORAGE_PROGRESS_LR, Refusal::WrongCallback { callback: CallbackRole::StorageFeed })?;
        let fresh = progress.fresh.as_ref().ok_or(missing(MissingEvidence::FreshBlock))?;
        require(fresh.event.0 == observed.id.0, Refusal::CoverageDiffers)?;
        let start = STAGING[0] + BLOCK * index;
        let expected = staged.get((BLOCK * index) as usize..(BLOCK * (index + 1)) as usize).ok_or(Refusal::CoverageDiffers)?;
        let (digest, difference) = checked_view(root, fresh, start, start + BLOCK, Some(expected))?;
        if let Some(address) = difference { return Err(Refusal::StagingByteMismatch { address }); }
        views.push(ViewEffect { kind: ViewBoundary::FreshBlock(index), start: Guest(start), bytes: BLOCK, digest });
    }
    Ok(views)
}

fn frames(report: &Report, evidence: &Evidence, interval: &Interval<'_>, program: &Program) -> Result<()> {
    let (entry, returned) = (interval.entry, interval.returned);
    let sp = entry.registers.sp.0; let bytes = program.frame_bytes();
    let slots = program.saved_slots(sp, &entry.registers.r, entry.registers.lr);
    let observed = report.events.iter().filter(|event| entry.id.0 <= event.id.0 && event.id.0 <= returned.id.0).collect::<Vec<_>>();
    require(evidence.frames.len() == observed.len(), missing(MissingEvidence::Frame))?;
    let original = entry.state.iter().find(|view| view.mapping.guest == Guest(sp) && view.values.len() == 8).ok_or(missing(MissingEvidence::Frame))?;
    for (frame, observed) in evidence.frames.iter().zip(observed) {
        require(frame.event.0 == observed.id.0, Refusal::CoverageDiffers)?; words(&frame.words).map_err(projection_error)?;
        require(frame.words.mapping.guest == Guest(sp - bytes) && frame.words.mapping.bytes.0 == bytes, Refusal::BadFrame)?;
        let caller = observed.state.iter().find(|view| view.mapping.guest == Guest(sp) && view.values.len() == 8).ok_or(missing(MissingEvidence::Frame))?;
        require(caller.values == original.values, Refusal::BadFrame)?;
        if carrier(observed.pc) { require(sp - bytes <= observed.registers.sp.0 && observed.registers.sp.0 <= sp && observed.registers.sp.0 % 4 == 0, Refusal::BadFrame)?; }
        if observed.id.0 == entry.id.0 { continue; }
        let adapter_done = observed.pc != Guest(CARRIER[0]);
        let body_done = program.rust_entry().is_none_or(|rust| report.events.iter().any(|event| entry.id.0 < event.id.0 && event.id.0 < observed.id.0 && event.pc == rust && event.core == entry.core));
        if !adapter_done { continue; }
        for &(address, value) in &slots { if address < sp - 8 && !body_done { continue; } require(word(&frame.words, address) == Some(value), Refusal::BadFrame)?; }
    }
    Ok(())
}

fn attribution_ranges(catalog: &Catalog) -> Result<Vec<[u64; 2]>> {
    #[derive(Deserialize)] struct CanonicalRun { boot_rom: BootRom }
    #[derive(Deserialize)] struct BootRom { address: Guest, bytes: super::continuation_view::ByteCount }
    let run: CanonicalRun = serde_json::from_slice(include_bytes!("../data/xm1-150.json")).map_err(|error| source_error(error.to_string()))?;
    let mut ranges = catalog.0.iter().map(|image| [u64::from(image.binding.start.0), u64::from(image.binding.start.0) + image.bytes.len() as u64]).collect::<Vec<_>>();
    ranges.push([u64::from(run.boot_rom.address.0), u64::from(run.boot_rom.address.0) + u64::from(run.boot_rom.bytes.0)]);
    ranges.sort_unstable(); let mut normalized: Vec<[u64; 2]> = Vec::with_capacity(ranges.len());
    for range in ranges { require(range[0] < range[1] && range[1] <= 1u64 << 32, Refusal::CoverageDiffers)?; if let Some(previous) = normalized.last_mut() { if range[0] <= previous[1] { previous[1] = previous[1].max(range[1]); continue; } } normalized.push(range); }
    Ok(normalized)
}

fn guards(root: &Path, report: &Report, catalog: &Catalog, evidence: &Evidence, interval: &Interval<'_>, calls: &[Call<'_>], program: &Program) -> Result<(Vec<FieldRead>, FlagEffect)> {
    let (entry, returned) = (interval.entry, interval.returned);
    let lease = evidence.stack.as_ref().ok_or(missing(MissingEvidence::Frame))?;
    require(lease.event.0 == entry.id.0, Refusal::BadFrame)?;
    for view in [&lease.current, &lease.task, &lease.creation] { words(view).map_err(projection_error)?; }
    require(lease.current.mapping.guest == Guest(0x40054760) && lease.current.values.len() == 1 && lease.task.values.len() == 3 && lease.creation.values.len() == 10, Refusal::BadFrame)?;
    let node = lease.current.values[0];
    require(lease.task.mapping.guest == Guest(node) && node.checked_sub(0x4005ff0c).is_some_and(|offset| offset % 56 == 0 && offset / 56 + 1 == 60) && lease.creation.mapping.guest == Guest(lease.task.values[2]) && lease.creation.values[0] == 2 && lease.creation.values[8] == 60 && lease.creation.values[2] == 0x73b3a0 && lease.creation.values[1] == 0x91482c, Refusal::BadFrame)?;
    let code_view = evidence.code.as_ref().ok_or(missing(MissingEvidence::CodeGuard))?;
    let code = untyped_view(root, code_view, entry.id, CARRIER[0], CARRIER[1])?;
    for (index, bytes) in code.chunks_exact(4).enumerate() { require(catalog.word(Guest(CARRIER[0] + 4 * index as u32)).map_err(source_error)?.1 == u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]), Refusal::WrongImageIdentity)?; }
    let entry_code = entry.state.iter().find(|view| view.mapping.guest == Guest(CARRIER[0]) && view.values.len() == code.len() / 4).ok_or(missing(MissingEvidence::CodeGuard))?;
    words(entry_code).map_err(projection_error)?;
    for (index, &value) in entry_code.values.iter().enumerate() { require(catalog.word(Guest(CARRIER[0] + 4 * index as u32)).map_err(source_error)?.1 == value, Refusal::WrongImageIdentity)?; }
    let stack_base = lease.creation.values[5] & !0x40000000; let stack_size = lease.creation.values[4];
    let stack_end = stack_base.checked_add(stack_size).ok_or(Refusal::BadFrame)?;
    let sp = entry.registers.sp.0; let frame_low = sp - program.frame_bytes();
    require(stack_base == 0xa1b600 && stack_size == 0xc00 && stack_end == 0xa1c200 && stack_base <= frame_low && sp <= stack_end, Refusal::BadFrame)?;
    untyped_view(root, &lease.view, entry.id, stack_base, stack_end)?;
    for call in calls { for observed in [call.entered, call.finished] { require(stack_base <= observed.registers.sp.0 && observed.registers.sp.0 < sp && observed.registers.sp.0 % 8 == 0, Refusal::BadFrame)?; } }

    let segment = calls.iter().find(|call| matches!(call.step, Step::Segment)).map(|call| call.finished).ok_or(missing(MissingEvidence::InputReads))?;
    let pointer = segment.registers.r[0] & !0x40000000;
    let first_marker = call(calls, |step| matches!(step, Step::Marker(MarkerId::MainStart)))?;
    let fill = call(calls, |step| matches!(step, Step::Fill))?;
    let mode = call(calls, |step| matches!(step, Step::Mode))?;
    let second_marker = call(calls, |step| matches!(step, Step::Marker(MarkerId::LoadStart)))?;
    let init = call(calls, |step| matches!(step, Step::DecoderInit))?;
    let storage = call(calls, |step| matches!(step, Step::StorageRead))?;
    let finish = call(calls, |step| matches!(step, Step::DecoderFinish))?;
    let windows = [(ReadPhase::BeforeFirstMarker, segment, first_marker.entered), (ReadPhase::BeforeFillRequest, first_marker.finished, fill.entered), (ReadPhase::BeforeDecoderInit, second_marker.finished, init.entered)];
    let expected_reads = [(Field::FlashOffset, ReadPhase::BeforeFirstMarker), (Field::FlashLength, ReadPhase::BeforeFirstMarker), (Field::BssBase, ReadPhase::BeforeFillRequest), (Field::BssLength, ReadPhase::BeforeFillRequest), (Field::Destination, ReadPhase::BeforeDecoderInit)];
    let mut record_reads = Vec::new(); let mut mode_read = false;
    let mut fetches = BTreeSet::new(); let mut saved_writes = BTreeSet::new();
    let slots = program.saved_slots(sp, &entry.registers.r, entry.registers.lr);
    let mut copied: Vec<Vec<[u64; 2]>> = vec![Vec::new(); BLOCKS as usize];
    let mut clears = Vec::new(); let mut completions = Vec::new(); let mut flag_reads = Vec::new(); let mut done_stores = Vec::new();
    let completion = report.events.iter().find(|observed| observed.pc == FILL_STORE_PC && between(observed, fill.entered, returned)).ok_or(missing(MissingEvidence::FillCompletion))?;
    require(report.events.iter().any(|observed| observed.pc == FILL_CALLBACK && observed.core == completion.core && fill.entered.id.0 < observed.id.0 && observed.id.0 < completion.id.0), missing(MissingEvidence::FillCompletion))?;
    let expected_attribution = attribution_ranges(catalog)?;
    for epoch in &report.epochs {
        let bytes = read(&artifact(root, &epoch.file).map_err(projection_error)?).map_err(projection_error)?;
        require(Digest::of(&bytes).as_str() == epoch.file.sha256.as_str(), Refusal::CoverageDiffers)?;
        let watch: Watch = serde_json::from_slice(&bytes).map_err(|error| projection_error(error.to_string()))?;
        require(watch.enabled && watch.violation.is_none() && watch.records.len() == epoch.record_count, Refusal::CoverageDiffers)?;
        let span = |[start, end]: [u32; 2]| [u64::from(start), u64::from(end)];
        let mut write_guards = vec![span([STAGING[0] - HALO, STAGING[1] + HALO]), span(STACK_GUARD), [0x89f8ac, 0x89f8b0], [0x944440, 0x944464]];
        for [start, end] in [DESTINATION, BSS] { write_guards.push(span([start - HALO, start])); write_guards.push(span([end, end + HALO])); }
        write_guards.extend(DECODER_GLOBALS.map(span));
        require(guarded(&watch.classes, span(CARRIER), true) && write_guards.iter().all(|range| guarded(&watch.classes, *range, false)), missing(MissingEvidence::CodeGuard))?;
        require([DESTINATION, BSS].iter().all(|range| watch.classes.iter().any(|class| class.range == span(*range) && class.kinds.contains(&Access::HostWrite) && class.kinds.contains(&Access::ModelBusWrite))), missing(MissingEvidence::WriterAttribution))?;
        require([[0x61c000, 0x61c220], [0x89f8ac, 0x89f8b0]].iter().all(|range| watch.classes.iter().any(|class| class.range == *range && class.kinds.contains(&Access::CpuRead))), missing(MissingEvidence::InputReads))?;
        let attribution = watch.fetch_attribution.as_ref().ok_or(missing(MissingEvidence::WriterAttribution))?;
        require(attribution.sources.as_slice() == [Access::CpuRead, Access::CpuWrite] && attribution.ranges == expected_attribution, Refusal::CoverageDiffers)?;
        let mut future = watch.records.iter().filter(|record| matches!(record.row.source, Access::CpuRead | Access::CpuWrite | Access::CpuFetch)).peekable(); let mut lower = None;
        for record in &watch.records {
            let row = &record.row; require(in_class(row, &watch.classes).map_err(projection_error)?, Refusal::CoverageDiffers)?;
            let cpu = matches!(row.source, Access::CpuRead | Access::CpuWrite | Access::CpuFetch); if cpu { lower = row.instructions.map(|clock| clock.0); future.next(); }
            let upper = future.peek().and_then(|record| record.row.instructions).map(|clock| clock.0);
            let low = u64::from(row.physical.0); let high = low.checked_add(row.size).ok_or(Refusal::CoverageDiffers)?;
            let clock = row.instructions.map(|clock| clock.0);
            let during = clock.is_some_and(|clock| entry.instructions.0 < clock && clock <= returned.instructions.0);
            let may_live = clock.map_or_else(|| !(upper.is_some_and(|clock| clock <= entry.instructions.0) || lower.is_some_and(|clock| returned.instructions.0 < clock)), |_| during);
            let within = |from: &Event, to: &Event| lower.is_some_and(|clock| from.instructions.0 <= clock) && upper.is_some_and(|clock| clock <= to.instructions.0 + 1);
            if matches!(row.source, Access::CpuRead | Access::CpuWrite) {
                let fetch = record.latest_fetch.as_ref().ok_or(missing(MissingEvidence::WriterAttribution))?;
                require(fetch.source == Access::CpuFetch && fetch.value.is_none() && fetch.before.is_none() && fetch.size == 4 && fetch.address == fetch.pc && fetch.physical.0 == fetch.address.0 & !0x40000000 && Some(fetch.core) == row.core && Some(fetch.pc) == row.pc && Some(fetch.instructions) == row.instructions && Some(fetch.tick) == row.tick && Some(fetch.core_instructions) == record.core_instructions && fetch.byte_order == ByteOrder::Little && record.byte_order == Some(ByteOrder::Little), Refusal::CoverageDiffers)?;
                let before = record.before.as_ref().ok_or(Refusal::CoverageDiffers)?; require(before.0.len() as u64 == row.size, Refusal::CoverageDiffers)?;
                if row.source == Access::CpuRead && row.size <= 8 { let value = before.0.iter().rev().fold(0u64, |value, byte| (value << 8) | u64::from(*byte)); require(row.value == Some(value), Refusal::CoverageDiffers)?; }
            }
            let producer_pc = row.pc.is_some_and(carrier);
            if row.source == Access::CpuFetch && overlaps(low, high, CARRIER[0], CARRIER[1]) {
                require(during && row.core == Some(entry.core) && row.address == row.physical && row.pc == Some(row.address) && row.size == 4 && program.execution_allowed(row.address), Refusal::WrongEntry)?;
                fetches.insert((row.address.0, clock.ok_or(Refusal::CoverageDiffers)?));
            }
            if matches!(row.source, Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite) {
                if overlaps(low, high, CARRIER[0], CARRIER[1]) { require(clock.is_some_and(|clock| clock <= entry.instructions.0) || upper.is_some_and(|clock| clock <= entry.instructions.0), Refusal::OutOfRangeWrite { address: row.physical.0 })?; }
                let flag = overlaps(low, high, FILL_FLAG.0, FILL_FLAG.0 + 4);
                if during && producer_pc {
                    let frame_store = inside(low, high, frame_low, sp);
                    let flag_clear = flag && row.size == 4 && row.physical == FILL_FLAG && row.value == Some(0);
                    require(row.source == Access::CpuWrite && row.core == Some(entry.core) && (frame_store || flag_clear), Refusal::OutOfRangeWrite { address: row.physical.0 })?;
                    if frame_store { for &(address, value) in &slots { if low <= u64::from(address) && u64::from(address) + 4 <= high { if row.size == 4 && address >= sp - 8 { require(row.value == Some(u64::from(value)), Refusal::BadFrame)?; } saved_writes.insert(address); } } }
                }
                if may_live && overlaps(low, high, frame_low, sp) && !producer_pc { return Err(Refusal::BadFrame); }
                if during && !producer_pc && row.source == Access::CpuWrite && overlaps(low, high, STAGING[0], STAGING[1]) { return Err(Refusal::Abi { boundary: Boundary::WriteConfinement }); }
                if may_live && overlaps(low, high, sp, stack_end) { return Err(Refusal::BadFrame); }
                if flag && may_live {
                    if producer_pc { clears.push(clock.ok_or(Refusal::CoverageDiffers)?); }
                    else if row.source == Access::CpuWrite && row.pc == Some(FILL_STORE_PC) && row.physical == FILL_FLAG && row.size == 4 && row.value == Some(1) && row.core == Some(completion.core) && clock == Some(completion.instructions.0 + 1) { completions.push(clock.ok_or(Refusal::CoverageDiffers)?); }
                    else { return Err(Refusal::FlagWriterDiffers { address: row.physical.0 }); }
                }
                if may_live && !producer_pc && DECODER_GLOBALS.iter().any(|&[start, end]| overlaps(low, high, start, end)) {
                    require(row.source == Access::CpuWrite, Refusal::Abi { boundary: Boundary::WriteConfinement })?;
                }
                if may_live && row.source != Access::CpuWrite {
                    for [start, end] in [STAGING, DESTINATION, BSS] {
                        if overlaps(low, high, start - HALO, start) || overlaps(low, high, end, end + HALO) { return Err(Refusal::OutOfRangeWrite { address: row.physical.0 }); }
                        if overlaps(low, high, start, end) && row.source == Access::HostWrite { return Err(Refusal::Abi { boundary: Boundary::WriteConfinement }); }
                    }
                    let model = row.source == Access::ModelBusWrite && row.core.is_none() && row.pc.is_none() && clock.is_none() && row.value.is_none() && row.address.0 & !0x40000000 == row.physical.0;
                    if overlaps(low, high, STAGING[0], STAGING[1]) {
                        require(model && within(storage.entered, storage.finished), Refusal::Abi { boundary: Boundary::WriteConfinement })?;
                        for (index, coverage) in copied.iter_mut().enumerate() {
                            let start = STAGING[0] + BLOCK * index as u32; let progress = boundary(report, evidence.progress.get(index).ok_or(missing(MissingEvidence::Progress))?.event)?;
                            if overlaps(low, high, start, start + BLOCK) && upper.is_some_and(|clock| clock <= progress.instructions.0 + 1) { coverage.push([low.max(u64::from(start)), high.min(u64::from(start + BLOCK))]); }
                        }
                    }
                    if overlaps(low, high, DESTINATION[0], DESTINATION[1]) { require(model && within(init.entered, finish.finished), Refusal::Abi { boundary: Boundary::WriteConfinement })?; }
                    if overlaps(low, high, BSS[0], BSS[1]) { require(model && within(fill.entered, completion), Refusal::Abi { boundary: Boundary::WriteConfinement })?; }
                }
                if row.source == Access::CpuWrite && row.pc == Some(Guest(0x73b6ac)) && row.physical == Guest(0x944440) && row.value == Some(1) && clock.is_some_and(|clock| entry.instructions.0 < clock) { done_stores.push(clock.ok_or(Refusal::CoverageDiffers)?); }
            }
            if during && row.source == Access::CpuRead && producer_pc {
                let clock = clock.ok_or(Refusal::CoverageDiffers)?;
                if overlaps(low, high, pointer, pointer + 4 * SEGMENT_WORDS as u32) {
                    let offset = row.physical.0.checked_sub(pointer).filter(|offset| offset % 4 == 0 && row.size == 4 && row.core == Some(entry.core)).ok_or(Refusal::FieldReadsDiffer)?;
                    record_reads.push((clock, offset, row.value));
                }
                if overlaps(low, high, FILL_FLAG.0, FILL_FLAG.0 + 4) {
                    require(row.physical == FILL_FLAG && row.size == 4 && row.core == Some(entry.core), Refusal::FlagOrderDiffers { step: FlagStep::WaitExit })?;
                    flag_reads.push((clock, row.value));
                }
            }
            if during && row.source == Access::CpuRead && row.core == Some(entry.core) && row.size == 4 && row.pc == Some(Guest(0x657194)) && row.physical == Guest(0x61c008) && row.value == Some(0) && clock.is_some_and(|clock| mode.entered.instructions.0 < clock && clock <= mode.finished.instructions.0) { mode_read = true; }
        }
    }
    require(mode_read && record_reads.len() >= expected_reads.len(), missing(MissingEvidence::InputReads))?;
    record_reads.sort_unstable_by_key(|read| read.0);
    let mut reads = Vec::with_capacity(expected_reads.len());
    for (&(clock, offset, value), &(field, phase)) in record_reads.iter().zip(&expected_reads) {
        let (_, after, until) = windows.iter().find(|window| window.0 == phase).ok_or(Refusal::CoverageDiffers)?;
        let expected = SEGMENT[(field.offset() / 4) as usize];
        require(offset == field.offset() && value == Some(u64::from(expected)) && after.instructions.0 < clock && clock <= until.instructions.0, Refusal::FieldReadsDiffer)?;
        reads.push(FieldRead { field, phase, value: expected });
    }
    require(record_reads.len() == expected_reads.len(), Refusal::FieldReadsDiffer)?;
    for observed in report.events.iter().filter(|event| carrier(event.pc)) { require(fetches.contains(&(observed.pc.0, observed.instructions.0 + 1)), missing(MissingEvidence::CodeGuard))?; }
    require(slots.iter().all(|(address, _)| saved_writes.contains(address)), missing(MissingEvidence::Frame))?;
    for (index, coverage) in copied.iter_mut().enumerate() {
        coverage.sort_unstable(); let start = u64::from(STAGING[0] + BLOCK * index as u32); let mut committed = start;
        for span in coverage.iter() { require(span[0] <= committed, missing(MissingEvidence::StagingWrites))?; committed = committed.max(span[1]); }
        require(committed == start + u64::from(BLOCK), missing(MissingEvidence::StagingWrites))?;
    }
    require(clears.len() == 1, missing(MissingEvidence::FlagClear))?;
    require(completions.len() == 1, missing(MissingEvidence::FillCompletion))?;
    let index = |wanted: fn(Step) -> bool| calls.iter().position(|call| wanted(call.step)).map(|index| OperationIndex(index as u32));
    let clear = position(calls, clears[0]);
    let length_read = record_reads.iter().find(|read| read.1 == Field::BssLength.offset()).map(|read| read.0).ok_or(missing(MissingEvidence::InputReads))?;
    require(clear == FlagPosition { after: index(|step| matches!(step, Step::Marker(MarkerId::MainStart))), before: index(|step| matches!(step, Step::Fill)) } && length_read < clears[0], Refusal::FlagOrderDiffers { step: FlagStep::Clear })?;
    flag_reads.sort_unstable_by_key(|read| read.0);
    let window = FlagPosition { after: index(|step| matches!(step, Step::Marker(MarkerId::DecodeEnd))), before: index(|step| matches!(step, Step::Marker(MarkerId::MainEnd))) };
    require(flag_reads.iter().all(|read| position(calls, read.0) == window), Refusal::FlagOrderDiffers { step: FlagStep::WaitExit })?;
    let exit = flag_reads.last().filter(|read| read.1.is_some_and(|value| value != 0) && completions[0] < read.0).map(|read| read.0).ok_or(Refusal::PrematurePublication { boundary: PublicationBoundary::ReturnBeforeFlag })?;
    let wait = position(calls, exit);
    require(done_stores.iter().all(|&clock| returned.instructions.0 < clock), Refusal::PrematurePublication { boundary: PublicationBoundary::FlashloaderDone })?;
    Ok((reads, FlagEffect { clear, completed_by: Writer::Retained(FILL_STORE_PC), completion_callback: FILL_CALLBACK, wait }))
}
/// The last operation returned before `clock` and the first entered at or after it.
fn position(calls: &[Call<'_>], clock: u64) -> FlagPosition {
    FlagPosition { after: calls.iter().rposition(|call| call.finished.instructions.0 < clock).map(|index| OperationIndex(index as u32)), before: calls.iter().position(|call| clock <= call.entered.instructions.0).map(|index| OperationIndex(index as u32)) }
}

fn samples(root: &Path, report: &Report, catalog: &Catalog, evidence: &Evidence, interval: &Interval<'_>, calls: &[Call<'_>], views: &mut Vec<ViewEffect>) -> Result<()> {
    let finish = call(calls, |step| matches!(step, Step::DecoderFinish))?.finished;
    let image5 = report.events.iter().find(|event| event.pc == IMAGE5_ENTRY).ok_or(missing(MissingEvidence::Image5Entry))?;
    let required = [(finish, SampleKind::DecoderFinish, Region::Destination, MissingEvidence::DecoderFinishView), (interval.returned, SampleKind::ProducerReturn, Region::Destination, MissingEvidence::ProducerReturnView), (interval.returned, SampleKind::ProducerReturn, Region::Bss, MissingEvidence::ProducerReturnView), (image5, SampleKind::Image5Entry, Region::Destination, MissingEvidence::Image5EntryView), (image5, SampleKind::Image5Entry, Region::Bss, MissingEvidence::Image5EntryView)];
    require(evidence.samples.len() == required.len(), Refusal::CoverageDiffers)?;
    let decoded = catalog.0.iter().find(|image| image.binding.kind == SourceKind::Image5).ok_or(Refusal::WrongImageIdentity)?;
    for (observed, kind, region, absent) in required {
        let matching = evidence.samples.iter().filter(|sample| sample.event.0 == observed.id.0 && sample.kind == kind && sample.region == region).collect::<Vec<_>>();
        require(matching.len() == 1, missing(absent))?;
        let sample = matching[0]; require(sample.view.event.0 == observed.id.0, Refusal::CoverageDiffers)?;
        let effect = match region {
            Region::Destination => {
                let (digest, difference) = checked_view(root, &sample.view, DESTINATION[0], DESTINATION[1], Some(&decoded.bytes))?;
                if let Some(address) = difference { return Err(Refusal::DecodedByteMismatch { address }); }
                ViewEffect { kind: ViewBoundary::Destination(kind), start: Guest(DESTINATION[0]), bytes: DESTINATION[1] - DESTINATION[0], digest }
            }
            Region::Bss => {
                let (digest, difference) = checked_view(root, &sample.view, BSS[0], BSS[1], None)?;
                if let Some(address) = difference { return Err(Refusal::BssNotZero { address }); }
                ViewEffect { kind: ViewBoundary::Bss(kind), start: Guest(BSS[0]), bytes: BSS[1] - BSS[0], digest }
            }
        };
        views.push(effect);
    }
    Ok(())
}

fn decoder(report: &Report, interval: &Interval<'_>, calls: &[Call<'_>]) -> Result<DecoderObservation> {
    let init = call(calls, |step| matches!(step, Step::DecoderInit))?;
    let finish = call(calls, |step| matches!(step, Step::DecoderFinish))?;
    let at_pc = |pc: u32, low: &Event, high: &Event| report.events.iter().filter(move |observed| observed.pc == Guest(pc) && between(observed, low, high)).collect::<Vec<_>>();
    let init_takes = at_pc(0x6ae3e8, init.entered, init.finished);
    let finish_takes = [at_pc(0x6ae440, finish.entered, finish.finished), at_pc(0x6ae4e0, finish.entered, finish.finished)].concat();
    let gives = at_pc(0x6ae4ec, finish.entered, finish.finished);
    let completions = at_pc(0x6ae310, init.entered, interval.returned);
    let completion_gives = at_pc(0x6ae358, init.entered, interval.returned);
    require(init_takes.len() == 1 && !finish_takes.is_empty() && gives.len() == 1 && !completions.is_empty(), missing(MissingEvidence::DecoderResource))?;
    for take in init_takes.iter().chain(&finish_takes) {
        require(take.registers.r[0] == DECODER_RESOURCE && take.registers.r[1] == u32::MAX && take.core == interval.entry.core, Refusal::SourceOperationDiffers)?;
        require(report.events.iter().any(|observed| observed.pc == Guest(take.pc.0 + 4) && observed.core == take.core && observed.registers.sp == take.registers.sp && take.id.0 < observed.id.0 && observed.id.0 < gives[0].id.0), missing(MissingEvidence::DecoderResource))?;
    }
    require(gives[0].registers.r[0] == DECODER_RESOURCE && completion_gives.iter().all(|give| give.registers.r[0] == DECODER_RESOURCE), Refusal::SourceOperationDiffers)?;
    require(completions.iter().all(|completion| completion.id.0 < gives[0].id.0), Refusal::DecoderCompletionOrder)?;
    Ok(DecoderObservation { completions: completions.len(), takes: init_takes.len() + finish_takes.len(), completion_gives: completion_gives.len() })
}

fn publication(report: &Report, interval: &Interval<'_>) -> Result<Vec<PublicationStep>> {
    let returned = interval.returned;
    let image5 = report.events.iter().find(|event| event.pc == IMAGE5_ENTRY).ok_or(missing(MissingEvidence::Image5Entry))?;
    require(returned.instructions < image5.instructions && returned.id.0 < image5.id.0, Refusal::PrematurePublication { boundary: PublicationBoundary::Image5Fetch })?;
    let after = |pc: u32, accept: &dyn Fn(&Event) -> bool| report.events.iter().find(|observed| observed.pc == Guest(pc) && observed.core == returned.core && returned.id.0 < observed.id.0 && accept(observed));
    let done = after(0x73b6ac, &|observed| observed.registers.r[5] == 0).ok_or(missing(MissingEvidence::Publication))?;
    let release = after(0x73b6c4, &|observed| observed.id.0 > done.id.0).ok_or(missing(MissingEvidence::Publication))?;
    let callback = after(0x73b6e8, &|observed| observed.id.0 > release.id.0).ok_or(missing(MissingEvidence::Publication))?;
    require(done.registers.r[6] & !0x40000000 == 0x944440 && release.registers.r[0] == FLASHLOADER_RESOURCE && release.registers.r[5] == 0, Refusal::SourceOperationDiffers)?;
    require(callback.registers.r[3] == REQUEST_CALLBACK.0 && callback.registers.r[1] == 0 && callback.registers.r[8] == returned.registers.r[0], Refusal::PrematurePublication { boundary: PublicationBoundary::RequestCallback })?;
    let transfer = report.calls.iter().find(|call| call.kind == CallKind::Image5Entry).ok_or(missing(MissingEvidence::Image5Entry))?;
    let before = boundary(report, transfer.before.ok_or(missing(MissingEvidence::Image5Entry))?)?;
    let entered = boundary(report, transfer.entry.ok_or(missing(MissingEvidence::Image5Entry))?)?;
    let descriptor = before.state.iter().find(|view| view.mapping.guest == Guest(before.registers.r[0]) && view.values.len() == 2).ok_or(missing(MissingEvidence::Image5Entry))?;
    let header = before.state.iter().find(|view| view.mapping.guest == Guest(descriptor.values[1]) && view.values.len() == 4).ok_or(missing(MissingEvidence::Image5Entry))?;
    require(descriptor.values[0] == 5 && header.values[3] == IMAGE5_ENTRY.0 && entered.pc == IMAGE5_ENTRY && entered.id.0 == image5.id.0 && entered.registers.r[0] == before.registers.r[0], Refusal::WrongEntry)?;
    require(done.id.0 < release.id.0 && release.id.0 < callback.id.0 && callback.id.0 < before.id.0, Refusal::PrematurePublication { boundary: PublicationBoundary::RequestCallback })?;
    Ok(vec![PublicationStep::ProducerReturn, PublicationStep::DoneStore { cell: Guest(0x944440) }, PublicationStep::Release { resource: FLASHLOADER_RESOURCE }, PublicationStep::RequestCallback { target: REQUEST_CALLBACK, failed: false }, PublicationStep::Image5Transfer { descriptor: Guest(entered.registers.r[0]) }, PublicationStep::Image5Entry])
}

fn image5_effect(report: &Report) -> Result<Image5Effect> {
    let transfer = report.calls.iter().find(|call| call.kind == CallKind::Image5Entry).ok_or(missing(MissingEvidence::Image5Entry))?;
    let entered = boundary(report, transfer.entry.ok_or(missing(MissingEvidence::Image5Entry))?)?;
    let finished = boundary(report, transfer.returned.ok_or(missing(MissingEvidence::Image5Entry))?)?;
    let initializer = report.calls.iter().filter(|call| matches!(call.kind, CallKind::Image5Service | CallKind::Image5Wait) && call.before.is_some_and(|id| entered.id.0 < id.0 && id.0 < finished.id.0)).map(|call| call.target).collect::<Vec<_>>();
    require(initializer.len() == 21 && finished.registers.r[0] == IMAGE5_OUTCOME, Refusal::InitializerDiffers)?;
    Ok(Image5Effect { core: entered.core, entry: entered.pc, initializer, outcome: IMAGE5_OUTCOME })
}

fn task1(report: &Report) -> Result<Task1Effect> {
    let submissions = report.calls.iter().filter(|call| call.kind == CallKind::Task && call.entry.and_then(|id| report.events.get(id.0)).and_then(|entry| entry.descriptor.as_ref()).is_some_and(|descriptor| descriptor.values.first() == Some(&1))).collect::<Vec<_>>();
    require(submissions.len() == 1, missing(MissingEvidence::Task1))?;
    let call = submissions[0];
    let entered = boundary(report, call.entry.ok_or(missing(MissingEvidence::Task1))?)?;
    let finished = boundary(report, call.returned.ok_or(missing(MissingEvidence::Task1))?)?;
    let descriptor = entered.descriptor.as_ref().ok_or(missing(MissingEvidence::Task1))?;
    let image5 = report.events.iter().find(|event| event.pc == IMAGE5_ENTRY).ok_or(missing(MissingEvidence::Image5Entry))?;
    require(descriptor.values.len() == 7 && descriptor.values[2] == 6 && descriptor.values[3] == 0xc00 && descriptor.values[4] == TASK1_ENTRY.0 && descriptor.values[5] == 0 && finished.registers.r[0] == 0 && image5.id.0 < entered.id.0, Refusal::Task1Differs)?;
    let consumer = report.events.iter().find(|event| event.pc == TASK1_ENTRY).ok_or(missing(MissingEvidence::Task1))?;
    require(entered.id.0 < consumer.id.0 && consumer.core == call.core && consumer.registers.cpsr & 0x010002bf == 0x13 && consumer.registers.r[0] == 0, Refusal::Task1Differs)?;
    let (current_address, node, creation) = match consumer.core { Core::Primary => (0x40054760, 0x4005ff0c, 0x4005beac), Core::Secondary => (0x400e4720, 0x400efecc, 0x400ebe6c) };
    let current = consumer.state.iter().find(|view| view.mapping.guest == Guest(current_address) && view.values.len() == 3).ok_or(missing(MissingEvidence::Task1))?;
    let task = consumer.state.iter().find(|view| view.mapping.guest == Guest(node) && view.values.len() == 3).ok_or(missing(MissingEvidence::Task1))?;
    let record = consumer.state.iter().find(|view| view.mapping.guest == Guest(creation) && view.values.len() == 10).ok_or(missing(MissingEvidence::Task1))?;
    require(current.values[0] == node && consumer.registers.r[1] == node && consumer.registers.r[2] == creation && task.values[2] == creation && record.values[0] == 2 && record.values[2] == TASK1_ENTRY.0 && record.values[3] == 5 && record.values[4] == 0xc00 && record.values[8] == 1, Refusal::Task1Differs)?;
    let base = record.values[5] & !0x40000000;
    require(base % 8 == 0 && base.checked_add(0xc00) == Some(consumer.registers.sp.0), Refusal::Task1Differs)?;
    Ok(Task1Effect { core: consumer.core, entry: TASK1_ENTRY, priority: 6, stack: 0xc00, argument: 0, status: Status(0) })
}

pub(super) fn compare(retained: &Admission, owned: &Admission) -> Result<()> {
    require(retained.role == Role::Retained && owned.role == Role::Owned, Refusal::Abi { boundary: Boundary::SourceIdentity })?;
    require(retained.effects == owned.effects, Refusal::SourceOperationDiffers)
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Control { Unchanged, WrongImage, WrongSegment, WrongDestination, WrongStaging, WrongEntry, WrongFillCallback, WrongFeedCallback, PrematureDone, PrematureImage5, ReturnBeforeFlag, BssNotZero, DecodedByteMismatch, StagingByteMismatch, MissingFreshBlock, MissingStagingWrites, OutOfRangeWrite, FlagWriterDiffers, FlagClearOrder, FieldReadOrder, WrongFifthArgument, BadFrame, BadStatus, SyntheticSkipMode, MissingCodeGuard, AliasDisagreement }
const CONTROLS: [Control; 26] = [Control::Unchanged, Control::WrongImage, Control::WrongSegment, Control::WrongDestination, Control::WrongStaging, Control::WrongEntry, Control::WrongFillCallback, Control::WrongFeedCallback, Control::PrematureDone, Control::PrematureImage5, Control::ReturnBeforeFlag, Control::BssNotZero, Control::DecodedByteMismatch, Control::StagingByteMismatch, Control::MissingFreshBlock, Control::MissingStagingWrites, Control::OutOfRangeWrite, Control::FlagWriterDiffers, Control::FlagClearOrder, Control::FieldReadOrder, Control::WrongFifthArgument, Control::BadFrame, Control::BadStatus, Control::SyntheticSkipMode, Control::MissingCodeGuard, Control::AliasDisagreement];
impl Control {
    fn name(self) -> String { serde_json::to_value(self).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default() }
    fn expected(self) -> Option<Refusal> {
        match self {
            Self::Unchanged => None,
            Self::WrongImage => Some(Refusal::WrongImageIdentity),
            Self::WrongSegment => Some(Refusal::WrongSegment),
            Self::WrongDestination => Some(Refusal::WrongDestination),
            Self::WrongStaging => Some(Refusal::WrongStaging),
            Self::WrongEntry => Some(Refusal::WrongEntry),
            Self::WrongFillCallback => Some(Refusal::WrongCallback { callback: CallbackRole::FillDone }),
            Self::WrongFeedCallback => Some(Refusal::WrongCallback { callback: CallbackRole::StorageFeed }),
            Self::PrematureDone => Some(Refusal::PrematurePublication { boundary: PublicationBoundary::FlashloaderDone }),
            Self::PrematureImage5 => Some(Refusal::PrematurePublication { boundary: PublicationBoundary::Image5Fetch }),
            Self::ReturnBeforeFlag => Some(Refusal::PrematurePublication { boundary: PublicationBoundary::ReturnBeforeFlag }),
            Self::BssNotZero => Some(Refusal::BssNotZero { address: BSS[0] }),
            Self::DecodedByteMismatch => Some(Refusal::DecodedByteMismatch { address: DESTINATION[0] }),
            Self::MissingFreshBlock => Some(missing(MissingEvidence::FreshBlock)),
            Self::MissingStagingWrites => Some(missing(MissingEvidence::StagingWrites)),
            Self::OutOfRangeWrite => Some(Refusal::OutOfRangeWrite { address: STAGING[1] }),
            Self::WrongFifthArgument => Some(Refusal::WrongFifthArgument),
            Self::BadFrame => Some(Refusal::BadFrame),
            Self::BadStatus => Some(Refusal::BadStatus),
            Self::SyntheticSkipMode => Some(Refusal::SyntheticSkipMode),
            Self::MissingCodeGuard => Some(missing(MissingEvidence::CodeGuard)),
            Self::AliasDisagreement => Some(Refusal::Abi { boundary: Boundary::Mapping }),
            Self::StagingByteMismatch => Some(Refusal::StagingByteMismatch { address: STAGING[0] }),
            Self::FlagWriterDiffers => Some(Refusal::FlagWriterDiffers { address: FILL_FLAG.0 }),
            Self::FlagClearOrder => Some(Refusal::FlagOrderDiffers { step: FlagStep::Clear }),
            Self::FieldReadOrder => Some(Refusal::FieldReadsDiffer),
        }
    }
}
#[derive(Serialize)]
struct ControlResult { control: Control, receipt: FileBinding, expected: Option<Refusal>, observed: Option<Refusal>, full_consumer_refused: bool, accepted: bool }
#[derive(Serialize)]
struct Controls { schema: u32, baseline: FileBinding, accepted: bool, cases: Vec<ControlResult> }

pub(super) fn controls<F>(root: &Path, analysis: &Path, output: &Path, mut full_consumer: F) -> std::result::Result<(), String>
where F: FnMut(&Path, &mut Report) -> std::result::Result<bool, String> {
    if output.try_exists().map_err(|error| error.to_string())? { return Err("image5 controls output already exists".into()); }
    let receipt = read(&root.join("continuations.json"))?;
    std::fs::create_dir(output).map_err(|error| error.to_string())?;
    let baseline = FileBinding { path: "baseline.json".into(), sha256: Digest::of(&receipt) };
    fujimod_xm1::bridge::write(&output.join(&baseline.path), &receipt)?;
    let mut results = Controls { schema: 1, baseline, accepted: false, cases: Vec::new() };
    let persist = |results: &Controls| fujimod_xm1::bridge::write(&output.join("image5-controls.json"), &serde_json::to_vec_pretty(results).map_err(|error| error.to_string())?);
    for control in CONTROLS {
        let case = output.join(control.name()); std::fs::create_dir(&case).map_err(|error| error.to_string())?;
        for file in std::fs::read_dir(root).map_err(|error| error.to_string())? {
            let file = file.map_err(|error| error.to_string())?;
            if file.file_name() == "continuations.json" || !file.file_type().map_err(|error| error.to_string())?.is_file() { continue; }
            std::fs::hard_link(file.path(), case.join(file.file_name())).map_err(|error| error.to_string())?;
        }
        let mut report: Report = serde_json::from_slice(&receipt).map_err(|error| error.to_string())?;
        mutate(control, &case, &mut report)?;
        let refused = full_consumer(&case, &mut report)?;
        let serialized = read(&case.join("continuations.json"))?;
        let report: Report = serde_json::from_slice(&serialized).map_err(|error| error.to_string())?;
        let mut catalog = Catalog::open(analysis)?;
        let binding = report.image5.as_ref().ok_or("control image5 witness is missing")?.binding.clone();
        let program = Program::from_binding(binding, &case, &mut catalog)?;
        let observed = admit(&case, &report, &catalog, &program).err();
        let expected = control.expected(); let accepted = observed == expected && refused == expected.is_some();
        results.cases.push(ControlResult { control, receipt: FileBinding { path: std::path::PathBuf::from(control.name()).join("continuations.json"), sha256: Digest::of(&serialized) }, expected, observed, full_consumer_refused: refused, accepted });
        persist(&results)?;
        if !accepted { return Err(format!("image5 control {control:?} did not reach its exact refusal")); }
    }
    results.accepted = true;
    persist(&results)
}

fn service<'a>(report: &'a Report, target: Guest) -> std::result::Result<&'a super::Call, String> {
    let evidence = report.image5.as_ref().ok_or("image5 evidence absent")?;
    let (entry, returned) = (evidence.entry.ok_or("image5 entry absent")?.0, evidence.caller_return.ok_or("image5 caller return absent")?.0);
    report.calls.iter().find(|call| call.kind == CallKind::Image5ProducerService && call.target == target && call.before.is_some_and(|id| entry < id.0 && id.0 < returned)).ok_or_else(|| format!("image5 service {:#x} absent", target.0))
}
fn set_argument(report: &mut Report, target: Guest, register: usize, value: u32) -> std::result::Result<(), String> {
    let call = service(report, target)?;
    let (before, entered) = (call.before.ok_or("service source absent")?.0, call.entry.ok_or("service entry absent")?.0);
    report.events[before].registers.r[register] = value; report.events[entered].registers.r[register] = value; Ok(())
}
fn rewrite_watches(root: &Path, report: &mut Report, mut change: impl FnMut(&mut Watch) -> std::result::Result<bool, String>) -> std::result::Result<(), String> {
    let mut changed = false;
    for (index, epoch) in report.epochs.iter_mut().enumerate() {
        let mut watch: Watch = serde_json::from_slice(&read(&artifact(root, &epoch.file)?)?).map_err(|error| error.to_string())?;
        if !change(&mut watch)? { continue; }
        let bytes = serde_json::to_vec_pretty(&watch).map_err(|error| error.to_string())?; let name = format!("control-watch-{index}.json");
        fujimod_xm1::bridge::write(&root.join(&name), &bytes)?;
        epoch.file = FileBinding { path: name.into(), sha256: Digest::of(&bytes) }; epoch.record_count = watch.records.len(); changed = true;
    }
    if changed { Ok(()) } else { Err("image5 control found no serialized row to change".into()) }
}
fn rebind_view(root: &Path, view: &mut RangeSnapshot, name: &str, change: impl FnOnce(&mut Vec<u8>, &mut RangeSnapshot)) -> std::result::Result<(), String> {
    let mut bytes = read(&artifact(root, &view.files[4])?)?;
    change(&mut bytes, view);
    fujimod_xm1::bridge::write(&root.join(name), &bytes)?;
    let binding = FileBinding { path: name.into(), sha256: Digest::of(&bytes) };
    for file in &mut view.files { *file = binding.clone(); }
    Ok(())
}

fn mutate(control: Control, root: &Path, report: &mut Report) -> std::result::Result<(), String> {
    let evidence = report.image5.as_ref().ok_or("image5 evidence absent")?;
    let entry = evidence.entry.ok_or("image5 entry absent")?.0; let returned = evidence.caller_return.ok_or("image5 caller return absent")?.0;
    let (entry_clock, returned_clock) = (report.events[entry].instructions, report.events[returned].instructions);
    match control {
        Control::Unchanged => {}
        Control::WrongImage => { report.identity.sources.iter_mut().find(|source| source.kind == SourceKind::Image5).ok_or("image5 source identity absent")?.sha256 = Digest::of(b"wrong image5 control"); }
        Control::WrongSegment => {
            let finished = service(report, SEGMENT_LOOKUP)?.returned.ok_or("segment return absent")?.0;
            let pointer = report.events[finished].registers.r[0];
            report.events[finished].state.iter_mut().find(|view| view.mapping.guest == Guest(pointer) && view.values.len() == SEGMENT_WORDS).ok_or("segment record view absent")?.values[0] = 6;
        }
        Control::WrongDestination => set_argument(report, DECODER_INIT, 0, DESTINATION[0] + 4)?,
        Control::WrongStaging => set_argument(report, STORAGE_READ, 2, (STAGING[0] + BLOCK) | 0x40000000)?,
        Control::WrongFillCallback => set_argument(report, FILL_REQUEST, 3, FILL_CALLBACK.0 + 4)?,
        Control::WrongFeedCallback => set_argument(report, STORAGE_READ, 3, FEED_CALLBACK.0 + 4)?,
        Control::WrongEntry => {
            let before = report.calls.iter().find(|call| call.kind == CallKind::Image5Entry).and_then(|call| call.before).ok_or("image5 transfer absent")?.0;
            let descriptor = Guest(report.events[before].registers.r[0]);
            let header = Guest(report.events[before].state.iter().find(|view| view.mapping.guest == descriptor && view.values.len() == 2).ok_or("image5 descriptor absent")?.values[1]);
            report.events[before].state.iter_mut().find(|view| view.mapping.guest == header && view.values.len() == 4).ok_or("image5 header absent")?.values[3] ^= 4;
        }
        Control::PrematureDone => rewrite_watches(root, report, |watch| {
            let Some(record) = watch.records.iter_mut().find(|record| record.row.source == Access::CpuWrite && record.row.pc == Some(Guest(0x73b6ac)) && record.row.physical == Guest(0x944440) && record.row.value == Some(1) && record.row.instructions.is_some_and(|clock| returned_clock < clock)) else { return Ok(false) };
            record.row.instructions = Some(returned_clock); record.latest_fetch.as_mut().ok_or("done writer fetch absent")?.instructions = returned_clock; Ok(true)
        })?,
        Control::PrematureImage5 => { let first = report.events.iter().position(|event| event.pc == IMAGE5_ENTRY).ok_or("image5 entry absent")?; report.events[first].instructions = returned_clock; }
        Control::ReturnBeforeFlag => rewrite_watches(root, report, |watch| {
            let mut changed = false;
            for record in watch.records.iter_mut().filter(|record| record.row.source == Access::CpuRead && record.row.pc.is_some_and(carrier) && record.row.physical == FILL_FLAG && record.row.value.is_some_and(|value| value != 0) && record.row.instructions.is_some_and(|clock| entry_clock < clock && clock <= returned_clock)) {
                record.row.value = Some(0); record.before = Some(super::plain_check::AccessBytes(vec![0; record.row.size as usize])); changed = true;
            }
            Ok(changed)
        })?,
        Control::BssNotZero | Control::DecodedByteMismatch => {
            let region = if control == Control::BssNotZero { Region::Bss } else { Region::Destination };
            let sample = report.image5.as_mut().and_then(|image5| image5.samples.iter_mut().find(|sample| sample.kind == SampleKind::ProducerReturn && sample.region == region)).ok_or("producer-return view absent")?;
            rebind_view(root, &mut sample.view, if region == Region::Bss { "control-bss.bin" } else { "control-destination.bin" }, |bytes, view| { bytes[0] ^= 0xff; view.expected_equal = false; view.first_difference = Some(view.mapping.guest); })?;
        }
        Control::MissingFreshBlock => { report.image5.as_mut().and_then(|image5| image5.progress.first_mut()).ok_or("progress absent")?.fresh = None; }
        Control::MissingStagingWrites => rewrite_watches(root, report, |watch| {
            let count = watch.records.len();
            watch.records.retain(|record| record.row.source != Access::ModelBusWrite || !overlaps(u64::from(record.row.physical.0), u64::from(record.row.physical.0) + record.row.size, STAGING[0], STAGING[1]));
            Ok(count != watch.records.len())
        })?,
        Control::OutOfRangeWrite => {
            let low = report.image5.as_ref().and_then(|image5| image5.frames.first()).map(|frame| frame.words.mapping.guest.0).ok_or("image5 frame samples absent")?;
            rewrite_watches(root, report, |watch| {
                let Some(record) = watch.records.iter_mut().find(|record| record.row.source == Access::CpuWrite && record.row.pc.is_some_and(carrier) && record.row.physical == Guest(low) && record.row.instructions.is_some_and(|clock| entry_clock < clock && clock <= returned_clock)) else { return Ok(false) };
                record.row.address = Guest(STAGING[1]); record.row.physical = Guest(STAGING[1]); Ok(true)
            })?;
        }
        Control::WrongFifthArgument => {
            let entered = service(report, STORAGE_READ)?.entry.ok_or("storage entry absent")?.0;
            let at_sp = report.events[entered].registers.sp.0;
            let frame = report.events[entered].frame.as_mut().map_err(|error| error.clone())?;
            let index = ((at_sp - frame.mapping.guest.0) / 4) as usize; *frame.values.get_mut(index).ok_or("fifth argument word absent")? = 2;
        }
        Control::BadFrame => { report.events[returned].registers.r[4] ^= 1; }
        Control::BadStatus => { report.events[returned].registers.r[0] = 1; }
        Control::SyntheticSkipMode => { let finished = service(report, MODE_QUERY)?.returned.ok_or("mode return absent")?.0; report.events[finished].registers.r[0] = 1; }
        Control::MissingCodeGuard => { report.image5.as_mut().ok_or("image5 evidence absent")?.code = None; }
        Control::StagingByteMismatch => {
            let fresh = report.image5.as_mut().and_then(|image5| image5.progress.first_mut()).and_then(|progress| progress.fresh.as_mut()).ok_or("fresh block absent")?;
            rebind_view(root, fresh, "control-staging.bin", |bytes, view| { bytes[0] ^= 0xff; view.expected_equal = false; view.first_difference = Some(view.mapping.guest); })?;
        }
        Control::FlagWriterDiffers => rewrite_watches(root, report, |watch| {
            let Some(record) = watch.records.iter_mut().find(|record| record.row.source == Access::CpuWrite && record.row.pc == Some(FILL_STORE_PC) && record.row.physical == FILL_FLAG && record.row.instructions.is_some_and(|clock| entry_clock < clock && clock <= returned_clock)) else { return Ok(false) };
            let writer = Guest(FILL_STORE_PC.0 - 4);
            record.row.pc = Some(writer); let fetch = record.latest_fetch.as_mut().ok_or("flag writer fetch absent")?; fetch.pc = writer; fetch.address = writer; fetch.physical = writer; Ok(true)
        })?,
        Control::FlagClearOrder => {
            let marked = service(report, MARKER)?.returned.ok_or("first marker return absent")?.0;
            let clock = report.events[marked].instructions;
            rewrite_watches(root, report, |watch| {
                let Some(record) = watch.records.iter_mut().find(|record| record.row.source == Access::CpuWrite && record.row.pc.is_some_and(carrier) && record.row.physical == FILL_FLAG && record.row.instructions.is_some_and(|row| entry_clock < row && row <= returned_clock)) else { return Ok(false) };
                record.row.instructions = Some(clock); record.latest_fetch.as_mut().ok_or("flag clear fetch absent")?.instructions = clock; Ok(true)
            })?;
        }
        Control::FieldReadOrder => {
            let segment = service(report, SEGMENT_LOOKUP)?.returned.ok_or("segment return absent")?.0;
            let pointer = report.events[segment].registers.r[0] & !0x40000000;
            rewrite_watches(root, report, |watch| {
                let field = |record: &super::plain_check::AttributedRow, offset: u32| record.row.source == Access::CpuRead && record.row.pc.is_some_and(carrier) && record.row.physical == Guest(pointer + offset) && record.row.instructions.is_some_and(|clock| entry_clock < clock && clock <= returned_clock);
                let (Some(first), Some(second)) = (watch.records.iter().position(|record| field(record, 0x14)), watch.records.iter().position(|record| field(record, 0x18))) else { return Ok(false) };
                let (early, late) = (watch.records[first].row.instructions.ok_or("read clock absent")?, watch.records[second].row.instructions.ok_or("read clock absent")?);
                for (index, clock) in [(first, late), (second, early)] { watch.records[index].row.instructions = Some(clock); watch.records[index].latest_fetch.as_mut().ok_or("read fetch absent")?.instructions = clock; }
                Ok(true)
            })?;
        }
        Control::AliasDisagreement => {
            let fresh = report.image5.as_mut().and_then(|image5| image5.progress.first_mut()).and_then(|progress| progress.fresh.as_mut()).ok_or("fresh block absent")?;
            let mut bytes = read(&artifact(root, &fresh.files[2])?)?; bytes[0] ^= 1;
            let name = "control-wrong-alias.bin"; fujimod_xm1::bridge::write(&root.join(name), &bytes)?;
            fresh.files[2] = FileBinding { path: name.into(), sha256: Digest::of(&bytes) }; fresh.guest_equal[2] = false;
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use super::super::continuation_view::{ByteCount as Bytes, DramChoice, Mapping};
    use std::path::PathBuf;

    struct Temporary(PathBuf);
    impl Temporary {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!("image5-check-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&path).expect("create test directory"); Self(path)
        }
        fn file(&self, name: &str, bytes: &[u8]) -> FileBinding { std::fs::write(self.0.join(name), bytes).expect("write view"); FileBinding { path: name.into(), sha256: Digest::of(bytes) } }
    }
    impl Drop for Temporary { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

    fn view(root: &Temporary, start: u32, contents: [&[u8]; 5], expected: Option<&[u8]>) -> RangeSnapshot {
        let mapping = |guest: u32| Mapping { guest: Guest(guest), physical: Guest(start), bytes: Bytes(contents[4].len() as u32), decoder: DramChoice::One, tcm: [[0; 2]; 2] };
        let files = std::array::from_fn(|index| root.file(&format!("view-{index}.bin"), contents[index]));
        let difference = contents[4].iter().enumerate().position(|(index, byte)| *byte != expected.map_or(0, |expected| expected[index])).map(|index| Guest(start + index as u32));
        RangeSnapshot { event: EventId(0), mapping: mapping(start), alias_mapping: mapping(start | 0x40000000), files, guest_equal: std::array::from_fn(|index| contents[index] == contents[4]), expected_equal: difference.is_none(), first_difference: difference }
    }

    #[test]
    fn a_view_reports_its_first_difference_from_the_source() {
        let root = Temporary::new(); let expected = [1u8, 2, 3, 4]; let observed = [1u8, 2, 9, 4];
        let sample = view(&root, BSS[0], [&observed; 5], Some(&expected));
        let (_, difference) = checked_view(&root.0, &sample, BSS[0], BSS[0] + 4, Some(&expected)).expect("admitted view");
        assert_eq!(difference, Some(BSS[0] + 2));
        let (_, zero) = checked_view(&root.0, &view(&root, BSS[0], [&[0u8; 4]; 5], None), BSS[0], BSS[0] + 4, None).expect("zero view");
        assert_eq!(zero, None);
    }
    #[test]
    fn an_alias_spelling_that_disagrees_with_physical_bytes_is_refused() {
        let root = Temporary::new(); let bytes = [0u8; 8]; let alias = [0u8, 1, 0, 0, 0, 0, 0, 0];
        let sample = view(&root, BSS[0], [&bytes, &bytes, &alias, &bytes, &bytes], None);
        assert_eq!(checked_view(&root.0, &sample, BSS[0], BSS[0] + 8, None).err(), Some(Refusal::Abi { boundary: Boundary::Mapping }));
    }
    #[test]
    fn a_view_of_a_different_extent_is_refused() {
        let root = Temporary::new(); let bytes = [0u8; 8];
        let sample = view(&root, BSS[0] + 4, [&bytes; 5], None);
        assert!(checked_view(&root.0, &sample, BSS[0], BSS[0] + 8, None).is_err());
    }
    #[test]
    fn the_staged_record_must_carry_the_canonical_segment5_header() {
        let at = super::super::image5_runtime::RECORD as usize;
        let mut payload = vec![0u8; at + (STAGING[1] - STAGING[0]) as usize];
        for (index, value) in super::super::image5_runtime::RECORD_HEADER.iter().enumerate() { payload[at + 4 * index..at + 4 * index + 4].copy_from_slice(&value.to_le_bytes()); }
        assert_eq!(record(&payload).expect("canonical header").len(), (STAGING[1] - STAGING[0]) as usize);
        assert!(record(&payload[..at + 16]).is_err());
        payload[at] ^= 1;
        assert!(record(&payload).is_err());
    }

    fn receipt(update: &str, sources: Vec<super::super::SourceBinding>) -> Report {
        let digest = Digest::of(b"synthetic image5 identity");
        let identity = super::super::InputIdentity { run: digest.clone(), update: Digest::try_from(update.to_owned()).expect("update digest"), update_file: FileBinding { path: "input-update.DAT".into(), sha256: digest.clone() }, rom: digest.clone(), executable: digest.clone(), sources };
        Report { schema: 3, program: super::super::loader_orchestration_owned::ProgramBinding::Retained, plain: None, image5: None, origin: None, identity, checkpoint: digest.clone(), instructions: super::super::Count(0), menu: Ok(None), error: None, machine_failure: None, events: Vec::new(), calls: Vec::new(), snapshots: Vec::new(), pulses: Vec::new(), epochs: Vec::new(), trace: FileBinding { path: "trace.json".into(), sha256: digest.clone() }, timer_transactions: FileBinding { path: "timer-transactions.json".into(), sha256: digest }, static_calls: Vec::new() }
    }
    fn admission(role: Role, status: u32) -> Admission {
        let position = FlagPosition { after: Some(OperationIndex(1)), before: Some(OperationIndex(2)) };
        let flag = FlagEffect { clear: position, completed_by: Writer::Retained(FILL_STORE_PC), completion_callback: FILL_CALLBACK, wait: FlagPosition { after: Some(OperationIndex(10)), before: Some(OperationIndex(11)) } };
        let effects = Effects { reads: Vec::new(), operations: vec![Operation::DecoderFinish], progress: vec![ByteCount(BLOCK)], views: Vec::new(), flag, frame: FrameEffect { status: Status(status), restored: true, link: Guest(0x422b8) }, publication: vec![PublicationStep::ProducerReturn], image5: Image5Effect { core: Core::Primary, entry: IMAGE5_ENTRY, initializer: Vec::new(), outcome: IMAGE5_OUTCOME }, task1: Task1Effect { core: Core::Primary, entry: TASK1_ENTRY, priority: 6, stack: 0xc00, argument: 0, status: Status(0) } };
        Admission { schema: 1, role, scope: Scope::NaturalReadSuccessObservedBoundaries, limits: Vec::new(), unqualified: [UnqualifiedBoundary::SkipMode; 11], decoder: DecoderObservation { completions: 1, takes: 2, completion_gives: 1 }, effects }
    }
    #[test]
    fn comparison_requires_retained_then_owned_roles_and_equal_effects() {
        assert!(compare(&admission(Role::Retained, 0), &admission(Role::Owned, 0)).is_ok());
        assert_eq!(compare(&admission(Role::Owned, 0), &admission(Role::Retained, 0)).err(), Some(Refusal::Abi { boundary: Boundary::SourceIdentity }));
        assert_eq!(compare(&admission(Role::Retained, 0), &admission(Role::Owned, 1)).err(), Some(Refusal::SourceOperationDiffers));
    }
    #[test]
    fn decoder_interleaving_is_not_a_comparison_obligation() {
        let mut owned = admission(Role::Owned, 0); owned.decoder.completions = 3;
        assert!(compare(&admission(Role::Retained, 0), &owned).is_ok());
    }
    #[test]
    #[ignore = "requires IMAGE5_OBSERVATION and LOADER_CONTINUATIONS_ANALYSIS"]
    fn every_typed_control_reaches_its_refusal() {
        let source = PathBuf::from(std::env::var_os("IMAGE5_OBSERVATION").expect("set an admitted image5 observation"));
        let analysis = PathBuf::from(std::env::var_os("LOADER_CONTINUATIONS_ANALYSIS").expect("set canonical analysis root"));
        let output = source.parent().expect("observation parent").join(format!("image5-controls-test-{}", std::process::id()));
        let result = super::super::loader_continuations_check::controls_image5(&source, &analysis, &output);
        let _ = std::fs::remove_dir_all(&output);
        assert!(result.is_ok(), "{result:?}");
    }
    #[test]
    #[ignore = "requires IMAGE5_STAGE and LOADER_CONTINUATIONS_ANALYSIS"]
    fn the_reviewed_stage_binds_and_a_changed_contract_is_refused() {
        let stage = PathBuf::from(std::env::var_os("IMAGE5_STAGE").expect("set the staged image5 directory"));
        let analysis = PathBuf::from(std::env::var_os("LOADER_CONTINUATIONS_ANALYSIS").expect("set canonical analysis root"));
        let output = Temporary::new();
        let mut catalog = Catalog::open(&analysis).expect("canonical catalog");
        let program = Program::from_staged(Role::Owned, Some(&stage), &output.0, &mut catalog).expect("reviewed stage binds");
        assert!(program.owned() && program.return_pc() == Guest(0x6a96d0) && program.return_lr() == Guest(0x422b8) && program.fill_return_pc() == Some(Guest(0x6a973c)));
        let image4 = catalog.0.iter().find(|image| image.binding.kind == SourceKind::Image4).expect("image4");
        assert_eq!(image4.binding.sha256.as_str(), "4808c1c321f3605fb36163614819d52efb11cf7f6ac6825f014b728dccb25f77");
        let staged_sources: Vec<_> = catalog.0.iter().map(|image| image.binding.clone()).collect();
        assert_eq!(identity(&receipt(program.update(), staged_sources), &catalog, &program), Ok(()));
        let canonical_sources: Vec<_> = Catalog::open(&analysis).expect("canonical catalog").0.iter().map(|image| image.binding.clone()).collect();
        assert_eq!(identity(&receipt(program.update(), canonical_sources), &catalog, &program), Err(Refusal::WrongImageIdentity));
        let changed = Temporary::new();
        for name in ["image5-load-v1.bin", "image5-load-v1.symbols"] { std::fs::copy(stage.join(name), changed.0.join(name)).expect("copy stage"); }
        let mut contract = read(&stage.join("owned-image5-stage-v1-contract.json")).expect("contract"); contract.push(b'\n');
        std::fs::write(changed.0.join("owned-image5-stage-v1-contract.json"), contract).expect("write contract");
        let rebound = Temporary::new();
        assert!(Program::from_staged(Role::Owned, Some(&changed.0), &rebound.0, &mut Catalog::open(&analysis).expect("catalog")).is_err());
        assert!(Program::from_staged(Role::Retained, Some(&stage), &rebound.0, &mut Catalog::open(&analysis).expect("catalog")).is_err());
    }
}
