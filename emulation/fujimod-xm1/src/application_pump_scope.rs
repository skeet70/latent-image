use super::*;
use fujimod_xm1::trace::{TraceStackScopeConfig, TraceStackScopeSummary};

pub(super) fn records(root: &Path, binding: &FileBinding) -> Result<Vec<TraceScopeRecord>, Failure> {
    let path = local(root, binding)?;
    let digest = capture::bounded_gzip::digest_file(&path, capture::scope_limits().compressed()).map_err(|error| refusal(Refusal::CaptureBound, error.to_string()))?;
    require(digest.0 == binding.sha256.as_str(), Refusal::ArtifactIdentity, "scope stream digest differs")?;
    let file = File::open(path).map_err(|error| refusal(Refusal::ArtifactIdentity, error.to_string()))?;
    let mut reader = capture::bounded_gzip::Reader::new(file, capture::scope_limits());
    let mut records = Vec::new();
    while let Some(bytes) = reader.next_line().map_err(|error| refusal(Refusal::CaptureBound, error.to_string()))? {
        records.push(parse(bytes, Refusal::CaptureBound)?);
    }
    Ok(records)
}

fn same_config(left: &TraceStackScopeConfig, right: &TraceStackScopeConfig) -> bool {
    left.stack() == right.stack() && left.owner() == right.owner() && left.current() == right.current()
}

struct Frozen {
    config: TraceStackScopeConfig,
    first: SampleId,
    epoch: EpochId,
    creation: Guest,
    identities: [u32; 2],
    current: [u32; 2],
}

fn freeze(report: &PumpReport) -> Result<Frozen, Failure> {
    let sample = report.samples.iter().find(|sample| physical(sample.pc) == 0xc22f60).ok_or_else(|| refusal(Refusal::TaskContext, "scope has no raw first UI entry"))?;
    task_context(sample)?;
    let layout = capture::TASK_LAYOUTS[KernelDomain::from_identity(sample.identities[usize::from(sample.core)])?.index()];
    let start = physical(Guest(projected(&sample.task, layout.creation.0 + 20)?));
    let end = start.checked_add(projected(&sample.task, layout.creation.0 + 16)?).ok_or_else(|| refusal(Refusal::StackContext, "frozen stack endpoint overflows"))?;
    let mut cells = [Guest(0); 2];
    let mut current = [0; 2];
    for core in 0..2 {
        let domain = KernelDomain::from_identity(sample.identities[core])?;
        let cell = capture::TASK_LAYOUTS[domain.index()].current;
        cells[core] = Guest(physical(cell));
        current[core] = projected(&sample.task, cell.0)?;
    }
    let config = TraceStackScopeConfig::new([Guest(start), Guest(end)], Guest(physical(layout.task)), cells).map_err(|error| refusal(Refusal::StackContext, error))?;
    Ok(Frozen { config, first: sample.id, epoch: EpochId(sample.host_reads.epoch.0 + 1), creation: layout.creation, identities: sample.identities, current })
}

#[derive(Clone, Copy)]
struct Registration { epoch: EpochId, cursor: RowIndex, instructions: Count }
struct Site { registration: Registration, recorded: [u64; 2], outside: [u64; 2] }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Fetch { instructions: u64, core: usize, site: u32 }
enum Stop<'a> { Full(&'a Sample), Outside { sp: Guest, current: Guest } }

fn cursor(hits: &[shared::Hit], sample: &Sample) -> Result<RowIndex, Failure> {
    let mut matching = hits.iter().enumerate().filter(|(_, hit)| shared::hit_matches(hit, sample.pc, sample.core, sample.stop, &sample.registers));
    let index = matching.next().map(|(index, _)| index).ok_or_else(|| refusal(Refusal::CaptureBound, "scope activation raw hit absent"))?;
    require(matching.next().is_none(), Refusal::CaptureBound, "scope activation raw hit duplicated")?;
    Ok(RowIndex(index + 1))
}

fn registrations(report: &PumpReport, hits: &[shared::Hit], frozen: &Frozen, catalog: &Catalog) -> Result<BTreeMap<u32, Registration>, Failure> {
    let first = &report.samples[frozen.first.0];
    let activation = Registration { epoch: frozen.epoch, cursor: cursor(hits, first)?, instructions: first.stop };
    let mut expected = BTreeMap::new();
    for site in capture::SHARED_QUEUE_SITES {
        for alias in [*site, *site | 0x40000000] { expected.insert(alias, activation); }
    }
    for sample in report.samples.iter().filter(|sample| sample.id.0 > first.id.0) {
        let target = match physical(sample.pc) {
            0x725378 => queue::service(sample, queue::ServiceSlot::Receive)?,
            0x725414 => queue::service(sample, queue::ServiceSlot::Release)?,
            0x7253f8 => Guest(0x722054),
            _ => continue,
        };
        let address = physical(target);
        require(!is_ui(address) && !capture::PUMP_SITES.contains(&address) && !capture::SHARED_QUEUE_SITES.contains(&address) && address & 3 == 0, Refusal::CodeIdentity, "dynamic scoped target overlaps an unfiltered boundary")?;
        catalog.word(target).map_err(|error| refusal(Refusal::CodeIdentity, error))?;
        if expected.contains_key(&address) { continue; }
        let epoch = report.epochs.iter().find(|epoch| epoch.id.0 >= frozen.epoch.0 && epoch.start == sample.stop && epoch.id.0 >= sample.host_reads.epoch.0 && epoch.id.0 <= sample.host_reads.epoch.0 + 1).ok_or_else(|| refusal(Refusal::CaptureBound, "dynamic target has no activation epoch"))?;
        let registration = Registration { epoch: epoch.id, cursor: cursor(hits, sample)?, instructions: sample.stop };
        for alias in [address, address | 0x40000000] { expected.insert(alias, registration); }
    }
    Ok(expected)
}

