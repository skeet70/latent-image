//! The per-role application checks of `--compare`, in the order they run, and the cross-role check.
use super::planes;
use super::readiness::{
    render_ready, Search, ADAPTER_RECEIVE_RETURN, CHAIN, IMAGE5_PUBLICATION, KERNEL_RETURN, QUEUE_ENTRY, RECEIVE_CALL, RECEIVE_RETURN,
};
use super::{
    read_evidence, view_byte, view_word, ApplicationReceipt, Evidence, GraphicsState, InputIndex, InputPurpose, RawView, Sample,
    SampleIndex, SiteRef, ALIAS,
};
use crate::composed_catalog::{physical_address as physical, Application, Core, Domain, Instructions, Role, SiteKind};
use crate::guest::RouteKey;
use crate::provenance::Guest;
use crate::render::Probe;
use crate::run::CoreIdentities;
use crate::trace::TraceStackScopeConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

const TASK1_ENTRY: u32 = CHAIN[0];
pub(crate) const TASK1_RESET: u32 = CHAIN[4];
const TASK1_STACK_BYTES: u32 = 0xc00;
/// Task1's priority 6 as its creation record stores it.
const TASK1_PRIORITY: u32 = 5;
/// The first-entry LR per domain: the kernel's task trampoline.
const TRAMPOLINE_RETURNS: [u32; 2] = [0x453f0, 0xd53b0];
const SVC_MODE: u32 = 0x13;
/// Controller state task1 zeroes before `0xc22f74`.
const STATE: u32 = 0x1467588;
const STATE_BYTES: u32 = 348;
const LOOP_HEAD: u32 = 0xc22f84;
pub(crate) const PUMP_CALL: u32 = 0xc23274;
pub(crate) const HANDBACK: u32 = 0xc23278;
pub(crate) const ADAPTER_ENTRY: u32 = 0xdb5124;
const ADAPTER_RECEIVE_CALL: u32 = 0xdb5170;
const ADAPTER_COPY: u32 = 0xdb5178;
const ADAPTER_RETURN: u32 = 0xdb51d0;
/// The adapter's selector for a blocking receive.
const BLOCKING_SELECTOR: u32 = 0x107;
pub(crate) const KERNEL_RECEIVE: u32 = 0x725378;
pub(crate) const RECEIVED: u32 = 0x7253c0;
pub(crate) const COPY_CALL: u32 = 0x7253f8;
pub(crate) const COPY_RETURN: u32 = 0x7253fc;
pub(crate) const MEMCPY: u32 = 0x722054;
pub(crate) const RELEASE_CALL: u32 = 0x725414;
pub(crate) const RELEASE_RETURN: u32 = 0x725418;
pub(crate) const QUEUE_EXIT: u32 = 0x72541c;
/// The interface root cells per domain and the roots they must hold.
const QUEUE_ROOT_CELLS: u32 = 0x96bd20;
pub(crate) const QUEUE_ROOTS: [u32; 2] = [0x4004dae0, 0x400ddaa0];
const RECEIVE_SLOT: u32 = 0xc8;
const RELEASE_SLOT: u32 = 0xf8;
/// The word whose value is queue1's routed domain field.
const QUEUE_DOMAIN: u32 = 0x7cad9c;
pub(crate) const PAYLOAD_LIMIT: u32 = 76;
pub(crate) const PRESS: u64 = 8_000_000;
/// Inputs before the settled inputs: twelve Up, OK, OK and MENU.
pub(crate) const FIRST_SETTLED: usize = 15;
const ROUTE: [(RouteKey, InputPurpose); 20] = {
    use InputPurpose::{AfterBack, BackHandback, FirstStart, Ordinary, UnhandledProbe};
    use RouteKey::{Back, Down, Left, Ok, Up};
    [
        (Up, FirstStart), (Up, FirstStart), (Up, FirstStart), (Up, FirstStart), (Up, FirstStart), (Up, FirstStart),
        (Up, FirstStart), (Up, FirstStart), (Up, FirstStart), (Up, FirstStart), (Up, FirstStart), (Up, FirstStart),
        (Ok, FirstStart), (Ok, FirstStart), (Ok, FirstStart),
        (Down, Ordinary), (Up, Ordinary), (Back, BackHandback), (Ok, AfterBack), (Left, UnhandledProbe),
    ]
};
const VISIBLE: usize = 6;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplicationCheck {
    Mapping,
    CodeIdentity,
    TaskContext,
    StackContext,
    Initializer,
    Readiness,
    QueueProvenance,
    HookSequence,
    Handback,
    NormalizerResult,
    RenderCompletion,
    /// Input keys, purposes and times.
    Route,
    CaptureBound,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApplicationRefusal {
    pub check: ApplicationCheck,
    pub detail: String,
}

impl ApplicationRefusal {
    pub fn new(check: ApplicationCheck, detail: impl Into<String>) -> Self { Self { check, detail: detail.into() } }
}

fn require(value: bool, check: ApplicationCheck, detail: &str) -> Result<(), ApplicationRefusal> {
    if value { Ok(()) } else { Err(ApplicationRefusal::new(check, detail)) }
}

/// One role's application section with its decoded evidence and the directory of its side files.
pub struct RoleEvidence<'a> {
    pub application: &'a Application,
    pub role: Role,
    pub receipt: &'a ApplicationReceipt,
    pub evidence: &'a Evidence,
    pub directory: &'a Path,
}

pub fn check_role(application: &Application, role: Role, receipt: &ApplicationReceipt, directory: &Path) -> Result<(), ApplicationRefusal> {
    let evidence = read_evidence(directory, &receipt.evidence).map_err(|error| ApplicationRefusal::new(ApplicationCheck::CaptureBound, error))?;
    check_evidence(&RoleEvidence { application, role, receipt, evidence: &evidence, directory })
}

