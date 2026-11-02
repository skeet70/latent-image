use super::*;
use super::super::continuation_view::{DramChoice, RawWords};
use super::super::loader_continuations_check as shared;
use std::{fs::File, io::Read, path::Component};

#[path = "application_pump_compare.rs"]
mod compare;
#[path = "application_pump_queue.rs"]
mod queue;
#[path = "application_pump_controls.rs"]
mod controls;
#[path = "application_pump_program.rs"]
mod program;

const METADATA_BYTES: u64 = 256 * 1024 * 1024;
const INPUT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Serialize)]
pub(super) struct Verification {
    schema: u32,
    mode: Mode,
    receipt: Digest,
    samples: usize,
    complete_pumps: usize,
    receive_backed_pumps: usize,
    pending_work_pumps: usize,
    queue_successes: usize,
    queue_failures: usize,
    visible_frames: usize,
    unqualified: Vec<Route>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Hook { Prehook, Normalizer, Posthook }
#[derive(Deserialize)]
struct DeclaredCall { phase: Hook, pc: Guest, target: Guest }
#[derive(Deserialize)]
struct DeclaredProof { call_sites: Vec<DeclaredCall>, return_pc: Guest, scratch_low: i32 }
#[derive(Deserialize)]
struct DeclaredEnvelope { application_pump: DeclaredProof }
struct Profile { calls: Vec<(Guest, Guest)>, returned: Guest, scratch_low: i32 }

fn refusal(reason: Refusal, detail: impl Into<String>) -> Failure { Failure::new(reason, detail) }
fn require(value: bool, reason: Refusal, detail: &str) -> Result<(), Failure> {
    if value { Ok(()) } else { Err(refusal(reason, detail)) }
}
fn bounded(path: &Path, limit: u64) -> Result<Vec<u8>, Failure> {
    let file = File::open(path).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?;
    let size = file.metadata().map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?.len();
    require(size <= limit, Refusal::CaptureBound, "file exceeds its decoded bound")?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?;
    require(bytes.len() as u64 == size, Refusal::CaptureBound, "file extent changed while reading")?;
    Ok(bytes)
}
fn local(root: &Path, binding: &FileBinding) -> Result<PathBuf, Failure> {
    let mut parts = binding.path.components();
    require(matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none(), Refusal::ArtifactIdentity, "receipt artifact is not one local filename")?;
    Ok(root.join(&binding.path))
}
fn checked_file(path: &Path, digest: &Digest, limit: u64) -> Result<Vec<u8>, Failure> {
    let bytes = bounded(path, limit)?;
    require(Digest::of(&bytes).as_str() == digest.as_str(), Refusal::ArtifactIdentity, "artifact digest differs")?;
    Ok(bytes)
}
fn artifact(root: &Path, binding: &FileBinding) -> Result<Vec<u8>, Failure> {
    checked_file(&local(root, binding)?, &binding.sha256, METADATA_BYTES)
}
fn input(binding: &FileBinding) -> Result<Vec<u8>, Failure> {
    checked_file(&binding.path, &binding.sha256, INPUT_BYTES)
}
fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8], reason: Refusal) -> Result<T, Failure> {
    serde_json::from_slice(bytes).map_err(|error| refusal(reason, error.to_string()))
}
fn physical(address: Guest) -> u32 { address.0 & !0x40000000 }
fn branch(pc: Guest, word: u32) -> Option<Guest> {
    if word >> 24 != 0xeb { return None; }
    let displacement = ((word << 8) as i32 >> 6) as i64;
    u32::try_from(i64::from(pc.0) + 8 + displacement).ok().map(Guest)
}
fn overlay(catalog: &Catalog, mode: Mode, carrier: &[u8]) -> Result<Catalog, Failure> {
    let mut images = Vec::new();
    for image in &catalog.0 {
        let mut bytes = image.bytes.clone();
        let mut binding = image.binding.clone();
        if mode == Mode::Owned && binding.kind == SourceKind::Image5 {
            let low = (CARRIER.0 - binding.start.0) as usize;
            bytes[low..low + CARRIER_BYTES].copy_from_slice(carrier);
            binding.sha256 = Digest::of(&bytes);
        }
        images.push(SourceImage { binding, bytes });
    }
    Ok(Catalog(images))
}
fn profile(catalog: &Catalog, declaration: Option<DeclaredProof>) -> Result<Profile, Failure> {
    let derived = program::derive(catalog)?;
    if let Some(proof) = declaration {
        require(proof.call_sites.len() == derived.calls.len(), Refusal::HookSequence, "artifact proof call count differs")?;
        for (index, (declared, actual)) in proof.call_sites.iter().zip(&derived.calls).enumerate() {
            require(declared.phase == [Hook::Prehook, Hook::Normalizer, Hook::Posthook][index] && (declared.pc, declared.target) == *actual, Refusal::HookSequence, "artifact proof does not match decoded calls")?;
        }
        require(proof.return_pc == derived.returned && proof.scratch_low == derived.scratch_low, Refusal::StackContext, "artifact proof differs from independently derived frame/return")?;
    }
    Ok(derived)
}
fn packaged_delta(original: &[u8], actual: &[u8], mode: Mode) -> Result<(), Failure> {
    if mode == Mode::Retained { return require(original == actual, Refusal::ArtifactIdentity, "retained DAT changed"); }
    let staging = 0x25cusize;
    let checksum = 0x20cusize;
    let window = staging + 0x2c0000..staging + 0x5a0000;
    require(original.len() == actual.len() && actual.len() >= window.end, Refusal::ArtifactIdentity, "candidate DAT extent differs")?;
    require(original[..checksum] == actual[..checksum] && original[checksum + 4..window.start] == actual[checksum + 4..window.start] && original[window.end..] == actual[window.end..], Refusal::ArtifactIdentity, "candidate DAT changes another header or stored image")?;
    let word = |offset| u32::from_le_bytes([actual[offset], actual[offset + 1], actual[offset + 2], actual[offset + 3]]);
    let start = staging.checked_add(word(0x23c) as usize).ok_or_else(|| refusal(Refusal::ArtifactIdentity, "main body start overflows"))?;
    let end = start.checked_add(word(0x240) as usize).ok_or_else(|| refusal(Refusal::ArtifactIdentity, "main body end overflows"))?;
    let body = actual.get(start..end).ok_or_else(|| refusal(Refusal::ArtifactIdentity, "main body exceeds candidate"))?;
    let derived = !body.iter().fold(0u32, |sum, byte| sum.wrapping_add(u32::from(!*byte)));
    require(word(checksum) == derived && actual != original, Refusal::ArtifactIdentity, "candidate checksum or changed image differs")
}
pub(super) fn admit_inputs(original: &[u8], actual: &[u8], mode: Mode, catalog: &Catalog) -> Result<(), String> {
    packaged_delta(original, actual, mode).and_then(|()| program::derive(catalog).map(|_| ())).map_err(|failure| format!("{:?}: {}", failure.reason, failure.detail))
}
fn identity(root: &Path, report: &PumpReport, canonical: &Catalog) -> Result<(Catalog, Profile, RunDocument), Failure> {
    require(report.schema == 1 && matches!(report.outcome, Outcome::Captured) && report.loader.error.is_none() && report.loader.machine_failure.is_none(), Refusal::Guest, "capture did not complete")?;
    let id = &report.identity;
    let observer = bounded(&std::env::current_exe().map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?, INPUT_BYTES)?;
    require(Digest::of(&observer).as_str() == id.observer.as_str(), Refusal::ArtifactIdentity, "receipt producer differs from reviewed consumer binary")?;
    for (binding, wanted) in [(&id.original_update, ORIGINAL_DAT), (&id.run, RUN_SHA), (&id.boot_rom, ROM_SHA)] {
        require(binding.sha256.as_str() == wanted, Refusal::ArtifactIdentity, "canonical input identity differs")?;
    }
    let original = input(&id.original_update)?;
    let run_bytes = input(&id.run)?;
    input(&id.boot_rom)?;
    let actual = if id.actual_update.path == id.original_update.path {
        require(id.actual_update.sha256.as_str() == id.original_update.sha256.as_str(), Refusal::ArtifactIdentity, "one input file has contradictory identities")?;
        std::borrow::Cow::Borrowed(original.as_slice())
    } else { std::borrow::Cow::Owned(input(&id.actual_update)?) };
    packaged_delta(&original, &actual, report.mode)?;
    let code = input(&id.code)?;
    let symbols = input(&id.symbols)?;
    let admission_bytes = input(&id.admission)?;
    let admission: PumpAdmission = parse(&admission_bytes, Refusal::ArtifactIdentity)?;
    let declared: DeclaredEnvelope = parse(&admission_bytes, Refusal::ArtifactIdentity)?;
    let carrier = artifact(root, &id.carrier)?;
    require(artifact(root, &FileBinding { path: "pump-code.bin".into(), sha256: id.code.sha256.clone() })? == code && artifact(root, &FileBinding { path: "pump-symbols.txt".into(), sha256: id.symbols.sha256.clone() })? == symbols, Refusal::ArtifactIdentity, "local linked inputs differ")?;
    require(artifact(root, &FileBinding { path: "pump-admission.json".into(), sha256: id.admission.sha256.clone() })? == admission_bytes, Refusal::ArtifactIdentity, "local packaging proof differs")?;
    require(!code.is_empty() && code.len() <= CARRIER_BYTES && code.len() % 4 == 0 && carrier.len() == CARRIER_BYTES && carrier[..code.len()] == code, Refusal::ArtifactIdentity, "linked image does not fit the carrier")?;
    require(carrier[code.len()..].chunks_exact(4).all(|bytes| bytes == 0xe320f000u32.to_le_bytes()), Refusal::ArtifactIdentity, "carrier padding is not admitted NOP")?;
    let mechanism = &admission.application_pump;
    require(admission.schema == 1 && admission.qualification == Qualification::ConditionalSourceOnly && mechanism.source_dat_sha256.as_str() == ORIGINAL_DAT && mechanism.source_image5_sha256.as_str() == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a" && mechanism.linked_sha256.as_str() == id.code.sha256.as_str() && mechanism.symbols_sha256.as_str() == id.symbols.sha256.as_str() && mechanism.carrier_sha256.as_str() == id.carrier.sha256.as_str() && mechanism.emitted_bytes.0 as usize == code.len(), Refusal::ArtifactIdentity, "typed packaging identities differ")?;
    require(mechanism.carrier == [CARRIER, Guest(CARRIER.0 + 32)] && mechanism.entry == CARRIER && mechanism.caller == Guest(0xc23274) && mechanism.continuation == Guest(0xc23278) && mechanism.backedge == Guest(0xc22f84) && mechanism.retained_calls == [Guest(0xbfb1b8), Guest(0xbfb20c), Guest(0xbfb1e0)] && mechanism.image5_base == Guest(0xbf1a00), Refusal::ArtifactIdentity, "typed packaging boundaries differ")?;
    require(report.loader.identity.run.as_str() == RUN_SHA && report.loader.identity.rom.as_str() == ROM_SHA && report.loader.identity.update.as_str() == id.actual_update.sha256.as_str() && report.loader.identity.executable.as_str() == id.observer.as_str() && report.loader.checkpoint.as_str() == report.checkpoint.as_str() && report.loader.instructions == report.instructions, Refusal::ArtifactIdentity, "loader and pump identities differ")?;
    require(id.canonical_sources.len() == canonical.0.len(), Refusal::ArtifactIdentity, "canonical source catalog count differs")?;
    for image in &canonical.0 {
        require(id.canonical_sources.iter().filter(|bound| bound.kind == image.binding.kind && bound.start == image.binding.start && bound.bytes.0 == image.binding.bytes.0 && bound.sha256.as_str() == image.binding.sha256.as_str()).count() == 1, Refusal::ArtifactIdentity, "canonical source binding differs")?;
    }
    match report.mode {
        Mode::Retained => require(id.actual_update.sha256.as_str() == ORIGINAL_DAT, Refusal::ArtifactIdentity, "retained mode has a noncanonical DAT")?,
        Mode::Owned => require(id.actual_update.sha256.as_str() != ORIGINAL_DAT && admission.candidate_dat_sha256.as_str() == id.actual_update.sha256.as_str(), Refusal::ArtifactIdentity, "owned DAT differs from typed staged candidate")?,
    }
    let candidate_catalog = overlay(canonical, Mode::Owned, &carrier)?;
    let image5 = candidate_catalog.0.iter().find(|image| image.binding.kind == SourceKind::Image5).ok_or_else(|| refusal(Refusal::ArtifactIdentity, "candidate image5 absent"))?;
    require(image5.binding.sha256.as_str() == admission.candidate_image5_sha256.as_str() && mechanism.staged_image5_sha256.as_str() == admission.candidate_image5_sha256.as_str(), Refusal::ArtifactIdentity, "candidate image5 is not exact carrier overlay")?;
    let catalog = match report.mode { Mode::Owned => candidate_catalog, Mode::Retained => overlay(canonical, Mode::Retained, &carrier)? };
    require(report.loader.identity.sources.len() == catalog.0.len(), Refusal::ArtifactIdentity, "runtime source catalog count differs")?;
    for image in &catalog.0 {
        require(report.loader.identity.sources.iter().filter(|bound| bound.kind == image.binding.kind && bound.start == image.binding.start && bound.bytes.0 == image.binding.bytes.0 && bound.sha256.as_str() == image.binding.sha256.as_str()).count() == 1, Refusal::ArtifactIdentity, "runtime source catalog differs from exact overlay")?;
    }
    let proof = profile(&catalog, if report.mode == Mode::Owned { Some(declared.application_pump) } else { None })?;
    require(checked_file(&root.join("reset.fujimod"), &report.checkpoint, INPUT_BYTES)?.len() > 0, Refusal::ArtifactIdentity, "reset checkpoint is empty")?;
    let document = RunDocument::parse(&run_bytes).map_err(|error| refusal(Refusal::ArtifactIdentity, error))?;
    Ok((catalog, proof, document))
}
fn projected(views: &[Words], address: u32) -> Result<u32, Failure> {
    let mut observed = None;
    for view in views {
        if let Some(offset) = address.checked_sub(view.mapping.guest.0) {
            if offset % 4 == 0 {
                if let Some(word) = view.values.get(offset as usize / 4) {
                    require(observed.is_none_or(|old| old == *word), Refusal::Mapping, "overlapping raw projections disagree")?;
                    observed = Some(*word);
                }
            }
        }
    }
    observed.ok_or_else(|| refusal(Refusal::Mapping, format!("required committed scalar {address:#x} absent")))
}
fn gzip_record<T: serde::de::DeserializeOwned>(root: &Path, binding: &FileBinding, limits: capture::bounded_gzip::Limits, reason: Refusal) -> Result<T, Failure> {
    use capture::bounded_gzip;
    let path = local(root, binding)?;
    let digest = bounded_gzip::digest_file(&path, limits.compressed()).map_err(|error| refusal(Refusal::CaptureBound, error.to_string()))?;
    require(digest.0 == binding.sha256.as_str(), Refusal::ArtifactIdentity, "compressed observation digest differs")?;
    let file = File::open(path).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?;
    let mut reader = bounded_gzip::Reader::new(file, limits);
    let bytes = reader.next_line().map_err(|error| refusal(Refusal::CaptureBound, error.to_string()))?.ok_or_else(|| refusal(reason, "compressed observation absent"))?;
    let value = parse(bytes, reason)?;
    require(reader.next_line().map_err(|error| refusal(Refusal::CaptureBound, error.to_string()))?.is_none(), reason, "compressed observation has extra records")?;
    Ok(value)
}
fn raw_views<'a>(root: &Path, binding: &FileBinding, projections: impl Iterator<Item = &'a Words> + Clone) -> Result<(), Failure> {
    let raw: Vec<RawWords> = gzip_record(root, binding, capture::raw_view_limits(), Refusal::Mapping)?;
    require(raw.len() == projections.clone().count() && raw.len() <= 256, Refusal::Mapping, "raw/projected span coverage differs")?;
    for (raw, words) in raw.iter().zip(projections) {
        shared::words(words).map_err(|error| refusal(Refusal::Mapping, error))?;
        let left = &raw.mapping;
        let right = &words.mapping;
        require(left.guest == right.guest && left.physical == right.physical && left.bytes.0 == right.bytes.0 && left.tcm == right.tcm && matches!((left.decoder, right.decoder), (DramChoice::One, DramChoice::One) | (DramChoice::Two, DramChoice::Two)), Refusal::Mapping, "raw/projected mapping differs")?;
        require(u64::from(right.physical.0) + u64::from(right.bytes.0) <= 0x20000000, Refusal::Mapping, "raw projection is outside admitted DRAM")?;
        require(right.bytes.0 <= 320, Refusal::Mapping, "raw projection exceeds80words")?;
        require(raw.physical.len() == right.bytes.0 as usize && raw.aliases.iter().flatten().all(|bytes| *bytes == raw.physical), Refusal::Mapping, "raw core/alias bytes differ")?;
        require(raw.physical.chunks_exact(4).zip(&words.values).all(|(bytes, word)| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == *word), Refusal::Mapping, "projected words differ from raw byte buffers")?;
    }
    Ok(())
}
#[derive(Clone, Copy)]
enum KernelDomain { A, B }
impl KernelDomain {
    fn from_identity(identity: u32) -> Result<Self, Failure> {
        match identity { 0x4003 => Ok(Self::A), 0x5003 => Ok(Self::B), _ => Err(refusal(Refusal::TaskContext, "modeled kernel identity is unadmitted")) }
    }
    fn from_selector(selector: u32) -> Result<Self, Failure> {
        match selector { 0 => Ok(Self::A), 1 => Ok(Self::B), _ => Err(refusal(Refusal::QueueProvenance, "kernel selector is outside two domains")) }
    }
    fn index(self) -> usize { match self { Self::A => 0, Self::B => 1 } }
}
fn task_context(sample: &Sample) -> Result<(), Failure> {
    let domain = KernelDomain::from_identity(sample.identities[usize::from(sample.core)])?;
    let layout = capture::TASK_LAYOUTS[domain.index()];
    let creation = layout.creation.0;
    let task = layout.task.0;
    let at = |offset| projected(&sample.task, creation + offset);
    require(physical(Guest(projected(&sample.task, layout.current.0)?)) == physical(layout.task), Refusal::TaskContext, "executing core has another current task")?;
    require(at(8)? == 0xc22f60 && at(4)? == 0 && at(12)? == 6 - 1 && at(16)? == 0xc00 && at(36)? == 1 && projected(&sample.task, layout.ids.0)? == at(32)? && physical(Guest(projected(&sample.task, task + 8)?)) == physical(layout.creation), Refusal::TaskContext, "current UI C/T/id/priority binding differs")?;
    require(at(32)? & 0xffff == 1 && projected(&sample.task, task + 32)? == domain.index() as u32 + 1 && sample.registers.cpsr & 0x3f == 0x13, Refusal::TaskContext, "UI identity/domain or task privilege differs")?;
    let base = at(20)?;
    let end = base.checked_add(at(16)?).ok_or_else(|| refusal(Refusal::StackContext, "UI stack endpoint overflows"))?;
    let sp = physical(sample.registers.sp);
    require(base & 7 == 0 && base <= sp && sp <= end && sp & 7 == 0, Refusal::StackContext, "live UI stack is outside its committed allocation")?;
    if physical(sample.pc) == 0xc22f60 {
        require(sp == end && sample.registers.r[0] == 0 && physical(Guest(sample.registers.r[1])) == physical(layout.task) && physical(Guest(sample.registers.r[2])) == physical(layout.creation) && physical(sample.registers.lr) == [0x453f0, 0xd53b0][domain.index()], Refusal::TaskContext, "first UI entry differs from committed trampoline context")?;
    }
    Ok(())
}
fn is_ui(pc: u32) -> bool {
    (0xc22f60..0xc2327c).contains(&pc) || (CARRIER.0..CARRIER.0 + 32).contains(&pc)
        || matches!(pc, 0xbfb124 | 0xc22bd0 | 0xbfb1b8 | 0xbfb20c | 0xbfb1e0 | 0xdb5124 | 0xdb515c | 0xdb5174 | 0xdb5178 | 0xdb51d0)
}
struct PendingCall<'a> { before: &'a Sample, target: Guest, continuation: Guest, entered: bool, phase: usize }
struct Invocation<'a> { entry: &'a Sample, phase: usize, call: Option<PendingCall<'a>>, result: Option<u32>, returned_instruction: bool }
#[derive(Default)]
struct Sequence { complete: usize, receive: usize, pending: usize, queue_success: usize, queue_failure: usize }
fn sequences(report: &PumpReport, proof: &Profile) -> Result<Sequence, Failure> {
    let mut total = Sequence::default();
    let mut invocation: Option<Invocation<'_>> = None;
    let mut caller = None;
    let mut initializer = [false; 4];
    let mut ui_started = false;
    let mut received = false;
    let mut pending = false;
    let mut backedge_due = false;
    for sample in &report.samples {
        let pc = physical(sample.pc);
        if is_ui(pc) { task_context(sample)?; }
        match pc {
            0xc22f60 => { require(!ui_started, Refusal::Initializer, "UI entry repeated without lifecycle reset")?; ui_started = true; }
            0xbfb124 => initializer[0] = ui_started,
            0xc22f70 => initializer[1] = initializer[0],
            0xc22bd0 => initializer[2] = initializer[1],
            0xc22f74 => {
                require(initializer[2], Refusal::Initializer, "state reset lacks initialization chain")?;
                for offset in (0..348).step_by(4) { require(projected(&sample.state, 0x1467588 + offset)? == 0, Refusal::Initializer, "controller state reset is incomplete")?; }
                initializer[3] = true;
            }
            0xc22f84 => { require(invocation.is_none(), Refusal::Handback, "loop restarted without pump return")?; require(initializer.iter().all(|done| *done), Refusal::Initializer, "loop starts before initializer handback")?; received = false; pending = false; backedge_due = false; }
            0xc22fd8 | 0xc23150 => { received = true; if sample.registers.r[0] != 0 { total.queue_failure += 1; } else { total.queue_success += 1; } }
            0xc230a0 | 0xc230c0 | 0xc23114 | 0xc2312c if sample.registers.cpsr & (1 << 30) != 0 => pending = true,
            0xc230d8 if sample.registers.r[0] == 1 => pending = true,
            0xc230f4 if sample.registers.cpsr & (1 << 30) == 0 => pending = true,
            0xc23274 => { require(!backedge_due && initializer[3], Refusal::Handback, "new pump caller precedes prior backedge or init")?; caller = Some(sample); }
            _ => {}
        }
        if pc == CARRIER.0 {
            require(invocation.is_none(), Refusal::HookSequence, "pump reentered before handback")?;
            let before = caller.take().ok_or_else(|| refusal(Refusal::Handback, "pump has no natural UI caller"))?;
            require(before.core == sample.core && before.registers.sp == sample.registers.sp && before.registers.r == sample.registers.r && physical(sample.registers.lr) == 0xc23278, Refusal::Handback, "pump caller transfer differs")?;
            if received { total.receive += 1; } else { require(pending, Refusal::QueueProvenance, "pump has neither receive nor source-backed pending work")?; total.pending += 1; }
            invocation = Some(Invocation { entry: sample, phase: 0, call: None, result: None, returned_instruction: false });
        }
        if let Some(active) = &mut invocation {
            if (CARRIER.0..CARRIER.0 + 32).contains(&pc) {
                let low = i64::from(physical(active.entry.registers.sp)) + i64::from(proof.scratch_low);
                let sp = i64::from(physical(sample.registers.sp));
                require(low <= sp && sp <= i64::from(physical(active.entry.registers.sp)), Refusal::StackContext, "carrier stack exceeds its admitted live frame")?;
            }
            if let Some(call) = &mut active.call {
                if physical(call.target) == pc {
                    require(!call.entered && sample.core == call.before.core && sample.registers.sp == call.before.registers.sp && sample.registers.r == call.before.registers.r && physical(sample.registers.lr) == call.continuation.0, Refusal::HookSequence, "retained callee entry differs from its BL")?;
                    call.entered = true;
                } else if pc == call.continuation.0 {
                    require(call.entered && sample.core == call.before.core && sample.registers.sp == call.before.registers.sp && sample.registers.r[4..12] == call.before.registers.r[4..12], Refusal::HookSequence, "retained callee did not restore its caller context")?;
                    if call.phase == 1 { active.result = Some(sample.registers.r[0]); }
                    active.phase += 1;
                    active.call = None;
                }
            }
            if let Some((index, (_, target))) = proof.calls.iter().enumerate().find(|(_, (site, _))| site.0 == pc) {
                require(active.call.is_none() && active.phase == index && sample.core == active.entry.core, Refusal::HookSequence, "retained calls were skipped, doubled or reordered")?;
                active.call = Some(PendingCall { before: sample, target: *target, continuation: Guest(pc + 4), entered: false, phase: index });
            }
            if pc == proof.returned.0 {
                require(active.phase == 3 && active.call.is_none(), Refusal::HookSequence, "carrier returns before all hooks complete")?;
                active.returned_instruction = true;
            }
        }
        if pc == 0xc23278 {
            let active = invocation.take().ok_or_else(|| refusal(Refusal::Handback, "caller continuation lacks a pump"))?;
            require(active.returned_instruction && active.phase == 3 && active.call.is_none() && sample.core == active.entry.core && sample.registers.sp == active.entry.registers.sp && sample.registers.r[4..12] == active.entry.registers.r[4..12], Refusal::Handback, "pump frame/callee saves/return transfer differs")?;
            require(active.result == Some(sample.registers.r[0]), Refusal::NormalizerResult, "posthook replaced the normalizer result")?;
            total.complete += 1;
            backedge_due = true;
        }
    }
    require(invocation.is_none() && !backedge_due && total.complete >= 2 && total.receive >= 2, Refusal::Handback, "repeated pump interval ends before complete handback")?;
    Ok(total)
}