fn increment(value: &mut u64) -> Result<(), Failure> {
    *value = value.checked_add(1).ok_or_else(|| refusal(Refusal::CaptureBound, "scope counter overflows"))?;
    Ok(())
}
fn add_fetch(fetches: &mut BTreeMap<Fetch, u64>, key: Fetch) -> Result<(), Failure> {
    increment(fetches.entry(key).or_default())
}
fn add_stop<'a>(stops: &mut BTreeMap<Fetch, Stop<'a>>, fetches: &mut BTreeMap<Fetch, u64>, key: Fetch, stop: Stop<'a>, endpoint: Count) -> Result<(), Failure> {
    require(key.core < 2 && key.instructions <= endpoint.0, Refusal::CaptureBound, "scope stop attribution exceeds capture")?;
    require(stops.insert(key, stop).is_none(), Refusal::CaptureBound, "scope logical stop duplicated")?;
    add_fetch(fetches, key)?;
    if key.instructions < endpoint.0 {
        let resumed = key.instructions.checked_add(1).ok_or_else(|| refusal(Refusal::CaptureBound, "scope resume clock overflows"))?;
        add_fetch(fetches, Fetch { instructions: resumed, ..key })?;
    }
    Ok(())
}

fn scalar_write(row: &Row, cell: Guest, value: &mut u32) -> Result<bool, Failure> {
    let start = u64::from(row.physical.0);
    let end = start.checked_add(row.size).ok_or_else(|| refusal(Refusal::TaskContext, "pointer write extent overflows"))?;
    let low = u64::from(cell.0);
    if start >= low + 4 || low >= end { return Ok(false); }
    require((1..=8).contains(&row.size) && physical(row.address) == row.physical.0, Refusal::TaskContext, "overlapping pointer write is not a known scalar")?;
    let scalar = row.value.ok_or_else(|| refusal(Refusal::TaskContext, "overlapping pointer write has unknown value"))?;
    require(row.size == 8 || scalar >> (row.size * 8) == 0, Refusal::TaskContext, "pointer scalar exceeds write width")?;
    let mut bytes = value.to_le_bytes();
    for address in start.max(low)..end.min(low + 4) { bytes[(address - low) as usize] = (scalar >> ((address - start) * 8)) as u8; }
    *value = u32::from_le_bytes(bytes);
    Ok(true)
}

fn sample_context(sample: &Sample, frozen: &Frozen, current: &[u32; 2]) -> Result<(), Failure> {
    require(sample.identities == frozen.identities, Refusal::TaskContext, "executing-core identity changed after scope activation")?;
    let stack = frozen.config.stack();
    require(physical(Guest(projected(&sample.task, frozen.creation.0 + 20)?)) == stack[0].0 && projected(&sample.task, frozen.creation.0 + 16)? == stack[1].0 - stack[0].0, Refusal::StackContext, "committed UI stack changed after scope activation")?;
    for core in 0..2 {
        let domain = KernelDomain::from_identity(frozen.identities[core])?;
        let observed = projected(&sample.task, capture::TASK_LAYOUTS[domain.index()].current.0)?;
        require(physical(Guest(observed)) == physical(Guest(current[core])), Refusal::TaskContext, "sample current task differs from replayed pointer writes")?;
    }
    if is_ui(physical(sample.pc)) {
        let sp = physical(sample.registers.sp);
        require(stack[0].0 <= sp && sp <= stack[1].0, Refusal::StackContext, "UI stack escaped its frozen allocation")?;
    }
    Ok(())
}

fn stopped(stop: &Stop<'_>, key: Fetch, frozen: &Frozen, current: &[u32; 2]) -> Result<(), Failure> {
    let owner = frozen.config.owner();
    let actual = Guest(physical(Guest(current[key.core])));
    let stack = frozen.config.stack();
    let sp = physical(match stop { Stop::Full(sample) => sample.registers.sp, Stop::Outside { sp, .. } => *sp });
    let inside = stack[0].0 <= sp && sp <= stack[1].0;
    require(actual != owner || inside, Refusal::StackContext, "actual UI owner stopped outside frozen stack")?;
    match stop {
        Stop::Outside { current: recorded, .. } => {
            require(*recorded == actual && actual != owner, Refusal::TaskContext, "scope omission forged current-task identity")?;
            require(!inside, Refusal::StackContext, "scope omission hides an in-stack hit")?;
        }
        Stop::Full(sample) => {
            require(actual == owner, Refusal::TaskContext, "scoped full hit belongs to another current task")?;
            require(inside, Refusal::StackContext, "scoped full hit escaped frozen stack")?;
            task_context(sample)?;
        }
    }
    Ok(())
}

