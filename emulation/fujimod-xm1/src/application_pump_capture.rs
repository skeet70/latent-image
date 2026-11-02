use super::*;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy};
use std::cell::RefCell;

#[path = "heap_accesses.rs"]
pub(super) mod bounded_gzip;

const SAMPLE_LIMIT: usize = 32768;
const WATCH_LIMIT: usize = 262144;
const WATCH_WINDOW: u64 = 250_000;
const SETTLE_LIMIT: u64 = 90_000_000;
pub(super) const PUMP_SITES: &[u32] = &[
    0x6a9840, 0xc22f60, 0xc22f64, 0xc22f6c, 0xc22f70, 0xc22f74, 0xc22f84,
    0xbfb124, 0xc22bd0, 0xc22fb4, 0xc22fd4, 0xc22fd8, 0xc23094, 0xc230a0,
    0xc230b4, 0xc230c0, 0xc230d8, 0xc230f4, 0xc23104, 0xc23114, 0xc2311c,
    0xc2312c, 0xc2314c, 0xc23150, 0xc231ec, 0xc231fc, 0xc23274, 0xc23278,
    0xbfbd20, 0xbfbd24, 0xbfbd28, 0xbfbd2c, 0xbfbd30, 0xbfbd34, 0xbfbd38, 0xbfbd3c,
    0xbfb1b8, 0xbfb20c, 0xbfb1e0, 0xdb5124, 0xdb515c, 0xdb5170, 0xdb5174, 0xdb5178, 0xdb51d0,
    0x725214,
    0x11090a4, 0x1109130, 0x1108a2c, 0x110f9d8,
];
pub(super) const SHARED_QUEUE_SITES: &[u32] = &[0x725318, 0x725378, 0x72537c, 0x7253c0, 0x7253f8, 0x7253fc, 0x725414, 0x725418, 0x72541c];
#[derive(Clone, Copy)]
pub(super) struct TaskLayout {
    pub(super) creation: Guest,
    pub(super) task: Guest,
    pub(super) ids: Guest,
    domains: Guest,
    pub(super) current: Guest,
    selected: Guest,
}
pub(super) const TASK_LAYOUTS: [TaskLayout; 2] = [
    TaskLayout { creation: Guest(0x4005beac), task: Guest(0x4005ff0c), ids: Guest(0x400622d4), domains: Guest(0x400545cc), current: Guest(0x40054760), selected: Guest(0x40054768) },
    TaskLayout { creation: Guest(0x400ebe6c), task: Guest(0x400efecc), ids: Guest(0x400f2294), domains: Guest(0x400e458c), current: Guest(0x400e4720), selected: Guest(0x400e4728) },
];

pub(super) fn raw_view_limits() -> bounded_gzip::Limits {
    use bounded_gzip::{ByteLimit, Limits};
    let compressed = ByteLimit::new(2 * 1024 * 1024).expect("nonzero compressed-view bound");
    let decoded = ByteLimit::new(16 * 1024 * 1024).expect("nonzero decoded-view bound");
    Limits::new(compressed, decoded, decoded).expect("ordered raw-view bounds")
}

pub(super) fn watch_limits() -> bounded_gzip::Limits {
    use bounded_gzip::{ByteLimit, Limits};
    let compressed = ByteLimit::new(16 * 1024 * 1024).expect("nonzero compressed-watch bound");
    let decoded = ByteLimit::new(128 * 1024 * 1024).expect("nonzero decoded-watch bound");
    Limits::new(compressed, decoded, decoded).expect("ordered raw-watch bounds")
}