pub fn check_evidence(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    mapping(evidence)?;
    code_identity(evidence)?;
    task_context(evidence)?;
    stack_context(evidence)?;
    initializer(evidence)?;
    readiness(evidence)?;
    queue_provenance(evidence)?;
    pump_sequences(evidence)?;
    render_completion(evidence)?;
    route(evidence)?;
    capture_bound(evidence)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Site(SiteKind),
    Queue,
    Target,
}

/// A sample with its stock PC.
#[derive(Clone, Copy)]
pub(crate) struct Located<'a> {
    pub(crate) sample: &'a Sample,
    pub(crate) stock: u32,
    pub(crate) kind: Kind,
}

impl Located<'_> {
    pub(crate) fn scoped(&self) -> bool { matches!(self.kind, Kind::Queue | Kind::Target) }
    pub(crate) fn attributed(&self) -> bool { self.scoped() || matches!(self.kind, Kind::Site(kind) if kind.task1()) }
    /// Every application-site sample and the queue entry, where task1's SP is 8-byte aligned.
    fn aligned(&self) -> bool { !self.scoped() || self.stock == QUEUE_ENTRY }
}

pub(crate) fn locate(application: &Application, sample: &Sample) -> Option<(u32, Kind)> {
    match &sample.site {
        SiteRef::Application(label) => application.site(label).map(|site| (site.retained_pc.0, Kind::Site(site.kind))),
        SiteRef::Queue(label) => application.queue(label).map(|site| (site.pc.0, Kind::Queue)),
        SiteRef::Target => Some((physical(sample.pc.0), Kind::Target)),
    }
}

fn located<'a>(evidence: &RoleEvidence<'a>) -> Result<Vec<Located<'a>>, ApplicationRefusal> {
    evidence.evidence.samples.iter().map(|sample| {
        let (stock, kind) = locate(evidence.application, sample)
            .ok_or_else(|| ApplicationRefusal::new(ApplicationCheck::CodeIdentity, "sample names no contract site"))?;
        Ok(Located { sample, stock, kind })
    }).collect()
}

fn sample_at<'a>(evidence: &RoleEvidence<'a>, index: SampleIndex) -> Option<&'a Sample> {
    usize::try_from(index.0).ok().and_then(|index| evidence.evidence.samples.get(index))
}

/// Task1's committed stack, physical and inclusive of its end.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Frame {
    pub(crate) domain: Domain,
    pub(crate) base: u32,
    pub(crate) end: u32,
}

impl Frame {
    pub(crate) fn holds(&self, sp: u32) -> bool { (self.base..=self.end).contains(&physical(sp)) }
}

/// Task1's committed context at `sample`: current task, creation record, priority, identity, domain, mode, stack
/// bounds and, at its entry, the trampoline's registers.
pub(crate) fn committed(application: &Application, stock: u32, sample: &Sample, aligned: bool) -> Result<Frame, String> {
    if CoreIdentities::of(sample.identities.map(|identity| identity.0)).is_none() {
        return Err("core identities are not one recorded model".into());
    }
    let domain = sample.identities[sample.core.index()].domain().ok_or("unknown kernel identity")?;
    let layout = application.layout(domain);
    let at = |address: u32| view_word(&sample.task, address).ok_or_else(|| format!("task word {address:#x} is not viewed"));
    let creation = layout.creation.0;
    let field = |offset: u32| at(creation + offset);
    let registers = &sample.registers;
    if physical(at(layout.current.0)?) != physical(layout.task.0) {
        return Err("the executing core has another current task".into());
    }
    if field(8)? != TASK1_ENTRY || field(4)? != 0 || field(12)? != TASK1_PRIORITY || field(16)? != TASK1_STACK_BYTES || field(36)? != 1
        || at(layout.ids.0)? != field(32)? || physical(at(layout.task.0 + 8)?) != physical(creation)
    {
        return Err("task1's creation, task, id or priority binding differs".into());
    }
    if field(32)? & 0xffff != 1 || at(layout.task.0 + 32)? != domain.index() as u32 + 1 || registers.cpsr.0 & 0x3f != SVC_MODE {
        return Err("task1's identity, domain or mode differs".into());
    }
    let base = field(20)?;
    let end = base.checked_add(field(16)?).ok_or("task1's stack overflows")?;
    let frame = Frame { domain, base: physical(base), end: physical(end) };
    let sp = physical(registers.sp.0);
    if base & 7 != 0 || !frame.holds(sp) || (aligned && sp & 7 != 0) {
        return Err("task1's live stack is outside its committed allocation".into());
    }
    if stock == TASK1_ENTRY
        && (sp != frame.end || registers.r[0].0 != 0 || physical(registers.r[1].0) != physical(layout.task.0)
            || physical(registers.r[2].0) != physical(creation) || physical(registers.lr.0) != TRAMPOLINE_RETURNS[domain.index()])
    {
        return Err("task1's first entry differs from its committed trampoline context".into());
    }
    Ok(frame)
}

/// Samples on task1's core inside its committed stack.
fn task1_stream<'a>(evidence: &RoleEvidence<'a>, check: ApplicationCheck, keep: impl Fn(&Located) -> bool)
    -> Result<Vec<Located<'a>>, ApplicationRefusal> {
    let task1 = evidence.receipt.task1.ok_or_else(|| ApplicationRefusal::new(check, "task1's entry was not observed"))?;
    let entry = sample_at(evidence, task1.entry).ok_or_else(|| ApplicationRefusal::new(check, "task1's entry sample is absent"))?;
    let frame = committed(evidence.application, TASK1_ENTRY, entry, true).map_err(|detail| ApplicationRefusal::new(check, detail))?;
    Ok(located(evidence)?.into_iter()
        .filter(|located| located.sample.core == task1.core && frame.holds(located.sample.registers.sp.0) && keep(located))
        .collect())
}

