//! `--application-controls`: each control mutates one copy of the composed role's receipt, evidence and side files and
//! requires `--compare` to refuse it with its named difference.
use super::checks::{locate, ApplicationCheck, FIRST_SETTLED, HANDBACK, KERNEL_RECEIVE, PUMP_CALL, RELEASE_CALL, TASK1_RESET};
use super::planes;
use super::readiness::KERNEL_RETURN;
use super::checks::PairField;
use super::{
    read_evidence, view_word, write_evidence, ApplicationWrite, CoreIdentity, Evidence, InputIndex, Instructions, RawView, Sample, SampleIndex,
    SideFile, ReadinessIndex, SiteRef, UiWorkViews, MENU_LEVEL, PRIORITY_COUNT,
};
use crate::composed_catalog::{physical_address as physical, Application, CodeLabel, Contract, Core, Sha256};
use crate::composed_receipt::{compare, CaptureDirectories, Difference, Receipt, VerdictKind, WriteKind, RECEIPT_FILE};
use crate::provenance::Guest;
use crate::run::CoreIdentities;
use crate::trace::TraceStackScopeConfig;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Control {
    CanonicalInput,
    StopClock,
    RawAlias,
    KernelIdentity,
    MissingInitialization,
    NormalizerResult,
    LostCalleeSave,
    RelabelledOlderSample,
    RenderReadiness,
    TruncatedPlane,
    WatchCoverage,
    PublishedWrite,
    PacketCopy,
    FrozenStackBound,
    ReadinessUnstable,
    UiWorkPending,
    FirstReadyMenu,
    OwnedPumpSkipsPosthook,
    StockPumpWord,
    InitializerSkipsEd4afc,
    StockInitializerPc,
}

/// The stock pump's first word, `push {r3,r4,r11,lr}`.
const STOCK_PUMP_ENTRY: u32 = 0xe92d4818;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "check", rename_all = "kebab-case")]
pub enum Expected {
    Identity,
    Application(ApplicationCheck),
    Pair(PairField),
}

impl Expected {
    pub fn matches(self, difference: &Difference) -> bool {
        match self {
            Self::Identity => matches!(difference, Difference::Identity { .. }),
            Self::Application(check) => matches!(difference, Difference::Application { refusal, .. } if refusal.check == check),
            Self::Pair(field) => matches!(difference, Difference::ApplicationPair { field: actual } if *actual == field),
        }
    }
}

