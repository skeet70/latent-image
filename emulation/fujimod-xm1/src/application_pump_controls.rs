use super::*;
use std::os::unix::fs::symlink;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Control { CanonicalInput, StopClock, RawAlias, KernelIdentity, MissingInitialization, NormalizerResult, LostCalleeSave, RenderReadiness, WatchCoverage, PublishedWrite, PacketCopy, RelabelledOlderSample, TruncatedPlane }
impl Control {
    fn reason(self) -> Refusal { match self {
        Self::CanonicalInput => Refusal::ArtifactIdentity,
        Self::StopClock | Self::RawAlias => Refusal::Mapping,
        Self::KernelIdentity => Refusal::TaskContext,
        Self::MissingInitialization => Refusal::Initializer,
        Self::NormalizerResult => Refusal::NormalizerResult,
        Self::LostCalleeSave | Self::RelabelledOlderSample => Refusal::Handback,
        Self::RenderReadiness | Self::TruncatedPlane => Refusal::RenderCompletion,
        Self::WatchCoverage => Refusal::CaptureBound,
        Self::PublishedWrite => Refusal::CodeIdentity,
        Self::PacketCopy => Refusal::QueueProvenance,
    } }
    fn name(self) -> &'static str { match self {
        Self::CanonicalInput => "canonical-input", Self::StopClock => "stop-clock", Self::RawAlias => "raw-alias",
        Self::KernelIdentity => "kernel-identity",
        Self::MissingInitialization => "missing-initialization", Self::NormalizerResult => "normalizer-result",
        Self::LostCalleeSave => "lost-callee-save", Self::RenderReadiness => "render-readiness",
        Self::WatchCoverage => "watch-coverage", Self::PublishedWrite => "published-write", Self::PacketCopy => "packet-copy",
        Self::RelabelledOlderSample => "relabelled-older-sample", Self::TruncatedPlane => "truncated-plane",
    } }
}
#[derive(Serialize)]
struct ResultRow { control: Control, expected: Refusal, actual: Option<Refusal>, accepted: bool, receipt: Digest }
#[derive(Serialize)]
struct Controls { schema: u32, source: Digest, baseline: Verification, cases: Vec<ResultRow>, passed: bool }
fn replace(root: &Path, name: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let path = root.join(name);
    if path.symlink_metadata().is_ok() { std::fs::remove_file(&path).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?; }
    let mut file = std::fs::OpenOptions::new().create_new(true).write(true).open(path).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?;
    std::io::Write::write_all(&mut file, bytes).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))
}
fn rewrite<T: Serialize>(root: &Path, name: &Path, value: &T) -> Result<Digest, Failure> {
    let bytes = serde_json::to_vec(value).map_err(|error| refusal(Refusal::Guest, error.to_string()))?;
    replace(root, name, &bytes)?;
    Ok(Digest::of(&bytes))
}
fn links(source: &Path, output: &Path) -> Result<(), Failure> {
    std::fs::create_dir(output).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?;
    for entry in std::fs::read_dir(source).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))? {
        let entry = entry.map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?;
        if entry.file_type().map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?.is_file() {
            symlink(entry.path(), output.join(entry.file_name())).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?;
        }
    }
    Ok(())
}
fn raw(root: &Path, binding: &FileBinding) -> Result<Vec<RawWords>, Failure> {
    gzip_record(root, binding, capture::raw_view_limits(), Refusal::Mapping)
}
fn write_gzip<T: Serialize + ?Sized>(root: &Path, binding: &mut FileBinding, value: &T, limits: capture::bounded_gzip::Limits) -> Result<(), Failure> {
    let bytes = serde_json::to_vec(value).map_err(|error| refusal(Refusal::Guest, error.to_string()))?;
    let mut writer = capture::bounded_gzip::Writer::new(Vec::new(), limits);
    writer.append(&bytes).map_err(|error| refusal(Refusal::CaptureBound, error.to_string()))?;
    let bytes = writer.finish().map_err(|error| refusal(Refusal::CaptureBound, error.to_string()))?;
    replace(root, &binding.path, &bytes)?;
    binding.sha256 = Digest::of(&bytes);
    Ok(())
}
fn alter_trace(root: &Path, report: &mut PumpReport, sample: &Sample) -> Result<(), Failure> {
    #[derive(Deserialize, Serialize)]
    struct Hit { site: String, address: String, core: usize, instructions: u64, registers: BTreeMap<String, String>, sp: String, lr: String, cpsr: u32 }
    let mut hits: Vec<Hit> = parse(&artifact(root, &report.loader.trace)?, Refusal::Guest)?;
    let hit = hits.iter_mut().find(|hit| hit.core == usize::from(sample.core) && hit.instructions == sample.stop.0 && hit.address == format!("0x{:08x}", sample.pc.0)).ok_or_else(|| refusal(Refusal::Guest, "control target raw hit absent"))?;
    hit.cpsr = sample.registers.cpsr;
    hit.sp = format!("0x{:08x}", sample.registers.sp.0);
    for (index, value) in sample.registers.r.iter().enumerate() { hit.registers.insert(format!("r{index}"), format!("0x{value:08x}")); }
    report.loader.trace.sha256 = rewrite(root, &report.loader.trace.path, &hits)?;
    Ok(())
}
fn mutate(root: &Path, report: &mut PumpReport, control: Control) -> Result<(), Failure> {
    match control {
        Control::CanonicalInput => report.identity.original_update.sha256 = Digest::of(b"wrong-source"),
        Control::StopClock => report.samples.first_mut().ok_or_else(|| refusal(Refusal::Guest, "no control sample"))?.resume.0 += 1,
        Control::RawAlias => {
            let sample = report.samples.first_mut().ok_or_else(|| refusal(Refusal::Guest, "no control sample"))?;
            let mut raw = raw(root, &sample.raw_views)?;
            raw.first_mut().and_then(|view| view.aliases[1][1].first_mut()).map(|value| *value ^= 1).ok_or_else(|| refusal(Refusal::Mapping, "no raw alias control byte"))?;
            write_gzip(root, &mut sample.raw_views, &raw, capture::raw_view_limits())?;
        }
        Control::KernelIdentity => {
            let sample = report.samples.iter_mut().find(|sample| physical(sample.pc) == 0xc22f60).ok_or_else(|| refusal(Refusal::TaskContext, "no first-entry context control sample"))?;
            let identity = &mut sample.identities[usize::from(sample.core)];
            *identity = match *identity { 0x4003 => 0x5003, 0x5003 => 0x4003, _ => return Err(refusal(Refusal::TaskContext, "baseline kernel identity is invalid")) };
        }
        Control::MissingInitialization => {
            let index = report.samples.iter().position(|sample| physical(sample.pc) == 0xc22f74).ok_or_else(|| refusal(Refusal::Initializer, "no reset handback control sample"))?;
            report.samples.remove(index);
            for (index, sample) in report.samples.iter_mut().enumerate() { sample.id = SampleId(index); }
        }
        Control::NormalizerResult | Control::LostCalleeSave => {
            let index = report.samples.iter().position(|sample| physical(sample.pc) == 0xc23278).ok_or_else(|| refusal(Refusal::Handback, "no pump handback control sample"))?;
            if matches!(control, Control::NormalizerResult) { report.samples[index].registers.r[0] ^= 1; }
            else { report.samples[index].registers.r[4] ^= 1; }
            let sample = report.samples[index].clone();
            alter_trace(root, report, &sample)?;
        }
        Control::RelabelledOlderSample => {
            let input = report.inputs.get(15).ok_or_else(|| refusal(Refusal::Handback, "no ordinary input control interval"))?;
            let (id, pressed) = (input.id, input.pressed);
            let handback = report.samples.iter().rposition(|sample| physical(sample.pc) == 0xc23278 && sample.stop.0 < pressed.0).ok_or_else(|| refusal(Refusal::Handback, "no older complete handback for relabelling"))?;
            let entry = report.samples[..handback].iter().rposition(|sample| physical(sample.pc) == CARRIER.0).ok_or_else(|| refusal(Refusal::Handback, "no older pump entry for relabelling"))?;
            require(report.samples[entry].core == report.samples[handback].core && report.samples[entry].registers.sp == report.samples[handback].registers.sp, Refusal::Handback, "older control samples are not a matching invocation")?;
            for sample in &mut report.samples { if sample.input == Some(id) { sample.input = None; } }
            report.samples[entry].input = Some(id);
            report.samples[handback].input = Some(id);
        }
        Control::TruncatedPlane => {
            let visible = report.visible.first_mut().ok_or_else(|| refusal(Refusal::RenderCompletion, "no visible plane control"))?;
            let mutation = compare::truncated_plane(root, visible)?;
            replace(root, &visible.planes.path, &mutation.planes)?;
            replace(root, &visible.scanout.path, &mutation.scanout)?;
            visible.planes.sha256 = Digest::of(&mutation.planes);
            visible.scanout.sha256 = Digest::of(&mutation.scanout);
        }
        Control::RenderReadiness => report.visible.first_mut().ok_or_else(|| refusal(Refusal::RenderCompletion, "no visible control"))?.probe.drained = false,
        Control::WatchCoverage => report.epochs.first_mut().ok_or_else(|| refusal(Refusal::CaptureBound, "no watch control"))?.enabled = false,
        Control::PublishedWrite => {
            let epoch = report.epochs.iter_mut().rev().find(|epoch| epoch.views == Views::Fixed).ok_or_else(|| refusal(Refusal::Publication, "no published epoch"))?;
            let mut watch: Watched = gzip_record(root, &epoch.raw, capture::watch_limits(), Refusal::CaptureBound)?;
            watch.records.push(Row { source: Access::CpuWrite, address: CARRIER, physical: CARRIER, size: 4, core: Some(Core::Primary), pc: Some(Guest(0xc23274)), instructions: Some(epoch.end), value: Some(0) });
            epoch.record_count = watch.records.len();
            write_gzip(root, &mut epoch.raw, &watch, capture::watch_limits())?;
        }
        Control::PacketCopy => {
            let sample = report.samples.iter_mut().find(|sample| physical(sample.pc) == 0x725414 && projected(&sample.packet, sample.registers.r[5] + 4).is_ok_and(|length| length > 0) && projected(&sample.packet, sample.registers.r[5] + 8).is_ok_and(|destination| sample.packet.iter().any(|view| view.mapping.guest.0 == destination))).ok_or_else(|| refusal(Refusal::QueueProvenance, "no nonempty packet release control sample"))?;
            let destination = projected(&sample.packet, sample.registers.r[5] + 8)?;
            let target = sample.packet.iter_mut().find(|view| view.mapping.guest.0 == destination).ok_or_else(|| refusal(Refusal::QueueProvenance, "copied payload projection absent"))?;
            target.values[0] ^= 1;
            let mut raw = raw(root, &sample.raw_views)?;
            let target = raw.iter_mut().find(|view| view.mapping.guest.0 == destination).ok_or_else(|| refusal(Refusal::Mapping, "raw copied payload absent"))?;
            target.physical[0] ^= 1;
            for bytes in target.aliases.iter_mut().flatten() { bytes[0] ^= 1; }
            write_gzip(root, &mut sample.raw_views, &raw, capture::raw_view_limits())?;
        }
    }
    Ok(())
}
pub(super) fn run(source: &Path, output: &Path, catalog: &Catalog) -> Result<(), String> {
    let result = (|| -> Result<(), Failure> {
        let source = std::fs::canonicalize(source).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?;
        let original = bounded(&source.join("application-pump.json"), METADATA_BYTES)?;
        let baseline: PumpReport = parse(&original, Refusal::Guest)?;
        let baseline = inspect(&source, &baseline, catalog)?;
        let mut rows = Vec::new();
        for control in [Control::CanonicalInput, Control::StopClock, Control::RawAlias, Control::KernelIdentity, Control::MissingInitialization, Control::NormalizerResult, Control::LostCalleeSave, Control::RenderReadiness, Control::WatchCoverage, Control::PublishedWrite, Control::PacketCopy, Control::RelabelledOlderSample, Control::TruncatedPlane] {
            let root = output.join(control.name());
            links(&source, &root)?;
            let mut report: PumpReport = parse(&original, Refusal::Guest)?;
            mutate(&root, &mut report, control)?;
            let receipt = rewrite(&root, Path::new("application-pump.json"), &report)?;
            let serialized: PumpReport = parse(&bounded(&root.join("application-pump.json"), METADATA_BYTES)?, Refusal::Guest)?;
            let actual = inspect(&root, &serialized, catalog).err().map(|failure| failure.reason);
            rows.push(ResultRow { control, expected: control.reason(), actual, accepted: actual == Some(control.reason()), receipt });
        }
        let passed = rows.iter().all(|row| row.accepted);
        let record = Controls { schema: 1, source: Digest::of(&original), baseline, cases: rows, passed };
        persist(&output.join("application-pump-controls.json"), &record).map_err(|error| refusal(Refusal::ArtifactIdentity, error))?;
        require(passed, Refusal::Guest, "a serialized refusal control reached another reason or was admitted")
    })();
    result.map_err(|failure| format!("{:?}: {}", failure.reason, failure.detail))
}