/// Clocks, indices and every view's spellings.
pub fn mapping(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    let check = ApplicationCheck::Mapping;
    let samples = &evidence.evidence.samples;
    let mut previous: Option<&Sample> = None;
    for (index, sample) in samples.iter().enumerate() {
        require(sample.index.0 == index as u64, check, "sample indices are not contiguous")?;
        require(sample.stop.0.checked_add(1) == Some(sample.resume.0), check, "resume is not the stop plus one")?;
        require(sample.counts[0].0.checked_add(sample.counts[1].0) == Some(sample.stop.0), check, "core counts do not sum to the stop")?;
        if let Some(previous) = previous {
            require(sample.stop >= previous.stop && sample.counts[0] >= previous.counts[0] && sample.counts[1] >= previous.counts[1], check,
                "stop clocks run backwards")?;
        }
        require(sample.views().all(RawView::agrees), check, "a view's spellings differ from its physical bytes")?;
        require(physical(sample.code.address.0) == physical(sample.pc.0) && sample.code.physical.0.len() == 4, check,
            "the code view is not the stopped word")?;
        previous = Some(sample);
    }
    let (mut preceding, mut at) = (0, Instructions(0));
    for (index, sample) in evidence.evidence.readiness.iter().enumerate() {
        require(sample.index.0 == index as u64 && sample.preceding >= preceding && sample.preceding <= samples.len() as u64
            && sample.instructions >= at, check, "readiness samples are out of order")?;
        if let Some(last) = sample.preceding.checked_sub(1).and_then(|last| samples.get(last as usize)) {
            require(sample.instructions >= last.stop, check, "a readiness sample precedes the sample before it")?;
        }
        let render = &sample.render;
        let views = [&render.initialized.view, &render.ui_busy.view].into_iter().chain(&render.idle)
            .chain(render.keys.iter().map(|key| &key.view)).chain(sample.tasks.views());
        require(views.into_iter().all(RawView::agrees), check, "a readiness view's spellings differ from its physical bytes")?;
        (preceding, at) = (sample.preceding, sample.instructions);
    }
    require(evidence.receipt.visible.iter().all(|visible| visible.controller.agrees()), check, "a visible capture's view spellings differ")
}

/// Code words at every sample, no application code write after publication, and the code digests.
pub fn code_identity(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    let check = ApplicationCheck::CodeIdentity;
    let (application, receipt) = (evidence.application, evidence.receipt);
    for located in located(evidence)? {
        let sample = located.sample;
        let (word, armed) = match &sample.site {
            SiteRef::Application(label) => {
                let site = application.site(label).ok_or_else(|| ApplicationRefusal::new(check, "unknown site"))?;
                (site.word, site.pc(evidence.role))
            }
            SiteRef::Queue(label) => {
                let site = application.queue(label).ok_or_else(|| ApplicationRefusal::new(check, "unknown queue site"))?;
                (site.word, site.pc)
            }
            SiteRef::Target => {
                let target = receipt.targets.iter().find(|target| target.pc.0 == located.stock)
                    .ok_or_else(|| ApplicationRefusal::new(check, "a sample stopped at an unarmed target"))?;
                (target.word, target.pc)
            }
        };
        require(physical(sample.pc.0) == physical(armed.0), check, "a sample's PC is not its site's PC in the role")?;
        require(sample.code.word(sample.pc.0) == Some(word.0), check, "a stopped instruction differs from the contract word")?;
    }
    let sites: BTreeSet<u32> = application.sites.iter().map(|site| physical(site.pc(evidence.role).0))
        .chain(application.queue_sites.iter().map(|site| physical(site.pc.0))).collect();
    for target in &receipt.targets {
        require(target.pc.0 % 4 == 0 && !sites.contains(&physical(target.pc.0)) && sample_at(evidence, target.sample).is_some(), check,
            "a dynamic target is not an independent aligned entry")?;
    }
    let publication = receipt.publication.ok_or_else(|| ApplicationRefusal::new(check, "image5 publication was not observed"))?;
    let published = sample_at(evidence, publication.sample)
        .and_then(|sample| locate(application, sample)).is_some_and(|(stock, _)| stock == IMAGE5_PUBLICATION);
    require(published, check, "the publication names another sample")?;
    if let Some(write) = evidence.evidence.writes.iter().find(|write| write.row >= publication.row) {
        return Err(ApplicationRefusal::new(check, format!("{:?} at {:#x} wrote application code after publication", write.source, write.physical)));
    }
    for range in &application.code {
        let observed = receipt.code.iter().find(|observed| observed.label == range.label)
            .ok_or_else(|| ApplicationRefusal::new(check, "a code range was not read"))?;
        require(observed.publication.as_ref() == Some(range.sha256(evidence.role)) && &observed.end == range.sha256(evidence.role), check,
            "an application code digest differs from the contract")?;
    }
    Ok(())
}

/// Task1's committed context at its entry and at every task1, adapter, pump, hook-target and scoped sample.
pub fn task_context(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    let check = ApplicationCheck::TaskContext;
    let located = located(evidence)?;
    let first = located.iter().find(|located| located.attributed()).map(|located| located.sample.identities);
    for located in located.iter().filter(|located| located.attributed()) {
        require(Some(located.sample.identities) == first, check, "core identities change within the receipt")?;
        committed(evidence.application, located.stock, located.sample, located.aligned())
            .map_err(|detail| ApplicationRefusal::new(check, format!("sample {}: {detail}", located.sample.index.0)))?;
    }
    let task1 = evidence.receipt.task1.ok_or_else(|| ApplicationRefusal::new(check, "task1's entry was not observed"))?;
    let entry = located.iter().find(|located| located.sample.index == task1.entry)
        .ok_or_else(|| ApplicationRefusal::new(check, "task1's entry sample is absent"))?;
    let domain = entry.sample.identities[entry.sample.core.index()].domain();
    require(entry.stock == TASK1_ENTRY && entry.sample.core == task1.core && domain == Some(task1.domain), check,
        "the recorded task1 entry differs from its sample")
}