/// Physically and in every spelling.
fn set_word(view: &mut RawView, address: u32, value: u32) -> Result<(), String> {
    let offset = physical(address).checked_sub(physical(view.address.0)).map(|offset| offset as usize)
        .filter(|offset| offset % 4 == 0 && offset + 4 <= view.physical.0.len()).ok_or("the view does not hold the word")?;
    for bytes in std::iter::once(&mut view.physical).chain(view.spellings.iter_mut().flatten()) {
        bytes.0[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn first_ready_ui<'a>(receipt: &Receipt, evidence: &'a mut Evidence) -> Result<&'a mut UiWorkViews, String> {
    let first = receipt.application.as_ref().and_then(|section| section.first_ready).ok_or("no first-ready")?;
    Ok(&mut evidence.readiness.get_mut(first.readiness.0 as usize).ok_or("the first-ready sample is absent")?.ui)
}

fn stock_of(application: &Application, sample: &Sample) -> Option<u32> { locate(application, sample).map(|(stock, _)| stock) }

fn position(application: &Application, evidence: &Evidence, stock: u32) -> Result<usize, String> {
    evidence.samples.iter().position(|sample| stock_of(application, sample) == Some(stock)).ok_or_else(|| format!("no sample at {stock:#x}"))
}

/// Indices at or after `from` move by `delta`.
fn shift(index: &mut SampleIndex, from: u64, delta: i64) {
    if index.0 >= from {
        index.0 = index.0.saturating_add_signed(delta);
    }
}

fn reindex(receipt: &mut Receipt, evidence: &mut Evidence, from: u64, delta: i64) {
    for (index, sample) in evidence.samples.iter_mut().enumerate() {
        sample.index = SampleIndex(index as u64);
    }
    if let Some(section) = receipt.application.as_mut() {
        if let Some(task1) = section.task1.as_mut() {
            shift(&mut task1.entry, from, delta);
        }
        if let Some(publication) = section.publication.as_mut() {
            shift(&mut publication.sample, from, delta);
        }
        if let Some(first_ready) = section.first_ready.as_mut() {
            shift(&mut first_ready.receive, from, delta);
        }
        section.targets.iter_mut().for_each(|target| shift(&mut target.sample, from, delta));
    }
    for readiness in &mut evidence.readiness {
        shift(&mut readiness.receive, from, delta);
    }
}

fn remove_sample(receipt: &mut Receipt, evidence: &mut Evidence, at: usize) {
    evidence.samples.remove(at);
    for readiness in &mut evidence.readiness {
        if readiness.preceding > at as u64 {
            readiness.preceding -= 1;
        }
    }
    reindex(receipt, evidence, at as u64 + 1, -1);
}

fn insert_sample(receipt: &mut Receipt, evidence: &mut Evidence, at: usize, sample: Sample) {
    let stop = sample.stop;
    evidence.samples.insert(at, sample);
    for readiness in &mut evidence.readiness {
        if readiness.preceding > at as u64 || (readiness.preceding == at as u64 && readiness.instructions > stop) {
            readiness.preceding += 1;
        }
    }
    reindex(receipt, evidence, at as u64, 1);
}

impl Control {
    pub const ALL: [Self; 21] = [
        Self::CanonicalInput, Self::StopClock, Self::RawAlias, Self::KernelIdentity, Self::MissingInitialization, Self::NormalizerResult,
        Self::LostCalleeSave, Self::RelabelledOlderSample, Self::RenderReadiness, Self::TruncatedPlane, Self::WatchCoverage, Self::PublishedWrite,
        Self::PacketCopy, Self::FrozenStackBound, Self::ReadinessUnstable, Self::UiWorkPending, Self::FirstReadyMenu,
        Self::OwnedPumpSkipsPosthook, Self::StockPumpWord, Self::InitializerSkipsEd4afc, Self::StockInitializerPc,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::CanonicalInput => "canonical-input",
            Self::StopClock => "stop-clock",
            Self::RawAlias => "raw-alias",
            Self::KernelIdentity => "kernel-identity",
            Self::MissingInitialization => "missing-initialization",
            Self::NormalizerResult => "normalizer-result",
            Self::LostCalleeSave => "lost-callee-save",
            Self::RelabelledOlderSample => "relabelled-older-sample",
            Self::RenderReadiness => "render-readiness",
            Self::TruncatedPlane => "truncated-plane",
            Self::WatchCoverage => "watch-coverage",
            Self::PublishedWrite => "published-write",
            Self::PacketCopy => "packet-copy",
            Self::FrozenStackBound => "frozen-stack-bound",
            Self::ReadinessUnstable => "readiness-unstable",
            Self::UiWorkPending => "ui-work-pending",
            Self::FirstReadyMenu => "first-ready-menu",
            Self::OwnedPumpSkipsPosthook => "owned-pump-skips-posthook",
            Self::StockPumpWord => "stock-pump-word",
            Self::InitializerSkipsEd4afc => "initializer-skips-ed4afc",
            Self::StockInitializerPc => "stock-initializer-pc",
        }
    }

    pub fn expected(self) -> Expected {
        use ApplicationCheck as Check;
        Expected::Application(match self {
            Self::CanonicalInput => return Expected::Identity,
            Self::FirstReadyMenu => return Expected::Pair(PairField::ReadinessMenu),
            Self::StopClock | Self::RawAlias => Check::Mapping,
            Self::KernelIdentity => Check::TaskContext,
            Self::MissingInitialization => Check::Initializer,
            Self::NormalizerResult => Check::NormalizerResult,
            Self::LostCalleeSave | Self::RelabelledOlderSample => Check::Handback,
            Self::RenderReadiness | Self::TruncatedPlane => Check::RenderCompletion,
            Self::WatchCoverage => Check::CaptureBound,
            Self::PublishedWrite | Self::StockPumpWord | Self::StockInitializerPc => Check::CodeIdentity,
            Self::OwnedPumpSkipsPosthook => Check::HookSequence,
            Self::PacketCopy => Check::QueueProvenance,
            Self::FrozenStackBound => Check::StackContext,
            Self::ReadinessUnstable | Self::UiWorkPending | Self::InitializerSkipsEd4afc => Check::Readiness,
        })
    }

    /// Mutates one role's receipt and evidence, and its side files in `directory`, which must be that role's copy.
    pub fn apply(self, application: &Application, receipt: &mut Receipt, evidence: &mut Evidence, directory: &Path) -> Result<(), String> {
        match self {
            Self::CanonicalInput => {
                receipt.identity.update = Sha256::of(b"canonical-input control");
                return Ok(());
            }
            Self::UiWorkPending => {
                let ui = first_ready_ui(receipt, evidence)?;
                let view = ui.words.iter_mut().find(|view| physical(view.address.0) == PRIORITY_COUNT).ok_or("no priority-queue count view")?;
                return set_word(view, PRIORITY_COUNT, 1);
            }
            Self::FirstReadyMenu => {
                let ui = first_ready_ui(receipt, evidence)?;
                let level = ui.depth.word(MENU_LEVEL).ok_or("no level word")?;
                return set_word(&mut ui.depth, MENU_LEVEL, level.wrapping_add(1));
            }
            Self::StockPumpWord => {
                let entry = application.pump.entry.0;
                let sample = evidence.samples.iter_mut().find(|sample| stock_of(application, sample) == Some(entry)).ok_or("no pump entry sample")?;
                let pc = sample.pc.0;
                return set_word(&mut sample.code, pc, STOCK_PUMP_ENTRY);
            }
            Self::OwnedPumpSkipsPosthook => {
                let posthook = application.pump.calls[2];
                let entry = position(application, evidence, application.pump.entry.0)?;
                for stock in [posthook.site.0, posthook.continuation.0] {
                    let at = entry + evidence.samples[entry..].iter().position(|sample| stock_of(application, sample) == Some(stock))
                        .ok_or_else(|| format!("no sample at {stock:#x} after the pump entry"))?;
                    remove_sample(receipt, evidence, at);
                }
                return Ok(());
            }
            Self::InitializerSkipsEd4afc => {
                let at = position(application, evidence, 0xd985d8)?;
                remove_sample(receipt, evidence, at);
                return Ok(());
            }
            Self::StockInitializerPc => {
                let at = position(application, evidence, 0xd985d8)?;
                let sample = &mut evidence.samples[at];
                sample.pc = crate::composed_catalog::Pc(0xd985d8);
                sample.code.address = Guest(0xd985d8);
                return Ok(());
            }
            _ => {}
        }
        let section = receipt.application.as_mut().ok_or("the receipt has no application section")?;
        let samples = &mut evidence.samples;
        match self {
            Self::CanonicalInput | Self::UiWorkPending | Self::FirstReadyMenu | Self::OwnedPumpSkipsPosthook | Self::StockPumpWord
                | Self::InitializerSkipsEd4afc | Self::StockInitializerPc => {}
            Self::StopClock => samples.first_mut().ok_or("no sample")?.resume.0 += 1,
            Self::RawAlias => *samples.first_mut().ok_or("no sample")?.code.spellings[1][1].0.first_mut().ok_or("an empty view")? ^= 1,
            Self::KernelIdentity => {
                let entry = section.task1.ok_or("task1's entry was not observed")?.entry;
                let sample = samples.get_mut(entry.0 as usize).ok_or("task1's entry sample is absent")?;
                let core = sample.core.index();
                let model = CoreIdentities::of(sample.identities.map(|identity| identity.0)).ok_or("the baseline identities are not a recorded model")?;
                sample.identities[core] = CoreIdentity(model.words()[1 - core]);
            }
            Self::MissingInitialization => {
                let at = position(application, evidence, TASK1_RESET)?;
                remove_sample(receipt, evidence, at);
            }
            Self::NormalizerResult | Self::LostCalleeSave => {
                let at = position(application, evidence, HANDBACK)?;
                let registers = &mut evidence.samples[at].registers;
                if self == Self::NormalizerResult { registers.r[0].0 ^= 1 } else { registers.r[4].0 ^= 1 }
            }
            Self::RelabelledOlderSample => {
                let id = InputIndex(FIRST_SETTLED as u64);
                let pressed = section.inputs.get(FIRST_SETTLED).ok_or("no settled input")?.pressed;
                let handback = samples.iter().rposition(|sample| stock_of(application, sample) == Some(HANDBACK) && sample.stop < pressed)
                    .ok_or("no handback before the first settled input")?;
                let (core, sp) = (samples[handback].core, samples[handback].registers.sp);
                let entry = samples[..handback].iter()
                    .rposition(|sample| stock_of(application, sample) == Some(application.pump.entry.0) && sample.core == core && sample.registers.sp == sp)
                    .ok_or("no pump entry for that handback")?;
                for sample in samples.iter_mut().filter(|sample| sample.input == Some(id)) {
                    sample.input = None;
                }
                samples[entry].input = Some(id);
                samples[handback].input = Some(id);
            }
            Self::RenderReadiness => section.visible.first_mut().ok_or("no visible capture")?.probe.drained = false,
            Self::TruncatedPlane => {
                let visible = section.visible.first_mut().ok_or("no visible capture")?;
                let (scanout, planes) = planes::truncate(&visible.scanout.read(directory)?, &visible.planes.read(directory)?)?;
                visible.planes = SideFile::replace(directory, visible.planes.path.as_str(), &planes)?;
                visible.scanout = SideFile::replace(directory, visible.scanout.path.as_str(), &scanout)?;
            }
            Self::WatchCoverage => {
                let pump = application.code_range(CodeLabel::Pump).ok_or("no pump code range")?;
                section.watch_classes.retain(|class| !(class.start <= pump.start && pump.end <= class.end));
            }
            Self::PublishedWrite => {
                let publication = section.publication.ok_or("publication was not observed")?;
                let pump = application.code_range(CodeLabel::Pump).ok_or("no pump code range")?;
                let row = evidence.writes.iter().map(|write| write.row + 1).max().unwrap_or(0).max(publication.row);
                evidence.writes.push(ApplicationWrite {
                    row, source: WriteKind::CpuWrite, physical: pump.start, size: 4, core: Some(Core::Primary), pc: Some(PUMP_CALL),
                });
            }
            Self::PacketCopy => {
                let sample = samples.iter_mut().find(|sample| {
                    let destination = sample.registers.r[5].0;
                    stock_of(application, sample) == Some(RELEASE_CALL)
                        && view_word(&sample.packet, destination.wrapping_add(4)).is_some_and(|length| length > 0)
                        && view_word(&sample.packet, destination.wrapping_add(8))
                            .is_some_and(|payload| sample.packet.iter().any(|view| physical(view.address.0) == physical(payload)))
                }).ok_or("no release of a nonempty copied payload")?;
                let payload = view_word(&sample.packet, sample.registers.r[5].0.wrapping_add(8)).ok_or("no copied payload")?;
                let view = sample.packet.iter_mut().find(|view| physical(view.address.0) == physical(payload)).ok_or("no copied payload view")?;
                for bytes in std::iter::once(&mut view.physical).chain(view.spellings.iter_mut().flatten()) {
                    *bytes.0.first_mut().ok_or("an empty view")? ^= 1;
                }
            }
            Self::FrozenStackBound => {
                let scope = section.scope.as_mut().ok_or("no stack scope")?;
                let config = scope.summary.config;
                let mut stack = config.stack();
                stack[0] = Guest(stack[0].0 + 8);
                scope.summary.config = TraceStackScopeConfig::new(stack, config.owner(), config.current())?;
            }
            Self::ReadinessUnstable => unstable(application, receipt, evidence)?,
        }
        Ok(())
    }
}

