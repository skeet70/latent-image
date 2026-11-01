mod heap_state;
#[path = "memory_view.rs"]
mod memory_view;

use heap_state::{Address, ContextKey, Core, Operation, Recorder, Snapshot, TaskId};
use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::bridge::{read, sha256, write};
use fujimod_xm1::guest::RouteKey;
use fujimod_xm1::machine::{Machine, RunOutcome};
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, fs::OpenOptions, io::{BufWriter, Write}, path::PathBuf, process::ExitCode};

const USAGE: &str = "heap-observe --run RUN --update DAT --boot-rom ROM --analysis ROOT --observer enabled|disabled --output DIR";
const ENTRIES: [u32; 8] = [0x89cde4, 0x89e560, 0x89da64, 0xbf1c44, 0x89e464, 0x72b110, 0x72b164, 0x72b1cc];
const CODE: [(u32, u32); 17] = [(0x89cda4, 0x89d4f8), (0x89da3c, 0x89dfbc), (0x89e464, 0x89e7c4),
    (0xbf1c1c, 0xbf1cd4), (0x72b110, 0x72b210), (0x723cb8, 0x723ce8), (0x890ed8, 0x890f04),
    (0x4118c, 0x411cc), (0xd118c, 0xd11cc), (0x412bc, 0x412cc), (0xd12bc, 0xd12cc),
    (0x427b8, 0x427d4), (0xd27b8, 0xd27d4), (0x4c620, 0x4c6a8), (0xdc5e0, 0xdc668),
    (0x4cbd8, 0x4cc00), (0xdcb98, 0xdcbc0)];
const HEAP_START: u32 = 0xa71a00;
const HEAP_END: u32 = 0xbf1a00;
const WATCH_LIMIT: usize = 65_536;
const EVENT_LIMIT: usize = 32_768;
const ROUTE_INSTRUCTIONS: u64 = 80_000_000 + 15 * 16_000_000 + 40_000_000 + 90_000_000;

struct ArtifactBytes(u64);
struct WatchOutput { writer: BufWriter<std::fs::File>, used: ArtifactBytes }
impl WatchOutput {
    fn new(path: &std::path::Path) -> Result<Self, String> {
        Ok(Self { writer: BufWriter::new(OpenOptions::new().create_new(true).write(true).open(path).map_err(|error| error.to_string())?), used: ArtifactBytes(0) })
    }
    fn append(&mut self, bytes: &[u8]) -> Result<(), String> {
        let next = self.used.0.checked_add(bytes.len() as u64 + 1).ok_or("watch byte count overflow")?;
        require(next <= 256 * 1024 * 1024, "watch output exceeds byte bound")?;
        self.writer.write_all(bytes).and_then(|_| self.writer.write_all(b"\n")).map_err(|error| error.to_string())?;
        self.used = ArtifactBytes(next);
        Ok(())
    }
    fn flush(&mut self) -> Result<(), String> { self.writer.flush().map_err(|error| error.to_string()) }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct Digest(String);
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ObserverMode { Enabled, Disabled }
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
struct Identity { run: Digest, update: Digest, boot_rom: Digest, observer: Digest, images: Vec<Digest> }
struct Image { base: u32, bytes: Vec<u8> }
#[derive(Deserialize)]
#[serde(transparent)]
struct HexAddress(String);
impl HexAddress {
    fn decode(&self) -> Result<u32, String> {
        u32::from_str_radix(self.0.strip_prefix("0x").ok_or("trace word is not hexadecimal")?, 16).map_err(|error| error.to_string())
    }
}
#[derive(Deserialize)]
struct HitRegisters {
    r0: HexAddress, r1: HexAddress, r2: HexAddress, r3: HexAddress, r4: HexAddress, r5: HexAddress, r6: HexAddress,
    r7: HexAddress, r8: HexAddress, r9: HexAddress, r10: HexAddress, r11: HexAddress, r12: HexAddress,
}
impl HitRegisters {
    fn decode(&self) -> Result<[u32; 13], String> {
        Ok([self.r0.decode()?, self.r1.decode()?, self.r2.decode()?, self.r3.decode()?, self.r4.decode()?,
            self.r5.decode()?, self.r6.decode()?, self.r7.decode()?, self.r8.decode()?, self.r9.decode()?,
            self.r10.decode()?, self.r11.decode()?, self.r12.decode()?])
    }
}
#[derive(Deserialize)]
struct RecordedHit {
    address: HexAddress, core: Core, instructions: heap_state::Counter,
    registers: HitRegisters, sp: HexAddress, lr: HexAddress, cpsr: u32,
}
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Access { CpuFetch, CpuWrite, CpuRead, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Deserialize, Serialize)]
struct AccessRecord {
    source: Access, address: u64, physical: u64, size: u64, core: Option<usize>, pc: Option<u64>, instructions: Option<u64>,
}
#[derive(Deserialize, Eq, PartialEq, Serialize)]
struct WatchClass { range: [u64; 2], kinds: Vec<Access> }
#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Views { Observe }
#[derive(Deserialize, Serialize)]
struct AccessBatch {
    enabled: bool, violation: Option<String>, records: Vec<AccessRecord>,
    classes: Vec<WatchClass>, ranges: Vec<[u64; 2]>, views: Views,
}
#[derive(Deserialize, Serialize)]
struct Boundary {
    snapshot: Snapshot,
    identities: [u32; 2],
    tcm: [[u32; 2]; 2],
    root: Address,
    context_word: u32,
    task_record_word: Address,
    code_word: u32,
}
#[derive(Deserialize, Serialize)]
struct Receipt {
    schema: u32,
    scope: String,
    mode: ObserverMode,
    identity: Identity,
    checkpoint: Digest,
    complete: bool,
    error: Option<String>,
    machine_failure: Option<String>,
    instructions: [u64; 2],
    menus: Vec<Option<[u32; 3]>>,
    frame: Option<Digest>,
    events: usize,
    pending: usize,
    watch_batches: usize,
    external_header_reads: usize,
    guest_code_writes: usize,
    watch_end_error: Option<String>,
    events_sha256: Digest,
    hits_sha256: Digest,
    boundaries_sha256: Digest,
    accesses_sha256: Digest,
    summary_sha256: Digest,
    limitations: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "kebab-case")]