/// The frozen scope against task1's stack and current cells, its violation and its site census.
pub fn stack_context(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    let check = ApplicationCheck::StackContext;
    let refuse = |detail: String| ApplicationRefusal::new(check, detail);
    let application = evidence.application;
    let task1 = evidence.receipt.task1.ok_or_else(|| refuse("task1's entry was not observed".into()))?;
    let entry = sample_at(evidence, task1.entry).ok_or_else(|| refuse("task1's entry sample is absent".into()))?;
    let frame = committed(application, TASK1_ENTRY, entry, true).map_err(refuse)?;
    let scope = evidence.receipt.scope.as_ref().ok_or_else(|| refuse("no stack scope was frozen".into()))?;
    let current = Domain::ALL.map(|domain| Guest(physical(application.layout(domain).current.0)));
    let expected = TraceStackScopeConfig::new([Guest(frame.base), Guest(frame.end)], Guest(physical(application.layout(frame.domain).task.0)), current)
        .map_err(refuse)?;
    require(scope.summary.config == expected, check, "the frozen scope differs from task1's stack and current cells")?;
    require(scope.summary.violation.is_none(), check, "the stack scope refused")?;
    let added: BTreeSet<u32> = scope.sites.iter().map(|site| site.0).collect();
    let census: BTreeSet<u32> = scope.summary.sites.iter().map(|site| site.site.0).collect();
    require(added.len() == scope.sites.len() && added == census, check, "the scope's site census differs from the sites added")?;
    let expected_sites: BTreeSet<u32> = application.queue_sites.iter().map(|site| site.pc.0)
        .chain(evidence.receipt.targets.iter().map(|target| target.pc.0))
        .flat_map(|pc| [physical(pc), physical(pc) | ALIAS]).collect();
    require(added == expected_sites, check, "the scope's sites are not the queue sites and targets in both spellings")?;
    let located = located(evidence)?;
    for site in &scope.summary.sites {
        for core in Core::ALL {
            let samples = located.iter()
                .filter(|located| located.scoped() && located.sample.pc.0 == site.site.0 && located.sample.core == core).count() as u64;
            require(site.recorded[core.index()].0 == samples, check, "scoped hits differ from the scoped samples")?;
        }
    }
    let outside: u64 = scope.summary.sites.iter().flat_map(|site| site.outside).map(|count| count.0).sum();
    require(outside == scope.skips, check, "drained skips differ from the scope's outside counts")
}

/// Entry, setup, `0xc22f70`, `0xc22bd0`, `0xc22f74` once each in order on task1's core, with the controller state
/// zero at `0xc22f74`.
pub fn initializer(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    let check = ApplicationCheck::Initializer;
    let task1 = evidence.receipt.task1.ok_or_else(|| ApplicationRefusal::new(check, "task1's entry was not observed"))?;
    let located = located(evidence)?;
    let chain: Vec<&Located> = located.iter()
        .filter(|located| located.sample.core == task1.core && matches!(located.kind, Kind::Site(_)) && CHAIN.contains(&located.stock))
        .collect();
    require(chain.iter().map(|located| located.stock).eq(CHAIN), check, "the initialization chain is not each site once in order")?;
    let reset = chain.iter().find(|located| located.stock == TASK1_RESET).map(|located| located.sample)
        .ok_or_else(|| ApplicationRefusal::new(check, "task1's state reset is absent"))?;
    require((0..STATE_BYTES).step_by(4).all(|offset| view_word(&reset.state, STATE + offset) == Some(0)), check,
        "the controller state is not zero at its reset")
}

/// The first-ready search replayed over the recorded samples, with every conjunct recomputed from views.
pub fn readiness(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    let check = ApplicationCheck::Readiness;
    let located = located(evidence)?;
    let mut search = Search::new(evidence.application.readiness);
    let mut fed = 0usize;
    for sample in &evidence.evidence.readiness {
        require(sample.probe.drained == render_ready(sample) && sample.probe.graphics == sample.graphics.graphics(), check,
            "a recorded probe differs from its views")?;
        let until = usize::try_from(sample.preceding).ok().filter(|&until| (fed..=located.len()).contains(&until))
            .ok_or_else(|| ApplicationRefusal::new(check, "readiness samples are out of order"))?;
        for located in &located[fed..until] {
            search.sample(located.stock, located.sample);
        }
        fed = until;
        search.readiness(sample).map_err(|fault| ApplicationRefusal::new(check, format!("readiness sample {}: {fault:?}", sample.index.0)))?;
    }
    for located in &located[fed..] {
        search.sample(located.stock, located.sample);
    }
    let recorded = evidence.receipt.first_ready.ok_or_else(|| ApplicationRefusal::new(check, "no stable first-ready was found"))?;
    require(search.first_ready() == Some(recorded), check, "the recorded first-ready is not the first stable candidate")?;
    require(recorded.instructions < evidence.application.readiness.end, check, "first-ready is not before the search end")
}

fn scalar(views: &[RawView], address: u32) -> Result<u32, ApplicationRefusal> {
    view_word(views, address).ok_or_else(|| ApplicationRefusal::new(ApplicationCheck::QueueProvenance, format!("word {address:#x} is not viewed")))
}

fn bytes(views: &[RawView], address: u32, length: usize) -> Result<Vec<u8>, ApplicationRefusal> {
    (0..length).map(|offset| {
        let address = address.checked_add(offset as u32)
            .ok_or_else(|| ApplicationRefusal::new(ApplicationCheck::QueueProvenance, "packet extent overflows"))?;
        view_byte(views, address).ok_or_else(|| ApplicationRefusal::new(ApplicationCheck::QueueProvenance, format!("byte {address:#x} is not viewed")))
    }).collect()
}

