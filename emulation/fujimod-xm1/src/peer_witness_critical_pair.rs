use super::*;

const CARRIER: (u64, u64) = (0x10400, 0x11800);
const PAIR: (u64, u64) = (0x7235d8, 0x723790);
const WRAPPER: (u64, u64) = (0x725dd8, 0x726098);
const ROOTS: (u64, u64) = (0x96bd20, 0x96bd28);
const TABLE_A: (u64, u64) = (0x4dae0, 0x4f10c);
const COUNTER: (u64, u64) = (0xa0f824, 0xa0f828);
const CALLER_FRAME: (u64, u64) = (0xf33a8, 0xf33e0);
const LOCK: (u64, u64) = (0xc0804, 0xc0808);
const LOADER_A: (u64, u64) = (0x40000, 0x4dae0);
const STAGE4: (u64, u64) = (0x69cdf8, 0x69cf38);
const TAIL: (u64, u64) = (0xd0ab0, 0xd0af0);
const SELECTOR: (u64, u64) = (0x7caf34, 0x7caf38);
const MAGIC: (u64, u64) = (0xc080c, 0xc0810);
const ENTER: u32 = 0x7235d8;
const LEAVE: u32 = 0x7236b0;
const ENTRY_RETURN: u32 = 0x725e00;
const CALLER_RETURN: u32 = 0x69cf38;
const EXIT_CALLS: [u32; 3] = [0x725e84, 0x725fb4, 0x726064];
const COMMON: [(u64, u64); 12] = [CARRIER, PAIR, WRAPPER, ROOTS, TABLE_A, TABLE, COUNTER, LOCK,
    LOADER_A, LOADER_B, STAGE4, TAIL];