/// A kernel return for the first-ready receive one instruction after first-ready, inside its settle interval, with
/// that receive's later readiness samples removed.
fn unstable(application: &Application, receipt: &mut Receipt, evidence: &mut Evidence) -> Result<(), String> {
    let section = receipt.application.as_ref().ok_or("the receipt has no application section")?;
    let first = section.first_ready.ok_or("no first-ready")?;
    let task1 = section.task1.ok_or("task1's entry was not observed")?;
    let stop = first.instructions.0 + 1;
    let at = evidence.samples.iter().position(|sample| sample.stop.0 > stop).unwrap_or(evidence.samples.len());
    let call = evidence.samples[..at].iter()
        .rfind(|sample| sample.index > first.receive && sample.core == task1.core && stock_of(application, sample) == Some(KERNEL_RECEIVE))
        .ok_or("no kernel receive call for the first-ready receive")?.clone();
    let code = evidence.samples.iter().find(|sample| stock_of(application, sample) == Some(KERNEL_RETURN))
        .ok_or("no kernel return sample to take a code view from")?.code.clone();
    let site = application.queue_sites.iter().find(|site| site.pc.0 == KERNEL_RETURN).ok_or("no kernel return queue site")?;
    let previous = at.checked_sub(1).map_or([Instructions(0); 2], |index| evidence.samples[index].counts);
    let next = evidence.samples.get(at).map(|sample| sample.counts);
    let first_core = next.map_or(u64::MAX, |counts| counts[0].0).min(stop - previous[1].0);
    let sample = Sample {
        index: SampleIndex(at as u64), site: SiteRef::Queue(site.label.clone()), pc: site.pc, core: task1.core, stop: Instructions(stop),
        resume: Instructions(stop + 1), counts: [Instructions(first_core), Instructions(stop - first_core)], identities: call.identities,
        registers: call.registers, code, task: call.task, stack: call.stack, state: call.state, packet: Vec::new(), input: call.input,
    };
    insert_sample(receipt, evidence, at, sample);
    evidence.readiness.retain(|readiness| readiness.receive != first.receive || readiness.instructions.0 <= stop);
    for (index, readiness) in evidence.readiness.iter_mut().enumerate() {
        readiness.index = ReadinessIndex(index as u64);
    }
    let scope = receipt.application.as_mut().and_then(|section| section.scope.as_mut()).ok_or("no stack scope")?;
    let census = scope.summary.sites.iter_mut().find(|entry| entry.site.0 == site.pc.0).ok_or("the kernel return is not a scope site")?;
    census.recorded[task1.core.index()].0 += 1;
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct ControlRow {
    pub control: Control,
    pub expected: Expected,
    pub actual: Option<Difference>,
    pub accepted: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ControlReport {
    pub contract: Sha256,
    pub rows: Vec<ControlRow>,
    pub passed: bool,
}

fn receipt_at(path: &Path) -> Result<Receipt, String> {
    serde_json::from_slice(&crate::bridge::read(path)?).map_err(|error| format!("{}: {error}", path.display()))
}

/// Links every file of `source` into the new directory `target`.
fn link(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|error| format!("{}: {error}", target.display()))?;
    for entry in std::fs::read_dir(source).map_err(|error| format!("{}: {error}", source.display()))? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_type().map_err(|error| error.to_string())?.is_file() {
            std::os::unix::fs::symlink(entry.path(), target.join(entry.file_name())).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Runs every control against `run`'s `retained/` and `composed/` receipts, writing each mutated composed copy under
/// `output/<control>/composed`.
pub fn run_controls(contract: &Contract, digest: &Sha256, run: &Path, output: &Path) -> Result<ControlReport, String> {
    let application = contract.application().ok_or("the contract has no application section")?;
    let run = std::fs::canonicalize(run).map_err(|error| format!("{}: {error}", run.display()))?;
    let (retained_directory, owned_directory) = (run.join("retained"), run.join("composed"));
    let retained = receipt_at(&retained_directory.join(RECEIPT_FILE))?;
    let owned = receipt_at(&owned_directory.join(RECEIPT_FILE))?;
    let baseline = compare(contract, digest, &retained, &owned, &CaptureDirectories { retained: &retained_directory, owned: &owned_directory });
    if baseline.verdict != VerdictKind::Equal {
        return Err(format!("the baseline pair differs: {:?}", baseline.first_difference));
    }
    let section = owned.application.as_ref().ok_or("the composed receipt has no application section")?;
    let evidence = read_evidence(&owned_directory, &section.evidence)?;
    std::fs::create_dir(output).map_err(|error| format!("{}: {error}", output.display()))?;
    let mut rows = Vec::with_capacity(Control::ALL.len());
    for control in Control::ALL {
        let directory = output.join(control.name()).join("composed");
        link(&owned_directory, &directory)?;
        let (mut receipt, mut evidence) = (owned.clone(), evidence.clone());
        control.apply(application, &mut receipt, &mut evidence, &directory)?;
        if let Some(section) = receipt.application.as_mut() {
            section.evidence = write_evidence(&directory, section.evidence.path.as_str(), &evidence)?;
            (section.samples, section.readiness, section.writes) = (evidence.samples.len() as u64, evidence.readiness.len() as u64, evidence.writes.len() as u64);
        }
        let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
        SideFile::replace(&directory, RECEIPT_FILE, &bytes)?;
        let mutated = receipt_at(&directory.join(RECEIPT_FILE))?;
        let verdict = compare(contract, digest, &retained, &mutated, &CaptureDirectories { retained: &retained_directory, owned: &directory });
        let expected = control.expected();
        let accepted = verdict.first_difference.as_ref().is_some_and(|difference| expected.matches(difference));
        rows.push(ControlRow { control, expected, actual: verdict.first_difference, accepted });
    }
    let passed = rows.iter().all(|row| row.accepted);
    Ok(ControlReport { contract: digest.clone(), rows, passed })
}