/// The kernel slot a queue service call must take: the root of the executing domain and `0x7cad9c`'s routed identity.
fn service(sample: &Sample, slot: u32) -> Result<u32, ApplicationRefusal> {
    let check = ApplicationCheck::QueueProvenance;
    let registers = &sample.registers;
    let domain = match registers.r[6].0 {
        0 => Domain::A,
        1 => Domain::B,
        _ => return Err(ApplicationRefusal::new(check, "the kernel selector is outside two domains")),
    };
    require(sample.identities[sample.core.index()].domain() == Some(domain), check, "the queue kernel selector differs from the executing domain")?;
    let root = scalar(&sample.state, QUEUE_ROOT_CELLS + 4 * domain.index() as u32)?;
    require(root == QUEUE_ROOTS[domain.index()], check, "the queue interface root differs")?;
    let target = scalar(&sample.state, root + slot)?;
    require(target & 3 == 0 && target == registers.r[3].0, check, "the queue's indirect call differs from its committed slot")?;
    require(registers.r[0].0 == 1 | scalar(&sample.state, QUEUE_DOMAIN)? << 16, check, "the routed queue1 identity differs")?;
    Ok(target)
}

struct Adapter<'a> {
    before: &'a Sample,
    before_stock: u32,
    entry: Option<&'a Sample>,
    receive_call: Option<&'a Sample>,
    inner: Option<&'a Sample>,
    exit: Option<&'a Sample>,
    status: Option<u32>,
    copied: bool,
    returned: bool,
}

/// Each task1 receive through the queue1 adapter; the pairs are the inner queue entry and exit.
fn adapters(stream: &[Located]) -> Result<Vec<(SampleIndex, SampleIndex)>, ApplicationRefusal> {
    let check = ApplicationCheck::QueueProvenance;
    let mut active: Option<Adapter> = None;
    let mut linked = Vec::new();
    for located in stream {
        let (pc, sample) = (located.stock, located.sample);
        let registers = &sample.registers;
        if matches!(pc, 0xc22fd4 | RECEIVE_CALL) {
            require(active.is_none(), check, "task1 receive reentered before its adapter returned")?;
            active = Some(Adapter {
                before: sample, before_stock: pc, entry: None, receive_call: None, inner: None, exit: None, status: None, copied: false, returned: false,
            });
            continue;
        }
        let Some(adapter) = active.as_mut() else {
            let boundary = matches!(pc, ADAPTER_ENTRY | 0xdb515c | ADAPTER_RECEIVE_CALL | ADAPTER_RECEIVE_RETURN | ADAPTER_COPY | ADAPTER_RETURN | 0xc22fd8 | RECEIVE_RETURN);
            require(!boundary, check, "an adapter boundary has no task1 caller")?;
            continue;
        };
        if pc == ADAPTER_ENTRY {
            require(adapter.entry.is_none() && registers.sp == adapter.before.registers.sp && registers.r == adapter.before.registers.r
                && physical(registers.lr.0) == physical(adapter.before.pc.0) + 4, check, "the adapter entry differs from its call")?;
            require(scalar(&sample.packet, registers.r[0].0)? == BLOCKING_SELECTOR, check, "task1 did not pass the blocking selector")?;
            adapter.entry = Some(sample);
        }
        let Some(entry) = adapter.entry else { continue };
        let frame = entry.registers.sp.0.checked_sub(32).ok_or_else(|| ApplicationRefusal::new(check, "the adapter frame underflows"))?;
        if pc == ADAPTER_RECEIVE_CALL {
            require(adapter.receive_call.is_none() && adapter.inner.is_none() && registers.sp.0 == frame && registers.r[0].0 == 1
                && Some(registers.r[1].0) == entry.registers.sp.0.checked_sub(28) && registers.r[2].0 == u32::MAX, check,
                "the adapter's blocking queue1 call changed")?;
            adapter.receive_call = Some(sample);
        }
        if pc == QUEUE_ENTRY {
            require(adapter.inner.is_none() && registers.sp.0 == frame && physical(registers.lr.0) == ADAPTER_RECEIVE_RETURN, check,
                "the adapter's inner queue1 call is absent or changed")?;
            let call = adapter.receive_call.ok_or_else(|| ApplicationRefusal::new(check, "queue1 entry lacks the adapter's receive call"))?;
            require(registers.r == call.registers.r && registers.sp == call.registers.sp, check, "the adapter-to-queue1 transfer differs")?;
            adapter.inner = Some(sample);
        }
        if pc == QUEUE_EXIT {
            let inner = adapter.inner.ok_or_else(|| ApplicationRefusal::new(check, "the queue1 exit has no entry"))?;
            require(adapter.exit.is_none() && registers.r[11].0.checked_add(4) == Some(inner.registers.sp.0), check,
                "the queue1 exit has another frame")?;
            adapter.exit = Some(sample);
            linked.push((inner.index, sample.index));
        }
        if matches!(pc, 0xdb515c | ADAPTER_RECEIVE_RETURN) {
            let inner = adapter.inner.ok_or_else(|| ApplicationRefusal::new(check, "the adapter's receive return hides its queue1 entry"))?;
            require(adapter.status.is_none() && pc == ADAPTER_RECEIVE_RETURN && registers.sp == inner.registers.sp
                && registers.r[4..12] == inner.registers.r[4..12], check, "the adapter's receive return differs from its call")?;
            let exit = adapter.exit.ok_or_else(|| ApplicationRefusal::new(check, "the adapter's receive return hides its queue1 exit"))?;
            require(registers.r[0] == exit.registers.r[0], check, "the adapter changed the queue1 status")?;
            adapter.status = Some(registers.r[0].0);
        }
        if pc == ADAPTER_COPY {
            require(adapter.status == Some(registers.r[5].0) && !adapter.copied, check, "the adapter copy lacks one receive return")?;
            adapter.copied = true;
        }
        if pc == ADAPTER_RETURN {
            require(adapter.copied && !adapter.returned && adapter.status == Some(registers.r[5].0), check, "the adapter returns before its receive and copy")?;
            adapter.returned = true;
        }
        if matches!(pc, 0xc22fd8 | RECEIVE_RETURN) {
            require(adapter.returned && pc == adapter.before_stock + 4 && registers.sp == adapter.before.registers.sp
                && registers.r[4..12] == adapter.before.registers.r[4..12] && adapter.status == Some(registers.r[0].0), check,
                "task1's receive handback differs from its adapter lifecycle")?;
            active = None;
        }
    }
    Ok(linked)
}