const PAIR_SHA: &str = "8d0b5366c29252bf28baf514f77921cc379d3f519836aba6589ccc977cdbb62a";
const WRAPPER_SHA: &str = "1432c11311a1a15437364f8d69372bec69a2e06357c94ca56e00c402fe96cf43";
const LOADER_A_SHA: &str = "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882";
const IMAGE4_SHA: &str = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16";

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "u32")]
struct FrameBytes(u32);
impl TryFrom<u32> for FrameBytes {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, String> {
        require((8..=1032).contains(&value) && value % 8 == 0, "critical frame extent differs")?;
        Ok(Self(value))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::peer_witness::loader) struct Contract {
    role: Role,
    source_sha256: Sha256Digest,
    input_sha256: Sha256Digest,
    image4_sha256: Sha256Digest,
    image_sha256: Sha256Digest,
    symbols_sha256: Option<Sha256Digest>,
    carrier: Vec<u8>,
    entry: Guest,
    exit: Guest,
    entry_frame_bytes: FrameBytes,
    exit_frame_bytes: FrameBytes,
    original_pair: Vec<u8>,
    wrapper: Vec<u8>,
    loader_a: Vec<u8>,
    loader_b: Vec<u8>,
}
impl Contract {
    pub(in crate::peer_witness::loader) fn admit_update(&self, bytes: &[u8]) -> Result<(), String> {
        require(Sha256Digest::of(bytes) == self.input_sha256
            && (self.role == Role::Owned || self.input_sha256 == self.source_sha256), "critical whole-DAT input identity differs")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::peer_witness) struct CriticalPairWire {
    schema: u32,
    initializer: InitializerWire,
    pair: Contract,
}
impl CriticalPairWire {
    pub(in crate::peer_witness) fn validate(self) -> Result<LoaderContract, String> {
        require(self.schema == 10, "critical pair requires schema 10")?;
        let mut prefix = self.initializer.validate()?;
        let pair = self.pair;
        require(pair.source_sha256 == Sha256Digest::parse(SOURCE)? && pair.image4_sha256 == Sha256Digest::parse(IMAGE4_SHA)?
            && pair.carrier.len() == (CARRIER.1 - CARRIER.0) as usize && Sha256Digest::of(&pair.carrier) == pair.image_sha256,
            "critical pair source or full carrier identity differs")?;
        for (bytes, extent, digest) in [(&pair.original_pair, PAIR, Sha256Digest::parse(PAIR_SHA)?),
            (&pair.wrapper, WRAPPER, Sha256Digest::parse(WRAPPER_SHA)?),
            (&pair.loader_a, LOADER_A, Sha256Digest::parse(LOADER_A_SHA)?), (&pair.loader_b, LOADER_B, LOADER_B_SHA256)] {
            require(bytes.len() == (extent.1 - extent.0) as usize && Sha256Digest::of(bytes) == digest,
                "critical pair independent canonical source pin differs")?;
        }
        let initializer = prefix.initializer.as_ref().ok_or("critical initializer absent")?;
        require(pair.role == initializer.role, "critical pair and initializer roles differ")?;
        match pair.role {
            Role::Retained => require(pair.entry == Guest(ENTER) && pair.exit == Guest(LEAVE)
                && pair.entry_frame_bytes == FrameBytes(40) && pair.exit_frame_bytes == FrameBytes(32)
                && pair.symbols_sha256.is_none(), "retained critical boundary differs")?,
            Role::Owned => require([pair.entry, pair.exit].iter().all(|site|
                (CARRIER.0..CARRIER.1).contains(&u64::from(site.0)) && site.0 % 4 == 0)
                && pair.entry != pair.exit && pair.symbols_sha256 == initializer.symbols_sha256
                && pair.symbols_sha256.is_some(), "owned critical boundary or linked symbols differ")?,
        }
        prefix.critical_pair = Some(pair);
        Ok(prefix)
    }
}

#[derive(Serialize)]
struct Resource { start: Address, views: Views }
#[derive(Serialize)]
struct BoundaryReceipt {
    incoming: Context,
    returned: Option<Context>,
    steps: Vec<Context>,
    before: Vec<Resource>,
    after: Vec<Resource>,
    watch: Option<WatchState>,
}
#[derive(Serialize)]
pub(in crate::peer_witness::loader) struct Receipt {
    role: Role,
    prelude_start: Context,
    entry: Option<BoundaryReceipt>,
    transition_watch: Option<WatchState>,
    exit: Option<BoundaryReceipt>,
    returned: Option<Context>,
    prelude_watch: Option<WatchState>,
    caller_watch: Option<WatchState>,
    caller_frame_before: Option<Resource>,
    caller_frame_after: Option<Resource>,
    resources: Vec<Resource>,
    decoder: Decoder,
    aliases: Vec<AliasBinding>,
}
#[derive(Serialize)]
pub(in crate::peer_witness::loader) struct OuterReceipt<'a, 'initializer> {
    pub schema: u32,
    pub result: WitnessResult,
    pub error: Option<&'a str>,
    pub machine_failure: Option<&'a str>,
    pub initializer: &'a super::super::Receipt<'initializer>,
    pub pair: &'a Option<Receipt>,
}

fn span((start, end): (u64, u64)) -> Result<ScopeSpan, String> {
    PhysicalRange::new(start, end).map(ScopeSpan::Physical)
}
fn resource(machine: &Machine, runner: &Runner<'_>, (start, end): (u64, u64)) -> Result<Resource, String> {
    let views = runner.views(machine, 1, ZERO_GUEST + start, (end - start) as usize)?;
    equal_views(&views)?;
    Ok(Resource { start: Address(start), views })
}
fn resources(machine: &Machine, runner: &Runner<'_>, frame: Option<(u64, u64)>) -> Result<Vec<Resource>, String> {
    COMMON.into_iter().chain(frame).map(|extent| resource(machine, runner, extent)).collect()
}
fn aliases(machine: &Machine, runner: &Runner<'_>, observed: &[Resource], result: &mut Vec<AliasBinding>) -> Result<(), String> {
    let guarded: Vec<_> = observed.iter().map(|resource|
        (resource.start.0, resource.start.0 + resource.views.physical.len() as u64)).collect();
    tcm_disjoint_views(&runner.committed, &tcm_words(machine)?, &guarded)?;
    for resource in observed {
        let start = resource.start.0;
        let size = resource.views.physical.len();
        let sha256 = Sha256Digest::of(&resource.views.physical);
        for core in 0..2 {
            require(machine.session.board.cpu(core)?.rprofile_identity(None)? == [0x4003, 0x5003][core],
                "critical CP14 identity differs")?;
            for guest in [start, ZERO_GUEST + start] {
                require(runner.physical_of(core, guest, size as u64)? == start
                    && wait::bytes(machine, core, guest, size)? == resource.views.physical, "critical alias bytes or mapping differ")?;
                result.push(AliasBinding { core, guest: Address(guest), physical: Address(start), size: size as u64, sha256 });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Phase { Prelude, Boundary, Transition }
fn begin(machine: &mut Machine, runner: &Runner<'_>, phase: Phase, frame: Option<(u64, u64)>) -> Result<Box<ScopedMemoryWatch>, String> {
    let prelude = matches!(phase, Phase::Prelude);
    let mut classes: Vec<_> = COMMON.into_iter().map(|extent| {
        let kinds = if prelude {
            if [CARRIER, PAIR, WRAPPER, STAGE4, TAIL].contains(&extent) { WatchKinds::CPU_FETCH.union(WatchKinds::WRITES) }
            else if [ROOTS, TABLE_A, TABLE, COUNTER, LOCK].contains(&extent) { WatchKinds::CPU_READ.union(WatchKinds::WRITES) }
            else { WatchKinds::WRITES }
        } else { WatchKinds::CPU_FETCH.union(WatchKinds::CPU_READ).union(WatchKinds::WRITES) };
        WatchRange { start: extent.0, end: extent.1, kinds }
    }).collect();
    if let Some((start, end)) = frame {
        classes.push(WatchRange { start, end, kinds: WatchKinds::CPU_FETCH.union(WatchKinds::CPU_READ).union(WatchKinds::WRITES) });
    }
    if prelude {
        classes.push(WatchRange { start: SELECTOR.0, end: SELECTOR.1, kinds: WatchKinds::CPU_READ.union(WatchKinds::WRITES) });
        classes.push(WatchRange { start: MAGIC.0, end: MAGIC.1, kinds: WatchKinds::CPU_READ });
    } else {
        classes.push(WatchRange { start: CALLER_FRAME.0, end: CALLER_FRAME.1, kinds: WatchKinds::CPU_READ.union(WatchKinds::WRITES) });
    }
    classes.push(WatchRange { start: RECORD.0, end: RECORD.1, kinds: WatchKinds::WRITES });
    let immutable: Vec<_> = COMMON.into_iter().filter(|extent| ![COUNTER, LOCK, CARRIER, TAIL].contains(extent)).collect();
    let mut confinement = if matches!(phase, Phase::Boundary) {
        vec![ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Write,
            rule: Confinement::Allow([Some(COUNTER), Some(LOCK), frame].into_iter().flatten().map(span).collect::<Result<_, _>>()?) },
            ConfinementRule { cores: CoreSet::new(&[0])?, access: Access::Write,
                rule: Confinement::Forbid(COMMON.into_iter().filter(|extent| ![CARRIER, TAIL].contains(extent))
                    .chain(frame).chain([CALLER_FRAME, RECORD]).map(span).collect::<Result<_, _>>()?) }]
    } else {
        vec![ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Write,
            rule: Confinement::Forbid(immutable.into_iter().chain(if prelude { Some(SELECTOR) } else { None })
                .chain(Some(RECORD)).map(span).collect::<Result<_, _>>()?) },
            ConfinementRule { cores: CoreSet::new(&[0])?, access: Access::Write,
                rule: Confinement::Forbid(COMMON.into_iter().filter(|extent| ![CARRIER, TAIL].contains(extent))
                    .chain(Some(if prelude { SELECTOR } else { CALLER_FRAME }))
                    .chain(Some(RECORD)).map(span).collect::<Result<_, _>>()?) }]
    };
    if runner.contract.initializer.as_ref().is_some_and(super::Contract::owns_body) {
        confinement.push(ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Read,
            rule: Confinement::Forbid(vec![span(BODY)?]) });
        confinement.push(ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Fetch,
            rule: Confinement::Forbid(vec![span(BODY)?]) });
    }
    machine.session.board.memory_watch_begin_scoped(ScopedWatch {
        classes, limit: WATCH_LIMIT, confinement, counters: Vec::new(), coverage: Vec::new(),
    }, runner.maps.clone())
}
fn capture(watch: &ScopedMemoryWatch) -> Result<WatchState, String> {
    serde_json::from_value(watch.state()).map_err(|error| format!("critical watch: {error}"))
}
fn healthy(state: &WatchState) -> Result<(), String> {
    require(state.enabled && state.fault.is_none() && state.violation.is_none(), "critical watch fault or violation")?;
    require(!state.records.iter().any(|event| matches!(event.source, Source::HostWrite | Source::ModelBusWrite)
        || event.tcm.is_some()), "critical protected writer or TCM access differs")
}
fn end(machine: &mut Machine, watch: &mut ScopedMemoryWatch, target: &mut Option<WatchState>, outcome: Result<(), String>) -> Result<(), String> {
    let state = capture(watch);
    let stopped = machine.session.board.memory_watch_end_scoped(watch);
    match state {
        Ok(state) => { let validity = healthy(&state); *target = Some(state); outcome.and(stopped).and(validity) }
        Err(error) => outcome.and(stopped).and(Err(error)),
    }
}
fn stop(machine: &mut Machine, address: u32) -> Result<Context, String> {
    machine.traces.arm(&machine.session, BTreeMap::from([(address, "critical-boundary".into())]))?;
    machine.traces.stop_labels = ["critical-boundary".into()].into_iter().collect();
    loop {
        require(machine.instructions() < BUDGET, "critical global instruction budget exhausted")?;
        let previous = machine.traces.hits.len();
        machine.run(BUDGET - machine.instructions());
        machine.require_running()?;
        let hits = &machine.traces.hits[previous..];
        require(!hits.is_empty(), "critical natural endpoint not reached")?;
        if hits.iter().any(|hit| hit.core == 1) {
            let observed = context(machine)?;
            require(observed.pc.0 == address, "critical stop context differs")?;
            return Ok(observed);
        }
    }
}
fn validate_prelude(state: &WatchState, start: &Context, incoming: &Context) -> Result<(), String> {
    let mut fetches = (TAIL.0..TAIL.1).step_by(4).chain((0x69cdf8..0x69ce0c).step_by(4))
        .chain([0x69ce44,0x69ce48,0x69ce90,0x69ce94,0x69ce98,0x69ce9c,0x69cea0,0x69cea4])
        .chain((0x69ceec..0x69cf38).step_by(4)).chain((WRAPPER.0..0x725e00).step_by(4))
        .chain([u64::from(ENTER)]);
    let mut selector = false;
    let mut magic = false;
    let mut last = start.counts.0.iter().sum::<u64>();
    let mut tick = 0;
    for event in &state.records {
        let count = event.instructions.ok_or("critical prelude instruction attribution absent")?;
        let next_tick = event.tick.as_ref().ok_or("critical prelude tick attribution absent")?.0;
        require(matches!(event.source, Source::CpuRead | Source::CpuFetch) && event.size == 4
            && event.byte_order == Some(ByteOrder::Little) && event.tcm.is_none()
            && matches!(event.core, Some(0 | 1)) && event.pc.is_some()
            && event.physical.is_some_and(|physical| event.address == physical || event.address == ZERO_GUEST + physical)
            && count >= last && count <= incoming.counts.0.iter().sum::<u64>() + 1 && next_tick >= tick,
            "critical prelude provenance differs")?;
        last = count;
        tick = next_tick;
        if event.core != Some(1) { continue; }
        match event.source {
            Source::CpuFetch => {
                let expected = fetches.next().ok_or("critical prelude extra fetch")?;
                require(event.pc == Some(expected) && event.address == expected && event.physical == Some(expected)
                    && event.before.is_none() && event.value.is_none(), "critical phase4 source sequence differs")?;
            }
            Source::CpuRead => {
                let value = event.value.and_then(|value| u32::try_from(value).ok()).ok_or("critical prelude read value absent")?;
                require(event.before.as_ref().is_some_and(|before| before.len == 4 && before.bytes[..4] == value.to_le_bytes()),
                    "critical prelude read preimage differs")?;
                match event.physical {
                    Some(address) if address == SELECTOR.0 => {
                        require(!selector && event.pc == Some(0x725df8) && value == 2, "critical task103 selector differs")?;
                        selector = true;
                    }
                    Some(address) if address == MAGIC.0 => {
                        require(matches!(event.pc, Some(0xd38b8 | 0xd38cc | 0xd391c)), "critical magic provenance differs")?;
                        magic = event.pc == Some(0xd391c) && value == 0x87654321;
                    }
                    _ => return Err("critical prelude unexpected protected read".into()),
                }
            }
            _ => return Err("critical prelude unexpected source".into()),
        }
    }
    require(fetches.next().is_none() && selector && magic, "critical natural prelude evidence incomplete")
}
fn source_bindings(machine: &Machine, contract: &Contract) -> Result<(), String> {
    require(machine.read(CARRIER.0, contract.carrier.len())? == contract.carrier
        && machine.read(WRAPPER.0, contract.wrapper.len())? == contract.wrapper, "critical carrier or wrapper source differs")?;
    let mut expected_pair = contract.original_pair.clone();
    if contract.role == Role::Owned {
        for (site, target) in [(ENTER, contract.entry), (LEAVE, contract.exit)] {
            let displacement = i64::from(target.0) - (i64::from(site) + 8);
            let branch = 0xea00_0000_u32 | (((displacement >> 2) as u32) & 0x00ff_ffff);
            let offset = (u64::from(site) - PAIR.0) as usize;
            expected_pair[offset..offset + 4].copy_from_slice(&branch.to_le_bytes());
        }
    }
    require(machine.read(PAIR.0, expected_pair.len())? == expected_pair, "critical original or branch-patched pair differs")?;
    for (extent, canonical) in [(LOADER_A, &contract.loader_a), (LOADER_B, &contract.loader_b)] {
        let actual = machine.read(extent.0, canonical.len())?;
        for (offset, (&actual, &expected)) in actual.iter().zip(canonical.iter()).enumerate() {
            let address = extent.0 + offset as u64;
            let replaced = contract.role == Role::Owned && [REPLACED[0], REPLACED[1], REPLACED[2],
                (0xd0aac, 0xd0ab0), LOADER_A_COMPLETION, LOADER_A_LITERAL].iter().any(|&(start, end)| (start..end).contains(&address));
            require(replaced || actual == expected, "critical independent retained loader bytes differ")?;
        }
    }
    for (root, table, base) in [(ROOTS.0, TABLE_A.0, LOADER_A.0), (ROOTS.0 + 4, TABLE.0, LOADER_B.0)] {
        require(word(machine, root)?.0 == (ZERO_GUEST + table) as u32, "critical publication root differs")?;
        for (offset, target) in [(0x164,0x1140),(0x168,0x1164),(0x1618,0x12bc),(0x161c,0x12cc),(0x1620,0x12e8),(0x1624,0x1304)] {
            require(word(machine, table + offset)?.0 == (base + target) as u32, "critical retained service publication differs")?;
        }
    }
    Ok(())
}
fn allowed_instruction(pc: u32, contract: &Contract) -> bool {
    let pc = u64::from(pc);
    (match contract.role {
        Role::Retained => (PAIR.0..PAIR.1).contains(&pc),
        Role::Owned => [u64::from(ENTER), u64::from(LEAVE)].contains(&pc) || (CARRIER.0..CARRIER.1).contains(&pc),
    })
        || [LOADER_A.0, LOADER_B.0].into_iter().any(|base| [(0x1140,0x118c),(0x12bc,0x1320),(0x27b8,0x2830)]
            .into_iter().any(|(start, end)| (base + start..base + end).contains(&pc)))
}
fn boundary(machine: &mut Machine, runner: &Runner<'_>, contract: &Contract, incoming: Context,
    return_pc: u32, frame_bytes: FrameBytes, target: &mut Option<BoundaryReceipt>, bindings: &mut Vec<AliasBinding>) -> Result<(), String> {
    *target = Some(BoundaryReceipt { incoming, returned: None, steps: Vec::new(), before: Vec::new(), after: Vec::new(), watch: None });
    let receipt = target.as_mut().ok_or("critical boundary receipt absent")?;
    require(receipt.incoming.sp.0 == 0x400f33a8 && receipt.incoming.sp.0 % 8 == 0, "critical natural caller stack differs")?;
    let frame = (u64::from(receipt.incoming.sp.0) - ZERO_GUEST - u64::from(frame_bytes.0),
        u64::from(receipt.incoming.sp.0) - ZERO_GUEST);
    let mut watch = begin(machine, runner, Phase::Boundary, Some(frame))?;
    let mut inspected = machine.instructions();
    let outcome = (|| {
        for extent in COMMON.into_iter().chain(Some(frame)) {
            receipt.before.push(resource(machine, runner, extent)?);
        }
        aliases(machine, runner, &receipt.before, bindings)?;
        machine.traces.disarm(&machine.session)?;
        machine.traces.stop_labels.clear();
        loop {
            let before = context(machine)?;
            if before.pc.0 == return_pc { receipt.returned = Some(before); break; }
            require(machine.instructions() < BUDGET, "critical global instruction budget exhausted")?;
            require(allowed_instruction(before.pc.0, contract) && before.pc.0 % 4 == 0, "critical instruction leaves admitted pair, carrier or service")?;
            require(before.tcm == receipt.incoming.tcm, "critical TCM state changed within boundary")?;
            machine.run(1);
            let after = context(machine)?;
            if after.counts.0[1] == before.counts.0[1] + 1 { receipt.steps.push(before); }
            else { require(after.counts.0[1] == before.counts.0[1], "critical continuation executed multiple core1 instructions")?; }
            machine.require_running()?;
            if machine.instructions() - inspected >= 256 {
                let state = watch.state();
                require(!state.get("violation").is_some_and(|fault| !fault.is_null())
                    && !state.get("fault").is_some_and(|fault| !fault.is_null()), "critical watch faulted during continuation")?;
                inspected = machine.instructions();
            }
        }
        let returned = receipt.returned.as_ref().ok_or("critical boundary return absent")?;
        require(returned.sp == receipt.incoming.sp && returned.registers[4..12] == receipt.incoming.registers[4..12]
            && returned.tcm == receipt.incoming.tcm, "critical return callee-saved frame differs")?;
        if receipt.incoming.pc.0 == ENTER {
            require(returned.registers[0].0 == receipt.incoming.cpsr.0 & 0x80 && returned.cpsr.0 & 0x80 == 0x80,
                "critical entry prior mask or masked return differs")?;
        } else {
            require(returned.cpsr.0 & 0x80 == receipt.incoming.registers[0].0 & 0x80,
                "critical exit restored mask differs")?;
        }
        decoder(machine)?;
        Ok(())
    })();
    let readback = (|| {
        for extent in COMMON.into_iter().chain(Some(frame)) {
            receipt.after.push(resource(machine, runner, extent)?);
        }
        aliases(machine, runner, &receipt.after, bindings)
    })();
    end(machine, &mut watch, &mut receipt.watch, outcome.and(readback))
}

pub(in crate::peer_witness::loader) fn observe(machine: &mut Machine, runner: &Runner<'_>, initializer: &super::Receipt,
    contract: &Contract, target: &mut Option<Receipt>) -> Result<(), String> {
    let start = context(machine)?;
    require(start.pc == initializer.after.pc && start.registers == initializer.after.registers && start.sp == initializer.after.sp
        && start.lr == initializer.after.lr && start.cpsr == initializer.after.cpsr && start.counts.0 == initializer.after.counts.0
        && start.tcm == initializer.after.tcm, "critical pair does not continue initializer exit")?;
    *target = Some(Receipt { role: contract.role, prelude_start: start, entry: None, transition_watch: None, exit: None,
        returned: None, prelude_watch: None, caller_watch: None, caller_frame_before: None, caller_frame_after: None,
        resources: Vec::new(), decoder: decoder(machine)?, aliases: Vec::new() });
    let receipt = target.as_mut().ok_or("critical receipt absent")?;
    let mut prelude = begin(machine, runner, Phase::Prelude, None)?;
    let mut entry = None;
    let outcome = stop(machine, ENTER).and_then(|incoming| {
        require(incoming.sp.0 == 0x400f33a8 && incoming.lr.0 == ENTRY_RETURN && incoming.cpsr.0 & 0xdf == 0xd3
            && incoming.tcm == receipt.prelude_start.tcm, "critical natural entry context differs")?;
        source_bindings(machine, contract)?;
        let state = capture(&prelude)?;
        validate_prelude(&state, &receipt.prelude_start, &incoming)?;
        receipt.resources = resources(machine, runner, None)?;
        aliases(machine, runner, &receipt.resources, &mut receipt.aliases)?;
        let caller_frame = resource(machine, runner, CALLER_FRAME)?;
        aliases(machine, runner, std::slice::from_ref(&caller_frame), &mut receipt.aliases)?;
        receipt.caller_frame_before = Some(caller_frame);
        entry = Some(incoming);
        Ok(())
    });
    end(machine, &mut prelude, &mut receipt.prelude_watch, outcome)?;
    boundary(machine, runner, contract, entry.ok_or("critical entry absent")?, ENTRY_RETURN,
        contract.entry_frame_bytes, &mut receipt.entry, &mut receipt.aliases)?;
    let mut transition = begin(machine, runner, Phase::Transition, None)?;
    let mut incoming_exit = None;
    let outcome = (|| {
        let sites = EXIT_CALLS.into_iter().map(|site| (site, "critical-exit-call".into())).collect();
        machine.traces.arm(&machine.session, sites)?;
        machine.traces.stop_labels = ["critical-exit-call".into()].into_iter().collect();
        loop {
            require(machine.instructions() < BUDGET, "critical global instruction budget exhausted")?;
            let previous = machine.traces.hits.len();
            machine.run(BUDGET - machine.instructions());
            machine.require_running()?;
            require(machine.traces.hits.len() > previous, "critical wrapper exit call not reached")?;
            if machine.traces.hits[previous..].iter().any(|hit| hit.core == 1) { break; }
        }
        let call = context(machine)?;
        require(EXIT_CALLS.contains(&call.pc.0) && call.registers[0] == call.registers[7]
            && call.registers[7].0 == receipt.entry.as_ref().and_then(|entry| entry.returned.as_ref())
                .ok_or("critical entry return absent")?.registers[0].0, "critical exit argument is not saved prior mask")?;
        require(word(machine, u64::from(call.pc.0) - 4)?.0 == 0xe1a00007, "critical exit call lacks MOV r0,r7")?;
        let incoming = stop(machine, LEAVE)?;
        require(incoming.lr.0 == call.pc.0 + 4 && incoming.counts.0[1] == call.counts.0[1] + 1
            && incoming.registers == call.registers && incoming.sp == call.sp && incoming.cpsr == call.cpsr,
            "critical exit did not follow actual wrapper BL")?;
        incoming_exit = Some(incoming);
        Ok(())
    })();
    end(machine, &mut transition, &mut receipt.transition_watch, outcome)?;
    let incoming = incoming_exit.ok_or("critical exit absent")?;
    let return_pc = incoming.lr.0;
    boundary(machine, runner, contract, incoming, return_pc, contract.exit_frame_bytes, &mut receipt.exit, &mut receipt.aliases)?;
    let mut continuation = begin(machine, runner, Phase::Transition, None)?;
    let outcome = stop(machine, CALLER_RETURN).and_then(|returned| {
        receipt.returned = Some(returned);
        source_bindings(machine, contract)?;
        aliases(machine, runner, &resources(machine, runner, None)?, &mut receipt.aliases)?;
        let caller_frame = resource(machine, runner, CALLER_FRAME)?;
        aliases(machine, runner, std::slice::from_ref(&caller_frame), &mut receipt.aliases)?;
        receipt.caller_frame_after = Some(caller_frame);
        decoder(machine)?;
        Ok(())
    });
    let mut continuation_state = None;
    let result = end(machine, &mut continuation, &mut continuation_state, outcome);
    receipt.caller_watch = continuation_state;
    result
}
