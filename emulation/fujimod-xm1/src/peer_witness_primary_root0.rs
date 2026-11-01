use super::*;

const ROOTS: (u64, u64) = (0x96bd20, 0x96bd28);
const HEADERS: [(u64, u64); 2] = [(0x40010, 0x40014), (0xd0010, 0xd0014)];
const CODE: [(u32, &[u32]); 4] = [
    (0x725a04, &[0xe92d4800,0xe28db004,0xe30b0d20,0xe3400096,0xe3003000,0xe3403004,0xe5933010,0xe5803000,0xe3003000,0xe340300d,0xe5933010,0xe5803004]),
    (0x725b74, &[0xe92d48f0,0xe28db014,0xe1a05000,0xe30b4d20,0xe3404096,0xe5942000,0xe3013618,0xe7923003,0xe12fff33,0xe1a06000,0xe7942100,0xe2453001,0xe3a0102c,0xe0212391,0xe5911184,0xe5911000]),
    (0x412bc, &[0xe92d4800,0xe28db004,0xeb00053b,0xe8bd8800]),
    (0x427b8, &[0xee121e10,0xe3a02901,0xe2822003,0xe1510002,0x03a00000,0x13a00001,0xe12fff1e]),
];

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Kind { PrimaryRoot0 }
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Evidence { SourceOnly }
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum PoolPremise { RequiresLivePreimage }
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StackPremise { LiveConsumerContext }
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Premise {
    NaturalPrimaryArrival, OwnedCarrierCommittedAndImmutable, CurrentDecoderAndBothCoreAliases,
    BothCoreTcmExclusions, CallerFrameAndDerivedScratchBacking, ConsumerCallerStackBacking,
    RootPairInstallationObserved, PrimaryCp14SelectionObserved,
    #[serde(rename = "descriptor-identifier-in-1..120")]
    DescriptorIdentifierBounded,
    RetainedPoolBackingAndContents, ConsumerCodeFetchable, NoInterveningSelectedWordOrSelectorWriter,
    AlternateAndComputedCallerClosureOpen,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Installation { code: [u32; 2], header_words: [u32; 2], root_words: [u32; 2], root_values: [u32; 2] }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection { extension_offset: u32, service: Guest, identity_helper: Guest, cp14_identity: Identity, root_index: usize }
#[derive(Clone, Copy, Deserialize)]
#[serde(try_from = "u32")]
struct DescriptorId(u32);
impl TryFrom<u32> for DescriptorId {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        require((1..=120).contains(&value), "root0 descriptor identifier leaves 1..120")?;
        Ok(Self(value))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishedAccess { identifier: DescriptorId, table_word: Guest, pool_word: Guest }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    entry: Guest, pointer_read: Guest, pool_read: Guest, after_pool_read: Guest,
    identifiers: [u32; 2], record_bytes: u32, root_relative_field: u32,
    retained_pool_word_value: PoolPremise, published_accesses: Vec<PublishedAccess>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StackSource { saved_bytes: u32, selector_saved_bytes: u32, entry_sp_premise: StackPremise }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Code { start: Guest, words: Vec<Word> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::peer_witness) struct Contract {
    kind: Kind, evidence: Evidence, source_sha256: Sha256Digest, loader_a_sha256: Sha256Digest, image4_sha256: Sha256Digest,
    installation: Installation, selection: Selection, descriptor: Descriptor, consumer_stack_source: StackSource,
    unproved_runtime_premises: Vec<Premise>, code: Vec<Code>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::peer_witness) struct RootWire { schema: u32, initializer: PrimaryWire, consumer: Contract }
impl RootWire {
    pub(in crate::peer_witness) fn validate(self) -> Result<PrimaryContract, String> {
        require(self.schema == 9, "primary root0 requires schema 9")?;
        self.consumer.validate()?;
        let mut initializer = self.initializer.validate()?;
        initializer.consumer = Some(self.consumer);
        Ok(initializer)
    }
}
impl Contract {
    fn validate(&self) -> Result<(), String> {
        require(self.source_sha256 == Sha256Digest::parse(SOURCE)? && self.loader_a_sha256 == Sha256Digest::parse(LOADER_SHA)?
            && self.image4_sha256 == Sha256Digest::parse("f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")?,
            "root0 canonical source identities differ")?;
        require(self.installation.code == [0x725a04,0x725a34] && self.installation.header_words == [0x40010,0xd0010]
            && self.installation.root_words == [0x96bd20,0x96bd24] && self.installation.root_values == [0x4004dae0,0x400ddaa0]
            && self.selection.extension_offset == 0x1618 && self.selection.service.0 == 0x412bc
            && self.selection.identity_helper.0 == 0x427b8 && self.selection.cp14_identity.0 == 0x4003 && self.selection.root_index == 0,
            "root0 installation or selection source differs")?;
        let descriptor = &self.descriptor;
        require(descriptor.entry.0 == 0x725b74 && descriptor.pointer_read.0 == 0x725bac && descriptor.pool_read.0 == 0x725bb0
            && descriptor.after_pool_read.0 == 0x725bb4 && descriptor.identifiers == [1,120]
            && descriptor.record_bytes == 44 && descriptor.root_relative_field == 0x184
            && descriptor.published_accesses.len() == 120 && self.consumer_stack_source.saved_bytes == 24
            && self.consumer_stack_source.selector_saved_bytes == 8, "root0 descriptor or dynamic stack source differs")?;
        for (index, access) in descriptor.published_accesses.iter().enumerate() {
            require(access.identifier.0 == index as u32 + 1 && access.table_word.0 == 0x4004dae0 + index as u32 * 44 + 0x184
                && access.pool_word.0 == 0x4005beac + index as u32 * 40 + 8, "root0 published descriptor access differs")?;
        }
        require(self.code.len() == CODE.len() && self.code.iter().zip(CODE).all(|(actual, (start, words))|
            actual.start.0 == start && actual.words.iter().map(|word| word.0).eq(words.iter().copied())), "root0 code source differs")?;
        require(self.unproved_runtime_premises == [Premise::NaturalPrimaryArrival,Premise::OwnedCarrierCommittedAndImmutable,
            Premise::CurrentDecoderAndBothCoreAliases,Premise::BothCoreTcmExclusions,Premise::CallerFrameAndDerivedScratchBacking,
            Premise::ConsumerCallerStackBacking,Premise::RootPairInstallationObserved,Premise::PrimaryCp14SelectionObserved,
            Premise::DescriptorIdentifierBounded,Premise::RetainedPoolBackingAndContents,Premise::ConsumerCodeFetchable,
            Premise::NoInterveningSelectedWordOrSelectorWriter,Premise::AlternateAndComputedCallerClosureOpen], "root0 runtime premises differ")
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Boundary { Installation, Entry, Selected, After }
impl Boundary {
    fn address(self) -> u32 { match self { Self::Installation => 0x725a04, Self::Entry => 0x725b74, Self::Selected => 0x725b9c, Self::After => 0x725bb4 } }
    fn label(self) -> &'static str { match self { Self::Installation => "root0-installation", Self::Entry => "root0-entry", Self::Selected => "root0-selected", Self::After => "root0-after" } }
}
#[derive(Serialize)]
struct BoundaryObservation { boundary: Boundary, context: Context }
#[derive(Serialize)]
struct Continuation { start: Context, roots_start: Views, prelude_watch: WatchState, watch: Option<WatchState> }
#[derive(Serialize)]
struct CodeViews { start: Guest, views: Views }
#[derive(Serialize)]
struct Observation {
    entry: Context, selected: Context, after: Context, roots_before: Views, roots_after: Views,
    table_before: Views, table_after: Views, stack_before: Views, stack_after: Views,
    pool_before: Views, pool_after: Views, code: Vec<CodeViews>, decoder: Decoder, aliases: Vec<AliasBinding>,
}
#[derive(Serialize)]
struct Receipt<'a, 'b> {
    schema: u32, result: WitnessResult, status: ObservationStatus, error: Option<&'a str>, machine_failure: Option<&'a str>,
    instructions: Instructions, cp14_identities: [Identity; 2], r#final: Context,
    boundaries: Vec<BoundaryObservation>, initializer: &'a PrimaryReceipt<'b>, continuation: Option<Continuation>, consumer: Option<Observation>,
}

fn code_spans() -> impl Iterator<Item = (u64, u64)> {
    CODE.into_iter().map(|(start, words)| (u64::from(start), u64::from(start) + words.len() as u64 * 4))
}
fn span(extent: (u64, u64)) -> Result<ScopeSpan, String> { PhysicalRange::new(extent.0, extent.1).map(ScopeSpan::Physical) }
fn begin(machine: &mut Machine, maps: &[factory::MemoryMap], body: Option<((u64, u64), (u64, u64))>) -> Result<Box<ScopedMemoryWatch>, String> {
    let class = |(start, end), kinds| WatchRange { start, end, kinds };
    let data = WatchKinds::CPU_READ.union(WatchKinds::WRITES);
    let mut classes = vec![class(PRIMARY_TABLE, data), class(ROOTS, data)];
    let confinement = if let Some((stack, pool)) = body {
        classes.extend([class(stack, data), class(pool, data)]);
        classes.extend(code_spans().map(|extent| class(extent, WatchKinds::WRITES)));
        classes.extend(HEADERS.map(|extent| class(extent, WatchKinds::WRITES)));
        let resources = [PRIMARY_TABLE,ROOTS,stack,pool].into_iter().chain(code_spans()).chain(HEADERS).map(span).collect::<Result<Vec<_>, _>>()?;
        vec![ConfinementRule { cores: CoreSet::new(&[0])?, access: Access::Write, rule: Confinement::Allow(vec![span(stack)?]) },
            ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Write, rule: Confinement::Forbid(resources) },
            ConfinementRule { cores: CoreSet::new(&[0])?, access: Access::Read,
                rule: Confinement::Allow([stack,ROOTS,PRIMARY_TABLE,pool].into_iter().map(span).collect::<Result<_, _>>()?) }]
    } else {
        classes.extend(HEADERS.map(|extent| class(extent, data)));
        classes.extend(code_spans().enumerate().map(|(index, extent)| class(extent,
            if index < 2 { WatchKinds::CPU_FETCH.union(WatchKinds::WRITES) } else { WatchKinds::WRITES })));
        vec![ConfinementRule { cores: CoreSet::new(&[0,1])?, access: Access::Write,
            rule: Confinement::Forbid(HEADERS.into_iter().chain(code_spans().skip(2)).map(span).collect::<Result<_, _>>()?) }]
    };
    machine.session.board.memory_watch_begin_scoped(ScopedWatch { classes, limit: WATCH_LIMIT, confinement,
        counters: Vec::new(), coverage: Vec::new() }, maps.to_vec())
}
fn observed(watch: &ScopedMemoryWatch) -> Result<WatchState, String> {
    let state: WatchState = serde_json::from_value(watch.state()).map_err(|error| error.to_string())?;
    require(state.enabled && state.violation.is_none() && state.fault.is_none(), "root0 watch faulted")?;
    Ok(state)
}
fn stop(machine: &mut Machine, boundary: Boundary, start: &Context, boundaries: &mut Vec<BoundaryObservation>) -> Result<Option<Context>, String> {
    machine.traces.arm_primary(&machine.session, BTreeMap::from([(boundary.address(), boundary.label().into())]))?;
    machine.traces.stop_labels = [boundary.label().into()].into_iter().collect();
    let previous = machine.traces.hits.len();
    let remaining = BUDGET.checked_sub(machine.instructions()).ok_or("root0 instruction deadline exceeded")?;
    if remaining == 0 { return Ok(None); }
    machine.run(remaining); machine.require_running()?;
    let hits = &machine.traces.hits[previous..];
    if hits.is_empty() { require(machine.instructions() == BUDGET, "root0 stopped before finite deadline")?; return Ok(None); }
    require(hits.len() == 1 && hits[0].core == 0 && hits[0].site == boundary.label()
        && hit_address(&hits[0])? == boundary.address(), "root0 boundary attribution differs")?;
    let context = context_for(machine, 0)?;
    require(context.pc.0 == boundary.address() && context.tcm == start.tcm
        && context.counts.0.iter().sum::<u64>() <= BUDGET && context.counts.0.iter().zip(start.counts.0).all(|(a,b)| *a >= b),
        "root0 preexecution context, TCM or chronology differs")?;
    boundaries.push(BoundaryObservation { boundary, context: context.clone() });
    Ok(Some(context))
}
fn snapshot(machine: &Machine, maps: &[factory::MemoryMap], extent: (u64, u64)) -> Result<Views, String> {
    views(machine, maps, ZERO_GUEST + extent.0, (extent.1 - extent.0) as usize)
}
fn preimage(event: &Event, bytes: &[u8]) -> bool {
    event.before.as_ref().is_some_and(|raw| usize::from(raw.len) == bytes.len() && raw.bytes[..bytes.len()] == *bytes)
}
fn word(bytes: &[u8]) -> Result<u32, String> { Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| "root0 word extent differs")?)) }
fn raw(event: &Event, last_instruction: &mut u64, last_tick: &mut u64, end: u64) -> Result<u32, String> {
    let instructions = event.instructions.ok_or("root0 access lacks instruction clock")?;
    let tick = event.tick.as_ref().ok_or("root0 access lacks tick clock")?.0;
    let physical = event.physical.ok_or("root0 access lacks physical attribution")?;
    require(event.size == 4 && event.byte_order == Some(ByteOrder::Little) && event.tcm.is_none()
        && event.pc.is_some() && matches!(event.core, Some(0 | 1))
        && (event.address == physical || event.address == ZERO_GUEST + physical)
        && instructions >= *last_instruction && instructions <= end && tick >= *last_tick,
        "root0 raw event attribution or chronology differs")?;
    let value = event.value.and_then(|value| u32::try_from(value).ok()).ok_or("root0 access lacks word value")?;
    *last_instruction = instructions; *last_tick = tick;
    Ok(value)
}