struct Call<'a> {
    before: &'a Sample,
    target: u32,
    continuation: u32,
    entered: bool,
}

struct Receive<'a> {
    entry: &'a Sample,
    destination: u32,
    call: Option<Call<'a>>,
    status: Option<u32>,
    packet: Option<u32>,
    payload: Vec<u8>,
    copied: bool,
    release: bool,
    released: bool,
}

/// Each queue1 receive's entry, service call, status and, on success, envelope, copy, release and zero exit, linked
/// to its adapter; at least two successes.
pub fn queue_provenance(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    let check = ApplicationCheck::QueueProvenance;
    let refuse = |detail: &str| ApplicationRefusal::new(check, detail);
    let stream = task1_stream(evidence, check, |located| located.scoped() || matches!(located.kind, Kind::Site(SiteKind::Task1 | SiteKind::Adapter)))?;
    let linked = adapters(&stream)?;
    let mut active: Vec<Receive> = Vec::new();
    let mut completed = BTreeSet::new();
    let mut successful = 0usize;
    for located in &stream {
        let (pc, sample) = (located.stock, located.sample);
        let registers = &sample.registers;
        if pc == QUEUE_ENTRY && located.scoped() {
            require(registers.r[0].0 == 1 && registers.r[2].0 == u32::MAX && !active.iter().any(|receive| receive.entry.registers.sp == registers.sp),
                check, "the blocking queue1 entry or its context differs")?;
            let destination = registers.r[1].0;
            require(sample.packet.iter().any(|view| physical(view.address.0) == physical(destination) && view.physical.0.len() == 12), check,
                "the destination envelope was not viewed at entry")?;
            active.push(Receive { entry: sample, destination, call: None, status: None, packet: None, payload: Vec::new(), copied: false, release: false, released: false });
        }
        let Some(index) = active.iter().position(|receive| {
            registers.r[11].0.checked_add(4) == Some(receive.entry.registers.sp.0) || receive.entry.index == sample.index
        }) else { continue };
        let receive = &mut active[index];
        if let Some(call) = &mut receive.call {
            if physical(call.target) == pc && located.kind == Kind::Target {
                require(!call.entered && registers.sp == call.before.registers.sp && registers.r == call.before.registers.r
                    && physical(registers.lr.0) == call.continuation, check, "a queue service entry differs from its call")?;
                call.entered = true;
            } else if pc == call.continuation {
                require(call.entered && registers.sp == call.before.registers.sp && registers.r[4..12] == call.before.registers.r[4..12], check,
                    "a queue service did not restore its caller's context")?;
                receive.call = None;
            }
        }
        if !located.scoped() {
            continue;
        }
        match pc {
            KERNEL_RECEIVE => {
                let target = service(sample, RECEIVE_SLOT)?;
                require(receive.call.is_none() && Some(registers.r[1].0) == registers.r[11].0.checked_sub(32) && registers.r[2].0 == u32::MAX, check,
                    "the kernel receive's arguments differ")?;
                receive.call = Some(Call { before: sample, target, continuation: KERNEL_RETURN, entered: false });
            }
            KERNEL_RETURN => {
                require(receive.call.is_none(), check, "the receive continuation lacks its kernel return")?;
                receive.status = Some(registers.r[0].0);
            }
            RECEIVED => {
                require(receive.status == Some(0), check, "a success copy follows a nonzero receive")?;
                let slot = registers.r[11].0.checked_sub(32).ok_or_else(|| refuse("the packet slot underflows"))?;
                let packet = scalar(&sample.packet, slot)?;
                let length = scalar(&sample.packet, packet.wrapping_add(8))?;
                require(length <= PAYLOAD_LIMIT && registers.r[5].0 == receive.destination, check, "the received envelope or its payload extent differs")?;
                receive.packet = Some(packet);
                receive.payload = bytes(&sample.packet, packet.wrapping_add(12), length as usize)?;
                receive.copied = length == 0;
            }
            COPY_CALL => {
                let packet = receive.packet.ok_or_else(|| refuse("the copy lacks a source packet"))?;
                require(receive.call.is_none() && !receive.copied && registers.r[0].0 == scalar(&sample.packet, receive.destination.wrapping_add(8))?
                    && registers.r[1].0 == packet.wrapping_add(12) && registers.r[2].0 as usize == receive.payload.len(), check,
                    "the packet copy's arguments differ")?;
                // memcpy's entry is not sampled.
                receive.call = Some(Call { before: sample, target: MEMCPY, continuation: COPY_RETURN, entered: true });
            }
            COPY_RETURN => {
                require(receive.call.is_none() && receive.status == Some(0), check, "the packet copy's return is absent")?;
                receive.copied = true;
            }
            RELEASE_CALL => {
                let target = service(sample, RELEASE_SLOT)?;
                let packet = receive.packet.ok_or_else(|| refuse("the release lacks a received packet"))?;
                require(receive.call.is_none() && receive.copied && !receive.release && registers.r[1].0 == packet, check,
                    "a packet was released before its copy, twice or under another identity")?;
                let command = scalar(&sample.packet, packet.wrapping_add(4))?;
                require(scalar(&sample.packet, receive.destination)? == command
                    && scalar(&sample.packet, receive.destination.wrapping_add(4))? as usize == receive.payload.len(), check,
                    "the copied envelope header differs from the packet")?;
                let destination = scalar(&sample.packet, receive.destination.wrapping_add(8))?;
                require(bytes(&sample.packet, destination, receive.payload.len())? == receive.payload, check,
                    "the copied payload differs before the packet's release")?;
                receive.release = true;
                receive.call = Some(Call { before: sample, target, continuation: RELEASE_RETURN, entered: false });
            }
            RELEASE_RETURN => {
                require(receive.release && receive.call.is_none(), check, "the packet release lacks its return")?;
                receive.released = true;
            }
            QUEUE_EXIT => {
                require(receive.call.is_none() && receive.status.is_some(), check, "a receive exits with an unfinished service")?;
                let adapter = linked.iter().find(|(entry, _)| *entry == receive.entry.index)
                    .ok_or_else(|| refuse("a queue1 receive is unrelated to an adapter receive"))?;
                require(adapter.1 == sample.index && completed.insert(receive.entry.index), check, "the adapter names another queue1 exit")?;
                if receive.status == Some(0) {
                    require(receive.copied && receive.released && registers.r[0].0 == 0, check, "a successful receive lacks its copy, release or zero return")?;
                    successful += 1;
                } else {
                    require(!receive.release && Some(registers.r[0].0) == receive.status, check, "an error receive released a packet or changed its status")?;
                }
                active.remove(index);
            }
            _ => {}
        }
    }
    require(linked.len() == completed.len(), check, "an adapter receive omitted its queue1 lifecycle")?;
    require(successful >= 2, check, "fewer than two successful queue1 receives")
}

