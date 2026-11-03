use super::*;
use super::contract::{CARRIERS, SENTINEL};

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Role { Stock, Owned }

pub(super) fn classes(recorder: &Recorder) -> Vec<WatchClass> {
    let mut result: Vec<_> = CARRIERS.iter().map(|(low, high, _)| WatchClass {
        range: [u64::from(*low), u64::from(*high)], kinds: vec![Access::CpuWrite, Access::CpuFetch, Access::HostWrite, Access::ModelBusWrite],
    }).collect();
    result.push(WatchClass { range: [u64::from(SENTINEL), u64::from(SENTINEL + 4)], kinds: vec![Access::CpuWrite, Access::CpuFetch, Access::HostWrite, Access::ModelBusWrite] });
    result.extend(ENTRIES.into_iter().chain(IMAGE5_LOAD_POINTS).map(|pc| WatchClass { range: [u64::from(pc), u64::from(pc + 4)], kinds: vec![Access::CpuFetch] }));
    let allocations = recorder.summary().live_allocations;
    let mut extents: Vec<_> = allocations.iter().map(|allocation| {
        let start = u64::from(allocation.address.0 & !0x40000000);
        [start, start + allocation.requested.0.max(1)]
    }).collect();
    extents.sort_unstable();
    let group = extents.len().div_ceil(256 - result.len()).max(1);
    for chunk in extents.chunks(group) {
        result.push(WatchClass {
            range: [chunk[0][0], chunk.iter().map(|extent| extent[1]).max().unwrap_or(chunk[0][1])],
            kinds: vec![Access::CpuWrite],
        });
    }
    result
}

pub(super) fn ranges_for(recorder: &Recorder) -> Vec<WatchRange> {
    classes(recorder).into_iter().map(|class| {
        let mut kinds = if class.kinds.contains(&Access::CpuFetch) { WatchKinds::CPU_FETCH }
            else if class.kinds.contains(&Access::CpuRead) { WatchKinds::CPU_READ } else { WatchKinds::CPU_WRITE };
        if class.kinds.contains(&Access::CpuWrite) { kinds = kinds.union(WatchKinds::CPU_WRITE); }
        if class.kinds.contains(&Access::HostWrite) || class.kinds.contains(&Access::ModelBusWrite) { kinds = kinds.union(WatchKinds::WRITES); }
        WatchRange { start: class.range[0], end: class.range[1], kinds }
    }).collect()
}

#[derive(Deserialize, Serialize)]
pub(super) struct SealWord { pub address: u32, pub physical: u32, pub aliases: [[u32; 2]; 2] }

pub(super) fn sentinel(machine: &Machine, maps: &[MemoryMap]) -> Result<SealWord, String> {
    let banks = tcm(machine)?;
    let mut bytes = [0; 4];
    machine.session.board.read_physical(u64::from(SENTINEL), &mut bytes)?;
    let physical = u32::from_le_bytes(bytes);
    let mut aliases = [[0; 2]; 2];
    for (core, values) in aliases.iter_mut().enumerate() {
        for (alias, value) in values.iter_mut().enumerate() {
            *value = word(machine, maps, &banks, core, SENTINEL | if alias == 1 { 0x40000000 } else { 0 })?;
        }
    }
    Ok(SealWord { address: SENTINEL, physical, aliases })
}

pub(super) fn seal(machine: &Machine, maps: &[MemoryMap]) -> Result<Vec<SealWord>, String> {
    let banks = tcm(machine)?;
    let mut result = Vec::new();
    for (low, high, image) in CARRIERS {
        if image != 4 { continue; }
        for address in (low..high).step_by(4) {
            let mut bytes = [0; 4];
            machine.session.board.read_physical(u64::from(address), &mut bytes)?;
            let physical = u32::from_le_bytes(bytes);
            let mut aliases = [[0; 2]; 2];
            for (core, views) in aliases.iter_mut().enumerate() {
                for (alias, value) in views.iter_mut().enumerate() {
                    *value = word(machine, maps, &banks, core, address | if alias == 1 { 0x40000000 } else { 0 })?;
                }
            }
            result.push(SealWord { address, physical, aliases });
        }
    }
    Ok(result)
}