fn prelude(state: &WatchState, continuation: &Continuation, installation: &Context, entry: &Context,
    table_start: &[u8], table_before: &[u8], descriptor: u64) -> Result<(), String> {
    let mut table = table_start.to_vec();
    let mut roots = continuation.roots_start.physical.clone();
    let mut next_install = 0;
    let expected = [(Source::CpuRead,0x40010,0x725a1c,0x4004dae0_u64),(Source::CpuWrite,ROOTS.0,0x725a20,0x4004dae0),
        (Source::CpuRead,0xd0010,0x725a2c,0x400ddaa0),(Source::CpuWrite,ROOTS.0 + 4,0x725a30,0x400ddaa0)];
    let mut fetches = [0x725a04_u64].into_iter().chain((0x725a04..0x725a34_u64).step_by(4)).chain([0x725b74]);
    let mut instructions = continuation.start.counts.0.iter().sum();
    let mut tick = 0;
    let mut installation_clocks = [None; 12];
    let mut previous_core1 = installation.counts.0[1];
    let mut stopped_installation_fetch = false;
    for event in &state.records {
        let clock = event.instructions.ok_or("root0 prelude lacks instruction clock")?;
        let event_tick = event.tick.as_ref().ok_or("root0 prelude lacks tick clock")?.0;
        let physical = event.physical.ok_or("root0 prelude lacks physical attribution")?;
        require(clock >= instructions && event_tick >= tick && event.tcm.is_none()
            && matches!(event.core, Some(0 | 1)) && event.pc.is_some()
            && event.byte_order == Some(ByteOrder::Little)
            && (event.address == physical || event.address == ZERO_GUEST + physical),
            "root0 prelude attribution or chronology differs")?;
        instructions = clock; tick = event_tick;
        if matches!(event.source, Source::CpuFetch) {
            if event.core == Some(1) {
                require(event.address == physical && event.pc == Some(physical) && physical % 4 == 0
                    && event.size == 4 && event.before.is_none() && event.value.is_none()
                    && code_spans().take(2).any(|(low,high)| physical >= low && physical < high && high - physical >= 4)
                    && clock > continuation.start.counts.0.iter().sum() && clock <= entry.counts.0.iter().sum(),
                    "root0 prelude independent core1 fetch attribution or clock differs")?;
                continue;
            }
            let address = fetches.next().ok_or("root0 extra installer or consumer fetch")?;
            require(event.core == Some(0) && event.address == address && physical == address && event.pc == Some(address)
                && event.size == 4 && event.before.is_none() && event.value.is_none(),
                "root0 natural installer/first consumer fetch differs")?;
            if !stopped_installation_fetch {
                require(address == 0x725a04 && clock == installation.counts.0.iter().sum::<u64>() + 1,
                    "root0 installation stopped fetch clock differs")?;
                stopped_installation_fetch = true;
                continue;
            }
            if address == 0x725b74 {
                require(clock == entry.counts.0.iter().sum::<u64>() + 1, "root0 consumer stopped fetch clock differs")?;
            } else {
                let ordinal = (address - 0x725a04) / 4;
                let core1 = clock.checked_sub(installation.counts.0[0] + ordinal + 1).ok_or("root0 installer fetch predates ordinal")?;
                require(core1 >= previous_core1 && core1 <= entry.counts.0[1] && clock <= entry.counts.0.iter().sum(),
                    "root0 installer fetch ordinal or peer chronology differs")?;
                installation_clocks[ordinal as usize] = Some(clock);
                previous_core1 = core1;
            }
            continue;
        }
        require(matches!(event.size, 1 | 2 | 4 | 8) && clock <= entry.counts.0.iter().sum(),
            "root0 prelude data width or clock differs")?;
        let value = event.value.ok_or("root0 prelude lacks value")?;
        require(event.size == 8 || value < (1_u64 << (event.size * 8)), "root0 prelude value exceeds width")?;
        let bytes = value.to_le_bytes();
        let size = event.size as usize;
        let end = physical.checked_add(event.size).ok_or("root0 prelude access overflows")?;
        let write = matches!(event.source, Source::CpuWrite | Source::HostWrite | Source::ModelBusWrite);
        require(write || matches!(event.source, Source::CpuRead), "root0 unexpected prelude access")?;
        if !write { require(preimage(event, &bytes[..size]), "root0 prelude read preimage differs")?; }
        let is_header = HEADERS.iter().position(|(low, high)| physical >= *low && end <= *high);
        if let Some(index) = is_header {
            require(!write && event.size == 4 && value == [0x4004dae0,0x400ddaa0][index], "root0 header write or value differs")?;
        } else if physical >= PRIMARY_TABLE.0 && end <= PRIMARY_TABLE.1 {
            let offset = (physical - PRIMARY_TABLE.0) as usize;
            require(preimage(event, &table[offset..offset + size]), "root0 intervening table preimage differs")?;
            if write {
                require((end <= 0x4f0f8 || physical >= 0x4f0fc) && (end <= descriptor || physical >= descriptor + 4),
                    "root0 intervening selected publication writer")?;
                table[offset..offset + size].copy_from_slice(&bytes[..size]);
            } else { require(table[offset..offset + size] == bytes[..size], "root0 intervening table read differs")?; }
        } else if physical >= ROOTS.0 && end <= ROOTS.1 {
            let offset = (physical - ROOTS.0) as usize;
            require(preimage(event, &roots[offset..offset + size]), "root0 intervening root preimage differs")?;
            if write { roots[offset..offset + size].copy_from_slice(&bytes[..size]); }
            else { require(roots[offset..offset + size] == bytes[..size], "root0 intervening root read differs")?; }
        } else {
            require(write && code_spans().take(2).any(|(low,high)| physical < high && low < end)
                && clock <= installation.counts.0.iter().sum() && event.before.as_ref().is_some_and(|raw| usize::from(raw.len) == size),
                "root0 code changed after installation boundary or access crosses resource")?;
        }
        let installation_event = (write && physical >= ROOTS.0 && end <= ROOTS.1)
            || (is_header.is_some() && matches!(event.pc, Some(0x725a1c | 0x725a2c)));
        if installation_event {
            let (source,address,pc,expected_value) = expected.get(next_install).ok_or("root0 extra installation event")?;
            require(event.source == *source && event.core == Some(0) && event.address == *address && event.size == 4
                && physical == *address && event.pc == Some(*pc) && value == *expected_value
                && installation_clocks[((*pc - 0x725a04) / 4) as usize] == Some(clock),
                "root0 natural ordered installation or data/fetch clock differs")?;
            next_install += 1;
        }
    }
    require(fetches.next().is_none() && next_install == 4 && table == table_before
        && roots == [0xe0,0xda,0x04,0x40,0xa0,0xda,0x0d,0x40], "root0 installation or table continuity incomplete")
}