#[derive(Deserialize, Serialize)]
struct Watched { enabled: bool, violation: Option<String>, classes: Vec<Class>, ranges: Vec<[u64; 2]>, views: Views, records: Vec<Row> }
fn same_classes(left: &[Class], right: &[Class]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(left, right)| left.range == right.range && left.kinds == right.kinds)
}
fn class_ranges(classes: &[Class]) -> Result<Vec<[u64; 2]>, Failure> {
    require(!classes.is_empty() && classes.len() <= 256, Refusal::Mapping, "watch class count differs")?;
    let mut spans = classes.iter().map(|class| class.range).collect::<Vec<_>>();
    require(spans.iter().all(|span| span[0] < span[1] && span[1] <= 1u64 << 32), Refusal::Mapping, "watch class has an invalid span")?;
    spans.sort_unstable();
    let mut merged: Vec<[u64; 2]> = Vec::new();
    for span in spans {
        if let Some(previous) = merged.last_mut().filter(|previous| span[0] <= previous[1]) {
            previous[1] = previous[1].max(span[1]);
        } else { merged.push(span); }
    }
    Ok(merged)
}
fn host_rows<'a>(rows: &[Row], reads: &HostReads, epoch: &PumpEpoch, projections: impl Iterator<Item = &'a Words>) -> Result<(), Failure> {
    require(reads.epoch == epoch.id && epoch.start.0 <= reads.stopped.0 && reads.stopped.0 <= epoch.end.0 && reads.first.0 <= reads.end.0 && reads.end.0 <= rows.len(), Refusal::Mapping, "host read attribution interval differs")?;
    let inside = &rows[reads.first.0..reads.end.0];
    require(inside.iter().all(|row| row.source == Access::HostRead && row.core.is_none() && row.pc.is_none() && row.instructions.is_none()), Refusal::Mapping, "projection interval includes an attributed writer or guest step")?;
    for words in projections {
        require(inside.iter().any(|row| row.physical == words.mapping.physical && row.address == words.mapping.physical && row.size == u64::from(words.mapping.bytes.0)), Refusal::Mapping, "raw physical projection read is absent from host journal")?;
    }
    Ok(())
}
fn epochs(root: &Path, report: &PumpReport, publication: Count) -> Result<(), Failure> {
    require(!report.epochs.is_empty(), Refusal::CaptureBound, "watch epochs absent")?;
    let mut previous = Count(0);
    for (index, epoch) in report.epochs.iter().enumerate() {
        require(epoch.id.0 == index && epoch.start == previous && epoch.end.0 >= epoch.start.0 && epoch.end.0 <= report.instructions.0 && epoch.enabled && !epoch.refused && epoch.teardown_error.is_none(), Refusal::CaptureBound, "watch interval is incomplete or discontinuous")?;
        previous = epoch.end;
        let raw: Watched = gzip_record(root, &epoch.raw, capture::watch_limits(), Refusal::CaptureBound)?;
        require(raw.enabled && raw.violation.is_none() && raw.records.len() == epoch.record_count && raw.records.len() < 262144 && raw.views == epoch.views && same_classes(&raw.classes, &epoch.classes), Refusal::CaptureBound, "raw watch and epoch declaration differ")?;
        require(raw.ranges == class_ranges(&raw.classes)?, Refusal::Mapping, "installed watch ranges differ from declared classes")?;
        require(if epoch.end.0 <= publication.0 { epoch.views == Views::Observe } else { epoch.start.0 >= publication.0 && epoch.views == Views::Fixed }, Refusal::Publication, "fixed-view watch does not align with publication")?;
        if epoch.views == Views::Fixed {
            require(raw.classes.iter().any(|class| class.range == [u64::from(CARRIER.0), u64::from(CARRIER.0 + 32)] && [Access::CpuFetch, Access::CpuWrite, Access::HostWrite, Access::ModelBusWrite].iter().all(|kind| class.kinds.contains(kind))), Refusal::CodeIdentity, "continuous carrier guard coverage absent")?;
        }
        let mut cpu_clock = epoch.start.0;
        for row in &raw.records {
            let end = u64::from(row.physical.0).checked_add(row.size).ok_or_else(|| refusal(Refusal::Mapping, "raw access endpoint overflows"))?;
            require(row.size > 0 && end <= 1u64 << 32 && raw.classes.iter().any(|class| class.kinds.contains(&row.source) && u64::from(row.physical.0) < class.range[1] && class.range[0] < end), Refusal::Mapping, "raw access escapes declared class")?;
            if matches!(row.source, Access::CpuFetch | Access::CpuRead | Access::CpuWrite) {
                let count = row.instructions.ok_or_else(|| refusal(Refusal::Mapping, "CPU clock absent"))?.0;
                require(row.core.is_some() && row.pc.is_some() && epoch.start.0 <= count && count <= epoch.end.0 && cpu_clock <= count, Refusal::Mapping, "CPU attribution/stop-fetch clock differs")?;
                cpu_clock = count;
            } else { require(row.core.is_none() && row.pc.is_none() && row.instructions.is_none(), Refusal::Mapping, "host/model access has invented CPU attribution")?; }
            if u64::from(row.physical.0) < u64::from(CARRIER.0 + 32) && u64::from(CARRIER.0) < end {
                require(!matches!(row.source, Access::CpuFetch) || epoch.views == Views::Fixed, Refusal::Publication, "pump fetched before publication")?;
                require(epoch.views != Views::Fixed || !matches!(row.source, Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite), Refusal::CodeIdentity, "published carrier was written")?;
                if row.source == Access::CpuFetch {
                    require(row.size == 4 && row.pc.is_some_and(|pc| physical(pc) == row.physical.0) && report.samples.iter().any(|sample| row.core == Some(sample.core) && physical(sample.pc) == row.physical.0 && row.instructions.is_some_and(|count| count == sample.stop || count == sample.resume)), Refusal::CodeIdentity, "carrier fetch lacks its raw stopped/resumed instruction sample")?;
                }
            }
        }
        for sample in report.samples.iter().filter(|sample| sample.host_reads.epoch == epoch.id) {
            host_rows(&raw.records, &sample.host_reads, epoch, sample_projections(sample))?;
        }
        for visible in report.visible.iter().filter(|visible| visible.host_reads.epoch == epoch.id) {
            host_rows(&raw.records, &visible.host_reads, epoch, visible.state.iter().chain(&visible.task_tables))?;
        }
    }
    require(previous == report.instructions, Refusal::CaptureBound, "watch ends before observation")
}
fn sample_projections(sample: &Sample) -> impl Iterator<Item = &Words> + Clone {
    sample.carrier.iter().chain(std::iter::once(&sample.code)).chain(&sample.task).chain(&sample.stack).chain(&sample.packet).chain(&sample.state)
}
fn publication(root: &Path, report: &PumpReport, catalog: &Catalog, hits: &[shared::Hit]) -> Result<Count, Failure> {
    let published = report.loader.events.iter().filter(|event| physical(event.pc) == 0x6a9840).collect::<Vec<_>>();
    require(published.len() == 1, Refusal::Publication, "image5 publication boundary is not unique")?;
    let published = published[0];
    require(published.registers.r[0] == 0, Refusal::Publication, "image5 producer returned nonzero")?;
    let sample = report.loader.snapshots.iter().find(|snapshot| snapshot.event.0 == published.id.0 && snapshot.resource == Resource::Image5).ok_or_else(|| refusal(Refusal::Publication, "image5 publication snapshot absent"))?;
    shared::snapshot(root, sample, catalog).map_err(|error| refusal(Refusal::Publication, error))?;
    require(sample.expected_equal, Refusal::Publication, "published image5 differs from exact admitted image")?;
    for hit in hits {
        let (pc, _, count) = shared::hit_location(hit);
        require(count.0 >= published.instructions.0 || !is_ui(physical(pc)), Refusal::Publication, "application executes before admitted image publication")?;
    }
    let entry = report.loader.events.iter().find(|event| physical(event.pc) == 0xd985a4).ok_or_else(|| refusal(Refusal::Initializer, "finite image5 initializer entry absent"))?;
    let ui = report.samples.iter().find(|sample| physical(sample.pc) == 0xc22f60).ok_or_else(|| refusal(Refusal::TaskContext, "natural task1 first entry absent"))?;
    require(published.instructions.0 < entry.instructions.0 && entry.instructions.0 < ui.stop.0, Refusal::Initializer, "application publication/init/task partial order differs")?;
    Ok(published.instructions)
}
struct CallBoundaries<'a> { before: &'a Event, entered: &'a Event, returned: &'a Event }
fn loader_event(report: &PumpReport, id: EventId) -> Result<&Event, Failure> {
    report.loader.events.get(id.0).filter(|event| event.id.0 == id.0).ok_or_else(|| refusal(Refusal::Initializer, "initializer call references a missing raw boundary"))
}
fn completed_call<'a>(report: &'a PumpReport, call: &Call) -> Result<CallBoundaries<'a>, Failure> {
    let before = loader_event(report, call.before.ok_or_else(|| refusal(Refusal::Initializer, "initializer call site absent"))?)?;
    let entered = loader_event(report, call.entry.ok_or_else(|| refusal(Refusal::Initializer, "initializer callee entry absent"))?)?;
    let returned = loader_event(report, call.returned.ok_or_else(|| refusal(Refusal::Initializer, "initializer handback absent"))?)?;
    require(entered.core == before.core && returned.core == before.core && call.core == before.core && entered.pc == call.target && returned.pc == call.continuation && physical(before.pc) + 4 == physical(call.continuation) && entered.registers.sp == before.registers.sp && returned.registers.sp == before.registers.sp && entered.registers.lr == call.continuation && entered.registers.r == before.registers.r && returned.registers.r[4..12] == entered.registers.r[4..12] && before.instructions.0 < entered.instructions.0 && entered.instructions.0 < returned.instructions.0, Refusal::Initializer, "initializer transfer, ABI or chronology differs")?;
    Ok(CallBoundaries { before, entered, returned })
}
fn initializers(report: &PumpReport, catalog: &Catalog) -> Result<(), Failure> {
    let mut previous = Count(0);
    let mut services = 0usize;
    for pc in (0xd985a4..0xd986ac).step_by(4) {
        let Some(target) = branch(Guest(pc), catalog.word(Guest(pc)).map_err(|error| refusal(Refusal::CodeIdentity, error))?.1) else { continue; };
        let mut calls = report.loader.calls.iter().filter(|call| call.before.and_then(|id| report.loader.events.get(id.0)).is_some_and(|event| physical(event.pc) == pc));
        let call = calls.next().ok_or_else(|| refusal(Refusal::Initializer, "required image5 initialization call absent"))?;
        require(calls.next().is_none() && physical(call.target) == target.0 && call.kind == if pc == 0xd985c0 { CallKind::Image5Wait } else { CallKind::Image5Service }, Refusal::Initializer, "image5 initialization call is duplicated or misclassified")?;
        let observed = completed_call(report, call)?;
        require(observed.before.instructions.0 >= previous.0, Refusal::Initializer, "image5 services were reordered")?;
        if catalog.word(Guest(pc + 4)).map_err(|error| refusal(Refusal::CodeIdentity, error))?.1 == 0xe3500000 {
            require(observed.returned.registers.r[0] == 0, Refusal::Initializer, "checked initializer service returned nonzero")?;
        }
        previous = observed.returned.instructions;
        services += 1;
    }
    require(services == 21, Refusal::Initializer, "source initializer service census differs")?;
    let constructors = report.loader.calls.iter().filter(|call| call.kind == CallKind::Image5Constructor).collect::<Vec<_>>();
    require(constructors.len() == 3, Refusal::Initializer, "reverse constructor census differs")?;
    for (call, target) in constructors.into_iter().zip([0x107277c, 0x1066304, 0x1062114]) {
        let observed = completed_call(report, call)?;
        require(physical(observed.entered.pc) == target, Refusal::Initializer, "reverse constructor target order differs")?;
    }
    let mut entries = report.loader.calls.iter().filter(|call| call.kind == CallKind::Image5Entry);
    let entry = entries.next().ok_or_else(|| refusal(Refusal::Initializer, "finite image5 entry call absent"))?;
    require(entries.next().is_none(), Refusal::Initializer, "finite image5 entry was repeated")?;
    let entry = completed_call(report, entry)?;
    let first_pump = report.samples.iter().find(|sample| physical(sample.pc) == CARRIER.0).ok_or_else(|| refusal(Refusal::Initializer, "no application pump after initialization"))?;
    require(previous.0 < entry.returned.instructions.0 && entry.returned.instructions.0 < first_pump.stop.0, Refusal::Initializer, "pump precedes completion of admitted initializer calls")?;
    Ok(())
}
fn inspect(root: &Path, report: &PumpReport, canonical: &Catalog) -> Result<Verification, Failure> {
    let (catalog, proof, document) = identity(root, report, canonical)?;
    let hits: Vec<shared::Hit> = parse(&artifact(root, &report.loader.trace)?, Refusal::Guest)?;
    require(hits.len() < 65536 && !report.samples.is_empty() && report.samples.len() < 32768, Refusal::CaptureBound, "trace/sample bounds exceeded")?;
    for event in &report.loader.events {
        require(hits.iter().any(|hit| shared::hit_matches(hit, event.pc, event.core, event.instructions, &event.registers)), Refusal::Publication, "loader boundary missing from raw trace")?;
        shared::words(&event.code).map_err(|error| refusal(Refusal::Mapping, error))?;
        require(event.code.values == [catalog.word(event.pc).map_err(|error| refusal(Refusal::CodeIdentity, error))?.1], Refusal::CodeIdentity, "loader code observation differs from admitted source")?;
        for value in event.state.iter().chain(event.descriptor.iter()).chain(event.payload.iter()) {
            shared::words(value).map_err(|error| refusal(Refusal::Mapping, error))?;
        }
    }
    let published = publication(root, report, &catalog, &hits)?;
    initializers(report, &catalog)?;
    let mut previous = Count(0);
    let mut counts = [0u64; 2];
    for (index, sample) in report.samples.iter().enumerate() {
        require(sample.id.0 == index && sample.stop.0 >= previous.0 && sample.stop.0 <= report.instructions.0 && sample.resume.0 == sample.stop.0.checked_add(1).ok_or_else(|| refusal(Refusal::CaptureBound, "resume clock overflows"))? && sample.counts.iter().zip(counts).all(|(now, old)| *now >= old) && sample.counts[0].checked_add(sample.counts[1]) == Some(sample.stop.0) && sample.host_reads.stopped == sample.stop, Refusal::Mapping, "sample clock/host stop checkpoint differs")?;
        previous = sample.stop;
        counts = sample.counts;
        require(hits.iter().filter(|hit| shared::hit_matches(hit, sample.pc, sample.core, sample.stop, &sample.registers)).count() == 1, Refusal::Guest, "sample/raw register coverage differs")?;
        raw_views(root, &sample.raw_views, sample_projections(sample))?;
        let expected = catalog.word(sample.pc).map_err(|error| refusal(Refusal::CodeIdentity, error))?.1;
        require(sample.code.mapping.guest == sample.pc && sample.code.values == [expected], Refusal::CodeIdentity, "stopped instruction differs from admitted source")?;
        for (alias, carrier) in sample.carrier.iter().enumerate() {
            require(carrier.mapping.guest.0 == CARRIER.0 | if alias == 0 { 0 } else { 0x40000000 } && carrier.values.len() == 8, Refusal::CodeIdentity, "carrier alias span differs")?;
            for (word, value) in carrier.values.iter().enumerate() { require(*value == catalog.word(Guest(CARRIER.0 + word as u32 * 4)).map_err(|error| refusal(Refusal::CodeIdentity, error))?.1, Refusal::CodeIdentity, "original or foreign carrier bytes executed")?; }
        }
    }
    epochs(root, report, published)?;
    let sequence = sequences(report, &proof)?;
    queue::check(report)?;
    compare::visible(root, report, &document)?;
    let mut unqualified = vec![Route::Unhandled, Route::ReceiveFailure, Route::ServiceFailure];
    if sequence.pending == 0 { unqualified.push(Route::PendingWork); }
    Ok(Verification { schema: 1, mode: report.mode, receipt: Digest::of(&bounded(&root.join("application-pump.json"), METADATA_BYTES)?), samples: report.samples.len(), complete_pumps: sequence.complete, receive_backed_pumps: sequence.receive, pending_work_pumps: sequence.pending, queue_successes: sequence.queue_success, queue_failures: sequence.queue_failure, visible_frames: report.visible.len(), unqualified })
}
pub(super) fn check(root: &Path, catalog: &Catalog) -> Result<Verification, String> {
    let bytes = bounded(&root.join("application-pump.json"), METADATA_BYTES).map_err(|failure| format!("{:?}: {}", failure.reason, failure.detail))?;
    let report: PumpReport = parse(&bytes, Refusal::Guest).map_err(|failure| format!("{:?}: {}", failure.reason, failure.detail))?;
    inspect(root, &report, catalog).map_err(|failure| format!("{:?}: {}", failure.reason, failure.detail))
}

pub(super) fn paired(retained: &Path, owned: &Path, catalog: &Catalog, output: &Path) -> Result<(), String> {
    let result = compare::paired(retained, owned, catalog).map_err(|failure| format!("{:?}: {}", failure.reason, failure.detail))?;
    persist(&output.join("application-pump-comparison.json"), &result)
}

pub(super) fn refusal_controls(source: &Path, catalog: &Catalog, output: &Path) -> Result<(), String> {
    controls::run(source, output, catalog)
}
