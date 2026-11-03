#[path = "../../emulation/fujimod-xm1/src/heap_state.rs"]
mod heap_state;
#[path = "../../emulation/fujimod-xm1/src/heap_replay_state.rs"]
mod heap_replay_state;

use heap_replay_state::{AllocationOutcome, Layout as RequestLayout, Request, Tape};
use heap_state::{AllocationId, Event, Summary};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::{
    alloc::{alloc_zeroed, dealloc, Layout},
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    ptr::NonNull,
};
use xm1_general_heap::{Heap, HeapAccess, HeapError, StockBridge};

const LIMIT: u64 = 256 * 1024 * 1024;
const GUARD_BYTES: usize = 32;
const GUARD_VALUE: u8 = 0xa5;
const ERRNO_MARKER: u32 = 0x45;
const USAGE: &str = "heap-bridge-replay --trace DIRECTORY --admission JSON --update DAT --image4 BIN --image5 BIN --output NEW_JSON\nReplays the admitted 8,529-operation no-card tape through the production stock bridge at original and recipe capacities.";

type Result<T, E = Failure> = std::result::Result<T, E>;

#[derive(Debug)]
enum Failure {
    Arguments,
    InvalidDigest,
    Io(std::io::Error),
    Json(serde_json::Error),
    InputLimit { path: PathBuf },
    InputIdentity { path: PathBuf },
    Tape(heap_replay_state::Failure),
    Population,
    HostAllocation,
    ArenaLayout(std::alloc::LayoutError),
    HeapInitialization(HeapError),
    Guard,
    PayloadRange,
    Scope,
    Errno,
    Bridge { step: usize, error: HeapError },
    AllocationDivergence { step: usize },
    Overlap { step: usize },
    DuplicateLifetime { step: usize },
    UnknownLifetime { step: usize },
    FreeLayout { step: usize },
    Payload { identity: AllocationId, offset: usize },
    UnsupportedOperation { step: usize },
}
impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self { Self::Io(error) }
}
impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Self { Self::Json(error) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Digest([u8; 32]);
impl Digest {
    fn parse(text: &str) -> Result<Self> {
        if text.len() != 64 { return Err(Failure::InvalidDigest); }
        let mut bytes = [0; 32];
        for (byte, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
            let text = std::str::from_utf8(pair).map_err(|_| Failure::InvalidDigest)?;
            *byte = u8::from_str_radix(text, 16).map_err(|_| Failure::InvalidDigest)?;
        }
        Ok(Self(bytes))
    }
    fn of(bytes: &[u8]) -> Self { Self(Sha256::digest(bytes).into()) }
}
impl Serialize for Digest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut text = [0; 64];
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for (index, byte) in self.0.iter().enumerate() {
            text[index * 2] = HEX[(byte >> 4) as usize];
            text[index * 2 + 1] = HEX[(byte & 15) as usize];
        }
        serializer.serialize_str(std::str::from_utf8(&text).map_err(serde::ser::Error::custom)?)
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Flag { Trace, Admission, Update, Image4, Image5, Output }
impl Flag {
    fn parse(text: &str) -> Result<Self> {
        match text {
            "--trace" => Ok(Self::Trace), "--admission" => Ok(Self::Admission),
            "--update" => Ok(Self::Update), "--image4" => Ok(Self::Image4),
            "--image5" => Ok(Self::Image5), "--output" => Ok(Self::Output),
            _ => Err(Failure::Arguments),
        }
    }
}
#[derive(Serialize)]
struct Arguments { trace: PathBuf, admission: PathBuf, update: PathBuf, image4: PathBuf, image5: PathBuf, output: PathBuf }
impl Arguments {
    fn parse() -> Result<Self> {
        let mut arguments = std::env::args_os().skip(1);
        let mut paths = BTreeMap::new();
        while let Some(flag) = arguments.next() {
            let flag = Flag::parse(flag.to_str().ok_or(Failure::Arguments)?)?;
            let path = PathBuf::from(arguments.next().ok_or(Failure::Arguments)?);
            if paths.insert(flag, path).is_some() { return Err(Failure::Arguments); }
        }
        let mut take = |flag| paths.remove(&flag).ok_or(Failure::Arguments);
        Ok(Self { trace: take(Flag::Trace)?, admission: take(Flag::Admission)?, update: take(Flag::Update)?,
            image4: take(Flag::Image4)?, image5: take(Flag::Image5)?, output: take(Flag::Output)? })
    }
}
fn read(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    if length > LIMIT { return Err(Failure::InputLimit { path: path.to_owned() }); }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT { return Err(Failure::InputLimit { path: path.to_owned() }); }
    Ok(bytes)
}
fn pinned(path: &Path, expected: Digest) -> Result<(Vec<u8>, Digest)> {
    let bytes = read(path)?;
    let digest = Digest::of(&bytes);
    if digest != expected { return Err(Failure::InputIdentity { path: path.to_owned() }); }
    Ok((bytes, digest))
}
#[derive(Serialize)]
struct CanonicalSource { dat_sha256: Digest, image4_sha256: Digest, image5_sha256: Digest }
#[derive(Serialize)]
struct Evidence { canonical_source: CanonicalSource, events_sha256: Digest, summary_sha256: Digest, admission_sha256: Digest, executable_sha256: Digest }
fn prepare(arguments: &Arguments) -> Result<(Tape, Evidence)> {
    let (_, dat_sha256) = pinned(&arguments.update, Digest::parse("43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")?)?;
    let (_, image4_sha256) = pinned(&arguments.image4, Digest::parse("f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")?)?;
    let (_, image5_sha256) = pinned(&arguments.image5, Digest::parse("3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")?)?;
    let (_, admission_sha256) = pinned(&arguments.admission, Digest::parse("d661a4ee39c4676f2998bbd3875e2477d13c1a921188beb2021b4a99e75a12c2")?)?;
    let (event_bytes, events_sha256) = pinned(&arguments.trace.join("events.json"), Digest::parse("798b8dd160509598a2b01d4f37bb811794c3bd247028bcd7c48510fe528e6bab")?)?;
    let events: Vec<Event> = serde_json::from_slice(&event_bytes)?;
    let (summary_bytes, summary_sha256) = pinned(&arguments.trace.join("summary.json"), Digest::parse("db6414549e0b895d1a9242bcffa8ba1996f080a776c169284ee4f571f384665e")?)?;
    let summary: Summary = serde_json::from_slice(&summary_bytes)?;
    let tape = Tape::from_events(&events).map_err(Failure::Tape)?;
    tape.bind_summary(&summary).map_err(Failure::Tape)?;
    let mallocs = tape.steps().iter().filter(|step| matches!(step.request, Request::Malloc { outcome: AllocationOutcome::Success { .. }, .. })).count();
    let frees = tape.steps().iter().filter(|step| matches!(step.request, Request::Free { .. })).count();
    if events.len() != 51_214 || tape.steps().len() != 8_529 || mallocs != 4_490 || frees != 4_039
        || tape.final_live().len() != 451 || tape.requested_live().0 != 113_309 || tape.requested_peak().0 != 115_185 {
        return Err(Failure::Population);
    }
    let executable_sha256 = Digest::of(&read(&std::env::current_exe()?)?);
    Ok((tape, Evidence { canonical_source: CanonicalSource { dat_sha256, image4_sha256, image5_sha256 },
        events_sha256, summary_sha256, admission_sha256, executable_sha256 }))
}

struct Arena { pointer: NonNull<u8>, layout: Layout, capacity: usize }
impl Arena {
    fn new(capacity: usize) -> Result<Self> {
        let layout = Layout::from_size_align(capacity + GUARD_BYTES * 2, 16).map_err(Failure::ArenaLayout)?;
        let pointer = NonNull::new(unsafe { alloc_zeroed(layout) }).ok_or(Failure::HostAllocation)?;
        let arena = Self { pointer, layout, capacity };
        unsafe {
            arena.pointer.as_ptr().write_bytes(GUARD_VALUE, GUARD_BYTES);
            arena.base().as_ptr().add(capacity).write_bytes(GUARD_VALUE, GUARD_BYTES);
        }
        Ok(arena)
    }
    fn base(&self) -> NonNull<u8> { unsafe { NonNull::new_unchecked(self.pointer.as_ptr().add(GUARD_BYTES)) } }
    fn guards(&self) -> Result<()> {
        let before = unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), GUARD_BYTES) };
        let after = unsafe { std::slice::from_raw_parts(self.base().as_ptr().add(self.capacity), GUARD_BYTES) };
        if before.iter().chain(after).any(|byte| *byte != GUARD_VALUE) { return Err(Failure::Guard); }
        Ok(())
    }
    fn range(&self, pointer: NonNull<u8>, bytes: usize) -> Result<std::ops::Range<usize>> {
        let low = pointer.as_ptr() as usize;
        let high = low.checked_add(bytes.max(1)).ok_or(Failure::PayloadRange)?;
        let base = self.base().as_ptr() as usize;
        if low % 8 != 0 || low < base || high > base + self.capacity { return Err(Failure::PayloadRange); }
        Ok(low..high)
    }
}
impl Drop for Arena {
    fn drop(&mut self) { unsafe { dealloc(self.pointer.as_ptr(), self.layout); } }
}
struct HostAccess {
    heap: RefCell<Heap>, errno: Cell<u32>, active: Cell<bool>, entries: Cell<u64>, exits: Cell<u64>,
    capacity: usize,
}
struct Scope<'a>(&'a HostAccess);
impl Drop for Scope<'_> {
    fn drop(&mut self) { self.0.active.set(false); self.0.exits.set(self.0.exits.get() + 1); }
}
unsafe impl HeapAccess for &HostAccess {
    fn capacity(&self) -> usize { self.capacity }
    fn with_heap<T>(&self, operation: impl FnOnce(&mut Heap) -> T) -> T {
        assert!(!self.active.replace(true), "recursive bridge heap scope");
        self.entries.set(self.entries.get() + 1);
        let _scope = Scope(self);
        operation(&mut self.heap.borrow_mut())
    }
    fn set_errno(&self, errno: u32) { self.errno.set(errno); }
}
impl HostAccess {
    fn balanced(&self) -> Result<()> {
        if self.active.get() || self.entries.get() != self.exits.get() { return Err(Failure::Scope); }
        if self.errno.get() != ERRNO_MARKER { return Err(Failure::Errno); }
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Capacity { Original, Recipe }
impl Capacity {
    const ALL: [Self; 2] = [Self::Original, Self::Recipe];
    fn bytes(self) -> usize { match self { Self::Original => 1_572_832, Self::Recipe => 1_507_296 } }
}
struct Live { pointer: NonNull<u8>, layout: RequestLayout }
fn pattern(identity: AllocationId, offset: usize) -> u8 {
    let seed = identity.0.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    (seed.rotate_left((offset % 64) as u32) as u8).wrapping_add((offset as u8).wrapping_mul(37))
}
fn check_payloads(arena: &Arena, live: &BTreeMap<AllocationId, Live>) -> Result<u64> {
    let mut checked = 0;
    for (&identity, allocation) in live {
        let bytes = allocation.layout.size.0 as usize;
        arena.range(allocation.pointer, bytes)?;
        let payload = unsafe { std::slice::from_raw_parts(allocation.pointer.as_ptr(), bytes) };
        if let Some(offset) = payload.iter().enumerate().position(|(offset, byte)| *byte != pattern(identity, offset)) {
            return Err(Failure::Payload { identity, offset });
        }
        checked += bytes as u64;
    }
    Ok(checked)
}
#[derive(Serialize)]
struct ReplayResult {
    capacity: Capacity, arena_bytes: usize, operations: usize, mallocs: usize, frees: usize,
    final_live: usize, final_requested_bytes: u32, peak_requested_bytes: u32,
    payload_bytes_checked: u64, bridge_scope_entries: u64, bridge_scope_exits: u64,
    errno_before: u32, errno_after: u32,
}
fn replay(tape: &Tape, capacity: Capacity) -> Result<ReplayResult> {
    let arena = Arena::new(capacity.bytes())?;
    let heap = unsafe { Heap::new(arena.base(), capacity.bytes()) }.map_err(Failure::HeapInitialization)?;
    let access = HostAccess { heap: RefCell::new(heap), errno: Cell::new(ERRNO_MARKER), active: Cell::new(false),
        entries: Cell::new(0), exits: Cell::new(0), capacity: capacity.bytes() };
    let bridge = StockBridge::new(&access);
    let mut live = BTreeMap::<AllocationId, Live>::new();
    let mut mallocs = 0;
    let mut frees = 0;
    let mut checked = 0;
    let mut requested = 0u32;
    let mut peak = 0u32;
    for (index, step) in tape.steps().iter().enumerate() {
        match step.request {
            Request::Malloc { layout, outcome: AllocationOutcome::Success { identity } } => {
                let pointer = bridge.malloc(layout.size.0).map_err(|error| Failure::Bridge { step: index, error })?
                    .ok_or(Failure::AllocationDivergence { step: index })?;
                let range = arena.range(pointer, layout.size.0 as usize)?;
                for allocation in live.values() {
                    let occupied = arena.range(allocation.pointer, allocation.layout.size.0 as usize)?;
                    if range.start < occupied.end && occupied.start < range.end { return Err(Failure::Overlap { step: index }); }
                }
                if live.contains_key(&identity) { return Err(Failure::DuplicateLifetime { step: index }); }
                for offset in 0..layout.size.0 as usize { unsafe { pointer.as_ptr().add(offset).write(pattern(identity, offset)); } }
                live.insert(identity, Live { pointer, layout });
                requested = requested.checked_add(layout.size.0).ok_or(Failure::Population)?;
                peak = peak.max(requested);
                mallocs += 1;
            }
            Request::Free { identity, layout } => {
                let pointer = if let Some(identity) = identity {
                    let allocation = live.remove(&identity).ok_or(Failure::UnknownLifetime { step: index })?;
                    if Some(allocation.layout) != layout { return Err(Failure::FreeLayout { step: index }); }
                    requested = requested.checked_sub(allocation.layout.size.0).ok_or(Failure::Population)?;
                    Some(allocation.pointer)
                } else {
                    if layout.is_some() { return Err(Failure::FreeLayout { step: index }); }
                    None
                };
                unsafe { bridge.free(pointer) }.map_err(|error| Failure::Bridge { step: index, error })?;
                frees += 1;
            }
            _ => return Err(Failure::UnsupportedOperation { step: index }),
        }
        access.balanced()?;
        arena.guards()?;
        checked += check_payloads(&arena, &live)?;
    }
    if live.len() != 451 || mallocs != 4_490 || frees != 4_039 || requested != tape.requested_live().0 || peak != tape.requested_peak().0
        || !live.iter().map(|(identity, allocation)| (*identity, allocation.layout)).eq(tape.final_live().iter().map(|(identity, layout)| (*identity, *layout))) {
        return Err(Failure::Population);
    }
    Ok(ReplayResult { capacity, arena_bytes: capacity.bytes(), operations: tape.steps().len(), mallocs, frees,
        final_live: live.len(), final_requested_bytes: requested, peak_requested_bytes: peak,
        payload_bytes_checked: checked, bridge_scope_entries: access.entries.get(), bridge_scope_exits: access.exits.get(),
        errno_before: ERRNO_MARKER, errno_after: access.errno.get() })
}
#[derive(Serialize)]
struct Report<'a> { schema: u32, scope: &'static str, inputs: &'a Arguments, evidence: Evidence, results: Vec<ReplayResult>, limitations: [&'static str; 3] }
fn execute() -> Result<()> {
    let arguments = Arguments::parse()?;
    let (tape, evidence) = prepare(&arguments)?;
    let results = Capacity::ALL.into_iter().map(|capacity| replay(&tape, capacity)).collect::<Result<Vec<_>>>()?;
    let report = Report { schema: 1, scope: "host-production-stock-bridge-admitted-no-card-tape", inputs: &arguments, evidence, results,
        limitations: ["Native host execution of the production shared StockBridge, not A32 wrapper execution.",
            "Host serialized heap scopes, not retained RTOS locking or contention qualification.",
            "Finite no-card tape, not capture, development, storage or silicon timing qualification."] };
    let bytes = serde_json::to_vec_pretty(&report)?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(&arguments.output)?;
    output.write_all(&bytes)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("heap-bridge-replay: {error:?}");
            if matches!(error, Failure::Arguments) { eprintln!("{USAGE}"); }
            ExitCode::FAILURE
        }
    }
}