struct PendingCall<'a> {
    before: &'a Sample,
    target: u32,
    continuation: u32,
    entered: bool,
    phase: usize,
}

struct Invocation<'a> {
    entry: &'a Sample,
    phase: usize,
    call: Option<PendingCall<'a>>,
    result: Option<u32>,
    returned: bool,
}

/// Per pump call: entry, the three hooks in order, handback with callee saves and the normalizer's result; at least
/// two complete and two receive-backed pumps; each settled input's pump inside its pressed-to-settled interval.
pub fn pump_sequences(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    use ApplicationCheck::{Handback, HookSequence, Initializer, NormalizerResult, QueueProvenance};
    let pump = &evidence.application.pump;
    let stream = task1_stream(evidence, Handback,
        |located| matches!(located.kind, Kind::Site(SiteKind::Task1 | SiteKind::Pump | SiteKind::HookTarget)))?;
    let mut invocation: Option<Invocation> = None;
    let mut caller: Option<&Sample> = None;
    let (mut initialized, mut received, mut pending, mut backedge_due) = (false, false, false, false);
    let (mut receive_backed, mut completed) = (0usize, Vec::new());
    for located in &stream {
        let (pc, sample) = (located.stock, located.sample);
        let registers = &sample.registers;
        let zero = registers.cpsr.0 & (1 << 30) != 0;
        match pc {
            TASK1_RESET => initialized = true,
            LOOP_HEAD => {
                require(invocation.is_none(), Handback, "the loop restarted without a pump return")?;
                require(initialized, Initializer, "the loop starts before the initializer handback")?;
                (received, pending, backedge_due) = (false, false, false);
            }
            0xc22fd8 | RECEIVE_RETURN => received = true,
            0xc230a0 | 0xc230c0 | 0xc23114 | 0xc2312c if zero => pending = true,
            0xc230d8 if registers.r[0].0 == 1 => pending = true,
            0xc230f4 if !zero => pending = true,
            PUMP_CALL => {
                require(!backedge_due && initialized, Handback, "a pump call precedes the previous backedge or initialization")?;
                caller = Some(sample);
            }
            _ => {}
        }
        if pc == pump.entry.0 {
            require(invocation.is_none(), HookSequence, "the pump reentered before its handback")?;
            let before = caller.take().ok_or_else(|| ApplicationRefusal::new(Handback, "the pump has no task1 caller"))?;
            require(before.core == sample.core && before.registers.sp == registers.sp && before.registers.r == registers.r
                && physical(registers.lr.0) == HANDBACK, Handback, "the pump caller's transfer differs")?;
            if received {
                receive_backed += 1;
            } else {
                require(pending, QueueProvenance, "the pump has neither a receive nor pending work")?;
            }
            invocation = Some(Invocation { entry: sample, phase: 0, call: None, result: None, returned: false });
        }
        if let Some(active) = invocation.as_mut() {
            if let Some(call) = active.call.as_mut() {
                if call.target == pc {
                    require(!call.entered && registers.sp == call.before.registers.sp && registers.r == call.before.registers.r
                        && physical(registers.lr.0) == call.continuation, HookSequence, "a hook entry differs from its call")?;
                    call.entered = true;
                } else if pc == call.continuation {
                    require(call.entered && registers.sp == call.before.registers.sp && registers.r[4..12] == call.before.registers.r[4..12],
                        HookSequence, "a hook did not restore its caller's context")?;
                    if call.phase == 1 {
                        active.result = Some(registers.r[0].0);
                    }
                    active.phase += 1;
                    active.call = None;
                }
            }
            if let Some((index, call)) = pump.calls.iter().enumerate().find(|(_, call)| call.site.0 == pc) {
                require(active.call.is_none() && active.phase == index && sample.core == active.entry.core, HookSequence,
                    "hook calls were skipped, doubled or reordered")?;
                active.call = Some(PendingCall { before: sample, target: call.target.0, continuation: call.continuation.0, entered: false, phase: index });
            }
            if pc == pump.returned.0 {
                require(active.phase == 3 && active.call.is_none(), HookSequence, "the pump returns before all hooks complete")?;
                active.returned = true;
            }
        }
        if pc == HANDBACK {
            let active = invocation.take().ok_or_else(|| ApplicationRefusal::new(Handback, "the handback has no pump"))?;
            require(active.returned && active.phase == 3 && active.call.is_none() && sample.core == active.entry.core
                && registers.sp == active.entry.registers.sp && registers.r[4..12] == active.entry.registers.r[4..12], Handback,
                "the pump's frame, callee saves or return differ")?;
            require(active.result == Some(registers.r[0].0), NormalizerResult, "the handback's r0 is not the normalizer's result")?;
            completed.push((active.entry, sample));
            backedge_due = true;
        }
    }
    require(invocation.is_none() && !backedge_due && completed.len() >= 2 && receive_backed >= 2, Handback,
        "fewer than two complete receive-backed pumps")?;
    for (index, input) in evidence.receipt.inputs.iter().enumerate().skip(FIRST_SETTLED) {
        let id = Some(InputIndex(index as u64));
        let matched = completed.iter().any(|(entry, handback)| {
            entry.input == id && handback.input == id && input.pressed <= entry.stop && entry.stop < handback.stop
                && input.settled.is_some_and(|settled| handback.stop <= settled)
        });
        require(matched, Handback, "a settled input lacks a complete pump inside its pressed-to-settled interval")?;
    }
    Ok(())
}

