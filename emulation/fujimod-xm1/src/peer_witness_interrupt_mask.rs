use super::*;

const ROOTS: (u64, u64) = (0x96bd20, 0x96bd28);
const SLOT: (u64, u64) = (0xddc04, 0xddc08);
const CALL_FRAME: (u64, u64) = (0xf3384, 0xf33a8);
const SELECTOR: (u64, u64) = (0x7caf34, 0x7caf38);
const MAGIC: (u64, u64) = (0xc080c, 0xc0810);
const DISPATCH: u32 = 0x723600;
const ENTERED: u32 = 0xd1140;
const RETURNED: u32 = 0x723608;
const AFTER: u32 = 0x72360c;
const IMAGE_SHA: &str = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16";
const STAGE4: &[u32] = &[
    0xe92d4830,0xe28db00c,0xe24dd020,0xe3500000,0x1a00000d,0xebfee8dc,0xe3500001,0x1a000006,
    0xe3022e00,0xe3402035,0xe30e0c00,0xe3400089,0xe3a01000,0xe1a02122,0xeb0214da,0xeb0222f2,
    0xe3500000,0x0a00004c,0xeafffffe,0xe3500001,0x1a000010,0xe3a03068,0xe50b3028,0xe3013960,
    0xe3403078,0xe50b3024,0xe3a03001,0xe50b3020,0xe3a03b01,0xe50b301c,0xe30d3160,0xe3403069,
    0xe50b3018,0xe3a04000,0xe50b4014,0xe24b0028,0xeb0223d2,0xea000039,0xe3500002,0x030c4c6c,
    0x03404069,0x0a000035,0xe3500003,0x1a000010,0xe3a03066,0xe50b3028,0xe3013968,0xe3403078,
    0xe50b3024,0xe3a0300f,0xe50b3020,0xe3a03b01,0xe50b301c,0xe30c3bf0,0xe3403069,0xe50b3018,
    0xe3a04000,0xe50b4014,0xe24b0028,0xeb0223bb,0xea000022,0xe3500004,0x13a04000,0x1a00001f,
    0xe3a03067,0xe50b3028,0xe3013970,0xe3403078,0xe50b3024,0xe3a0300f,0xe50b3020,0xe3a05b01,
    0xe50b501c,0xe30c3bf0,0xe3403069,0xe50b3018,0xe3a04000,0xe50b4014,0xe24b0028,0xeb0223a7,
];
const WRAPPER: &[u32] = &[0xe92d48f0,0xe28db014,0xe24dd020,0xe1a04000,0xe30a3d9c,0xe340307c,
    0xe1d020b0,0xe2422001,0xe7936102,0xebfff5f5,0xe1a07000];
const DISPATCH_CODE: &[u32] = &[0xe92d4df0,0xe28db01c,0xe30b5d20,0xe3405096,0xe5952000,0xe3013618,
    0xe7923003,0xe12fff33,0xe1a04000,0xe7953100,0xe5933164,0xe12fff33,0xe1a0a000];
const SERVICE: &[u32] = &[0xe52db004,0xe28db000,0xe10f0000,0xe3801080,0xe121f001,0xe2000080,
    0xe28bd000,0xe8bd0800,0xe12fff1e];
const CODE: [(u32, &[u32]); 4] = [(0x69cdf8, STAGE4),(0x725dd8, WRAPPER),(0x7235d8, DISPATCH_CODE),(ENTERED, SERVICE)];

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ConsumerKind { InterruptMask }

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Code { start: Guest, words: Vec<Word> }

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::peer_witness::loader) struct Contract {
    kind: ConsumerKind,
    source_sha256: Sha256Digest,
    loader_b_sha256: Sha256Digest,
    image4_sha256: Sha256Digest,
    code: Vec<Code>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::peer_witness) struct ConsumerWire {
    schema: u32,
    initializer: InitializerWire,
    consumer: Contract,
}
impl ConsumerWire {
    pub(in crate::peer_witness) fn validate(self) -> Result<LoaderContract, String> {
        require(self.schema == 7, "initializer consumer requires schema 7")?;
        let mut contract = self.initializer.validate()?;
        let consumer = self.consumer;
        require(consumer.source_sha256 == Sha256Digest::parse(SOURCE)?
            && consumer.loader_b_sha256 == Sha256Digest::parse(LOADER_B_SHA256)?
            && consumer.image4_sha256 == Sha256Digest::parse(IMAGE_SHA)?, "consumer source identities differ")?;
        require(consumer.code.len() == CODE.len() && consumer.code.iter().zip(CODE).all(|(code, (start, words))|
            code.start == Guest(start) && code.words.len() == words.len()
                && code.words.iter().zip(words).all(|(actual, expected)| actual.0 == *expected)),
            "consumer canonical source code differs")?;
        contract.consumer = Some(consumer);
        Ok(contract)
    }
}