pub(super) fn admit_seal(words: &[SealWord], images: &[Image]) -> Result<(), String> {
    let addresses: Vec<_> = CARRIERS.into_iter().filter(|(_, _, image)| *image == 4).flat_map(|(low, high, _)| (low..high).step_by(4)).collect();
    require(words.len() == addresses.len(), "image4 seal has missing bytes")?;
    for (view, address) in words.iter().zip(addresses) {
        let wanted = expected(images, address)?;
        require(view.address == address && view.physical == wanted && view.aliases == [[wanted; 2]; 2], "image4 seal bytes or aliases differ")?;
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct Guard { pub sealed: bool, sentinel_writes: usize, pub payload_writes_checked: u64, pub fetches_checked: u64 }

fn overlaps(low: u64, high: u64, row: &AccessRecord) -> bool {
    row.physical < high && low < row.physical.saturating_add(row.size)
}

impl Guard {
    pub(super) fn finish(&self, role: Role) -> Result<(), String> {
        require(self.sealed && (role == Role::Stock || self.sentinel_writes == 1), "owned initialization or initial carrier seal is incomplete")
    }

    pub(super) fn batch(&mut self, batch: &AccessBatch, recorder: &Recorder, phase: Image5Phase, role: Role,
        owned: &BTreeSet<u32>, begin: [u64; 2], end: [u64; 2], next: Option<&Boundary>, seal: Option<&[SealWord]>, sentinel: &SealWord, images: &[Image]) -> Result<(), String> {
        let wanted = classes(recorder);
        require(batch.enabled && batch.violation.is_none() && batch.records.len() <= WATCH_LIMIT
            && batch.classes == wanted && batch.ranges == merged_ranges(&wanted) && batch.views == Views::Observe,
            "cutover watch is incomplete or has different coverage")?;
        require(begin.iter().zip(end).all(|(before, after)| *before <= after), "cutover watch clocks moved backwards")?;
        let start_clock = begin[0].checked_add(begin[1]).ok_or("cutover clock overflow")?;
        let end_clock = end[0].checked_add(end[1]).ok_or("cutover clock overflow")?;
        let mut previous = start_clock;
        let mut first_image4_fetch = false;
        let summary = recorder.summary();
        let contexts = recorder.records().last().map(|event| event.active_context).unwrap_or([None; 2]);
        for row in &batch.records {
            let high = row.physical.checked_add(row.size).ok_or("cutover memory extent overflow")?;
            require(row.size > 0 && wanted.iter().any(|class| class.range[0] < high && row.physical < class.range[1] && class.kinds.contains(&row.source)), "cutover watch row outside coverage")?;
            let cpu = matches!(row.source, Access::CpuRead | Access::CpuWrite | Access::CpuFetch);
            if cpu {
                let clock = row.instructions.ok_or("cutover CPU event lacks a clock")?;
                require(row.core.is_some_and(|core| core < 2) && row.pc.is_some() && clock >= previous && clock > start_clock && clock <= end_clock,
                    "cutover CPU event attribution or clock differs")?;
                previous = clock;
            }
            if row.source == Access::CpuFetch {
                let physical = u32::try_from(row.physical).map_err(|error| error.to_string())?;
                require(row.size == 4 && row.pc == Some(row.address) && row.address & !0x40000000 == row.physical,
                    "cutover fetch is not a mapped A32 word")?;
                if CARRIERS.iter().any(|(low, high, _)| u64::from(*low) < row.physical + row.size && row.physical < u64::from(*high)) || physical == SENTINEL {
                    require(physical != SENTINEL && (role == Role::Stock || owned.contains(&physical)), "stock-body fetch or unknown owned executable PC")?;
                    if physical >= 0xbf1c1c { require(phase == Image5Phase::Admitted, "image5 fetch precedes publication")?; }
                    else if !self.sealed {
                        require(!first_image4_fetch && next.is_some_and(|boundary| boundary.snapshot.pc.0 & !0x40000000 == physical), "first image4 fetch has no seal boundary")?;
                        first_image4_fetch = true;
                        self.sealed = true;
                    }
                    self.fetches_checked += 1;
                }
            }
            if matches!(row.source, Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite) {
                if overlaps(u64::from(SENTINEL), u64::from(SENTINEL + 4), row) {
                    let installing = !self.sealed && row.source == Access::ModelBusWrite && row.physical >= 0x61c400 && high <= 0xbf1a00;
                    let stock_bins = role == Role::Stock && row.source == Access::CpuWrite && self.sealed;
                    let owned_initialization = role == Role::Owned && self.sealed && row.source == Access::CpuWrite
                        && row.physical == u64::from(SENTINEL) && row.size == 4
                        && row.pc.is_some_and(|pc| u32::try_from(pc & !0x40000000).is_ok_and(|pc| owned.contains(&pc)));
                    require(installing || stock_bins || owned_initialization, "sentinel write has no admitted initialization source")?;
                    if owned_initialization {
                        self.sentinel_writes += 1;
                        require(self.sentinel_writes == 1, "owned sentinel initialization repeated")?;
                    }
                }
                for (low, high, image) in CARRIERS {
                    if !overlaps(u64::from(low), u64::from(high), row) { continue; }
                    let initial_image4 = image == 4 && !self.sealed && row.source == Access::ModelBusWrite
                        && row.physical >= 0x61c400 && row.physical.checked_add(row.size).is_some_and(|end| end <= 0xbf1a00);
                    let installing_image5 = image == 5 && phase == Image5Phase::Installing && row.source == Access::ModelBusWrite
                        && row.physical >= 0xbf1a00 && row.physical.checked_add(row.size).is_some_and(|end| end <= 0x10e3860);
                    let stock_bins = role == Role::Stock && row.source == Access::CpuWrite && self.sealed
                        && low == 0x89d4fc && row.physical >= u64::from(SENTINEL) && row.physical.checked_add(row.size).is_some_and(|end| end <= 0x89d908);
                    require(initial_image4 || installing_image5 || stock_bins, "retired carrier modified outside its installation lifecycle")?;
                }
            }
            if row.source == Access::CpuWrite {
                let pc = u32::try_from(row.pc.ok_or("payload write has no PC")?).map_err(|error| error.to_string())? & !0x40000000;
                let core = row.core.ok_or("payload write has no core")?;
                let pending = || summary.pending.iter().filter(|frame| Some(frame.entry.context) == contexts[core]);
                let helper = (0x89d908..0x89da3c).contains(&pc) || (0x43550..0x4357c).contains(&pc);
                let allocator = CODE.iter().any(|(low, high)| *low <= pc && pc < *high) || owned.contains(&pc)
                    || helper && pending().any(|frame| matches!(frame.operation, Operation::Malloc | Operation::Free | Operation::Realloc | Operation::Calloc));
                if !allocator { continue; }
                for allocation in &summary.live_allocations {
                    let low = u64::from(allocation.address.0 & !0x40000000);
                    if !overlaps(low, low + allocation.requested.0, row) { continue; }
                    self.payload_writes_checked += 1;
                    let released = pending().any(|frame| frame.operation == Operation::Free && frame.entry.registers[1] == allocation.address.0);
                    let resized = pending().any(|frame| frame.operation == Operation::Realloc && frame.entry.registers[1] == allocation.address.0);
                    let initializing = pending().any(|frame| matches!(frame.operation, Operation::Malloc | Operation::Calloc | Operation::Realloc)
                        && frame.invocation == allocation.logical_invocation);
                    require(released || resized || initializing, "allocator wrote another live allocation payload")?;
                }
            }
        }
        require(sentinel.address == SENTINEL && sentinel.aliases == [[sentinel.physical; 2]; 2], "sentinel physical or alias views differ")?;
        if role == Role::Owned {
            let wanted = if self.sentinel_writes == 0 { 0 } else { 0x48454150 };
            require(sentinel.physical == wanted, "owned sentinel initialization value differs")?;
        }
        require(first_image4_fetch == seal.is_some(), "image4 seal is missing or detached from first fetch")?;
        if let Some(words) = seal { admit_seal(words, images)?; }
        if let Some(boundary) = next {
            let pc = boundary.snapshot.pc.0;
            if ENTRIES.contains(&(pc & !0x40000000)) || IMAGE5_LOAD_POINTS.contains(&(pc & !0x40000000)) || first_image4_fetch {
                require(batch.records.iter().any(|row| row.source == Access::CpuFetch && row.core == Some(boundary.snapshot.core.0)
                    && row.address == u64::from(pc) && row.instructions == Some(end_clock)), "boundary lacks its watched fetch")?;
            }
        }
        Ok(())
    }
}