enum AdmissionFailure { BoundaryRegisters, WatchClasses, WatchRanges, MissingFetch, FinalCounters, Other(String) }
impl std::fmt::Display for AdmissionFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(formatter, "{self:?}") }
}
impl From<String> for AdmissionFailure { fn from(error: String) -> Self { Self::Other(error) } }
impl From<&str> for AdmissionFailure { fn from(error: &str) -> Self { Self::Other(error.into()) } }
fn require(condition: bool, reason: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(reason.into()) }
}
fn register(machine: &Machine, core: usize, reg: arm::Register) -> Result<u32, String> {
    u32::try_from(machine.session.board.read_register(core, Register::Arm32(reg))?).map_err(|error| error.to_string())
}
fn tcm(machine: &Machine) -> Result<[[u32; 2]; 2], String> {
    let mut values = [[0; 2]; 2];
    for (core, banks) in values.iter_mut().enumerate() {
        for (bank, value) in banks.iter_mut().enumerate() {
            *value = u32::try_from(machine.session.board.cpu(core)?.coprocessor(arm::Coprocessor {
                cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank as u32, secure: 0, wide: false,
            }, None)?).map_err(|error| error.to_string())?;
        }
    }
    Ok(values)
}
fn word(machine: &Machine, maps: &[MemoryMap], banks: &[[u32; 2]; 2], core: usize, address: u32) -> Result<u32, String> {
    require(address & 3 == 0 && address < 0x60000000, "sample is not an aligned RAM word")?;
    let end = u64::from(address) + 4;
    for values in banks {
        for value in values {
            if let Some((low, high)) = tcm_window(TcmRegister(*value))? {
                require(end <= low || u64::from(address) >= high, "sample overlaps active TCM")?;
            }
        }
    }
    let span = static_span(maps, MemoryCore(core), MemoryAddress(u64::from(address))..MemoryAddress(end))?;
    let mut guest = [0; 4];
    let mut physical = [0; 4];
    machine.session.board.read_memory(core, u64::from(address), &mut guest)?;
    machine.session.board.read_physical(span.physical.0, &mut physical)?;
    require(guest == physical, "sample guest/physical bytes disagree")?;
    Ok(u32::from_le_bytes(guest))
}
fn expected(images: &[Image], address: u32) -> Result<u32, String> {
    let physical = address & !0x40000000;
    images.iter().find_map(|image| physical.checked_sub(image.base).and_then(|offset| image.bytes.get(offset as usize..offset as usize + 4)))
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .ok_or_else(|| "instruction point is outside canonical images".into())
}
fn code_word(machine: &Machine, maps: &[MemoryMap], banks: &[[u32; 2]; 2], images: &[Image], core: usize, address: u32) -> Result<u32, String> {
    let observed = word(machine, maps, banks, core, address)?;
    require(observed == expected(images, address)?, "current instruction differs from canonical bytes")?;
    Ok(observed)
}
fn chunk(machine: &Machine, maps: &[MemoryMap], banks: &[[u32; 2]; 2], core: usize, pointer: u32) -> Result<Option<u32>, String> {
    if pointer == 0 { return Ok(None); }
    let physical = pointer & !0x40000000;
    require((HEAP_START + 8..HEAP_END - 32).contains(&physical) && pointer & 7 == 0, "heap pointer escapes backing or alignment")?;
    let bytes = (word(machine, maps, banks, core, pointer - 4)? & !3).checked_sub(4).ok_or("chunk size underflow")?;
    require(bytes >= 12 && physical.checked_add(bytes).is_some_and(|end| end <= HEAP_END - 32), "chunk usable extent leaves heap")?;
    Ok(Some(bytes))
}
fn sample(machine: &Machine, maps: &[MemoryMap], images: &[Image], recorder: &Recorder, core: usize) -> Result<Boundary, String> {
    require(core < 2, "unexpected instruction-point core")?;
    let banks = tcm(machine)?;
    let mut identities = [0; 2];
    for (index, value) in identities.iter_mut().enumerate() {
        *value = machine.session.board.cpu(index)?.rprofile_identity(None)?;
        require(matches!(*value, 0x4003 | 0x5003), "unadmitted current CP14 identity")?;
    }
    let domain = usize::from(identities[core] != 0x4003);
    let root = word(machine, maps, &banks, core, 0x96bd20 + 4 * domain as u32)?;
    require(root == [0x4004dae0, 0x400ddaa0][domain], "selected interface root differs")?;
    require(word(machine, maps, &banks, core, root + 0x16c)? == [0x4118c, 0xd118c][domain], "task identity binding differs")?;
    require(word(machine, maps, &banks, core, root + 0x1618)? == [0x412bc, 0xd12bc][domain], "core selection binding differs")?;
    let context_word = word(machine, maps, &banks, core, [0x400545a4, 0x400e4564][domain])?;
    let task_record_word = word(machine, maps, &banks, core, [0x40054760, 0x400e4720][domain])?;
    let offset = task_record_word.checked_sub([0x4005ff0c, 0x400efecc][domain]).ok_or("current task precedes task table")?;
    require(offset % 56 == 0 && offset / 56 < 120, "current task record is outside canonical task table")?;
    let mut registers = [0; 16];
    for (index, value) in registers.iter_mut().enumerate() { *value = register(machine, core, arm::Register::R(index as u8))?; }
    let cpsr = register(machine, core, arm::Register::Cpsr)?;
    let context = ContextKey { domain: Core(domain), task: TaskId(offset / 56 + 1), task_record: Address(task_record_word), mode: (cpsr & 31) as u8, exception_depth: context_word };
    let pc = Address(registers[15]);
    let returning = recorder.return_operation(context, pc, Address(registers[13]));
    let operation = returning.or_else(|| heap_state::entry(pc));
    let context_pointer = if returning.is_some() {
        recorder.return_context(context, pc, Address(registers[13])).map(|address| address.0)
    } else { operation.map(|_| registers[0]) };
    let errno = if matches!(operation, Some(Operation::Malloc | Operation::Realloc | Operation::Calloc)) {
        Some(word(machine, maps, &banks, core, context_pointer.ok_or("allocation context absent")?)?)
    } else { None };
    let pointer = if returning.is_some() && matches!(operation, Some(Operation::Malloc | Operation::Realloc | Operation::Calloc)) {
        Some(registers[0])
    } else if returning.is_none() && matches!(operation, Some(Operation::Free | Operation::Realloc)) { Some(registers[1]) } else { None };
    let observed_chunk = pointer.map(|pointer| chunk(machine, maps, &banks, core, pointer)).transpose()?.flatten();
    let code_word = code_word(machine, maps, &banks, images, core, pc.0)?;
    Ok(Boundary { snapshot: Snapshot { pc, core: Core(core), context, registers, cpsr,
        counts: machine.session.board.instruction_counts().try_into().map_err(|_| "expected two instruction counters")?, errno, chunk: observed_chunk,
        heap_break: word(machine, maps, &banks, core, 0x829a08)?, owner: word(machine, maps, &banks, core, 0x40a6c7d8)?, depth: word(machine, maps, &banks, core, 0x40a6c7dc)? },
        identities, tcm: banks, root: Address(root), context_word, task_record_word: Address(task_record_word), code_word })
}
fn watch_classes(recorder: &Recorder) -> Vec<WatchClass> {
    let mut classes: Vec<_> = CODE.into_iter().map(|(start, end)| WatchClass { range: [u64::from(start), u64::from(end)], kinds: vec![Access::CpuWrite, Access::HostWrite, Access::ModelBusWrite] }).collect();
    classes.extend(ENTRIES.into_iter().map(|start| WatchClass { range: [u64::from(start), u64::from(start + 4)], kinds: vec![Access::CpuFetch] }));
    let mut headers: Vec<_> = recorder.live_headers().into_iter().map(|address| address.0 & !0x40000000).collect();
    headers.sort_unstable();
    headers.dedup();
    let group = headers.len().div_ceil(256 - CODE.len() - ENTRIES.len()).max(1);
    classes.extend(headers.chunks(group).map(|chunk| WatchClass {
        range: [u64::from(chunk[0]), u64::from(chunk[chunk.len() - 1]) + 4], kinds: vec![Access::CpuRead],
    }));
    classes
}
fn ranges(recorder: &Recorder) -> Vec<WatchRange> {
    watch_classes(recorder).into_iter().map(|class| WatchRange { start: class.range[0], end: class.range[1],
        kinds: match class.kinds.as_slice() { [Access::CpuRead] => WatchKinds::CPU_READ, [Access::CpuFetch] => WatchKinds::CPU_FETCH, _ => WatchKinds::WRITES } }).collect()
}
fn merged_ranges(classes: &[WatchClass]) -> Vec<[u64; 2]> {
    let mut sorted: Vec<_> = classes.iter().map(|class| class.range).collect();
    sorted.sort_unstable();
    let mut merged: Vec<[u64; 2]> = Vec::new();
    for range in sorted {
        if let Some(last) = merged.last_mut().filter(|last| range[0] <= last[1]) { last[1] = last[1].max(range[1]); }
        else { merged.push(range); }
    }
    merged
}
#[derive(Eq, PartialEq)]
struct FetchStamp { core: Core, pc: Address, clock: u64 }
fn admit_final(counts: [u64; 2], boundaries: &[Boundary]) -> Result<(), AdmissionFailure> {
    if counts[0].checked_add(counts[1]) != Some(ROUTE_INSTRUCTIONS)
        || boundaries.iter().any(|boundary| boundary.snapshot.counts.iter().zip(counts).any(|(before, after)| *before > after)) {
        return Err(AdmissionFailure::FinalCounters);
    }
    Ok(())
}
fn admit_interval(batch: &AccessBatch, classes: &[WatchClass], headers: &[Address], start: &Boundary, next: Option<&Boundary>, final_counts: [u64; 2]) -> Result<(usize, usize), AdmissionFailure> {
    if batch.classes != classes || batch.views != Views::Observe { return Err(AdmissionFailure::WatchClasses); }
    if batch.ranges != merged_ranges(classes) { return Err(AdmissionFailure::WatchRanges); }
    let begin = start.snapshot.counts[0].checked_add(start.snapshot.counts[1]).ok_or(AdmissionFailure::FinalCounters)?;
    let end_counts = next.map_or(final_counts, |boundary| boundary.snapshot.counts);
    let end = end_counts[0].checked_add(end_counts[1]).ok_or(AdmissionFailure::FinalCounters)?;
    if end < begin || start.snapshot.counts.iter().zip(end_counts).any(|(before, after)| *before > after) { return Err(AdmissionFailure::FinalCounters); }
    let mut expected = Vec::new();
    for boundary in std::iter::once(start).chain(next) {
        if ENTRIES.contains(&boundary.snapshot.pc.0) {
            expected.push(FetchStamp { core: boundary.snapshot.core, pc: boundary.snapshot.pc,
                clock: boundary.snapshot.counts[0].checked_add(boundary.snapshot.counts[1]).and_then(|clock| clock.checked_add(1)).ok_or(AdmissionFailure::FinalCounters)? });
        }
    }
    let mut fetches = Vec::new();
    let mut previous_clock = begin;
    let mut external_reads = 0;
    for row in &batch.records {
        require(row.size != 0 && row.physical.checked_add(row.size).is_some(), "invalid memory event extent")?;
        let high = row.physical + row.size;
        require(classes.iter().any(|class| class.range[0] < high && row.physical < class.range[1] && class.kinds.contains(&row.source)), "memory event is outside declared classes")?;
        if matches!(row.source, Access::CpuFetch | Access::CpuRead | Access::CpuWrite) {
            let clock = row.instructions.ok_or(AdmissionFailure::MissingFetch)?;
            let upper = if matches!(row.source, Access::CpuFetch) { end.checked_add(1).ok_or(AdmissionFailure::FinalCounters)? } else { end };
            if clock <= begin || clock > upper || clock < previous_clock { return Err(AdmissionFailure::FinalCounters); }
            previous_clock = clock;
        }
        if matches!(row.source, Access::CpuFetch) {
            require(row.pc == Some(row.address), "fetch PC differs from address")?;
            fetches.push(FetchStamp { core: Core(row.core.ok_or(AdmissionFailure::MissingFetch)?), pc: Address(u32::try_from(row.address).map_err(|error| error.to_string())?), clock: row.instructions.ok_or(AdmissionFailure::MissingFetch)? });
        }
        if matches!(row.source, Access::CpuRead)
            && headers.iter().any(|header| row.physical < u64::from(header.0 & !0x40000000) + 4 && u64::from(header.0 & !0x40000000) < high)
            && row.pc.is_some_and(|pc| !CODE.iter().any(|(low, high)| u64::from(*low) <= (pc & !0x40000000) && (pc & !0x40000000) < u64::from(*high))) { external_reads += 1; }
    }
    if fetches != expected { return Err(AdmissionFailure::MissingFetch); }
    Ok((external_reads, admit_batch(batch)?))
}
fn source_images(analysis: &std::path::Path) -> Result<(Vec<Image>, Vec<Digest>), String> {
    let sources = [
        ("decode/boot-40048.bin", 0, "c2da900dcd3f193b05b5ae968dfcb87723571462eafd84c0fed8467f66f7dff3"),
        ("decode/boot-50048.bin", 0x10000, "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4"),
        ("decode/loader-40000.bin", 0x40000, "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"),
        ("decode/loader-d0000.bin", 0xd0000, "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"),
        ("filmsim/id2-00130000.bin", 0x130000, "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5"),
        ("lz/id4-0061c400.bin", 0x61c400, "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"),
        ("lz/id5-00bf1a00.bin", 0xbf1a00, "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"),
        ("lz/plain-01100000.bin", 0x1100000, "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"),
    ];
    let mut images = Vec::new();
    let mut digests = Vec::new();
    for (path, base, digest) in sources {
        let bytes = read(&analysis.join(path))?;
        let actual = sha256(&bytes);
        require(actual == digest, "canonical source image differs")?;
        images.push(Image { base, bytes });
        digests.push(Digest(actual));
    }
    Ok((images, digests))
}
fn menu(machine: &Machine) -> Result<Option<[u32; 3]>, String> {
    machine.menu().map(|value| value.map(|value| [value.menu_id, value.setting_id, value.cursor]))
}
fn admit_batch(batch: &AccessBatch) -> Result<usize, String> {
    require(batch.enabled && batch.violation.is_none(), "memory watch overflowed or refused")?;
    let mut code_writes = 0;
    for row in &batch.records {
        if matches!(row.source, Access::CpuFetch | Access::CpuRead | Access::CpuWrite) {
            require(row.core.is_some_and(|core| core < 2) && row.pc.is_some(), "CPU memory event lacks attribution")?;
        }
        match row.source {
            Access::CpuFetch => require(row.address == row.physical && row.size == 4, "unsampled allocator executable alias")?,
            Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite => code_writes += 1,
            _ => {}
        }
    }
    Ok(code_writes)
}
fn bounded_read(path: &std::path::Path, limit: u64) -> Result<Vec<u8>, String> {
    require(std::fs::metadata(path).map_err(|error| error.to_string())?.len() <= limit, "observation artifact exceeds byte bound")?;
    read(path)
}
fn admit_receipt(directory: &std::path::Path, images: &[Image], expected_mode: ObserverMode) -> Result<Receipt, AdmissionFailure> {
    let receipt: Receipt = serde_json::from_slice(&bounded_read(&directory.join("heap-observation.json"), 65_536)?).map_err(|error| error.to_string())?;
    require(receipt.schema == 1 && receipt.mode == expected_mode && receipt.complete && receipt.error.is_none()
        && receipt.machine_failure.is_none() && receipt.watch_end_error.is_none() && receipt.guest_code_writes == 0, "observation receipt is not complete")?;
    require(receipt.menus == [Some([69, 52, 12]), Some([1, 5, 2])] && receipt.frame.is_some(), "observation endpoints differ")?;
    require(receipt.identity.observer.0 == sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?), "receipt producer differs from consumer")?;
    let event_bytes = bounded_read(&directory.join("events.json"), 64 * 1024 * 1024)?;
    let boundary_bytes = bounded_read(&directory.join("boundaries.json"), 64 * 1024 * 1024)?;
    let access_bytes = bounded_read(&directory.join("accesses.jsonl"), 256 * 1024 * 1024)?;
    let summary_bytes = bounded_read(&directory.join("summary.json"), 4 * 1024 * 1024)?;
    let hit_bytes = bounded_read(&directory.join("trace-hits.json"), 64 * 1024 * 1024)?;
    require(sha256(&event_bytes) == receipt.events_sha256.0 && sha256(&boundary_bytes) == receipt.boundaries_sha256.0
        && sha256(&access_bytes) == receipt.accesses_sha256.0 && sha256(&summary_bytes) == receipt.summary_sha256.0
        && sha256(&hit_bytes) == receipt.hits_sha256.0
        && sha256(&bounded_read(&directory.join("reset.fujimod"), 128 * 1024 * 1024)?) == receipt.checkpoint.0, "receipt artifact digest differs")?;
    let boundaries: Vec<Boundary> = serde_json::from_slice(&boundary_bytes).map_err(|error| error.to_string())?;
    require(boundaries.len() <= EVENT_LIMIT, "boundary count exceeds storage bound")?;
    let hits: Vec<RecordedHit> = serde_json::from_slice(&hit_bytes).map_err(|error| error.to_string())?;
    require(hits.len() == boundaries.len(), "instruction hits and sampled boundaries differ")?;
    for (hit, boundary) in hits.iter().zip(&boundaries) {
        let pc = hit.address.decode()?;
        require(hit.core == boundary.snapshot.core && pc == boundary.snapshot.pc.0
            && Some(hit.instructions.0) == boundary.snapshot.counts[0].checked_add(boundary.snapshot.counts[1]), "raw hit context differs from boundary")?;
        if hit.registers.decode()? != boundary.snapshot.registers[..13]
            || hit.sp.decode()? != boundary.snapshot.registers[13] || hit.lr.decode()? != boundary.snapshot.registers[14]
            || hit.cpsr != boundary.snapshot.cpsr { return Err(AdmissionFailure::BoundaryRegisters); }
    }
    admit_final(receipt.instructions, &boundaries)?;
    let mut batch_lines = access_bytes.split(|byte| *byte == b'\n').filter(|line| !line.is_empty());
    let mut batches = 0;
    let mut reads = 0;
    let mut writes = 0;
    let mut replay = Recorder::new();
    for (index, boundary) in boundaries.iter().enumerate() {
        let snapshot = &boundary.snapshot;
        require(snapshot.core.0 < 2 && matches!(boundary.identities[snapshot.core.0], 0x4003 | 0x5003), "boundary core identity differs")?;
        let domain = usize::from(boundary.identities[snapshot.core.0] != 0x4003);
        require(snapshot.context.domain.0 == domain && boundary.root.0 == [0x4004dae0, 0x400ddaa0][domain]
            && boundary.context_word == snapshot.context.exception_depth && boundary.task_record_word == snapshot.context.task_record, "boundary context binding differs")?;
        let offset = boundary.task_record_word.0.checked_sub([0x4005ff0c, 0x400efecc][domain]).ok_or("task record precedes table")?;
        require(offset % 56 == 0 && offset / 56 < 120 && snapshot.context.task.0 == offset / 56 + 1, "boundary task index differs")?;
        require(boundary.code_word == expected(images, snapshot.pc.0)? && snapshot.registers[15] == snapshot.pc.0, "boundary instruction binding differs")?;
        replay.observe(snapshot.clone()).map_err(|error| error.to_string())?;
        let batch: AccessBatch = serde_json::from_slice(batch_lines.next().ok_or(AdmissionFailure::MissingFetch)?).map_err(|error| error.to_string())?;
        let (batch_reads, batch_writes) = admit_interval(&batch, &watch_classes(&replay), &replay.live_headers(), boundary, boundaries.get(index + 1), receipt.instructions)?;
        reads += batch_reads;
        writes += batch_writes;
        batches += 1;
    }
    require(serde_json::to_vec(replay.records()).map_err(|error| error.to_string())? == event_bytes
        && replay.records().len() == receipt.events && replay.pending_count() == receipt.pending, "event replay differs")?;
    require(serde_json::to_vec(&replay.summary()).map_err(|error| error.to_string())? == summary_bytes, "lifetime summary replay differs")?;
    if batch_lines.next().is_some() { return Err(AdmissionFailure::MissingFetch); }
    require((batches, reads, writes) == (receipt.watch_batches, receipt.external_header_reads, receipt.guest_code_writes), "memory-watch projection differs")?;
    match expected_mode {
        ObserverMode::Enabled => {
            require(receipt.events > 0 && receipt.pending == 0 && batches == boundaries.len(), "enabled observation has missing intervals")?;
            replay.finish().map_err(|error| error.to_string())?;
        }
        ObserverMode::Disabled => require(boundaries.is_empty() && receipt.events == 0 && batches == 0, "disabled observation contains instrumentation")?,
    }
    Ok(receipt)
}
fn compare(arguments: &[std::ffi::OsString]) -> Result<(), String> {
    require(arguments.len() == 5, "heap-observe --compare ENABLED_DIR DISABLED_DIR ANALYSIS_ROOT OUTPUT_JSON")?;
    let (images, digests) = source_images(std::path::Path::new(&arguments[3]))?;
    let enabled = admit_receipt(std::path::Path::new(&arguments[1]), &images, ObserverMode::Enabled).map_err(|error| error.to_string())?;
    let disabled = admit_receipt(std::path::Path::new(&arguments[2]), &images, ObserverMode::Disabled).map_err(|error| error.to_string())?;
    require(enabled.identity == disabled.identity && enabled.identity.images == digests && enabled.checkpoint == disabled.checkpoint
        && enabled.identity.run.0 == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
        && enabled.identity.update.0 == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
        && enabled.identity.boot_rom.0 == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be", "comparison inputs differ")?;
    require(enabled.menus == disabled.menus && enabled.frame == disabled.frame, "observer changes user-visible outcome")?;
    #[derive(Serialize)]
    struct Comparison { schema: u32, admitted: bool, enabled: Receipt, disabled: Receipt }
    let bytes = serde_json::to_vec_pretty(&Comparison { schema: 1, admitted: true, enabled, disabled }).map_err(|error| error.to_string())?;
    OpenOptions::new().create_new(true).write(true).open(std::path::Path::new(&arguments[4]))
        .and_then(|mut file| file.write_all(&bytes)).map_err(|error| error.to_string())
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ReceiptFault { BoundarySize, RemovedClass, OmittedFetch, FinalCounter }
impl ReceiptFault {
    fn directory(self) -> &'static str {
        match self { Self::BoundarySize => "boundary-size", Self::RemovedClass => "removed-class", Self::OmittedFetch => "omitted-fetch", Self::FinalCounter => "final-counter" }
    }
    fn targets(self, error: &AdmissionFailure) -> bool {
        matches!((self, error), (Self::BoundarySize, AdmissionFailure::BoundaryRegisters)
            | (Self::RemovedClass, AdmissionFailure::WatchClasses) | (Self::OmittedFetch, AdmissionFailure::MissingFetch)
            | (Self::FinalCounter, AdmissionFailure::FinalCounters))
    }
}
#[derive(Serialize)]
struct Refusal { fault: ReceiptFault, targeted: bool, observed: Option<AdmissionFailure> }
fn refusal_controls(arguments: &[std::ffi::OsString]) -> Result<(), String> {
    require(arguments.len() == 5, "heap-observe --refusal-controls ENABLED_DIR DISABLED_DIR ANALYSIS_ROOT NEW_OUTPUT_DIR")?;
    let source = std::path::Path::new(&arguments[1]);
    let output = std::path::Path::new(&arguments[4]);
    std::fs::create_dir(output).map_err(|error| error.to_string())?;
    let comparison = vec!["--compare".into(), arguments[1].clone(), arguments[2].clone(), arguments[3].clone(), output.join("baseline-comparison.json").into_os_string()];
    compare(&comparison)?;
    let (images, _) = source_images(std::path::Path::new(&arguments[3]))?;
    let mut results = Vec::new();
    for fault in [ReceiptFault::BoundarySize, ReceiptFault::RemovedClass, ReceiptFault::OmittedFetch, ReceiptFault::FinalCounter] {
        let directory = output.join(fault.directory());
        std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
        let mut receipt: Receipt = serde_json::from_slice(&bounded_read(&source.join("heap-observation.json"), 65_536)?).map_err(|error| error.to_string())?;
        let changed: &[&str] = match fault {
            ReceiptFault::BoundarySize => &["boundaries.json", "events.json", "summary.json"],
            ReceiptFault::RemovedClass | ReceiptFault::OmittedFetch => &["accesses.jsonl"],
            ReceiptFault::FinalCounter => &[],
        };
        for name in ["boundaries.json", "events.json", "summary.json", "accesses.jsonl", "trace-hits.json", "reset.fujimod"] {
            if !changed.contains(&name) { std::fs::hard_link(source.join(name), directory.join(name)).map_err(|error| error.to_string())?; }
        }
        match fault {
            ReceiptFault::BoundarySize => {
                let events: Vec<heap_state::Event> = serde_json::from_slice(&bounded_read(&source.join("events.json"), 64 * 1024 * 1024)?).map_err(|error| error.to_string())?;
                let origin = events.iter().find(|event| matches!(&event.boundary, heap_state::Boundary::Enter { operation: Operation::Malloc, parent: None, .. })).ok_or("control needs a logical malloc entry")?;
                let mut boundaries: Vec<Boundary> = serde_json::from_slice(&bounded_read(&source.join("boundaries.json"), 64 * 1024 * 1024)?).map_err(|error| error.to_string())?;
                let boundary = boundaries.iter_mut().find(|boundary| boundary.snapshot == origin.snapshot).ok_or("control malloc boundary absent")?;
                boundary.snapshot.registers[1] = if boundary.snapshot.registers[1] == 0 { 1 } else { boundary.snapshot.registers[1] - 1 };
                let mut replay = Recorder::new();
                for boundary in &boundaries { replay.observe(boundary.snapshot.clone()).map_err(|error| error.to_string())?; }
                replay.finish().map_err(|error| error.to_string())?;
                let boundary_bytes = serde_json::to_vec(&boundaries).map_err(|error| error.to_string())?;
                let event_bytes = serde_json::to_vec(replay.records()).map_err(|error| error.to_string())?;
                let summary_bytes = serde_json::to_vec(&replay.summary()).map_err(|error| error.to_string())?;
                write(&directory.join("boundaries.json"), &boundary_bytes)?;
                write(&directory.join("events.json"), &event_bytes)?;
                write(&directory.join("summary.json"), &summary_bytes)?;
                receipt.boundaries_sha256 = Digest(sha256(&boundary_bytes));
                receipt.events_sha256 = Digest(sha256(&event_bytes));
                receipt.summary_sha256 = Digest(sha256(&summary_bytes));
            }
            ReceiptFault::RemovedClass | ReceiptFault::OmittedFetch => {
                let access_bytes = bounded_read(&source.join("accesses.jsonl"), 256 * 1024 * 1024)?;
                let mut output_bytes = Vec::with_capacity(access_bytes.len());
                let mut changed = false;
                for line in access_bytes.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
                    let mut batch: AccessBatch = serde_json::from_slice(line).map_err(|error| error.to_string())?;
                    if !changed {
                        match fault {
                            ReceiptFault::RemovedClass if !batch.classes.is_empty() => { batch.classes.remove(0); changed = true; }
                            ReceiptFault::OmittedFetch => {
                                if let Some(index) = batch.records.iter().position(|row| matches!(row.source, Access::CpuFetch)) { batch.records.remove(index); changed = true; }
                            }
                            _ => {}
                        }
                    }
                    serde_json::to_writer(&mut output_bytes, &batch).map_err(|error| error.to_string())?;
                    output_bytes.push(b'\n');
                }
                require(changed, "control has no suitable watch row or class")?;
                write(&directory.join("accesses.jsonl"), &output_bytes)?;
                receipt.accesses_sha256 = Digest(sha256(&output_bytes));
            }
            ReceiptFault::FinalCounter => {
                receipt.instructions[0] = receipt.instructions[0].checked_sub(1).ok_or("control needs a primary instruction")?;
            }
        }
        write(&directory.join("heap-observation.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
        let observed = admit_receipt(&directory, &images, ObserverMode::Enabled).err();
        let targeted = observed.as_ref().is_some_and(|error| fault.targets(error));
        results.push(Refusal { fault, targeted, observed });
    }
    let all_targeted = results.iter().all(|result| result.targeted);
    write(&output.join("refusal-controls.json"), &serde_json::to_vec_pretty(&results).map_err(|error| error.to_string())?)?;
    require(all_targeted, "receipt control was accepted or refused at a different boundary")
}
fn drive() -> Result<(), String> {
    let compare_args: Vec<_> = std::env::args_os().skip(1).collect();
    if compare_args.first().is_some_and(|argument| argument == "--compare") { return compare(&compare_args); }
    if compare_args.first().is_some_and(|argument| argument == "--refusal-controls") { return refusal_controls(&compare_args); }
    let mut arguments = BTreeMap::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag = flag.into_string().map_err(|_| USAGE)?;
        require(["--run", "--update", "--boot-rom", "--analysis", "--observer", "--output"].contains(&flag.as_str()), USAGE)?;
        require(arguments.insert(flag, PathBuf::from(args.next().ok_or(USAGE)?)).is_none(), USAGE)?;
    }
    let mut take = |flag| arguments.remove(flag).ok_or_else(|| USAGE.to_owned());
    let run_bytes = read(&take("--run")?)?;
    let update = read(&take("--update")?)?;
    let rom = read(&take("--boot-rom")?)?;
    let analysis = take("--analysis")?;
    let mode = match take("--observer")?.to_str() { Some("enabled") => ObserverMode::Enabled, Some("disabled") => ObserverMode::Disabled, _ => return Err(USAGE.into()) };
    let output = take("--output")?;
    let (images, image_digests) = source_images(&analysis)?;
    let identity = Identity { run: Digest(sha256(&run_bytes)), update: Digest(sha256(&update)), boot_rom: Digest(sha256(&rom)),
        observer: Digest(sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?)), images: image_digests };
    require(identity.run.0 == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
        && identity.update.0 == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
        && identity.boot_rom.0 == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be", "observer requires canonical no-card inputs")?;
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    let access_path = output.join("accesses.jsonl");
    let mut accesses = WatchOutput::new(&access_path)?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(update), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    let checkpoint = machine.session.checkpoint(None)?.export()?;
    write(&output.join("reset.fujimod"), &checkpoint)?;
    let restored = machine.session.import_snapshot(&checkpoint)?;
    machine.session.restore(&restored)?;
    machine.rearm()?;
    require(machine.instructions() == 0, "checkpoint is not preexecution reset")?;
    let mut recorder = Recorder::new();
    let mut boundaries = Vec::new();
    let mut armed: BTreeSet<_> = ENTRIES.into_iter().collect();
    if mode == ObserverMode::Enabled {
        machine.traces.bound_full_registers(std::num::NonZeroUsize::new(EVENT_LIMIT).ok_or("invalid hit bound")?);
        machine.traces.arm(&machine.session, armed.iter().map(|address| (*address, "heap".into())).collect())?;
        machine.traces.stop_labels = ["heap".into()].into_iter().collect();
    }
    let mut watch: Option<Box<fujimod_emulator::board::observation::MemoryWatch>> = None;
    let mut watch_batches = 0;
    let mut external_header_reads = 0;
    let mut guest_code_writes = 0;
    let mut watch_end_error = None;
    let mut menus = Vec::new();
    let result = (|| -> Result<(), String> {
        let mut processed = 0;
        let mut advance = |machine: &mut Machine, budget: u64| -> Result<(), String> {
            let target = machine.instructions().checked_add(budget).ok_or("route instruction bound overflow")?;
            while machine.instructions() < target {
                let outcome = machine.run(target - machine.instructions());
                machine.require_running()?;
                if outcome != RunOutcome::Stopped { break; }
                let hits = &machine.traces.hits[processed..];
                require(hits.len() == 1, "instruction boundary contains multiple unsampled hits")?;
                let boundary = sample(machine, &maps, &images, &recorder, hits[0].core)?;
                require(format!("{:#x}", boundary.snapshot.pc.0) == hits[0].address, "stopped PC differs from recorded hit")?;
                if watch.is_none() {
                    for (start, end) in CODE {
                        for address in (start..end).step_by(4) {
                            for core in 0..2 { code_word(machine, &maps, &boundary.tcm, &images, core, address)?; }
                        }
                    }
                }
                if let Some(mut active) = watch.take() {
                    let raw = active.records.borrow().state();
                    let batch: AccessBatch = serde_json::from_value(raw.clone()).map_err(|error| error.to_string())?;
                    let bytes = serde_json::to_vec(&raw).map_err(|error| error.to_string())?;
                    machine.session.board.memory_watch_end(&mut active)?;
                    accesses.append(&bytes)?;
                    watch_batches += 1;
                    let start = boundaries.last().ok_or("watch has no initial boundary")?;
                    let (reads, writes) = admit_interval(&batch, &watch_classes(&recorder), &recorder.live_headers(), start, Some(&boundary), boundary.snapshot.counts).map_err(|error| error.to_string())?;
                    external_header_reads += reads;
                    guest_code_writes += writes;
                    require(writes == 0, "allocator code changed during observation")?;
                }
                boundaries.push(boundary);
                recorder.observe(boundaries.last().ok_or("boundary disappeared")?.snapshot.clone()).map_err(|error| error.to_string())?;
                processed = machine.traces.hits.len();
                let additional: BTreeMap<_, _> = recorder.returns().into_iter().filter_map(|address| armed.insert(address.0).then_some((address.0, "heap".into()))).collect();
                if !additional.is_empty() { machine.traces.add_sites(&machine.session, additional)?; }
                watch = Some(machine.session.board.memory_watch_begin_classified(ranges(&recorder), WATCH_LIMIT, maps.clone(), ViewPolicy::Observe)?);
            }
            require(machine.instructions() == target, "route instruction endpoint differs")
        };
        advance(&mut machine, 80_000_000)?;
        menus.push(menu(&machine)?);
        require(menus.last() == Some(&Some([69, 52, 12])), "initial menu endpoint differs")?;
        for key in std::iter::repeat_n(RouteKey::Up, 12).chain([RouteKey::Ok, RouteKey::Ok]) {
            machine.press(key)?;
            advance(&mut machine, 8_000_000)?;
            machine.release_key(key)?;
            advance(&mut machine, 8_000_000)?;
        }
        advance(&mut machine, 40_000_000)?;
        machine.press(RouteKey::Ok)?;
        advance(&mut machine, 8_000_000)?;
        machine.release_key(RouteKey::Ok)?;
        advance(&mut machine, 8_000_000)?;
        advance(&mut machine, 90_000_000)?;
        menus.push(menu(&machine)?);
        require(menus.last() == Some(&Some([1, 5, 2])), "final menu endpoint differs")?;
        if mode == ObserverMode::Enabled {
            require(!recorder.records().is_empty(), "no heap operation observed")?;
            recorder.finish().map_err(|error| error.to_string())?;
        }
        Ok(())
    })();
    if let Some(mut active) = watch.take() {
        let raw = active.records.borrow().state();
        watch_end_error = machine.session.board.memory_watch_end(&mut active).err();
        let final_batch = (|| -> Result<(usize, usize), String> {
            accesses.append(&serde_json::to_vec(&raw).map_err(|error| error.to_string())?)?;
            watch_batches += 1;
            let batch = serde_json::from_value(raw).map_err(|error| error.to_string())?;
            let counts = machine.session.board.instruction_counts().try_into().map_err(|_| "expected two instruction counters")?;
            admit_interval(&batch, &watch_classes(&recorder), &recorder.live_headers(), boundaries.last().ok_or("final watch has no initial boundary")?, None, counts).map_err(|error| error.to_string())
        })();
        match final_batch {
            Ok((reads, writes)) => { external_header_reads += reads; guest_code_writes += writes; }
            Err(error) => watch_end_error = Some(error),
        }
    }
    accesses.flush()?;
    let event_bytes = serde_json::to_vec(recorder.records()).map_err(|error| error.to_string())?;
    let boundary_bytes = serde_json::to_vec(&boundaries).map_err(|error| error.to_string())?;
    let summary_bytes = serde_json::to_vec(&recorder.summary()).map_err(|error| error.to_string())?;
    let hit_bytes = serde_json::to_vec(&machine.traces.hits).map_err(|error| error.to_string())?;
    write(&output.join("trace-hits.json"), &hit_bytes)?;
    write(&output.join("events.json"), &event_bytes)?;
    write(&output.join("boundaries.json"), &boundary_bytes)?;
    write(&output.join("summary.json"), &summary_bytes)?;
    let frame = machine.frame().map(|value| value.map(|bytes| Digest(sha256(&bytes))));
    let counts: [u64; 2] = machine.session.board.instruction_counts().try_into().map_err(|_| "expected two instruction counters")?;
    let final_counters = admit_final(counts, &boundaries);
    let complete = result.is_ok() && frame.is_ok() && final_counters.is_ok() && watch_end_error.is_none() && guest_code_writes == 0;
    let receipt = Receipt { schema: 1, scope: "canonical-natural-reset-no-card-menu-roundtrip-general-heap".into(), mode, identity,
        checkpoint: Digest(sha256(&checkpoint)), complete, error: result.err().or_else(|| frame.as_ref().err().cloned()).or_else(|| final_counters.err().map(|error| error.to_string())), machine_failure: machine.failure.clone(),
        instructions: counts, menus,
        frame: frame.ok().flatten(), events: recorder.records().len(), pending: recorder.pending_count(), watch_batches,
        external_header_reads, guest_code_writes, watch_end_error, events_sha256: Digest(sha256(&event_bytes)), boundaries_sha256: Digest(sha256(&boundary_bytes)),
        hits_sha256: Digest(sha256(&hit_bytes)),
        accesses_sha256: Digest(sha256(&read(&access_path)?)), summary_sha256: Digest(sha256(&summary_bytes)),
        limitations: vec!["Finite no-card boot/menu route, not capture, development or storage workload coverage.".into(),
            "Instruction counters are not silicon cycles; event intervals include preemption and lock waits.".into(),
            "Header watches cover returned live allocations, not unreturned internal split chunks or pre-return pointer escapes.".into(),
            "Pending incomplete invocations or any observer refusal prohibit full-trace and peak-live claims.".into(),
            "Task identity is derived from current scheduler records and kernel context; CPSR mode and context depth retain exception attribution.".into()] };
    write(&output.join("heap-observation.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    require(complete, "heap observation incomplete; preserved receipt records refusal")
}
fn main() -> ExitCode {
    match drive() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } }
}