#[derive(Serialize)]
struct CodeViews { start: Guest, views: Views }
#[derive(Serialize)]
struct ServiceAlias { core: usize, guest: Address, physical: Address, words: [Word; 9] }
#[derive(Serialize)]
pub(in crate::peer_witness::loader) struct Receipt {
    kind: ConsumerKind,
    start: Context, dispatch: Context, entered: Context, returned: Context, after: Context,
    roots: Views, slot_before: Views, slot_after: Views, frame_before: Views, frame_after: Views,
    table_before: Views, table_after: Views, code: Vec<CodeViews>, decoder: Decoder,
    aliases: Vec<AliasBinding>, service_aliases: Vec<ServiceAlias>, prelude_watch: WatchState, watch: WatchState,
}
#[derive(Serialize)]
pub(in crate::peer_witness::loader) struct OuterReceipt<'a, I: Serialize> {
    pub schema: u32,
    pub result: WitnessResult,
    pub error: Option<&'a str>,
    pub machine_failure: Option<&'a str>,
    pub initializer: &'a I,
    pub consumer: &'a Option<Receipt>,
}

fn span((start, end): (u64, u64)) -> Result<ScopeSpan, String> {
    PhysicalRange::new(start, end).map(ScopeSpan::Physical)
}
fn code_spans() -> impl Iterator<Item = (u64, u64)> {
    CODE.into_iter().map(|(start, words)| (u64::from(start), u64::from(start) + words.len() as u64 * 4))
}
fn protected(frame: bool) -> Vec<(u64, u64)> {
    let mut spans = vec![TABLE, ROOTS, SELECTOR];
    spans.extend(code_spans());
    spans.push((0xd0ab0, 0xd0af0));
    if frame { spans.push(CALL_FRAME); }
    spans
}
fn begin(machine: &mut Machine, runner: &Runner<'_>, prelude: bool) -> Result<Box<ScopedMemoryWatch>, String> {
    let class = |(start, end), kinds| WatchRange { start, end, kinds };
    let mut classes = vec![class(TABLE, WatchKinds::CPU_READ.union(WatchKinds::WRITES)),
        class(ROOTS, if prelude { WatchKinds::CPU_READ.union(WatchKinds::WRITES) } else { WatchKinds::WRITES })];
    if !prelude { classes.push(class(CALL_FRAME, WatchKinds::CPU_READ.union(WatchKinds::WRITES))); }
    classes.extend(code_spans().enumerate().map(|(index, extent)| class(extent,
        if prelude && index < 3 { WatchKinds::CPU_FETCH.union(WatchKinds::WRITES) } else { WatchKinds::WRITES })));
    classes.push(class((0xd0ab0, 0xd0af0), if prelude {
        WatchKinds::CPU_FETCH.union(WatchKinds::WRITES)
    } else { WatchKinds::WRITES }));
    if prelude {
        classes.extend([class(SELECTOR, WatchKinds::CPU_READ.union(WatchKinds::WRITES)), class(MAGIC, WatchKinds::CPU_READ)]);
    }
    let guarded = protected(!prelude);
    let owned = runner.contract.initializer.as_ref().is_some_and(ContractInitializer::owns_body);
    let mut forbidden_reads = Vec::with_capacity(2);
    if prelude { forbidden_reads.push(span(TABLE)?); }
    if owned { forbidden_reads.push(span(BODY)?); }
    let mut confinement = if prelude {
        vec![ConfinementRule { cores: CoreSet::new(&[0, 1])?, access: Access::Write,
            rule: Confinement::Forbid(guarded.into_iter().map(span).collect::<Result<_, _>>()?) }]
    } else {
        vec![ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Write,
            rule: Confinement::Allow(vec![span((CALL_FRAME.0, CALL_FRAME.0 + 4))?]) },
            ConfinementRule { cores: CoreSet::new(&[0])?, access: Access::Write,
                rule: Confinement::Forbid(guarded.into_iter().map(span).collect::<Result<_, _>>()?) }]
    };
    if !forbidden_reads.is_empty() {
        confinement.push(ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Read,
            rule: Confinement::Forbid(forbidden_reads) });
    }
    if owned {
        confinement.push(ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Fetch,
            rule: Confinement::Forbid(vec![span(BODY)?]) });
    }
    machine.session.board.memory_watch_begin_scoped(ScopedWatch {
        classes, limit: WATCH_LIMIT, confinement, counters: Vec::new(), coverage: Vec::new(),
    }, runner.maps.clone())
}