fn settled(probe: Option<Probe>) -> bool { probe.is_some_and(|probe| probe.drained && probe.consumer_finished()) }

/// Visible captures drained around their inputs, new frames with advanced graphics counters, complete planes.
pub fn render_completion(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    let check = ApplicationCheck::RenderCompletion;
    let receipt = evidence.receipt;
    let bound = |error: String| ApplicationRefusal::new(ApplicationCheck::CaptureBound, error);
    for input in receipt.inputs.iter().skip(FIRST_SETTLED) {
        require(settled(input.before) && settled(input.after) && input.settled.is_some_and(|at| at >= input.debounced), check,
            "a settled input lacks a drained handback")?;
    }
    for (index, visible) in receipt.visible.iter().enumerate() {
        let input = (index > 0).then(|| InputIndex((FIRST_SETTLED - 1 + index) as u64));
        require(visible.input == input, check, "a visible capture's input attribution differs")?;
        require(settled(Some(visible.probe)), check, "a visible capture is not drained")?;
        let save: serde_json::Value = serde_json::from_slice(&visible.graphics.read(evidence.directory).map_err(bound)?)
            .map_err(|error| ApplicationRefusal::new(check, error.to_string()))?;
        let graphics = GraphicsState::of(&save).map_err(|error| ApplicationRefusal::new(check, error))?;
        require(graphics.graphics() == visible.probe.graphics, check, "the graphics state differs from the probe")?;
        let scanout = visible.scanout.read(evidence.directory).map_err(bound)?;
        let planes = visible.planes.read(evidence.directory).map_err(bound)?;
        planes::complete(&scanout, &visible.png, &visible.planes, planes.len() as u64).map_err(|error| ApplicationRefusal::new(check, error))?;
        if let Some(id) = visible.input {
            let input = receipt.inputs.get(id.0 as usize).ok_or_else(|| ApplicationRefusal::new(check, "a visible capture names no input"))?;
            require(input.settled == Some(visible.instructions) && input.after == Some(visible.probe), check,
                "a settled input and its visible capture differ")?;
            if input.purpose != InputPurpose::UnhandledProbe {
                let before = &receipt.visible[index - 1];
                let advanced = visible.probe.graphics.started > before.probe.graphics.started
                    && visible.probe.graphics.completed > before.probe.graphics.completed
                    && visible.probe.graphics.bytes_completed > before.probe.graphics.bytes_completed;
                require(before.png.sha256 != visible.png.sha256 && advanced, check, "an input has no newly acknowledged visible result")?;
            }
        }
    }
    Ok(())
}

/// The route's keys and purposes, the first press at the search end, each held 8,000,000 instructions and debounced
/// within 8,000,000 more.
pub fn route(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    let check = ApplicationCheck::Route;
    let receipt = evidence.receipt;
    require(receipt.inputs.len() == ROUTE.len() && receipt.visible.len() == VISIBLE, check, "the route's inputs or captures are incomplete")?;
    let start = evidence.application.readiness.end;
    let mut previous = start;
    for (index, (input, (key, purpose))) in receipt.inputs.iter().zip(ROUTE).enumerate() {
        require(input.key == key && input.purpose == purpose, check, "an input's key or purpose differs from the route")?;
        require(index != 0 || input.pressed == start, check, "the first press is not at the search end")?;
        require(input.pressed >= previous && input.released.0 == input.pressed.0 + PRESS && input.debounced >= input.released
            && input.debounced.0 - input.released.0 <= PRESS, check, "an input's press, release or debounce differs")?;
        previous = input.settled.unwrap_or(input.debounced);
    }
    Ok(())
}

/// Receipt counts against the evidence, and every code range a recorded write class.
pub fn capture_bound(evidence: &RoleEvidence) -> Result<(), ApplicationRefusal> {
    let check = ApplicationCheck::CaptureBound;
    let (receipt, recorded) = (evidence.receipt, evidence.evidence);
    require(receipt.samples == recorded.samples.len() as u64 && receipt.readiness == recorded.readiness.len() as u64
        && receipt.writes == recorded.writes.len() as u64, check, "the receipt's counts differ from its evidence")?;
    for range in &evidence.application.code {
        require(receipt.watch_classes.iter().any(|class| class.start <= range.start && range.end <= class.end), check,
            "an application code range is not a recorded write class")?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PairField {
    /// Input keys and purposes.
    Inputs,
    /// Visible planes, menus and controller records.
    Visible,
    /// Dynamic targets and their words.
    Targets,
}

pub fn check_pair(retained: &ApplicationReceipt, owned: &ApplicationReceipt) -> Result<(), PairField> {
    let keys = |receipt: &ApplicationReceipt| receipt.inputs.iter().map(|input| (input.key, input.purpose)).collect::<Vec<_>>();
    if keys(retained) != keys(owned) {
        return Err(PairField::Inputs);
    }
    let same = retained.visible.len() == owned.visible.len()
        && retained.visible.iter().zip(&owned.visible).all(|(a, b)| a.planes.sha256 == b.planes.sha256 && a.menu == b.menu && a.controller.physical == b.controller.physical);
    if !same {
        return Err(PairField::Visible);
    }
    let targets = |receipt: &ApplicationReceipt| receipt.targets.iter().map(|target| (target.pc, target.word)).collect::<BTreeSet<_>>();
    if targets(retained) != targets(owned) {
        return Err(PairField::Targets);
    }
    Ok(())
}