fn aliases(machine: &Machine, maps: &[factory::MemoryMap], tcm: &TcmWords,
    resources: &[((u64,u64), &Views)], hashes: &[Sha256Digest], result: &mut Vec<AliasBinding>) -> Result<(), String> {
    let extents: Vec<_> = resources.iter().map(|(extent,_)| *extent).collect();
    tcm_disjoint_views(maps, tcm, &extents)?;
    let mut buffer = vec![0; resources.iter().map(|(_,view)| view.physical.len()).max().unwrap_or(0)];
    for core in 0..2 {
        require(machine.session.board.cpu(core)?.rprofile_identity(None)? == [0x4003,0x5003][core], "root0 active CP14 differs")?;
        for ((&(low,high),view), hash) in resources.iter().map(|(extent,view)| (extent,*view)).zip(hashes) {
            for guest in [low,ZERO_GUEST + low] {
                require(static_span(maps, MemoryCore(core), MemoryAddress(guest)..MemoryAddress(guest + high - low))?.physical.0 == low,
                    "root0 alias mapping differs")?;
                let buffer = &mut buffer[..view.physical.len()];
                machine.session.board.read_memory(core, guest, buffer)?;
                require(buffer == view.physical.as_slice(), "root0 alias bytes differ")?;
                result.push(AliasBinding { core, guest: Address(guest), physical: Address(low), size: high - low, sha256: *hash });
            }
        }
    }
    Ok(())
}