use super::Contract as ContractInitializer;

fn stop(machine: &mut Machine, address: u32) -> Result<Context, String> {
    machine.traces.arm(&machine.session, BTreeMap::from([(address, "interrupt-mask-boundary".into())]))?;
    machine.traces.stop_labels = ["interrupt-mask-boundary".into()].into_iter().collect();
    loop {
        require(machine.instructions() < BUDGET, "consumer global instruction budget exhausted")?;
        let previous = machine.traces.hits.len();
        machine.run(BUDGET - machine.instructions());
        machine.require_running()?;
        let hits = &machine.traces.hits[previous..];
        require(!hits.is_empty(), "consumer boundary not reached")?;
        if hits.iter().any(|hit| hit.core == 1) {
            let observed = context(machine)?;
            require(observed.pc.0 == address, "consumer boundary context differs")?;
            return Ok(observed);
        }
    }
}
fn snapshot(machine: &Machine, runner: &Runner<'_>, extent: (u64, u64)) -> Result<Views, String> {
    let views = runner.views(machine, 1, ZERO_GUEST + extent.0, (extent.1 - extent.0) as usize)?;
    equal_views(&views)?;
    Ok(views)
}
fn digest_resources<'a>(roots: &'a Views, slot: &'a Views, frame: &'a Views, table: &'a Views,
    code: &'a [CodeViews]) -> [((u64, u64), &'a Views); 7] {
    std::array::from_fn(|index| match index {
        0 => (ROOTS, roots), 1 => (SLOT, slot), 2 => (CALL_FRAME, frame), 3 => (TABLE, table),
        _ => {
            let code = &code[index - 4];
            let start = u64::from(code.start.0);
            ((start, start + code.views.physical.len() as u64), &code.views)
        }
    })
}
fn aliases(machine: &Machine, runner: &Runner<'_>, tcm: &TcmWords,
    resources: &[((u64, u64), &Views); 7], hashes: &[Sha256Digest; 7], result: &mut Vec<AliasBinding>) -> Result<(), String> {
    let guarded: [(u64, u64); 7] = std::array::from_fn(|index| resources[index].0);
    tcm_disjoint_views(&runner.committed, tcm, &guarded)?;
    let mut buffer = [0; (TABLE.1 - TABLE.0) as usize];
    for core in 0..2 {
        require(machine.session.board.cpu(core)?.rprofile_identity(None)? == [0x4003, 0x5003][core], "consumer CP14 identity differs")?;
        for ((&(start, end), views), hash) in resources.iter().map(|(extent, views)| (extent, *views)).zip(hashes) {
            for guest in [start, ZERO_GUEST + start] {
                require(runner.physical_of(core, guest, end - start)? == start, "consumer alias mapping differs")?;
                let buffer = &mut buffer[..views.physical.len()];
                machine.session.board.read_memory(core, guest, buffer)?;
                require(buffer == views.physical.as_slice(), "consumer alias bytes differ")?;
                result.push(AliasBinding { core, guest: Address(guest), physical: Address(start), size: end - start, sha256: *hash });
            }
        }
    }
    Ok(())
}
fn service_aliases(machine: &Machine, runner: &Runner<'_>, tcm: &TcmWords,
    result: &mut Vec<ServiceAlias>) -> Result<(), String> {
    let start = u64::from(ENTERED);
    tcm_disjoint_views(&runner.committed, tcm, &[(start, start + 36)])?;
    let mut bytes = [0; 36];
    for core in 0..2 {
        for guest in [start, ZERO_GUEST + start] {
            require(runner.physical_of(core, guest, 36)? == start, "consumer service alias mapping differs")?;
            machine.session.board.read_memory(core, guest, &mut bytes)?;
            let words: [Word; 9] = std::array::from_fn(|index| {
                let offset = index * 4;
                Word(u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]))
            });
            require(words.iter().zip(SERVICE).all(|(actual, expected)| actual.0 == *expected),
                "consumer service alias words differ")?;
            result.push(ServiceAlias { core, guest: Address(guest), physical: Address(start), words });
        }
    }
    Ok(())
}
fn observed(watch: &ScopedMemoryWatch) -> Result<WatchState, String> {
    let state: WatchState = serde_json::from_value(watch.state()).map_err(|error| error.to_string())?;
    require(state.enabled && state.violation.is_none() && state.fault.is_none(), "consumer watch faulted")?;
    Ok(state)
}
fn validate_prelude(state: &WatchState, start: &Context, dispatch: &Context) -> Result<(), String> {
    let mut fetches = (0xd0ab0_u64..0xd0af0).step_by(4)
        .chain((0x69cdf8..0x69ce0c).step_by(4))
        .chain([0x69ce44,0x69ce48,0x69ce90,0x69ce94,0x69ce98,0x69ce9c,0x69cea0,0x69cea4])
        .chain((0x69ceec..0x69cf38).step_by(4))
        .chain((0x725dd8..0x725e00).step_by(4))
        .chain((0x7235d8..0x723604).step_by(4));
    let mut selector = false;
    let mut magic = false;
    let mut primary = false;
    let mut secondary = false;
    let mut last_instruction: u64 = start.counts.0.iter().sum();
    let dispatch_instructions: u64 = dispatch.counts.0.iter().sum();
    for event in &state.records {
        require(!matches!(event.source, Source::CpuWrite | Source::HostWrite | Source::ModelBusWrite), "consumer prelude protected write")?;
        let instructions = event.instructions.ok_or("consumer prelude instruction attribution absent")?;
        // The fetch observer precedes the instruction point's count rollback.
        let stopped_fetch = matches!(event.source, Source::CpuFetch) && event.pc == Some(u64::from(DISPATCH));
        let upper_bound = dispatch_instructions + u64::from(stopped_fetch);
        require(event.size == 4 && event.byte_order == Some(ByteOrder::Little) && event.pc.is_some() && event.tick.is_some()
            && matches!(event.core, Some(0 | 1)) && matches!(event.source, Source::CpuRead | Source::CpuFetch)
            && event.physical.is_some_and(|physical| event.address == physical || event.address == ZERO_GUEST + physical)
            && instructions >= last_instruction && instructions <= upper_bound,
            "consumer prelude event attribution differs")?;
        last_instruction = instructions;
        if event.core != Some(1) { continue; }
        if matches!(event.source, Source::CpuFetch) {
            let expected = fetches.next().ok_or("consumer prelude executed extra selected instructions")?;
            require(event.pc == Some(expected) && event.address == expected && event.physical == Some(expected),
                "consumer authentic source fetch order differs")?;
            if expected == u64::from(DISPATCH) {
                require(instructions == dispatch_instructions + 1, "consumer stopped fetch attribution differs")?;
            }
        } else if matches!(event.source, Source::CpuRead) {
            match event.physical {
                Some(address) if address == SELECTOR.0 => {
                    require(!selector && event.pc == Some(0x725df8) && event.value == Some(2), "consumer task103 selector differs")?;
                    selector = true;
                }
                Some(address) if address == MAGIC.0 => {
                    require(matches!(event.pc, Some(0xd38b8 | 0xd38cc | 0xd391c)), "consumer magic read provenance differs")?;
                    magic = event.pc == Some(0xd391c) && event.value == Some(0x87654321);
                }
                Some(address) if address == ROOTS.0 => {
                    require(!primary && event.pc == Some(0x7235e8) && event.value == Some(0x4004dae0), "consumer primary root differs")?;
                    primary = true;
                }
                Some(address) if address == ROOTS.0 + 4 => {
                    require(!secondary && event.pc == Some(0x7235fc) && event.value == Some(TABLE_GUEST), "consumer secondary root differs")?;
                    secondary = true;
                }
                _ => return Err("consumer prelude unexpected published-table read".into()),
            }
        } else { return Err("consumer prelude unexpected event source".into()); }
    }
    require(fetches.next().is_none() && selector && magic && primary && secondary, "consumer authentic prelude evidence incomplete")
}