fn summary(summary: &TraceStackScopeSummary, frozen: &Frozen, sites: &BTreeMap<u32, Site>) -> Result<(), Failure> {
    require(same_config(&summary.config, &frozen.config), Refusal::StackContext, "scope footer changes frozen configuration")?;
    require(summary.violation.is_none(), Refusal::StackContext, "native stack scope refused")?;
    require(summary.sites.len() == sites.len(), Refusal::CaptureBound, "scope footer site census differs")?;
    let mut seen = BTreeSet::new();
    for recorded in &summary.sites {
        let site = sites.get(&recorded.site.0).ok_or_else(|| refusal(Refusal::CaptureBound, "scope footer names an unregistered site"))?;
        require(seen.insert(recorded.site.0) && recorded.entered.0 == site.registration.instructions.0 && recorded.recorded.iter().zip(site.recorded).all(|(left, right)| left.0 == right) && recorded.outside.iter().zip(site.outside).all(|(left, right)| left.0 == right), Refusal::CaptureBound, "scope footer counters or registration clock differ")?;
    }
    Ok(())
}

pub(super) fn check(root: &Path, report: &PumpReport, hits: &[shared::Hit], catalog: &Catalog) -> Result<(), Failure> {
    let frozen = freeze(report)?;
    let binding = report.trace_scope.as_ref().ok_or_else(|| refusal(Refusal::CaptureBound, "complete capture has no scope stream"))?;
    let records = records(root, binding)?;
    let expected = registrations(report, hits, &frozen, catalog)?;
    let mut records = records.into_iter();
    let first = &report.samples[frozen.first.0];
    match records.next() {
        Some(TraceScopeRecord::Begin { sample, epoch, config, instructions }) => {
            require(sample == frozen.first && epoch == frozen.epoch && instructions == first.stop, Refusal::CaptureBound, "scope activation does not follow first raw UI entry")?;
            require(same_config(&config, &frozen.config), Refusal::StackContext, "scope bounds/owner/cells differ from first raw UI entry")?;
        }
        _ => return Err(refusal(Refusal::CaptureBound, "scope stream has no initial Begin")),
    }
    let activation = report.epochs.get(frozen.epoch.0).ok_or_else(|| refusal(Refusal::CaptureBound, "scope activation epoch absent"))?;
    require(activation.start == first.stop, Refusal::CaptureBound, "scope activation epoch has another clock")?;
    let mut sites = BTreeMap::new();
    let mut stops = BTreeMap::new();
    let mut fetches = BTreeMap::new();
    let mut footer = None;
    let mut previous_skip = first.stop.0;
    for record in records {
        require(footer.is_none(), Refusal::CaptureBound, "scope stream continues after Finish")?;
        match record {
            TraceScopeRecord::Site { site, epoch, first_full_hit, instructions } => {
                let wanted = expected.get(&site.0).ok_or_else(|| refusal(Refusal::CaptureBound, "scope registers an unapproved site"))?;
                require(epoch == wanted.epoch && instructions == wanted.instructions && wanted.cursor.0 <= first_full_hit.0 && first_full_hit.0 <= hits.len(), Refusal::CaptureBound, "scope site activation/cursor differs")?;
                require(hits[wanted.cursor.0..first_full_hit.0].iter().all(|hit| shared::hit_location(hit).2 == instructions), Refusal::CaptureBound, "scope cursor hides a hit from another activation clock")?;
                let registration = Registration { cursor: first_full_hit, ..*wanted };
                require(sites.insert(site.0, Site { registration, recorded: [0; 2], outside: [0; 2] }).is_none(), Refusal::CaptureBound, "scope site registration repeats")?;
            }
            TraceScopeRecord::Skip { hit } => {
                require(hit.core < 2 && hit.instructions.0 >= previous_skip, Refusal::CaptureBound, "scope omission clock/core differs")?;
                previous_skip = hit.instructions.0;
                let site = sites.get_mut(&hit.site.0).ok_or_else(|| refusal(Refusal::CaptureBound, "scope omission precedes site registration"))?;
                require(hit.instructions.0 >= site.registration.instructions.0, Refusal::CaptureBound, "scope omission precedes activation")?;
                increment(&mut site.outside[hit.core])?;
                add_stop(&mut stops, &mut fetches, Fetch { instructions: hit.instructions.0, core: hit.core, site: hit.site.0 }, Stop::Outside { sp: hit.sp, current: hit.current }, report.instructions)?;
            }
            TraceScopeRecord::Finish { summary } => footer = Some(summary),
            TraceScopeRecord::Begin { .. } => return Err(refusal(Refusal::StackContext, "scope configuration restarted")),
        }
    }
    require(sites.len() == expected.len(), Refusal::CaptureBound, "scope stream omits required sites")?;
    let footer = footer.ok_or_else(|| refusal(Refusal::CaptureBound, "scope stream has no Finish"))?;
    let samples: BTreeMap<_, _> = report.samples.iter().map(|sample| (Fetch { instructions: sample.stop.0, core: usize::from(sample.core), site: sample.pc.0 }, sample)).collect();
    require(samples.len() == report.samples.len(), Refusal::CaptureBound, "raw stop has duplicated samples")?;
    let mut previous_hit = 0;
    for (index, hit) in hits.iter().enumerate() {
        let (pc, core, count) = shared::hit_location(hit);
        require(count.0 >= previous_hit && count.0 <= report.instructions.0, Refusal::CaptureBound, "raw hit chronology differs")?;
        previous_hit = count.0;
        let Some(site) = sites.get_mut(&pc.0) else { continue; };
        if index < site.registration.cursor.0 { continue; }
        let key = Fetch { instructions: count.0, core: usize::from(core), site: pc.0 };
        let sample = samples.get(&key).copied().ok_or_else(|| refusal(Refusal::CaptureBound, "scoped full raw hit has no sample"))?;
        require(shared::hit_matches(hit, sample.pc, sample.core, sample.stop, &sample.registers), Refusal::TaskContext, "scoped sample differs from full raw registers")?;
        increment(&mut site.recorded[key.core])?;
        add_stop(&mut stops, &mut fetches, key, Stop::Full(sample), report.instructions)?;
    }
    let mut current = frozen.current;
    let mut bounds = [frozen.config.stack()[1].0 - frozen.config.stack()[0].0, frozen.config.stack()[0].0];
    for epoch in report.epochs.iter().skip(frozen.epoch.0) {
        let watch: Watched = gzip_record(root, &epoch.raw, capture::watch_limits(), Refusal::CaptureBound)?;
        for cell in frozen.config.current() {
            require([Access::CpuWrite, Access::HostWrite, Access::ModelBusWrite].iter().all(|kind| watch.classes.iter().any(|class| class.range[0] <= u64::from(cell.0) && class.range[1] >= u64::from(cell.0) + 4 && class.kinds.contains(kind))), Refusal::TaskContext, "current-task pointer writes lack continuous coverage")?;
        }
        let allocation = u64::from(physical(frozen.creation)) + 16;
        require([Access::CpuWrite, Access::HostWrite, Access::ModelBusWrite].iter().all(|kind| watch.classes.iter().any(|class| class.range[0] <= allocation && class.range[1] >= allocation + 8 && class.kinds.contains(kind))), Refusal::StackContext, "frozen stack allocation lacks continuous write coverage")?;
        for (address, site) in sites.iter().filter(|(_, site)| site.registration.epoch.0 <= epoch.id.0) {
            let address = u64::from(physical(Guest(*address)));
            require([Access::CpuFetch, Access::CpuWrite, Access::HostWrite, Access::ModelBusWrite].iter().all(|kind| watch.classes.iter().any(|class| class.range[0] <= address && class.range[1] >= address + 4 && class.kinds.contains(kind))), Refusal::CodeIdentity, "scoped code word lacks continuous fetch/write coverage")?;
            if !capture::SHARED_QUEUE_SITES.contains(&(address as u32)) {
                require(watch.classes.iter().any(|class| class.range == [address, address + 4] && [Access::CpuFetch, Access::CpuWrite, Access::HostWrite, Access::ModelBusWrite].iter().all(|kind| class.kinds.contains(kind))), Refusal::CodeIdentity, "dynamic target lacks its exact watched code word")?;
            }
            require(epoch.start.0 >= site.registration.instructions.0, Refusal::CaptureBound, "scoped code epoch precedes registration")?;
        }
        let mut observations = report.samples.iter().filter(|sample| sample.id.0 > frozen.first.0 && sample.host_reads.epoch == epoch.id).peekable();
        for index in 0..=watch.records.len() {
            while observations.peek().is_some_and(|sample| sample.host_reads.first.0 == index) {
                let sample = observations.next().ok_or_else(|| refusal(Refusal::Mapping, "sample observation cursor absent"))?;
                sample_context(sample, &frozen, &current)?;
            }
            let Some(row) = watch.records.get(index) else { break; };
            if matches!(row.source, Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite) {
                for core in 0..2 { scalar_write(row, frozen.config.current()[core], &mut current[core])?; }
                for (offset, bound) in bounds.iter_mut().enumerate() {
                    if scalar_write(row, Guest(physical(frozen.creation) + 16 + offset as u32 * 4), bound)? {
                        require(*bound == [frozen.config.stack()[1].0 - frozen.config.stack()[0].0, frozen.config.stack()[0].0][offset], Refusal::StackContext, "frozen stack allocation was written")?;
                    }
                }
                let end = u64::from(row.physical.0).checked_add(row.size).ok_or_else(|| refusal(Refusal::Mapping, "write endpoint overflows"))?;
                require(!sites.iter().any(|(address, site)| site.registration.epoch.0 <= epoch.id.0 && u64::from(row.physical.0) < u64::from(physical(Guest(*address))) + 4 && u64::from(physical(Guest(*address))) < end), Refusal::CodeIdentity, "registered scoped code was written")?;
            }
            if row.source != Access::CpuFetch { continue; }
            if !sites.iter().any(|(address, site)| site.registration.epoch.0 <= epoch.id.0 && physical(Guest(*address)) == row.physical.0) { continue; }
            let pc = row.pc.ok_or_else(|| refusal(Refusal::CaptureBound, "scoped fetch PC absent"))?;
            let site = sites.get(&pc.0).filter(|site| site.registration.epoch.0 <= epoch.id.0).ok_or_else(|| refusal(Refusal::CodeIdentity, "scoped fetch has another PC/alias"))?;
            let core = row.core.ok_or_else(|| refusal(Refusal::CaptureBound, "scoped fetch core absent"))?;
            let count = row.instructions.ok_or_else(|| refusal(Refusal::CaptureBound, "scoped fetch clock absent"))?;
            require(row.size == 4 && row.physical.0 == physical(pc) && physical(row.address) == row.physical.0 && count.0 >= site.registration.instructions.0, Refusal::CodeIdentity, "scoped fetch word/address differs")?;
            let key = Fetch { instructions: count.0, core: usize::from(core), site: pc.0 };
            let remaining = fetches.get_mut(&key).ok_or_else(|| refusal(Refusal::CaptureBound, "watched scoped fetch has no full hit or omission"))?;
            require(*remaining > 0, Refusal::CaptureBound, "watched scoped fetch multiplicity exceeds stops")?;
            *remaining -= 1;
            // At a shared clock, a resumed fetch precedes the next stopped fetch.
            if *remaining == 0 {
                if let Some(stop) = stops.remove(&key) { stopped(&stop, key, &frozen, &current)?; }
            }
        }
        require(observations.next().is_none(), Refusal::Mapping, "sample read chronology differs")?;
    }
    require(stops.is_empty() && fetches.values().all(|count| *count == 0), Refusal::CaptureBound, "scoped stop lacks its stopped/resumed fetch multiset")?;
    summary(&footer, &frozen, &sites)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fujimod_xm1::trace::{EnteredInstructions, ScopedHitCount, TraceStackSiteSummary, TraceStackSkip};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
    struct Fixture { root: PathBuf, report: PumpReport, hits: Vec<shared::Hit>, records: Vec<TraceScopeRecord>, rows: Vec<Row> }
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); }
    }
    fn unused() -> FileBinding { FileBinding { path: "unused".into(), sha256: Digest::of(b"") } }
    fn words(address: Guest, values: Vec<u32>) -> Words {
        Words { mapping: continuation_view::Mapping { guest: address, physical: Guest(physical(address)), bytes: ByteCount(values.len() as u32 * 4), decoder: DramChoice::One, tcm: [[0; 2]; 2] }, values, guest_equal: [true; 2] }
    }
    fn config() -> TraceStackScopeConfig {
        TraceStackScopeConfig::new([Guest(0x100000), Guest(0x100c00)], Guest(physical(capture::TASK_LAYOUTS[0].task)), capture::TASK_LAYOUTS.map(|layout| Guest(physical(layout.current)))).unwrap()
    }
    fn sample(id: usize, pc: Guest, stop: u64) -> Sample {
        let layout = capture::TASK_LAYOUTS[0];
        let mut creation = vec![0; 10];
        creation[2] = 0xc22f60;
        creation[3] = 5;
        creation[4] = 0xc00;
        creation[5] = 0x100000;
        creation[8] = 1;
        creation[9] = 1;
        let mut task = vec![0; 14];
        task[2] = layout.creation.0;
        task[8] = 1;
        let task = vec![words(layout.creation, creation), words(layout.task, task), words(layout.ids, vec![1]), words(layout.current, vec![layout.task.0]), words(capture::TASK_LAYOUTS[1].current, vec![0x2000])];
        let mut registers = Registers { r: [0; 13], sp: Guest(0x100b00), lr: Guest(0), cpsr: 0x13 };
        if id == 0 {
            registers.sp = Guest(0x100c00);
            registers.lr = Guest(0x453f0);
            registers.r[1] = layout.task.0;
            registers.r[2] = layout.creation.0;
        }
        let empty = words(Guest(0), vec![0]);
        Sample { id: SampleId(id), stop: Count(stop), resume: Count(stop + 1), counts: [stop, 0], identities: [0x4003, 0x5003], core: Core::Primary, pc, registers, code: empty.clone(), carrier: [empty.clone(), empty], task, stack: Vec::new(), state: Vec::new(), packet: Vec::new(), host_reads: HostReads { epoch: EpochId(usize::from(id != 0)), first: RowIndex(usize::from(id != 0)), end: RowIndex(usize::from(id != 0)), stopped: Count(stop) }, raw_views: unused(), input: None }
    }
    fn hit(sample: &Sample) -> shared::Hit {
        #[derive(Serialize)]
        struct RawHit { address: String, core: usize, instructions: u64, registers: BTreeMap<String, String>, sp: String, lr: String, cpsr: u32 }
        let hit = RawHit { address: format!("0x{:08x}", sample.pc.0), core: usize::from(sample.core), instructions: sample.stop.0, registers: sample.registers.r.iter().enumerate().map(|(number, word)| (format!("r{number}"), format!("0x{word:08x}"))).collect(), sp: format!("0x{:08x}", sample.registers.sp.0), lr: format!("0x{:08x}", sample.registers.lr.0), cpsr: sample.registers.cpsr };
        serde_json::from_slice(&serde_json::to_vec(&hit).unwrap()).unwrap()
    }
    fn fetch(core: Core, count: u64) -> Row {
        Row { source: Access::CpuFetch, address: Guest(0x725318), physical: Guest(0x725318), size: 4, core: Some(core), pc: Some(Guest(0x725318)), instructions: Some(Count(count)), value: Some(0xe320f000), tick: None }
    }
    fn classes() -> Vec<Class> {
        capture::SHARED_QUEUE_SITES.iter().map(|site| Class { range: [u64::from(*site), u64::from(*site) + 4], kinds: vec![Access::CpuFetch, Access::CpuWrite, Access::HostWrite, Access::ModelBusWrite] }).chain(capture::TASK_LAYOUTS.into_iter().map(|layout| Class { range: [u64::from(physical(layout.current)), u64::from(physical(layout.current)) + 4], kinds: vec![Access::CpuWrite, Access::HostWrite, Access::ModelBusWrite] })).chain(std::iter::once(Class { range: [u64::from(physical(capture::TASK_LAYOUTS[0].creation)) + 16, u64::from(physical(capture::TASK_LAYOUTS[0].creation)) + 24], kinds: vec![Access::CpuWrite, Access::HostWrite, Access::ModelBusWrite] })).collect()
    }
    fn fixture() -> Fixture {
        let root = std::env::temp_dir().join(format!("app154-scope-{}-{}", std::process::id(), NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        let samples = vec![sample(0, Guest(0xc22f60), 10), sample(1, Guest(0x725318), 20)];
        let hits = samples.iter().map(hit).collect();
        let identity = PumpIdentity { original_update: unused(), actual_update: unused(), code: unused(), carrier: unused(), symbols: unused(), admission: unused(), run: unused(), boot_rom: unused(), observer: Digest::of(b""), canonical_sources: Vec::new() };
        let loader = LoaderEvidence { schema: 1, identity: LoaderIdentity { run: Digest::of(b""), update: Digest::of(b""), rom: Digest::of(b""), executable: Digest::of(b""), sources: Vec::new() }, checkpoint: Digest::of(b""), instructions: Count(30), menu: Ok(None), error: None, machine_failure: None, events: Vec::new(), calls: Vec::new(), snapshots: Vec::new(), pulses: Vec::new(), epochs: Vec::new(), trace: unused(), static_calls: Vec::new() };
        let epochs = vec![
            PumpEpoch { id: EpochId(0), start: Count(0), end: Count(10), views: Views::Fixed, enabled: true, refused: false, teardown_error: None, classes: Vec::new(), record_count: 0, raw: unused() },
            PumpEpoch { id: EpochId(1), start: Count(10), end: Count(30), views: Views::Fixed, enabled: true, refused: false, teardown_error: None, classes: classes(), record_count: 2, raw: FileBinding { path: "watch.gz".into(), sha256: Digest::of(b"") } },
        ];
        let report = PumpReport { schema: 2, mode: Mode::Retained, identity, checkpoint: Digest::of(b""), instructions: Count(30), outcome: Outcome::Captured, loader, samples, epochs, inputs: Vec::new(), visible: Vec::new(), routes: Vec::new(), trace_scope: Some(FileBinding { path: "scope.gz".into(), sha256: Digest::of(b"") }) };
        let mut records = vec![TraceScopeRecord::Begin { sample: SampleId(0), epoch: EpochId(1), config: config(), instructions: Count(10) }];
        let mut sites = Vec::new();
        for site in capture::SHARED_QUEUE_SITES.iter().flat_map(|site| [*site, *site | 0x40000000]) {
            records.push(TraceScopeRecord::Site { site: Guest(site), epoch: EpochId(1), first_full_hit: RowIndex(1), instructions: Count(10) });
            sites.push(TraceStackSiteSummary { site: Guest(site), entered: EnteredInstructions(10), recorded: [ScopedHitCount(u64::from(site == 0x725318)), ScopedHitCount(0)], outside: [ScopedHitCount(0), ScopedHitCount(0)] });
        }
        records.push(TraceScopeRecord::Finish { summary: TraceStackScopeSummary { config: config(), sites, violation: None } });
        Fixture { root, report, hits, records, rows: vec![fetch(Core::Primary, 20), fetch(Core::Primary, 21)] }
    }
    impl Fixture {
        fn save(&mut self) {
            let mut writer = capture::bounded_gzip::Writer::new(Vec::new(), capture::scope_limits());
            for record in &self.records { writer.append(&serde_json::to_vec(record).unwrap()).unwrap(); }
            let bytes = writer.finish().unwrap();
            std::fs::write(self.root.join("scope.gz"), &bytes).unwrap();
            self.report.trace_scope.as_mut().unwrap().sha256 = Digest::of(&bytes);
            let mut writer = capture::bounded_gzip::Writer::new(Vec::new(), capture::watch_limits());
            let watch = Watched { enabled: true, violation: None, classes: classes(), ranges: Vec::new(), views: Views::Fixed, records: std::mem::take(&mut self.rows) };
            writer.append(&serde_json::to_vec(&watch).unwrap()).unwrap();
            self.rows = watch.records;
            self.report.epochs[1].record_count = self.rows.len();
            let bytes = writer.finish().unwrap();
            std::fs::write(self.root.join("watch.gz"), &bytes).unwrap();
            self.report.epochs[1].raw.sha256 = Digest::of(&bytes);
        }
        fn omission(&mut self, core: Core, current: Guest, sp: Guest) {
            self.report.samples.pop();
            self.hits.pop();
            self.rows = vec![fetch(core, 20), fetch(core, 21)];
            let last = self.records.len() - 1;
            self.records.insert(last, TraceScopeRecord::Skip { hit: TraceStackSkip { core: usize::from(core), site: Guest(0x725318), instructions: EnteredInstructions(20), sp, current } });
            let Some(TraceScopeRecord::Finish { summary }) = self.records.last_mut() else { unreachable!() };
            let site = summary.sites.iter_mut().find(|site| site.site == Guest(0x725318)).unwrap();
            site.recorded = [ScopedHitCount(0), ScopedHitCount(0)];
            site.outside[usize::from(core)] = ScopedHitCount(1);
        }
        fn check(&self) -> Result<(), Failure> { super::check(&self.root, &self.report, &self.hits, &Catalog(Vec::new())) }
    }

    #[test]
    fn admits_complete_scoped_hit_and_independent_non_owner_omission() {
        let mut full = fixture();
        full.save();
        assert!(full.check().is_ok());
        let mut outside = fixture();
        outside.omission(Core::Secondary, Guest(0x2000), Guest(0x120000));
        outside.save();
        assert!(outside.check().is_ok());
    }

    #[test]
    fn rejects_omitted_in_scope_hit_even_when_footer_claims_zero() {
        let mut fixture = fixture();
        fixture.hits.pop();
        fixture.report.samples.pop();
        let Some(TraceScopeRecord::Finish { summary }) = fixture.records.last_mut() else { unreachable!() };
        summary.sites.iter_mut().find(|site| site.site == Guest(0x725318)).unwrap().recorded[0] = ScopedHitCount(0);
        fixture.save();
        assert!(matches!(fixture.check(), Err(Failure { reason: Refusal::CaptureBound, .. })));
    }

    #[test]
    fn rejects_incorrect_outside_counts() {
        let mut fixture = fixture();
        fixture.omission(Core::Secondary, Guest(0x2000), Guest(0x120000));
        let Some(TraceScopeRecord::Finish { summary }) = fixture.records.last_mut() else { unreachable!() };
        summary.sites.iter_mut().find(|site| site.site == Guest(0x725318)).unwrap().outside[1] = ScopedHitCount(0);
        fixture.save();
        assert!(matches!(fixture.check(), Err(Failure { reason: Refusal::CaptureBound, .. })));
    }

    #[test]
    fn refuses_actual_owner_outside_frozen_stack() {
        let mut fixture = fixture();
        fixture.omission(Core::Primary, config().owner(), Guest(0x120000));
        fixture.save();
        assert!(matches!(fixture.check(), Err(Failure { reason: Refusal::StackContext, .. })));
    }

    #[test]
    fn refuses_forged_omission_identity() {
        let mut fixture = fixture();
        fixture.omission(Core::Secondary, config().owner(), Guest(0x120000));
        fixture.save();
        assert!(matches!(fixture.check(), Err(Failure { reason: Refusal::TaskContext, .. })));
    }

    #[test]
    fn replays_partial_scalar_current_task_writes_and_refuses_unknown_writes() {
        let mut fixture = fixture();
        fixture.omission(Core::Secondary, Guest(0x12342000), Guest(0x120000));
        let cell = config().current()[1];
        fixture.rows.insert(0, Row { source: Access::HostWrite, address: Guest(cell.0 + 2), physical: Guest(cell.0 + 2), size: 2, core: None, pc: None, instructions: None, value: Some(0x1234), tick: None });
        fixture.save();
        assert!(fixture.check().is_ok());
        fixture.rows[0].value = None;
        fixture.save();
        assert!(matches!(fixture.check(), Err(Failure { reason: Refusal::TaskContext, .. })));
    }

    #[test]
    fn refuses_missing_scope_footer_and_truncated_gzip() {
        let mut fixture = fixture();
        fixture.save();
        let mut bytes = std::fs::read(fixture.root.join("scope.gz")).unwrap();
        bytes.pop();
        std::fs::write(fixture.root.join("scope.gz"), &bytes).unwrap();
        fixture.report.trace_scope.as_mut().unwrap().sha256 = Digest::of(&bytes);
        assert!(matches!(fixture.check(), Err(Failure { reason: Refusal::CaptureBound, .. })));
        fixture.records.pop();
        fixture.save();
        assert!(matches!(fixture.check(), Err(Failure { reason: Refusal::CaptureBound, .. })));
    }

    #[test]
    fn distinguishes_resumed_fetch_from_next_stop_at_the_same_clock() {
        let mut fixture = fixture();
        fixture.omission(Core::Secondary, Guest(0x2000), Guest(0x120000));
        let last = fixture.records.len() - 1;
        fixture.records.insert(last, TraceScopeRecord::Skip { hit: TraceStackSkip { core: 1, site: Guest(0x725318), instructions: EnteredInstructions(21), sp: Guest(0x120000), current: Guest(0x3000) } });
        let Some(TraceScopeRecord::Finish { summary }) = fixture.records.last_mut() else { unreachable!() };
        summary.sites.iter_mut().find(|site| site.site == Guest(0x725318)).unwrap().outside[1] = ScopedHitCount(2);
        let cell = config().current()[1];
        fixture.rows.push(Row { source: Access::ModelBusWrite, address: cell, physical: cell, size: 4, core: None, pc: None, instructions: None, value: Some(0x3000), tick: None });
        fixture.rows.push(fetch(Core::Secondary, 21));
        fixture.rows.push(fetch(Core::Secondary, 22));
        fixture.save();
        assert!(fixture.check().is_ok());
    }

    #[test]
    fn refuses_frozen_allocation_mutation_between_samples() {
        let mut fixture = fixture();
        fixture.omission(Core::Secondary, Guest(0x2000), Guest(0x120000));
        let cell = Guest(physical(capture::TASK_LAYOUTS[0].creation) + 20);
        fixture.rows.insert(0, Row { source: Access::CpuWrite, address: cell, physical: cell, size: 4, core: Some(Core::Primary), pc: Some(Guest(0x8000)), instructions: Some(Count(15)), value: Some(0x110000), tick: None });
        fixture.save();
        assert!(matches!(fixture.check(), Err(Failure { reason: Refusal::StackContext, .. })));
    }

    #[test]
    fn retains_stop_resume_pairs_across_watch_epochs() {
        let mut fixture = fixture();
        fixture.save();
        let resumed = fixture.rows.split_off(1);
        let stopped = std::mem::take(&mut fixture.rows);
        let save = |name: &str, records| {
            let watch = Watched { enabled: true, violation: None, classes: classes(), ranges: Vec::new(), views: Views::Fixed, records };
            let mut writer = capture::bounded_gzip::Writer::new(Vec::new(), capture::watch_limits());
            writer.append(&serde_json::to_vec(&watch).unwrap()).unwrap();
            let bytes = writer.finish().unwrap();
            std::fs::write(fixture.root.join(name), &bytes).unwrap();
            FileBinding { path: name.into(), sha256: Digest::of(&bytes) }
        };
        fixture.report.epochs[1].end = Count(20);
        fixture.report.epochs[1].record_count = 1;
        fixture.report.epochs[1].raw = save("watch.gz", stopped);
        fixture.report.epochs.push(PumpEpoch { id: EpochId(2), start: Count(20), end: Count(30), views: Views::Fixed, enabled: true, refused: false, teardown_error: None, classes: classes(), record_count: 1, raw: save("resumed.gz", resumed) });
        assert!(fixture.check().is_ok());
        fixture.report.epochs[2].raw = save("resumed.gz", Vec::new());
        fixture.report.epochs[2].record_count = 0;
        assert!(matches!(fixture.check(), Err(Failure { reason: Refusal::CaptureBound, .. })));
    }

    #[test]
    fn allows_unresumed_stop_only_at_final_entered_endpoint() {
        let mut fixture = fixture();
        fixture.report.instructions = Count(20);
        fixture.report.loader.instructions = Count(20);
        fixture.report.epochs[1].end = Count(20);
        fixture.rows.pop();
        fixture.save();
        assert!(fixture.check().is_ok());
        fixture.report.instructions = Count(21);
        fixture.report.loader.instructions = Count(21);
        fixture.report.epochs[1].end = Count(21);
        assert!(matches!(fixture.check(), Err(Failure { reason: Refusal::CaptureBound, .. })));
    }
}