struct ExpectedData { source: Source, guest: u64, pc: u64, ordinal: u64, value: u32 }

fn body(state: &WatchState, entry: &Context, selected: &Context, after: &Context, stack_start: &[u8], stack_end: &[u8],
    descriptor: u64, pool: u64, pool_value: u32, prelude: &WatchState) -> Result<(), String> {
    let mut expected = Vec::with_capacity(15);
    for (index, value) in [entry.registers[4].0,entry.registers[5].0,entry.registers[6].0,entry.registers[7].0,
        entry.registers[11].0,entry.lr.0].into_iter().enumerate() {
        expected.push(ExpectedData { source: Source::CpuWrite, guest: u64::from(entry.sp.0) - 24 + index as u64 * 4,
            pc: 0x725b74, ordinal: 0, value });
    }
    expected.extend([
        ExpectedData { source: Source::CpuRead, guest: ROOTS.0, pc: 0x725b88, ordinal: 5, value: 0x4004dae0 },
        ExpectedData { source: Source::CpuRead, guest: 0x4004f0f8, pc: 0x725b90, ordinal: 7, value: 0x412bc },
        ExpectedData { source: Source::CpuWrite, guest: u64::from(entry.sp.0) - 32, pc: 0x412bc, ordinal: 9, value: entry.sp.0 - 4 },
        ExpectedData { source: Source::CpuWrite, guest: u64::from(entry.sp.0) - 28, pc: 0x412bc, ordinal: 9, value: 0x725b98 },
        ExpectedData { source: Source::CpuRead, guest: u64::from(entry.sp.0) - 32, pc: 0x412c8, ordinal: 19, value: entry.sp.0 - 4 },
        ExpectedData { source: Source::CpuRead, guest: u64::from(entry.sp.0) - 28, pc: 0x412c8, ordinal: 19, value: 0x725b98 },
        ExpectedData { source: Source::CpuRead, guest: ROOTS.0, pc: 0x725b9c, ordinal: 21, value: 0x4004dae0 },
        ExpectedData { source: Source::CpuRead, guest: ZERO_GUEST + descriptor, pc: 0x725bac, ordinal: 25, value: (ZERO_GUEST + pool) as u32 },
        ExpectedData { source: Source::CpuRead, guest: ZERO_GUEST + pool, pc: 0x725bb0, ordinal: 26, value: pool_value },
    ]);
    require(state.records.len() == expected.len(), "root0 bounded data event count differs")?;
    let mut stack = stack_start.to_vec();
    let mut instructions = prelude.records.last().and_then(|event| event.instructions).unwrap_or_else(|| entry.counts.0.iter().sum());
    let mut tick = prelude.records.last().and_then(|event| event.tick.as_ref()).map_or(0, |tick| tick.0);
    let mut previous_ordinal = None;
    let mut previous_core1 = entry.counts.0[1];
    for (index, (event, expected)) in state.records.iter().zip(expected).enumerate() {
        let previous_clock = instructions;
        let value = raw(event, &mut instructions, &mut tick, after.counts.0.iter().sum())?;
        let core0_clock = entry.counts.0[0].checked_add(expected.ordinal + 1).ok_or("root0 instruction clock overflow")?;
        let core1_clock = instructions.checked_sub(core0_clock).ok_or("root0 data clock precedes core0 ordinal")?;
        require(event.source == expected.source && event.core == Some(0) && event.address == expected.guest
            && event.pc == Some(expected.pc) && value == expected.value
            && core1_clock >= previous_core1 && core1_clock <= after.counts.0[1]
            && (if index < 12 { core1_clock <= selected.counts.0[1] } else { core1_clock >= selected.counts.0[1] })
            && (previous_ordinal != Some(expected.ordinal) || instructions == previous_clock)
            && (previous_ordinal == Some(expected.ordinal) || previous_ordinal.is_none() || instructions > previous_clock)
            && (if index < 12 { instructions <= selected.counts.0.iter().sum() } else { instructions > selected.counts.0.iter().sum() }),
            "root0 actual selected read, write or ordinal differs")?;
        if expected.guest >= u64::from(entry.sp.0) - 32 && expected.guest < u64::from(entry.sp.0) {
            let offset = (expected.guest - (u64::from(entry.sp.0) - 32)) as usize;
            require(preimage(event, &stack[offset..offset + 4]), "root0 dynamic stack preimage differs")?;
            if matches!(event.source, Source::CpuWrite) { stack[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
            else { require(word(&stack[offset..offset + 4])? == value, "root0 stack read differs")?; }
        } else { require(preimage(event, &value.to_le_bytes()), "root0 data read preimage differs")?; }
        previous_ordinal = Some(expected.ordinal); previous_core1 = core1_clock;
    }
    require(stack == stack_end, "root0 saved frame replay differs")
}

fn check_code(machine: &Machine, maps: &[factory::MemoryMap]) -> Result<Vec<CodeViews>, String> {
    CODE.into_iter().map(|(start, words)| {
        let view = snapshot(machine, maps, (u64::from(start), u64::from(start) + words.len() as u64 * 4))?;
        require(view.physical.chunks_exact(4).zip(words).all(|(bytes, word)| bytes == word.to_le_bytes()),
            "root0 actual code differs from canonical source")?;
        Ok(CodeViews { start: Guest(start), views: view })
    }).collect()
}

fn observe(machine: &mut Machine, initializer: &ObservationPrimary, continuation: &mut Option<Continuation>,
    boundaries: &mut Vec<BoundaryObservation>) -> Result<Option<Observation>, String> {
    let (maps, _) = bindings(&machine.document)?;
    let committed = ram_decoded(&machine.document, &maps)?;
    let start = context_for(machine, 0)?;
    require(start.pc == initializer.after.pc && start.registers == initializer.after.registers && start.sp == initializer.after.sp
        && start.lr == initializer.after.lr && start.cpsr == initializer.after.cpsr && start.tcm == initializer.after.tcm
        && start.counts.0 == initializer.after.counts.0, "root0 producer continuation has an execution gap")?;
    let roots_start = snapshot(machine, &committed, ROOTS)?;
    tcm_disjoint_views(&committed, &start.tcm, &[PRIMARY_TABLE,ROOTS,HEADERS[0],HEADERS[1]])?;
    let mut prelude_watch = begin(machine, &maps, None)?;
    *continuation = Some(Continuation { start: start.clone(), roots_start, prelude_watch: observed(&prelude_watch)?, watch: None });
    let prelude_outcome: Result<_, String> = (|| {
        let Some(installation) = stop(machine, Boundary::Installation, &start, boundaries)? else { return Ok(None) };
        check_code(machine, &committed)?;
        for (index, extent) in HEADERS.into_iter().enumerate() {
            require(word(&snapshot(machine, &committed, extent)?.physical)? == [0x4004dae0,0x400ddaa0][index],
                "root0 canonical header value differs")?;
        }
        let Some(entry) = stop(machine, Boundary::Entry, &installation, boundaries)? else { return Ok(None) };
        Ok(Some((installation, entry)))
    })();
    let state = serde_json::from_value(prelude_watch.state()).map_err(|error| error.to_string());
    let ended = machine.session.board.memory_watch_end_scoped(&mut prelude_watch);
    if let Some(continuation) = continuation.as_mut() { continuation.prelude_watch = state?; }
    ended?;
    let captured = continuation.as_ref().ok_or("root0 continuation absent")?;
    require(captured.prelude_watch.enabled && captured.prelude_watch.violation.is_none() && captured.prelude_watch.fault.is_none(),
        "root0 prelude watch faulted")?;
    let Some((installation, entry)) = prelude_outcome? else { return Ok(None) };
    require(entry.sp.0 % 4 == 0
        && (0x40000020..=0x40100000).contains(&entry.sp.0) && entry.cpsr.0 & 0x07f0_feff == 0xd3,
        "root0 actual descriptor identifier, stack or mode differs")?;
    let identifier = DescriptorId::try_from(entry.registers[0].0)?.0;
    let descriptor = PRIMARY_TABLE.0 + u64::from(identifier - 1) * 44 + 0x184;
    let pool = PRIMARY_POOLS.0 + u64::from(identifier - 1) * 40 + 8;
    let stack = (u64::from(entry.sp.0) - ZERO_GUEST - 32, u64::from(entry.sp.0) - ZERO_GUEST);
    let pool_span = (pool,pool + 4);
    require([PRIMARY_TABLE,ROOTS,pool_span].into_iter().chain(HEADERS).chain(code_spans())
        .all(|(low,high)| stack.1 <= low || high <= stack.0), "root0 actual stack overlaps protected resources")?;
    let roots_before = snapshot(machine, &committed, ROOTS)?;
    let table_before = snapshot(machine, &committed, PRIMARY_TABLE)?;
    let stack_before = snapshot(machine, &committed, stack)?;
    let pool_before = snapshot(machine, &committed, pool_span)?;
    let code = check_code(machine, &committed)?;
    require(roots_before.physical == [0xe0,0xda,0x04,0x40,0xa0,0xda,0x0d,0x40]
        && word(&table_before.physical[(descriptor - PRIMARY_TABLE.0) as usize..(descriptor - PRIMARY_TABLE.0) as usize + 4])? == (ZERO_GUEST + pool) as u32
        && word(&table_before.physical[0x1618..0x161c])? == 0x412bc, "root0 live roots or published selector/descriptor differ")?;
    prelude(&captured.prelude_watch, captured, &installation, &entry,
        &initializer.table_after.physical, &table_before.physical, descriptor)?;
    let decoder = decoder(machine)?;
    let resources: Vec<_> = [(ROOTS,&roots_before),(PRIMARY_TABLE,&table_before),(stack,&stack_before),(pool_span,&pool_before)]
        .into_iter().chain(code_spans().zip(code.iter().map(|code| &code.views))).collect();
    let hashes: Vec<_> = resources.iter().map(|(_,view)| Sha256Digest::of(&view.physical)).collect();
    let mut aliases_observed = Vec::with_capacity(128);
    aliases(machine, &committed, &entry.tcm, &resources, &hashes, &mut aliases_observed)?;
    let mut watch = begin(machine, &maps, Some((stack,pool_span)))?;
    let outcome = (|| {
        let Some(selected) = stop(machine, Boundary::Selected, &entry, boundaries)? else { return Ok(None) };
        let mut selected_registers = entry.registers;
        selected_registers[..7].copy_from_slice(&[Word(0),Word(0x4003),Word(0x4003),Word(0x412bc),Word(ROOTS.0 as u32),Word(identifier),Word(0)]);
        selected_registers[11] = Word(entry.sp.0 - 4);
        let cpsr = Word((entry.cpsr.0 & 0x0fff_ffff) | 0x6000_0000);
        require(selected.registers == selected_registers && selected.sp.0 == entry.sp.0 - 24 && selected.lr.0 == 0x412c8
            && selected.cpsr == cpsr && selected.counts.0[0] == entry.counts.0[0] + 21,
            "root0 primary CP14 selection or dynamic frame differs")?;
        let Some(after) = stop(machine, Boundary::After, &selected, boundaries)? else { return Ok(None) };
        let pool_value = word(&pool_before.physical)?;
        let mut after_registers = selected_registers;
        after_registers[1] = Word(pool_value); after_registers[2] = Word(0x4004dae0); after_registers[3] = Word(identifier - 1);
        require(after.registers == after_registers && after.sp == selected.sp && after.lr == selected.lr && after.cpsr == cpsr
            && after.counts.0[0] == entry.counts.0[0] + 27, "root0 actual descriptor/pool read or frame differs")?;
        decoder_fn(machine)?;
        let roots_after = snapshot(machine, &committed, ROOTS)?;
        let table_after = snapshot(machine, &committed, PRIMARY_TABLE)?;
        let stack_after = snapshot(machine, &committed, stack)?;
        let pool_after = snapshot(machine, &committed, pool_span)?;
        require(roots_before.physical == roots_after.physical && table_before.physical == table_after.physical
            && pool_before.physical == pool_after.physical, "root0 bounded roots/table/pool changed")?;
        let resources_after: Vec<_> = [(ROOTS,&roots_after),(PRIMARY_TABLE,&table_after),(stack,&stack_after),(pool_span,&pool_after)]
            .into_iter().chain(code_spans().zip(code.iter().map(|code| &code.views))).collect();
        let hashes_after: Vec<_> = resources_after.iter().enumerate().map(|(index,(_,view))|
            if index == 2 { Sha256Digest::of(&view.physical) } else { hashes[index] }).collect();
        aliases(machine, &committed, &after.tcm, &resources_after, &hashes_after, &mut aliases_observed)?;
        Ok(Some(Observation { entry,selected,after,roots_before,roots_after,table_before,table_after,stack_before,stack_after,
            pool_before,pool_after,code,decoder,aliases: aliases_observed }))
    })();
    let state = serde_json::from_value(watch.state()).map_err(|error| error.to_string());
    let ended = machine.session.board.memory_watch_end_scoped(&mut watch);
    let captured = continuation.as_mut().ok_or("root0 continuation absent")?;
    captured.watch = Some(state?);
    let state = captured.watch.as_ref().ok_or("root0 body watch absent")?;
    require(state.enabled && state.violation.is_none() && state.fault.is_none(), "root0 body watch faulted")?;
    match outcome {
        Ok(Some(observation)) => {
            ended?;
            body(state, &observation.entry, &observation.selected, &observation.after,
                &observation.stack_before.physical, &observation.stack_after.physical, descriptor,pool,
                word(&observation.pool_before.physical)?, &captured.prelude_watch)?;
            Ok(Some(observation))
        }
        Ok(None) => ended.map(|()| None),
        Err(error) => { let _ = ended; Err(error) }
    }
}

use super::Observation as ObservationPrimary;
use super::decoder as decoder_fn;

pub(in crate::peer_witness) fn run(mut machine: Machine, initializer: &PrimaryReceipt<'_>, output: &Path) -> Result<(), String> {
    let mut continuation = None;
    let mut boundaries = Vec::with_capacity(4);
    let (status, consumer, outcome) = if let Some(primary) = &initializer.primary {
        match observe(&mut machine, primary, &mut continuation, &mut boundaries) {
            Ok(Some(consumer)) => (ObservationStatus::Observed, Some(consumer), Ok(())),
            Ok(None) => (ObservationStatus::Absent, None, Err("root0 absent at finite instruction deadline".to_owned())),
            Err(error) => (ObservationStatus::Refused, None, Err(error)),
        }
    } else {
        (initializer.status, None, Err(initializer.error.unwrap_or("primary initializer failed").to_owned()))
    };
    let receipt = Receipt { schema: 9, result: if outcome.is_ok() { WitnessResult::Passed } else { WitnessResult::Failed }, status,
        error: outcome.as_ref().err().map(String::as_str), machine_failure: machine.failure.as_deref(),
        instructions: Instructions(machine.instructions()), cp14_identities: initializer.cp14_identities,
        r#final: context_for(&machine, 0)?, boundaries, initializer, continuation, consumer };
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
    require(bytes.len() <= 64 << 20, "root0 observation receipt exceeds 64 MiB")?;
    let path = output.join("peer-witness.json");
    std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    println!("{}", path.display());
    outcome
}