pub(in crate::peer_witness::loader) fn observe(machine: &mut Machine, runner: &Runner<'_>,
    initializer: &super::Receipt, contract: &Contract) -> Result<Receipt, String> {
    let start = context(machine)?;
    require(start.pc == initializer.after.pc && start.registers == initializer.after.registers && start.sp == initializer.after.sp
        && start.lr == initializer.after.lr && start.cpsr == initializer.after.cpsr && start.counts.0 == initializer.after.counts.0
        && start.tcm == initializer.after.tcm, "consumer does not continue the complete initializer exit")?;
    let mut prelude = begin(machine, runner, true)?;
    let prelude_outcome = (|| {
        let dispatch = stop(machine, DISPATCH)?;
        require(dispatch.registers[0].0 == 1 && dispatch.registers[3].0 == TABLE_GUEST as u32
            && dispatch.registers[4].0 == 1 && dispatch.registers[5].0 == ROOTS.0 as u32
            && dispatch.sp.0 == 0x400f3388 && dispatch.registers[11].0 == 0x400f33a4 && dispatch.lr.0 == 0x7235f8
            && dispatch.cpsr.0 & 0x07f0_fe20 == 0 && dispatch.cpsr.0 & 0xdf == 0xd3 && dispatch.tcm == start.tcm,
            "consumer source dispatch context differs")?;
        let state = observed(&prelude)?;
        validate_prelude(&state, &start, &dispatch)?;
        Ok((dispatch, state))
    })();
    let end = machine.session.board.memory_watch_end_scoped(&mut prelude);
    let (dispatch, prelude_watch) = match prelude_outcome {
        Ok(value) => { end?; value }, Err(error) => { let _ = end; return Err(error); }
    };
    let decoder_before = decoder(machine)?;
    let roots = snapshot(machine, runner, ROOTS)?;
    require(roots.physical == [0xe0,0xda,0x04,0x40,0xa0,0xda,0x0d,0x40], "consumer root readback differs")?;
    let slot_before = snapshot(machine, runner, SLOT)?;
    require(slot_before.physical == ENTERED.to_le_bytes(), "consumer InterruptMask slot differs")?;
    let frame_before = snapshot(machine, runner, CALL_FRAME)?;
    let caller_words = [0x400f33e4,0x400,2,dispatch.registers[7].0,dispatch.registers[8].0,
        dispatch.registers[10].0,0x400f33dc,0x725e00];
    require(frame_before.physical[4..].chunks_exact(4).zip(caller_words)
        .all(|(bytes, word)| bytes == word.to_le_bytes()), "consumer authentic caller frame differs")?;
    let table_before = snapshot(machine, runner, TABLE)?;
    require(table_before.physical == initializer.table_after.physical, "consumer table differs from complete initializer output")?;
    let mut code = Vec::with_capacity(CODE.len());
    for source in &contract.code {
        let extent = (u64::from(source.start.0), u64::from(source.start.0) + source.words.len() as u64 * 4);
        let views = snapshot(machine, runner, extent)?;
        require(views.physical.chunks_exact(4).zip(&source.words).all(|(bytes, word)| bytes == word.0.to_le_bytes()),
            "consumer source instruction readback differs")?;
        code.push(CodeViews { start: source.start, views });
    }
    let mut bindings = Vec::with_capacity(56);
    let mut service_bindings = Vec::with_capacity(8);
    let resources = digest_resources(&roots, &slot_before, &frame_before, &table_before, &code);
    let mut hashes: [Sha256Digest; 7] = std::array::from_fn(|index| Sha256Digest::of(&resources[index].1.physical));
    aliases(machine, runner, &dispatch.tcm, &resources, &hashes, &mut bindings)?;
    service_aliases(machine, runner, &dispatch.tcm, &mut service_bindings)?;
    let mut watch = begin(machine, runner, false)?;
    let outcome = (|| {
        let entered = stop(machine, ENTERED)?;
        let returned = stop(machine, RETURNED)?;
        let after = stop(machine, AFTER)?;
        let incoming = dispatch.cpsr.0;
        require(entered.counts.0[1] == dispatch.counts.0[1] + 2 && returned.counts.0[1] == dispatch.counts.0[1] + 11
            && after.counts.0[1] == dispatch.counts.0[1] + 12, "consumer core1 instruction extent differs")?;
        let mut entry_registers = dispatch.registers;
        entry_registers[3] = Word(ENTERED);
        require(entered.registers == entry_registers && entered.sp == dispatch.sp && entered.lr.0 == RETURNED
            && entered.cpsr == dispatch.cpsr, "consumer actual BLX entry differs")?;
        let mut return_registers = entered.registers;
        return_registers[0] = Word(incoming & 0x80);
        return_registers[1] = Word(incoming | 0x80);
        require(returned.registers == return_registers && returned.sp == entered.sp && returned.lr == entered.lr
            && returned.cpsr.0 == incoming | 0x80, "consumer service return effects differ")?;
        return_registers[10] = return_registers[0];
        require(after.registers == return_registers && after.sp == returned.sp && after.lr == returned.lr && after.cpsr == returned.cpsr,
            "consumer caller copy effects differ")?;
        require([&entered,&returned,&after].iter().all(|context| context.tcm == dispatch.tcm)
            && start.counts.0[0] <= dispatch.counts.0[0] && dispatch.counts.0[0] <= entered.counts.0[0]
            && entered.counts.0[0] <= returned.counts.0[0] && returned.counts.0[0] <= after.counts.0[0],
            "consumer TCM state or primary count ordering differs")?;
        decoder(machine)?;
        let slot_after = snapshot(machine, runner, SLOT)?;
        let frame_after = snapshot(machine, runner, CALL_FRAME)?;
        let table_after = snapshot(machine, runner, TABLE)?;
        require(slot_after.physical == slot_before.physical && table_after.physical == table_before.physical
            && frame_after.physical[4..] == frame_before.physical[4..]
            && frame_after.physical[..4] == entered.registers[11].0.to_le_bytes(), "consumer committed frame or table effects differ")?;
        for ((extent, before), actual) in code_spans().zip(&code).zip(&contract.code) {
            require(snapshot(machine, runner, extent)?.physical == before.views.physical && before.start == actual.start,
                "consumer code changed during execution")?;
        }
        require(snapshot(machine, runner, ROOTS)?.physical == roots.physical, "consumer roots changed")?;
        let resources = digest_resources(&roots, &slot_after, &frame_after, &table_after, &code);
        hashes[2] = Sha256Digest::of(&frame_after.physical);
        aliases(machine, runner, &after.tcm, &resources, &hashes, &mut bindings)?;
        service_aliases(machine, runner, &after.tcm, &mut service_bindings)?;
        let state = observed(&watch)?;
        require(state.records.len() == 3, "consumer data effect count differs")?;
        let mut last_instruction: u64 = dispatch.counts.0.iter().sum();
        for (event, (source, physical, pc, value)) in state.records.iter().zip([
            (Source::CpuRead,SLOT.0,u64::from(DISPATCH),ENTERED),
            (Source::CpuWrite,CALL_FRAME.0,u64::from(ENTERED),entered.registers[11].0),
            (Source::CpuRead,CALL_FRAME.0,0xd115c,entered.registers[11].0)]) {
            require(event.source == source && event.core == Some(1)
                && event.address == ZERO_GUEST + physical && event.physical == Some(physical) && event.size == 4
                && event.pc == Some(pc) && event.value == Some(u64::from(value)) && event.byte_order == Some(ByteOrder::Little)
                && event.tick.is_some() && event.instructions.is_some_and(|count|
                    count >= last_instruction && count < after.counts.0.iter().sum()),
                "consumer data effect origin, value or attribution differs")?;
            last_instruction = event.instructions.ok_or("consumer instruction attribution absent")?;
        }
        Ok(Receipt { kind: ConsumerKind::InterruptMask, start, dispatch, entered, returned, after,
            roots, slot_before, slot_after, frame_before, frame_after, table_before, table_after, code, decoder: decoder_before,
            aliases: bindings, service_aliases: service_bindings, prelude_watch, watch: state })
    })();
    let end = machine.session.board.memory_watch_end_scoped(&mut watch);
    match outcome { Ok(receipt) => end.map(|()| receipt), Err(error) => { let _ = end; Err(error) } }
}