pub(super) fn scope_limits() -> bounded_gzip::Limits {
    use bounded_gzip::{ByteLimit, Limits};
    Limits::new(
        ByteLimit::new(16 * 1024 * 1024).expect("nonzero scope compressed bound"),
        ByteLimit::new(128 * 1024 * 1024).expect("nonzero scope decoded bound"),
        ByteLimit::new(1024 * 1024).expect("nonzero scope line bound"),
    ).expect("ordered scope bounds")
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum ScopeWrite<'a> {
    Begin { sample: SampleId, epoch: EpochId, config: &'a TraceStackScopeConfig, instructions: Count },
    Site { site: Guest, epoch: EpochId, first_full_hit: RowIndex, instructions: Count },
    Skip { hit: TraceStackSkip },
    Finish { summary: &'a TraceStackScopeSummary },
}
fn write_scope(writer: &mut bounded_gzip::Writer<std::fs::File>, line: &mut Vec<u8>, record: &ScopeWrite<'_>) -> Result<(), Failure> {
    line.clear();
    serde_json::to_writer(&mut *line, record).map_err(|error| fail(Refusal::CaptureBound, error.to_string()))?;
    writer.append(line).map_err(|error| fail(Refusal::CaptureBound, error.to_string()))
}
fn sampled_word(views: &[Words], address: Guest) -> Result<u32, Failure> {
    let address = physical(address.0);
    views.iter().find_map(|view| {
        let offset = address.checked_sub(view.mapping.physical.0)?;
        if offset & 3 != 0 { return None; }
        view.values.get((offset / 4) as usize).copied()
    }).ok_or_else(|| fail(Refusal::TaskContext, "frozen scope source word is absent"))
}
fn identity_layout(identity: u32) -> Result<TaskLayout, Failure> {
    match identity { 0x4003 => Ok(TASK_LAYOUTS[0]), 0x5003 => Ok(TASK_LAYOUTS[1]), _ => Err(fail(Refusal::TaskContext, "unknown configured kernel identity")) }
}

struct Receive { core: Core, sp: Guest }

pub(super) struct Capture {
    pub(super) loader: Observation,
    pub(super) samples: Vec<Sample>,
    pub(super) epochs: Vec<PumpEpoch>,
    pub(super) inputs: Vec<Input>,
    pub(super) visible: Vec<Visible>,
    pub(super) trace_scope: Option<FileBinding>,
}
pub(super) struct Runner<'a> {
    machine: &'a mut Machine,
    catalog: &'a Catalog,
    maps: &'a [MemoryMap],
    output: &'a Path,
    calls: &'a [StaticCall],
    permanent: BTreeMap<u32, String>,
    pump_sites: BTreeSet<u32>,
    capture: Capture,
    watch: Option<Box<MemoryWatch>>,
    epoch_start: Count,
    published: bool,
    epoch_views: Views,
    cursor: usize,
    active_input: Option<InputId>,
    queue: Vec<Receive>,
    raw_views: RefCell<Vec<continuation_view::RawWords>>,
    scoped_sites: BTreeSet<u32>,
    dynamic_scopes: BTreeSet<u32>,
    scope_output: Option<bounded_gzip::Writer<std::fs::File>>,
    scope_line: Vec<u8>,
}
fn fail(reason: Refusal, error: impl Into<String>) -> Failure { Failure::new(reason, error) }
fn checked<T>(reason: Refusal, result: Result<T, String>) -> Result<T, Failure> { result.map_err(|error| fail(reason, error)) }
fn physical(address: u32) -> u32 { address & !0x40000000 }
fn watch_classes(published: bool, early: bool) -> Vec<WatchRange> {
    let mut ranges = classes(early);
    if published {
        ranges.push(WatchRange { start: u64::from(CARRIER.0), end: u64::from(CARRIER.0) + CARRIER_BYTES as u64, kinds: WatchKinds::ALL });
        for (start, end) in [(0xbfb124, 0xbfb244), (0xc22bd0, 0xc22f60), (0xc22f60, 0xc2327c), (0x725214, 0x725424), (0xdb5124, 0xdb51dc), (0x11090a4, 0x11091b0)] {
            ranges.push(WatchRange { start, end, kinds: WatchKinds::CPU_FETCH.union(WatchKinds::WRITES).union(WatchKinds::HOST_READ) });
        }
        for (start, end) in [(0x944580, 0x9445d0), (0x94f5e0, 0x952100), (0x1467588, 0x1467840), (0x14e3258, 0x14e32b0)] {
            ranges.push(WatchRange { start, end, kinds: WatchKinds::WRITES.union(WatchKinds::CPU_READ).union(WatchKinds::HOST_READ) });
        }
        for layout in TASK_LAYOUTS {
            for (start, bytes) in [(layout.creation.0, 40), (layout.task.0, 56), (layout.ids.0, 4), (layout.domains.0, 264), (layout.current.0, 4), (layout.selected.0, 4)] {
                ranges.push(WatchRange { start: u64::from(physical(start)), end: u64::from(physical(start)) + bytes, kinds: WatchKinds::WRITES.union(WatchKinds::HOST_READ) });
            }
        }
    }
    ranges
}
impl<'a> Runner<'a> {
    pub(super) fn new(machine: &'a mut Machine, catalog: &'a Catalog, maps: &'a [MemoryMap], output: &'a Path, calls: &'a [StaticCall]) -> Result<Self, String> {
        let mut permanent = sites(calls);
        let pump_sites: BTreeSet<_> = PUMP_SITES.iter().flat_map(|address| [*address, *address | 0x40000000]).collect();
        for address in &pump_sites { permanent.entry(*address).or_insert_with(|| "application-pump-boundary".into()); }
        machine.traces.bound_full_registers(NonZeroUsize::new(65536).ok_or("zero pump trace bound")?);
        machine.traces.stop_labels = permanent.values().cloned().chain(["temporary-boundary".into()]).collect();
        machine.traces.arm(&machine.session, permanent.clone())?;
        let mut runner = Self {
            machine, catalog, maps, output, calls, permanent, pump_sites,
            capture: Capture { loader: Observation::default(), samples: Vec::new(), epochs: Vec::new(), inputs: Vec::new(), visible: Vec::new(), trace_scope: None },
            watch: None, epoch_start: Count(0), published: false, epoch_views: Views::Observe, cursor: 0, active_input: None, queue: Vec::new(),
            raw_views: RefCell::new(Vec::new()),
            scoped_sites: BTreeSet::new(), dynamic_scopes: BTreeSet::new(), scope_output: None, scope_line: Vec::with_capacity(1024),
        };
        runner.begin_watch().map_err(|failure| failure.detail)?;
        Ok(runner)
    }
    fn begin_watch(&mut self) -> Result<(), Failure> {
        self.epoch_start = Count(self.machine.instructions());
        self.epoch_views = if self.published { Views::Fixed } else { Views::Observe };
        let mut ranges = watch_classes(self.published, self.machine.instructions() < 8_000_000);
        for &site in &self.dynamic_scopes {
            ranges.push(WatchRange { start: u64::from(site), end: u64::from(site) + 4, kinds: WatchKinds::CPU_FETCH.union(WatchKinds::WRITES).union(WatchKinds::HOST_READ) });
        }
        for backing in checked(Refusal::Mapping, run::memory_backings(&self.machine.document))? {
            let end = backing.base.checked_add(backing.size as u64).ok_or_else(|| fail(Refusal::Mapping, "backing extent overflows"))?;
            ranges.push(WatchRange { start: backing.base, end, kinds: if self.published { WatchKinds::HOST_READ.union(WatchKinds::WRITES) } else { WatchKinds::HOST_READ } });
        }
        self.watch = Some(checked(Refusal::Mapping, self.machine.session.board.memory_watch_begin_classified(
            ranges, WATCH_LIMIT, self.maps.to_vec(), if self.published { ViewPolicy::Fixed } else { ViewPolicy::Observe },
        ))?);
        Ok(())
    }
    fn row_count(&self) -> Result<usize, Failure> {
        let watch = self.watch.as_ref().ok_or_else(|| fail(Refusal::CaptureBound, "raw watch absent"))?;
        let raw = watch.records.borrow().state();
        raw.get("records").and_then(serde_json::Value::as_array).map(Vec::len).ok_or_else(|| fail(Refusal::CaptureBound, "raw rows absent"))
    }
    fn host_begin(&self) -> Result<HostReads, Failure> {
        let row = RowIndex(self.row_count()?);
        Ok(HostReads { epoch: EpochId(self.capture.epochs.len()), first: row, end: row, stopped: Count(self.machine.instructions()) })
    }
    fn host_end(&self, mut reads: HostReads) -> Result<HostReads, Failure> {
        reads.end = RowIndex(self.row_count()?);
        Ok(reads)
    }
    fn check_watch(&self) -> Result<(), Failure> {
        let watch = self.watch.as_ref().ok_or_else(|| fail(Refusal::CaptureBound, "raw watch absent"))?;
        let state: RawWatch = checked(Refusal::CaptureBound, serde_json::from_value(watch.records.borrow().state()).map_err(|error| error.to_string()))?;
        if !state.enabled || state.violation.is_some() { return Err(fail(Refusal::CaptureBound, "SDK memory watch refused or overflowed")); }
        if self.epoch_views == Views::Fixed && state.records.iter().any(|row| matches!(row.source, Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite) && row.physical.0 < CARRIER.0 + CARRIER_BYTES as u32 && CARRIER.0 < row.physical.0.saturating_add(u32::try_from(row.size).unwrap_or(u32::MAX))) {
            return Err(fail(Refusal::CodeIdentity, "published pump carrier was written"));
        }
        Ok(())
    }
    fn end_watch(&mut self) -> Result<(), Failure> {
        let Some(mut watch) = self.watch.take() else { return Ok(()); };
        let raw = watch.records.borrow().state();
        let name = format!("pump-watch-{:05}.json.gz", self.capture.epochs.len());
        let save = (|| -> Result<(), String> {
            let bytes = serde_json::to_vec(&raw).map_err(|error| error.to_string())?;
            let file = std::fs::OpenOptions::new().create_new(true).write(true).open(self.output.join(&name)).map_err(|error| error.to_string())?;
            let mut writer = bounded_gzip::Writer::new(file, watch_limits());
            writer.append(&bytes).map_err(|error| error.to_string())?;
            writer.finish().map_err(|error| error.to_string())?;
            Ok(())
        })();
        let teardown_error = self.machine.session.board.memory_watch_end(&mut watch).err();
        checked(Refusal::CaptureBound, save)?;
        let state: RawWatch = checked(Refusal::CaptureBound, serde_json::from_value(raw).map_err(|error| error.to_string()))?;
        let refused = !state.enabled || state.violation.is_some() || teardown_error.is_some();
        self.capture.epochs.push(PumpEpoch {
            id: EpochId(self.capture.epochs.len()), start: self.epoch_start, end: Count(self.machine.instructions()), views: self.epoch_views,
            enabled: state.enabled, refused: state.violation.is_some(), teardown_error, classes: state.classes, record_count: state.records.len(),
            raw: checked(Refusal::CaptureBound, binding(self.output, &name))?,
        });
        if refused { Err(fail(Refusal::CaptureBound, "raw memory-watch epoch did not complete")) } else { Ok(()) }
    }
    fn carrier(&self, reader: &continuation_view::Reader<'_>) -> Result<[Words; 2], Failure> {
        let low = self.words(reader, CARRIER.0, 8)?;
        let high = self.words(reader, CARRIER.0 | 0x40000000, 8)?;
        let mut expected = [0; 8];
        for (index, word) in expected.iter_mut().enumerate() {
            *word = checked(Refusal::CodeIdentity, self.catalog.word(Guest(CARRIER.0 + index as u32 * 4)))?.1;
        }
        if low.values.as_slice() != expected.as_slice() || high.values.as_slice() != expected.as_slice() || low.mapping.physical != CARRIER || high.mapping.physical != CARRIER {
            return Err(fail(Refusal::CodeIdentity, "published pump bytes or executable aliases differ"));
        }
        Ok([low, high])
    }
    fn words(&self, reader: &continuation_view::Reader<'_>, address: u32, count: usize) -> Result<Words, Failure> {
        if count == 0 || count > 80 { return Err(fail(Refusal::Mapping, "audited projection extent is outside1..80words")); }
        let (words, raw) = checked(Refusal::Mapping, reader.observed_words(Guest(address), count))?;
        let equal = words.guest_equal == [true; 2];
        let mut views = self.raw_views.borrow_mut();
        if views.len() >= 256 { return Err(fail(Refusal::CaptureBound, "raw projection bound reached")); }
        views.push(raw);
        if !equal { return Err(fail(Refusal::Mapping, "committed guest and physical words differ")); }
        Ok(words)
    }
    fn save_views(&self, name: &str) -> Result<FileBinding, Failure> {
        use bounded_gzip::Writer;
        let limits = raw_view_limits();
        let bytes = serde_json::to_vec(&*self.raw_views.borrow()).map_err(|error| fail(Refusal::CaptureBound, error.to_string()))?;
        let file = std::fs::OpenOptions::new().create_new(true).write(true).open(self.output.join(name)).map_err(|error| fail(Refusal::CaptureBound, error.to_string()))?;
        let mut writer = Writer::new(file, limits);
        writer.append(&bytes).map_err(|error| fail(Refusal::CaptureBound, error.to_string()))?;
        writer.finish().map_err(|error| fail(Refusal::CaptureBound, error.to_string()))?;
        self.raw_views.borrow_mut().clear();
        checked(Refusal::CaptureBound, binding(self.output, name))
    }
    fn receiving(&self, core: Core, regs: &Registers) -> bool {
        self.queue.iter().any(|receive| receive.core == core && regs.r[11].checked_add(4) == Some(receive.sp.0) && regs.r[8] == 1)
    }
    fn begin_scope(&mut self) -> Result<(), Failure> {
        if self.scope_output.is_some() { return Err(fail(Refusal::TaskContext, "UI stack scope was already frozen")); }
        let (origin, config) = {
            let sample = self.capture.samples.last().ok_or_else(|| fail(Refusal::TaskContext, "UI entry sample is absent"))?;
            let layout = identity_layout(sample.identities[usize::from(sample.core)])?;
            let creation = layout.creation.0;
            let at = |offset| sampled_word(&sample.task, Guest(creation + offset));
            let base = at(20)?;
            let end = base.checked_add(at(16)?).ok_or_else(|| fail(Refusal::StackContext, "UI frozen stack overflows"))?;
            if physical(sample.pc.0) != 0xc22f60 || physical(sampled_word(&sample.task, layout.current)?) != physical(layout.task.0)
                || physical(sampled_word(&sample.task, Guest(layout.task.0 + 8))?) != physical(layout.creation.0)
                || at(8)? != 0xc22f60 || at(4)? != 0 || at(12)? != 5 || at(16)? != 0xc00 || at(36)? != 1
                || at(32)? & 0xffff != 1 || sampled_word(&sample.task, layout.ids)? != at(32)?
                || base & 7 != 0 || physical(sample.registers.sp.0) != end || sample.registers.sp.0 & 7 != 0
                || sample.registers.cpsr & 0x3f != 0x13 || sample.registers.r[0] != 0
                || physical(sample.registers.r[1]) != physical(layout.task.0) || physical(sample.registers.r[2]) != physical(layout.creation.0) {
                return Err(fail(Refusal::TaskContext, "UI entry does not establish the committed stack owner"));
            }
            let current = [Guest(physical(identity_layout(sample.identities[0])?.current.0)), Guest(physical(identity_layout(sample.identities[1])?.current.0))];
            let config = checked(Refusal::StackContext, TraceStackScopeConfig::new([Guest(base), Guest(end)], Guest(physical(layout.task.0)), current))?;
            (sample.id, config)
        };
        self.end_watch()?;
        let file = std::fs::OpenOptions::new().create_new(true).write(true).open(self.output.join("pump-stack-scope.json.gz")).map_err(|error| fail(Refusal::CaptureBound, error.to_string()))?;
        self.scope_output = Some(bounded_gzip::Writer::new(file, scope_limits()));
        write_scope(self.scope_output.as_mut().ok_or_else(|| fail(Refusal::CaptureBound, "scope writer absent"))?, &mut self.scope_line,
            &ScopeWrite::Begin { sample: origin, epoch: EpochId(self.capture.epochs.len()), config: &config, instructions: Count(self.machine.instructions()) })?;
        checked(Refusal::StackContext, self.machine.traces.begin_stack_scope(config))?;
        let sites: [Guest; SHARED_QUEUE_SITES.len() * 2] = std::array::from_fn(|index| Guest(SHARED_QUEUE_SITES[index / 2] | if index & 1 == 0 { 0 } else { 0x40000000 }));
        self.register_scoped_sites(&sites)?;
        self.begin_watch()
    }
    fn register_scoped_sites(&mut self, sites: &[Guest]) -> Result<(), Failure> {
        checked(Refusal::StackContext, self.machine.traces.scope_stack_sites(&self.machine.session, sites))?;
        let mut added = BTreeMap::new();
        let cursor = RowIndex(self.machine.traces.hits.len());
        for &site in sites {
            if !self.scoped_sites.insert(site.0) { continue; }
            self.pump_sites.insert(site.0);
            self.permanent.insert(site.0, "application-pump-boundary".into());
            if !self.machine.traces.sites.contains_key(&site.0) { added.insert(site.0, "application-pump-boundary".into()); }
            write_scope(self.scope_output.as_mut().ok_or_else(|| fail(Refusal::CaptureBound, "scope writer absent"))?, &mut self.scope_line,
                &ScopeWrite::Site { site, epoch: EpochId(self.capture.epochs.len()), first_full_hit: cursor, instructions: Count(self.machine.instructions()) })?;
        }
        if !added.is_empty() { checked(Refusal::StackContext, self.machine.traces.add_sites(&self.machine.session, added))?; }
        Ok(())
    }
    fn flush_scope_skips(&mut self) -> Result<(), Failure> {
        let Some(writer) = &mut self.scope_output else { return Ok(()); };
        for hit in self.machine.traces.drain_stack_scope_skips() { write_scope(writer, &mut self.scope_line, &ScopeWrite::Skip { hit })?; }
        Ok(())
    }
    fn finish_scope(&mut self) -> Result<(), Failure> {
        self.flush_scope_skips()?;
        let Some(mut writer) = self.scope_output.take() else { return Ok(()); };
        let summary = self.machine.traces.stack_scope_summary().ok_or_else(|| fail(Refusal::CaptureBound, "scope summary absent"))?;
        write_scope(&mut writer, &mut self.scope_line, &ScopeWrite::Finish { summary })?;
        writer.finish().map_err(|error| fail(Refusal::CaptureBound, error.to_string()))?;
        self.capture.trace_scope = Some(checked(Refusal::CaptureBound, binding(self.output, "pump-stack-scope.json.gz"))?);
        Ok(())
    }
    fn target(&mut self, target: Guest) -> Result<(), Failure> {
        let target = physical(target.0);
        if target & 3 != 0 || PUMP_SITES.contains(&target) || SHARED_QUEUE_SITES.contains(&target) {
            return Err(fail(Refusal::QueueProvenance, "queue service target is not an independent aligned A32 entry"));
        }
        checked(Refusal::QueueProvenance, self.catalog.word(Guest(target)))?;
        if self.dynamic_scopes.contains(&target) { return Ok(()); }
        self.end_watch()?;
        self.dynamic_scopes.insert(target);
        self.register_scoped_sites(&[Guest(target), Guest(target | 0x40000000)])?;
        self.begin_watch()
    }
    fn task_views(&self, reader: &continuation_view::Reader<'_>) -> Result<Vec<Words>, Failure> {
        let mut views = Vec::new();
        for layout in TASK_LAYOUTS {
            for (address, count) in [(layout.creation.0, 10), (layout.task.0, 14), (layout.ids.0, 1), (layout.domains.0, 66), (layout.current.0, 1), (layout.selected.0, 1)] { views.push(self.words(reader, address, count)?); }
        }
        Ok(views)
    }
    fn state_views(&self, reader: &continuation_view::Reader<'_>) -> Result<Vec<Words>, Failure> {
        let mut views = Vec::new();
        for (address, count) in [(0x944580, 20), (0x972e84, 4), (0x1467588, 80), (0x14676c8, 7), (0x14676e4, 80), (0x1467824, 7), (0x96bd20, 2), (0x96ca50, 1), (0x944580 + 0x5854, 1), (0x944580 + 0xbbc8, 1), (0x944580 + 0xcfa8, 1), (0x944580 + 0xbb18, 1), (0x944580 + 0xb060, 1)] {
            views.push(self.words(reader, address, count)?);
        }
        let facts = &self.machine.document.firmware.render;
        for address in [facts.copied_command, facts.packet_token, facts.initialized, facts.display_suppression, facts.sender_token, facts.consumer_token, facts.sender_command, facts.staged_render_command, facts.staged_render_token, facts.batch_busy, facts.batch_callback, facts.commands_dirty, facts.display_pending, facts.ui_pending, facts.ui_busy] {
            let address = u32::try_from(address.0).map_err(|_| fail(Refusal::Mapping, "render address exceeds word"))? & !3;
            views.push(self.words(reader, address, 1)?);
        }
        for root in [0x4004dae0, 0x400ddaa0] {
            for offset in [0xc8, 0xf8, 0x1618] { views.push(self.words(reader, root + offset, 1)?); }
        }
        views.push(self.words(reader, 0x7cad9c, 1)?);
        Ok(views)
    }
    fn sample(&mut self, core: Core, pc: Guest) -> Result<(), Failure> {
        if self.capture.samples.len() >= SAMPLE_LIMIT { return Err(fail(Refusal::CaptureBound, "pump sample bound reached")); }
        let reads = self.host_begin()?;
        let reader = checked(Refusal::Mapping, continuation_view::Reader::new(&self.machine.session, self.maps))?;
        let regs = checked(Refusal::TaskContext, registers(self.machine, core))?;
        let carrier = self.carrier(&reader)?;
        let code = self.words(&reader, pc.0, 1)?;
        let expected = checked(Refusal::CodeIdentity, self.catalog.word(pc))?.1;
        if code.values != [expected] { return Err(fail(Refusal::CodeIdentity, "stopped instruction differs from the bound source")); }
        let task = self.task_views(&reader)?;
        let mut stack = Vec::new();
        if regs.sp.0 >= 64 && regs.sp.0 & 3 == 0 { stack.push(self.words(&reader, regs.sp.0 - 64, 64)?); }
        let mut packet = Vec::new();
        let address = physical(pc.0);
        if address == 0xdb5124 { packet.push(self.words(&reader, regs.r[0], 4)?); }
        if address == 0x725318 && regs.r[0] == 1 { packet.push(self.words(&reader, regs.r[1], 3)?); }
        if matches!(address, 0x725378 | 0x72537c | 0x7253c0 | 0x7253f8 | 0x7253fc | 0x725414 | 0x725418 | 0x72541c) && self.receiving(core, &regs) {
            let slot = self.words(&reader, regs.r[11].checked_sub(32).ok_or_else(|| fail(Refusal::QueueProvenance, "packet slot underflows"))?, 1)?;
            let pointer = slot.values[0];
            packet.push(slot);
            if matches!(address, 0x7253c0 | 0x7253f8 | 0x725414) {
                let header = self.words(&reader, pointer, 3)?;
                let length = header.values[2];
                if length > 76 { return Err(fail(Refusal::QueueProvenance, "queue1 payload exceeds its command/argument/68-byte data extent")); }
                packet.push(header);
                if length != 0 { packet.push(self.words(&reader, pointer.checked_add(12).ok_or_else(|| fail(Refusal::QueueProvenance, "packet payload overflows"))?, length.div_ceil(4) as usize)?); }
            }
            let destination = self.words(&reader, regs.r[5], 3)?;
            let length = destination.values[1];
            let payload = destination.values[2];
            packet.push(destination);
            if matches!(address, 0x7253fc | 0x725414 | 0x725418) && regs.r[4] == 0 && length != 0 {
                if length > 76 { return Err(fail(Refusal::QueueProvenance, "received destination payload exceeds76bytes")); }
                packet.push(self.words(&reader, payload, length.div_ceil(4) as usize)?);
            }
        }
        let state = self.state_views(&reader)?;
        let stop = Count(self.machine.instructions());
        // Stopped fetches use stop; resumed entries start at stop+1 (tests/peer_wait_sdk.rs:223-254).
        let resume = Count(stop.0.checked_add(1).ok_or_else(|| fail(Refusal::CaptureBound, "resume fetch clock overflows"))?);
        let counts = self.machine.session.board.instruction_counts().try_into().map_err(|_| fail(Refusal::TaskContext, "expected two core clocks"))?;
        let mut identities = [0; 2];
        for (index, identity) in identities.iter_mut().enumerate() {
            let cpu = checked(Refusal::TaskContext, self.machine.session.board.cpu(index))?;
            let value = checked(Refusal::TaskContext, cpu.rprofile_identity(None))?;
            *identity = u32::try_from(value).map_err(|_| fail(Refusal::TaskContext, "hardware kernel identity exceeds32bits"))?;
        }
        let host_reads = self.host_end(reads)?;
        let raw_views = self.save_views(&format!("pump-sample-{:05}.views.json.gz", self.capture.samples.len()))?;
        self.capture.samples.push(Sample { id: SampleId(self.capture.samples.len()), stop, resume, counts, identities, core, pc, registers: regs, code, carrier, task, stack, state, packet, host_reads, raw_views, input: self.active_input });
        Ok(())
    }
    fn collect(&mut self) -> Result<(), Failure> {
        while self.cursor < self.machine.traces.hits.len() {
            let hit = &self.machine.traces.hits[self.cursor];
            if hit.instructions != self.machine.instructions() { return Err(fail(Refusal::Guest, "trace stop is no longer live")); }
            let core = checked(Refusal::TaskContext, Core::try_from(hit.core))?;
            let pc = Guest(u32::from_str_radix(hit.address.strip_prefix("0x").ok_or_else(|| fail(Refusal::Guest, "trace PC prefix absent"))?, 16).map_err(|error| fail(Refusal::Guest, error.to_string()))?);
            let address = physical(pc.0);
            let loader_boundary = self.capture.loader.calls.iter().any(|call| call.core == core && (call.entry.is_none() && call.target == pc || call.returned.is_none() && call.continuation == pc));
            let loader_site = !self.pump_sites.contains(&pc.0) || address == 0x6a9840 || loader_boundary;
            if loader_site {
                self.capture.loader.cursor = self.cursor;
                checked(Refusal::Publication, self.capture.loader.collect(self.machine, self.catalog, self.maps, self.output, &self.permanent, self.calls))?;
            }
            self.cursor += 1;
            self.capture.loader.cursor = self.cursor;
            if address == 0x6a9840 && !self.published {
                self.end_watch()?;
                self.published = true;
                self.begin_watch()?;
            }
            if !self.published { continue; }
            let regs = checked(Refusal::QueueProvenance, registers(self.machine, core))?;
            if address == 0x725318 && regs.r[0] == 1 {
                if self.queue.iter().any(|receive| receive.core == core && receive.sp == regs.sp) { return Err(fail(Refusal::QueueProvenance, "queue1 receive context overlaps an unfinished receive")); }
                self.queue.push(Receive { core, sp: regs.sp });
            }
            let receiving = if address == 0x725318 { regs.r[0] == 1 } else { self.receiving(core, &regs) };
            if receiving && matches!(address, 0x725378 | 0x725414) { self.target(Guest(regs.r[3]))?; }
            if receiving && address == 0x7253f8 { self.target(Guest(0x722054))?; }
            if self.pump_sites.contains(&pc.0) {
                self.sample(core, pc)?;
                if address == 0xc22f60 { self.begin_scope()?; }
            }
            if receiving && address == 0x72541c { self.queue.retain(|receive| receive.core != core || regs.r[11].checked_add(4) != Some(receive.sp.0)); }
        }
        Ok(())
    }
    fn advance(&mut self, count: u64) -> Result<(), Failure> {
        let end = self.machine.instructions().checked_add(count).ok_or_else(|| fail(Refusal::CaptureBound, "instruction endpoint overflows"))?;
        let mut stopped_without_entry = [false; 2];
        while self.machine.instructions() < end {
            let before = self.machine.instructions();
            let first_hit = self.cursor;
            let epoch_end = self.epoch_start.0.checked_add(WATCH_WINDOW).ok_or_else(|| fail(Refusal::CaptureBound, "watch endpoint overflows"))?;
            let outcome = self.machine.run(end.min(epoch_end) - before);
            self.flush_scope_skips()?;
            if let Some(violation) = self.machine.traces.stack_scope_summary().and_then(|summary| summary.violation.as_ref()) {
                let reason = match violation { TraceStackViolation::OwnerOutsideStack { .. } => Refusal::StackContext, TraceStackViolation::SkipLimit { .. } | TraceStackViolation::CounterOverflow { .. } => Refusal::CaptureBound };
                return Err(fail(reason, format!("scoped instruction observation refused: {violation:?}")));
            }
            self.collect()?;
            checked(Refusal::Guest, self.machine.require_running())?;
            self.check_watch()?;
            if self.machine.instructions() == before {
                // A window endpoint may precede an unconsumed point at the same entered count (tests/peer_wait_sdk.rs:256-285).
                if outcome != fujimod_xm1::machine::RunOutcome::Stopped || self.cursor == first_hit {
                    return Err(fail(Refusal::Guest, "observer made neither entered-instruction nor live-stop progress"));
                }
                for hit in &self.machine.traces.hits[first_hit..self.cursor] {
                    if stopped_without_entry[hit.core] { return Err(fail(Refusal::Guest, "core repeated an unconsumed stop without entered-instruction progress")); }
                    stopped_without_entry[hit.core] = true;
                }
            } else { stopped_without_entry.fill(false); }
            if self.machine.instructions() >= epoch_end { self.end_watch()?; self.begin_watch()?; }
        }
        Ok(())
    }
    fn pulse(&mut self, key: RouteKey, purpose: InputPurpose, before: Option<Probe>) -> Result<InputId, Failure> {
        let id = InputId(self.capture.inputs.len());
        self.active_input = Some(id);
        self.capture.inputs.push(Input { id, key, purpose, pressed: Count(self.machine.instructions()), released: None, debounced: None, settled: None, before, after: None });
        checked(Refusal::Guest, self.machine.press(key))?;
        self.advance(8_000_000)?;
        checked(Refusal::Guest, self.machine.release_key(key))?;
        self.capture.inputs[id.0].released = Some(Count(self.machine.instructions()));
        let start = self.machine.instructions();
        loop {
            if checked(Refusal::Guest, self.machine.document.firmware.released(&self.machine.session, key))? { break; }
            if self.machine.instructions() - start >= 8_000_000 { return Err(fail(Refusal::RenderCompletion, "input release did not debounce")); }
            self.advance(WATCH_WINDOW)?;
        }
        self.capture.inputs[id.0].debounced = Some(Count(self.machine.instructions()));
        Ok(id)
    }
    fn probe(&self) -> Result<Probe, Failure> { checked(Refusal::RenderCompletion, self.machine.document.firmware.render.probe(self.machine)) }
    fn drain(&mut self) -> Result<Probe, Failure> {
        let start = self.machine.instructions();
        loop {
            let probe = self.probe()?;
            if probe.drained && probe.consumer_finished() { return Ok(probe); }
            if self.machine.instructions() - start >= SETTLE_LIMIT { return Err(fail(Refusal::RenderCompletion, "render drain deadline reached")); }
            self.advance(WATCH_WINDOW)?;
        }
    }
    fn visible(&mut self, input: Option<InputId>, probe: Probe) -> Result<(), Failure> {
        let reads = self.host_begin()?;
        let index = self.capture.visible.len();
        let name = format!("pump-visible-{index:03}");
        let frame = checked(Refusal::RenderCompletion, self.machine.capture_frame())?;
        checked(Refusal::RenderCompletion, frame.write(&self.output.join(format!("{name}.png"))))?;
        let graphics_name = format!("{name}.graphics.json");
        let node = checked(Refusal::RenderCompletion, self.machine.session.node(&self.machine.document.firmware.render.graphics.0))?;
        let graphics = checked(Refusal::RenderCompletion, self.machine.session.board.device_save(node))?;
        checked(Refusal::RenderCompletion, persist(&self.output.join(&graphics_name), &graphics))?;
        let reader = checked(Refusal::Mapping, continuation_view::Reader::new(&self.machine.session, self.maps))?;
        let mut state = self.state_views(&reader)?;
        let firmware = &self.machine.document.firmware;
        let menu_address = u32::try_from(firmware.current_menu.0).map_err(|_| fail(Refusal::Mapping, "menu address exceeds word"))?;
        let menu_node = self.words(&reader, menu_address, 3)?;
        let descriptor = menu_node.values[0];
        state.push(menu_node);
        if descriptor != 0 { state.push(self.words(&reader, descriptor, 3)?); }
        let scanner = u32::try_from(firmware.scanner_registered.0).map_err(|_| fail(Refusal::Mapping, "scanner address exceeds word"))?;
        state.push(self.words(&reader, scanner, 1)?);
        for key in &firmware.keys {
            let address = firmware.key_table.base.0.checked_add(key.row * firmware.key_table.row_bytes)
                .and_then(|address| address.checked_add(firmware.key_table.released_offset))
                .and_then(|address| u32::try_from(address).ok())
                .ok_or_else(|| fail(Refusal::Mapping, "key release address overflows"))?;
            state.push(self.words(&reader, address & !3, 1)?);
        }
        let mut task_tables = Vec::new();
        for layout in TASK_LAYOUTS {
            for (base, words) in [(layout.creation.0, 10 * 120), (layout.task.0, 14 * 120)] {
                for offset in (0..words).step_by(80) { task_tables.push(self.words(&reader, base + offset as u32 * 4, (words - offset).min(80))?); }
            }
        }
        let menu = checked(Refusal::RenderCompletion, self.machine.menu())?.map(|menu| [menu.menu_id, menu.setting_id, menu.cursor]);
        let host_reads = self.host_end(reads)?;
        let raw_views = self.save_views(&format!("{name}.views.json.gz"))?;
        self.capture.visible.push(Visible {
            input, instructions: Count(self.machine.instructions()), probe, menu, state, task_tables,
            png: checked(Refusal::RenderCompletion, binding(self.output, &format!("{name}.png")))?,
            planes: checked(Refusal::RenderCompletion, binding(self.output, &format!("{name}.planes.bin")))?,
            scanout: checked(Refusal::RenderCompletion, binding(self.output, &format!("{name}.scanout.json")))?,
            graphics: checked(Refusal::RenderCompletion, binding(self.output, &graphics_name))?, host_reads, raw_views,
        });
        Ok(())
    }
    fn settled_input(&mut self, key: RouteKey, purpose: InputPurpose) -> Result<(), Failure> {
        let before = self.drain()?;
        let old_frame = checked(Refusal::RenderCompletion, self.machine.frame())?;
        let id = self.pulse(key, purpose, Some(before))?;
        if purpose == InputPurpose::UnhandledProbe {
            let after = self.drain()?;
            self.capture.inputs[id.0].settled = Some(Count(self.machine.instructions()));
            self.capture.inputs[id.0].after = Some(after);
            self.visible(Some(id), after)?;
            self.active_input = None;
            return Ok(());
        }
        let start = self.machine.instructions();
        let mut requested = None;
        loop {
            let after = self.probe()?;
            if after.sender_token != before.sender_token { requested = Some(after.sender_token); }
            let consumed = requested.is_some_and(|token| {
                after.copied_command & 0xf000 == 0x4000 && after.packet_token == token && after.consumer_token == token && after.copied_command == after.sender_command
                    || after.auxiliary_acknowledged() && after.staged_render_token == token && before.staged_render_token != token
            });
            let new_work = after.graphics.started > before.graphics.started && after.graphics.completed > before.graphics.completed && after.graphics.bytes_completed > before.graphics.bytes_completed;
            if consumed && new_work && after.drained && after.consumer_finished() && checked(Refusal::RenderCompletion, self.machine.frame())? != old_frame {
                self.capture.inputs[id.0].settled = Some(Count(self.machine.instructions()));
                self.capture.inputs[id.0].after = Some(after);
                self.visible(Some(id), after)?;
                self.active_input = None;
                return Ok(());
            }
            if self.machine.instructions() - start >= SETTLE_LIMIT { return Err(fail(Refusal::RenderCompletion, "input has no acknowledged full-plane render completion")); }
            self.advance(WATCH_WINDOW)?;
        }
    }
    pub(super) fn observe(&mut self) -> Result<(), Failure> {
        self.advance(80_000_000)?;
        let menu = checked(Refusal::Guest, self.machine.menu())?;
        if !menu.is_some_and(|menu| (menu.menu_id, menu.setting_id, menu.cursor) == (69, 52, 12)) { return Err(fail(Refusal::RouteUnavailable, "canonical erased-settings start menu not reached")); }
        for key in std::iter::repeat_n(RouteKey::Up, 12).chain([RouteKey::Ok, RouteKey::Ok]) { self.pulse(key, InputPurpose::FirstStart, None)?; }
        self.advance(40_000_000)?;
        self.pulse(RouteKey::Ok, InputPurpose::FirstStart, None)?;
        self.advance(90_000_000)?;
        self.active_input = None;
        let probe = self.drain()?;
        self.visible(None, probe)?;
        self.settled_input(RouteKey::Down, InputPurpose::Ordinary)?;
        self.settled_input(RouteKey::Up, InputPurpose::Ordinary)?;
        self.settled_input(RouteKey::Back, InputPurpose::BackHandback)?;
        self.settled_input(RouteKey::Ok, InputPurpose::AfterBack)?;
        self.settled_input(RouteKey::Left, InputPurpose::UnhandledProbe)
    }
    pub(super) fn finish(&mut self) -> Result<(), Failure> {
        let partial = if self.raw_views.borrow().is_empty() { Ok(()) } else { self.save_views("pump-partial.views.json.gz").map(|_| ()) };
        let scope = self.finish_scope();
        let watch = self.end_watch();
        partial.and(scope).and(watch)
    }
    pub(super) fn into_capture(self) -> Capture { self.capture }
}
