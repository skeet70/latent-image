//! CPU0 primary publication observation from stock reset.
use super::*;

#[path = "peer_witness_primary_owned.rs"]
mod owned;
#[path = "peer_witness_primary_root0.rs"]
pub(in crate::peer_witness) mod root0;

const LOADER: (u64, u64) = (0x40000, 0x4dae0);
const WORKSPACE: (u64, u64) = (0x4dae0, 0x63460);
const PRIMARY_TABLE: (u64, u64) = (0x4dae0, 0x4f10c);
const PRIMARY_POOLS: (u64, u64) = (0x5beac, 0x6194c);
const STACK: u64 = 0x4006_3450;
const LEAF: (u32, u32) = (0x40430, 0x409ac);
const LOADER_SHA: &str = "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882";
const RUN_SHA: &str = "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef";
const ROM_SHA: &str = "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be";

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PrimaryRole { RetainedPrimary, OwnedPrimary }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrimaryDto {
    role: PrimaryRole,
    budget: Instructions,
    source_sha256: Sha256Digest,
    loader_a_sha256: Sha256Digest,
    run_sha256: Sha256Digest,
    boot_rom_sha256: Sha256Digest,
    #[serde(default, deserialize_with = "owned_payload")]
    owned: Option<owned::OwnedProgram>,
}

enum PrimaryProgram { Retained, Owned(owned::OwnedProgram) }

pub(in crate::peer_witness) struct PrimaryContract {
    program: PrimaryProgram,
    source_sha256: Sha256Digest,
    loader_a_sha256: Sha256Digest,
    run_sha256: Sha256Digest,
    boot_rom_sha256: Sha256Digest,
    consumer: Option<root0::Contract>,
}

fn owned_payload<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<owned::OwnedProgram>, D::Error> {
    owned::OwnedProgram::deserialize(deserializer).map(Some)
}

impl PrimaryContract {
    fn role(&self) -> PrimaryRole {
        match &self.program { PrimaryProgram::Retained => PrimaryRole::RetainedPrimary, PrimaryProgram::Owned(_) => PrimaryRole::OwnedPrimary }
    }

    fn owned(&self) -> Option<&owned::OwnedProgram> {
        match &self.program { PrimaryProgram::Retained => None, PrimaryProgram::Owned(program) => Some(program) }
    }

    pub(in crate::peer_witness) fn admit(&self, inputs: &InputIdentities) -> Result<(), String> {
        let update = self.owned().map_or(self.source_sha256, |program| program.input_sha256);
        require(inputs.update == update && inputs.run == self.run_sha256
            && inputs.boot_rom == self.boot_rom_sha256, "primary witness machine whole input identities differ")
    }

    pub(in crate::peer_witness) fn admit_update(&self, bytes: &[u8]) -> Result<(), String> {
        if let Some(program) = self.owned() { program.admit_update(bytes, self.source_sha256)?; }
        Ok(())
    }

    fn identity(&self) -> ContractIdentityPrimary {
        ContractIdentityPrimary { role: self.role(), source_sha256: self.source_sha256, loader_a_sha256: self.loader_a_sha256,
            run_sha256: self.run_sha256, boot_rom_sha256: self.boot_rom_sha256,
            owned: self.owned().map(owned::OwnedProgram::identity) }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::peer_witness) struct PrimaryWire { schema: u32, primary: PrimaryDto }
impl PrimaryWire {
    pub(in crate::peer_witness) fn validate(self) -> Result<PrimaryContract, String> {
        require(self.schema == 8 && self.primary.budget.0 == BUDGET
            && self.primary.source_sha256 == Sha256Digest::parse(SOURCE)?
            && (self.primary.role == PrimaryRole::OwnedPrimary || self.primary.loader_a_sha256 == Sha256Digest::parse(LOADER_SHA)?)
            && self.primary.run_sha256 == Sha256Digest::parse(RUN_SHA)?
            && self.primary.boot_rom_sha256 == Sha256Digest::parse(ROM_SHA)?,
            "primary observation requires canonical schema 8 and finite budget 50000000")?;
        let program = match (self.primary.role, self.primary.owned) {
            (PrimaryRole::RetainedPrimary, None) => PrimaryProgram::Retained,
            (PrimaryRole::OwnedPrimary, Some(program)) => {
                program.validate(self.primary.loader_a_sha256)?;
                PrimaryProgram::Owned(program)
            }
            _ => return Err("primary role and owned payload differ".into()),
        };
        Ok(PrimaryContract { program, source_sha256: self.primary.source_sha256, loader_a_sha256: self.primary.loader_a_sha256,
            run_sha256: self.primary.run_sha256, boot_rom_sha256: self.primary.boot_rom_sha256, consumer: None })
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PrimaryBoundary { Caller, Before, Entered, After }
impl PrimaryBoundary {
    fn address(self) -> u32 {
        match self { Self::Caller => 0x409ac, Self::Before => 0x409dc, Self::Entered => LEAF.0, Self::After => 0x409e0 }
    }
    fn label(self) -> &'static str {
        match self { Self::Caller => "primary-caller", Self::Before => "primary-before", Self::Entered => "primary-entered", Self::After => "primary-after" }
    }
}

#[derive(Serialize)]
struct BoundaryObservation { boundary: PrimaryBoundary, context: Context }

#[derive(Serialize)]
struct Observation {
    contract: ContractIdentityPrimary,
    caller: Context, before: Context, entered: Context, after: Context,
    frame_before: Views, frame_after: Views,
    table_before: Views, table_after: Views,
    tail_before: Views, tail_after: Views,
    pools_before: Views, pools_after: Views,
    code_before: Views, code_after: Views,
    #[serde(skip_serializing_if = "Option::is_none")]
    scratch_before: Option<Views>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scratch_after: Option<Views>,
    decoder: Decoder, aliases: Vec<AliasBinding>, publications: Vec<Publication>, watch: WatchState,
}

#[derive(Serialize)]
struct ContractIdentityPrimary {
    role: PrimaryRole, source_sha256: Sha256Digest, loader_a_sha256: Sha256Digest,
    run_sha256: Sha256Digest, boot_rom_sha256: Sha256Digest,
    #[serde(skip_serializing_if = "Option::is_none")]
    owned: Option<owned::OwnedIdentity>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ObservationStatus { Observed, Absent, Refused }

#[derive(Serialize)]
struct PrimaryReceipt<'a> {
    schema: u32, role: PrimaryRole, result: WitnessResult, status: ObservationStatus,
    contract: ContractIdentityPrimary,
    error: Option<&'a str>, machine_failure: Option<String>, instructions: Instructions,
    cp14_identities: [Identity; 2], r#final: Context, boundaries: Vec<BoundaryObservation>, primary: Option<Observation>,
}

struct ExpectedPublication { physical: Address, value: Word, pc: Guest }

fn source_publications(code: &[u8]) -> Result<Vec<ExpectedPublication>, String> {
    require(code.len() == (LOADER.1 - LOADER.0) as usize && Sha256Digest::of(code) == Sha256Digest::parse(LOADER_SHA)?,
        "primary complete initializer, caller and literal identity differs")?;
    let word = |offset: usize| u32::from_le_bytes([code[offset], code[offset + 1], code[offset + 2], code[offset + 3]]);
    let immediate = |word: u32| ((word >> 4) & 0xf000) | (word & 0xfff);
    let wide = |low: usize, high: usize, register: u32| -> Result<u32, String> {
        require(word(low) & 0xfff0_f000 == (0xe300_0000 | (register << 12))
            && word(high) & 0xfff0_f000 == (0xe340_0000 | (register << 12)), "primary source wide literal encoding differs")?;
        Ok(immediate(word(low)) | (immediate(word(high)) << 16))
    };
    require(word(0xa8) == 0xe59f4020 && word(0xac) == 0xe1a0d004 && word(0xd0) == (STACK + 16) as u32
        && word(0x9ac) == 0xe92d4830 && word(0x9b0) == 0xe28db00c && word(0x9dc) == 0xebfffe93
        && word(0x430) == 0xe92d0830 && word(0x434) == 0xe28db008
        && word(0x9a0) == 0xe24bd008 && word(0x9a4) == 0xe8bd0830 && word(0x9a8) == 0xe12fff1e,
        "primary stack frame, natural BL or return source differs")?;
    require(wide(0x438, 0x43c, 3)? == (ZERO_GUEST + PRIMARY_TABLE.0) as u32,
        "primary source root literal differs")?;
    let mut result = Vec::with_capacity(1419);
    for slot in 0..94_usize {
        let offset = 0x440 + slot * 12;
        require(word(offset + 8) == 0xe5832000 + slot as u32 * 4, "primary source service store differs")?;
        result.push(ExpectedPublication { physical: Address(PRIMARY_TABLE.0 + slot as u64 * 4),
            value: Word(wide(offset, offset + 4, 2)?), pc: Guest(LOADER.0 as u32 + offset as u32 + 8) });
    }
    for index in 0..5_usize {
        let offset = 0x8a8 + index * 16;
        require(word(offset + 12) == 0xe7831002, "primary source extension store differs")?;
        result.push(ExpectedPublication { physical: Address(PRIMARY_TABLE.0 + 0x1618 + index as u64 * 4),
            value: Word(wide(offset, offset + 8, 1)?), pc: Guest(LOADER.0 as u32 + offset as u32 + 12) });
    }
    let pool = wide(0x8fc, 0x900, 0)?;
    let extended = wide(0x904, 0x908, 5)?;
    let sites = [0x40918, 0x40928, 0x40930, 0x40940, 0x40948, 0x40958, 0x40960, 0x40970, 0x40978, 0x40988, 0x40990];
    for index in 0..120_u32 {
        let record = PRIMARY_TABLE.0 + 0x178 + u64::from(index) * 44;
        let values = [index + 1, pool + index * 40, pool + index * 40 + 4, pool + index * 40 + 8,
            pool + index * 40 + 12, pool + index * 40 + 16, pool + index * 40 + 20, pool + index * 40 + 24,
            pool + index * 40 + 28, extended + index * 56 + 24, extended + index * 56 + 28];
        result.extend(values.into_iter().zip(sites).enumerate().map(|(field, (value, pc))| ExpectedPublication {
            physical: Address(record + field as u64 * 4), value: Word(value), pc: Guest(pc),
        }));
    }
    Ok(result)
}

fn views(machine: &Machine, maps: &[factory::MemoryMap], guest: u64, length: usize) -> Result<Views, String> {
    let physical = static_span(maps, MemoryCore(0), MemoryAddress(guest)..MemoryAddress(guest + length as u64))?.physical.0;
    let observed = Views { r#virtual: wait::bytes(machine, 0, guest, length)?, physical: machine.read(physical, length)? };
    equal_views(&observed)?;
    Ok(observed)
}

fn aliases(machine: &Machine, maps: &[factory::MemoryMap], tcm: &TcmWords, code: &Views, workspace: &Views,
    result: &mut Vec<AliasBinding>) -> Result<(), String> {
    tcm_disjoint_views(maps, tcm, &[LOADER, WORKSPACE])?;
    for core in 0..2 {
        require(machine.session.board.cpu(core)?.rprofile_identity(None)? == [0x4003, 0x5003][core], "primary active CP14 identity differs")?;
    }
    let mut buffer = vec![0; workspace.physical.len().max(code.physical.len())];
    for ((start, end), captured) in [(LOADER, code), (WORKSPACE, workspace)] {
        let physical_bytes = &captured.physical;
        let sha256 = Sha256Digest::of(physical_bytes);
        for core in 0..2 {
            for guest in [start, ZERO_GUEST + start] {
                let physical = static_span(maps, MemoryCore(core), MemoryAddress(guest)..MemoryAddress(guest + end - start))?.physical.0;
                let buffer = &mut buffer[..physical_bytes.len()];
                machine.session.board.read_memory(core, guest, buffer)?;
                require(physical == start && buffer == physical_bytes.as_slice(),
                    "primary complete code/workspace mapping or byte view differs")?;
                result.push(AliasBinding { core, guest: Address(guest), physical: Address(physical), size: end - start, sha256 });
            }
        }
    }
    Ok(())
}

fn stop(machine: &mut Machine, boundary: PrimaryBoundary, boundaries: &mut Vec<BoundaryObservation>) -> Result<Option<Context>, String> {
    machine.traces.arm_primary(&machine.session, BTreeMap::from([(boundary.address(), boundary.label().into())]))?;
    machine.traces.stop_labels = [boundary.label().into()].into_iter().collect();
    let previous = machine.traces.hits.len();
    let remaining = BUDGET.checked_sub(machine.instructions()).ok_or("primary instruction deadline exceeded")?;
    if remaining == 0 { return Ok(None); }
    machine.run(remaining);
    machine.require_running()?;
    let hits = &machine.traces.hits[previous..];
    if hits.is_empty() {
        require(machine.instructions() == BUDGET, "primary boundary stopped before finite deadline")?;
        return Ok(None);
    }
    require(hits.len() == 1 && hits[0].core == 0 && hits[0].site == boundary.label()
        && hit_address(&hits[0])? == boundary.address(), "primary boundary attribution differs")?;
    let context = context_for(machine, 0)?;
    require(context.pc.0 == boundary.address(), "primary stop is not preexecution")?;
    boundaries.push(BoundaryObservation { boundary, context: context.clone() });
    Ok(Some(context))
}

fn observe(machine: &mut Machine, contract: &PrimaryContract, maps: &[factory::MemoryMap], committed: &[factory::MemoryMap],
    boundaries: &mut Vec<BoundaryObservation>) -> Result<Option<Observation>, String> {
    let Some(caller) = stop(machine, PrimaryBoundary::Caller, boundaries)? else { return Ok(None) };
    require(caller.sp.0 == (STACK + 16) as u32 && caller.registers[4].0 == (STACK + 16) as u32
        && caller.registers[0].0 == ARGUMENT && caller.lr.0 == 0x400c0 && caller.cpsr.0 & 0x07f0_feff == 0xd3,
        "primary natural caller source context differs")?;
    let Some(before) = stop(machine, PrimaryBoundary::Before, boundaries)? else { return Ok(None) };
    require(before.sp.0 == STACK as u32 && before.registers[11].0 == (STACK + 12) as u32
        && before.registers[4] == caller.registers[0] && before.lr.0 == 0x409dc
        && before.registers[5..11] == caller.registers[5..11] && before.tcm == caller.tcm
        && before.cpsr.0 & 0x0fff_ffff == caller.cpsr.0 & 0x0fff_ffff
        && before.cpsr.0 & 0x07f0_feff == 0xd3 && before.counts.0[0] > caller.counts.0[0]
        && before.counts.0[1] >= caller.counts.0[1], "primary pre-BL frame, mode or progress differs")?;
    let code_before = views(machine, committed, LOADER.0, (LOADER.1 - LOADER.0) as usize)?;
    let expected = match contract.owned() {
        Some(program) => {
            program.validate_loader(&code_before.physical, contract.loader_a_sha256)?;
            program.publications()?
        }
        None => source_publications(&code_before.physical)?,
    };
    let frame_before = views(machine, committed, STACK, 16)?;
    let saved = [caller.registers[4].0, caller.registers[5].0, caller.registers[11].0, caller.lr.0];
    require(frame_before.physical.chunks_exact(4).zip(saved).all(|(bytes, word)| bytes == word.to_le_bytes()),
        "primary caller saved frame provenance differs")?;
    let decoder_before = decoder(machine)?;
    let mut bindings = Vec::with_capacity(16);
    let workspace_before = views(machine, committed, WORKSPACE.0, (WORKSPACE.1 - WORKSPACE.0) as usize)?;
    aliases(machine, committed, &before.tcm, &code_before, &workspace_before, &mut bindings)?;
    let table_before = views(machine, committed, ZERO_GUEST + PRIMARY_TABLE.0, (PRIMARY_TABLE.1 - PRIMARY_TABLE.0) as usize)?;
    let tail_before = views(machine, committed, ZERO_GUEST + PRIMARY_TABLE.1, 16)?;
    let pools_before = views(machine, committed, ZERO_GUEST + PRIMARY_POOLS.0, (PRIMARY_POOLS.1 - PRIMARY_POOLS.0) as usize)?;
    let scratch = contract.owned().map(|program| program.scratch_span());
    let scratch_before = scratch.map(|(low, high)| views(machine, committed, ZERO_GUEST + low, (high - low) as usize)).transpose()?;
    let physical = |span: (u64, u64)| PhysicalRange::new(span.0, span.1).map(ScopeSpan::Physical);
    let mut classes = vec![WatchRange { start: PRIMARY_TABLE.0, end: PRIMARY_TABLE.1, kinds: WatchKinds::WRITES },
        WatchRange { start: WORKSPACE.0, end: WORKSPACE.1, kinds: WatchKinds::HOST_WRITE.union(WatchKinds::MODEL_BUS_WRITE) },
        WatchRange { start: LOADER.0, end: LOADER.1, kinds: WatchKinds::WRITES }];
    let mut allowed = vec![physical(PRIMARY_TABLE)?, physical(scratch.unwrap_or((STACK - ZERO_GUEST - 12, STACK - ZERO_GUEST)))?];
    if let Some((low, high)) = scratch {
        classes.push(WatchRange { start: u64::from(LEAF.0), end: u64::from(LEAF.1), kinds: WatchKinds::CPU_READ });
        classes.push(WatchRange { start: low, end: high, kinds: WatchKinds::CPU_READ.union(WatchKinds::CPU_WRITE) });
    }
    let mut confinement = vec![
        ConfinementRule { cores: CoreSet::new(&[0])?, access: Access::Write, rule: Confinement::Allow(std::mem::take(&mut allowed)) },
        ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Write,
            rule: Confinement::Forbid(vec![physical(WORKSPACE)?, physical(LOADER)?]) },
        ConfinementRule { cores: CoreSet::new(&[0])?, access: Access::Read,
            rule: match scratch {
                Some(extent) => Confinement::Allow(vec![physical((u64::from(LEAF.0), u64::from(LEAF.1)))?, physical(extent)?]),
                None => Confinement::Forbid(vec![physical(PRIMARY_TABLE)?]),
            } },
    ];
    if scratch.is_some() {
        confinement.push(ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Fetch,
            rule: Confinement::Forbid(vec![physical((u64::from(LEAF.0), u64::from(LEAF.1)))?]) });
    }
    let mut watch = machine.session.board.memory_watch_begin_scoped(ScopedWatch {
        classes, limit: WATCH_LIMIT,
        confinement, counters: Vec::new(), coverage: Vec::new(),
    }, maps.to_vec())?;
    let outcome = (|| {
        let Some(entered) = stop(machine, PrimaryBoundary::Entered, boundaries)? else { return Ok(None) };
        require(entered.registers == before.registers && entered.sp == before.sp && entered.lr.0 == 0x409e0
            && entered.cpsr == before.cpsr && entered.tcm == before.tcm
            && entered.counts.0[0] == before.counts.0[0] + 1 && entered.counts.0[1] >= before.counts.0[1],
            "primary entry is not the unchanged CPU0 context after its natural BL")?;
        let Some(after) = stop(machine, PrimaryBoundary::After, boundaries)? else { return Ok(None) };
        require(after.sp == before.sp && after.lr.0 == 0x409e0 && after.registers[4..12] == before.registers[4..12]
            && after.registers[..4] == [Word(0x4005beac), Word(0x4005d160), Word(0x40061930), Word(0x4004ef80)]
            && after.registers[12].0 == 120 && after.cpsr.0 == ((before.cpsr.0 & 0x0fff_ffff) | 0x6000_0000)
            && after.tcm == before.tcm
            && after.counts.0[0] == entered.counts.0[0] + contract.owned().map_or(4634, |program| program.instructions.0)
            && after.counts.0[1] >= entered.counts.0[1],
            "primary exact leaf return ABI, execution count or TCM differs")?;
        decoder(machine)?;
        let frame_after = views(machine, committed, STACK, 16)?;
        let table_after = views(machine, committed, ZERO_GUEST + PRIMARY_TABLE.0, (PRIMARY_TABLE.1 - PRIMARY_TABLE.0) as usize)?;
        let tail_after = views(machine, committed, ZERO_GUEST + PRIMARY_TABLE.1, 16)?;
        let pools_after = views(machine, committed, ZERO_GUEST + PRIMARY_POOLS.0, (PRIMARY_POOLS.1 - PRIMARY_POOLS.0) as usize)?;
        let code_after = views(machine, committed, LOADER.0, (LOADER.1 - LOADER.0) as usize)?;
        let workspace_after = views(machine, committed, WORKSPACE.0, (WORKSPACE.1 - WORKSPACE.0) as usize)?;
        aliases(machine, committed, &after.tcm, &code_after, &workspace_after, &mut bindings)?;
        let scratch_after = scratch.map(|(low, high)| views(machine, committed, ZERO_GUEST + low, (high - low) as usize)).transpose()?;
        require(frame_before.physical == frame_after.physical && tail_before.physical == tail_after.physical
            && pools_before.physical == pools_after.physical && code_before.physical == code_after.physical,
            "primary caller frame, guard, retained pool or complete loader changed")?;
        let observed: WatchState = serde_json::from_value(watch.state()).map_err(|error| error.to_string())?;
        require(observed.enabled && observed.violation.is_none() && observed.fault.is_none(),
            "primary continuous confinement differs")?;
        let records = if let Some(program) = contract.owned() {
            program.replay(&observed, &entered, &after, scratch_before.as_ref().ok_or("primary scratch preimage absent")?,
                scratch_after.as_ref().ok_or("primary scratch readback absent")?, &table_before, &table_after)?
        } else {
            require(observed.records.len() == expected.len(), "primary raw publication count differs")?;
            observed.records.iter().collect()
        };
        require(records.len() == expected.len(), "primary publication count differs")?;
        let mut publications = Vec::with_capacity(expected.len());
        let mut last_instruction = entered.counts.0.iter().sum::<u64>();
        let mut last_tick = 0;
        for (event, source) in records.into_iter().zip(&expected) {
            let pc = event.pc.ok_or("primary store lacks PC attribution")?;
            let instructions = event.instructions.ok_or("primary store lacks instruction attribution")?;
            let tick = event.tick.as_ref().ok_or("primary store lacks tick attribution")?.0;
            let preimage = event.before.as_ref().ok_or("primary store lacks raw preimage")?;
            let offset = (source.physical.0 - PRIMARY_TABLE.0) as usize;
            require(matches!(event.source, Source::CpuWrite) && event.core == Some(0) && event.physical == Some(source.physical.0)
                && event.address == ZERO_GUEST + source.physical.0 && event.size == 4 && event.value == Some(u64::from(source.value.0))
                && pc == u64::from(source.pc.0) && instructions < after.counts.0.iter().sum()
                && (if contract.owned().is_some() { instructions >= last_instruction } else { instructions > last_instruction })
                && event.byte_order == Some(ByteOrder::Little) && event.tcm.is_none() && tick >= last_tick
                && preimage.len == 4 && preimage.bytes[..4] == table_before.physical[offset..offset + 4]
                && table_after.physical[offset..offset + 4] == source.value.0.to_le_bytes(),
                "primary source publication order, value, PC, raw preimage or committed readback differs")?;
            last_instruction = instructions;
            last_tick = tick;
            publications.push(Publication { guest: Guest(event.address as u32), physical: source.physical, value: source.value,
                pc: source.pc, instructions: Instructions(instructions) });
        }
        Ok(Some(Observation { contract: contract.identity(),
            caller, before, entered, after, frame_before, frame_after, table_before, table_after, tail_before, tail_after,
            pools_before, pools_after, code_before, code_after, scratch_before, scratch_after,
            decoder: decoder_before, aliases: bindings, publications, watch: observed }))
    })();
    let health = if contract.owned().is_some() && matches!(&outcome, Ok(None)) {
        (|| {
            let state: WatchState = serde_json::from_value(watch.state()).map_err(|error| error.to_string())?;
            require(state.enabled && state.violation.is_none() && state.fault.is_none(), "primary continuous confinement differs")
        })()
    } else { Ok(()) };
    let ended = machine.session.board.memory_watch_end_scoped(&mut watch);
    match outcome { Ok(receipt) => { health?; ended.map(|()| receipt) }, Err(error) => { let _ = ended; Err(error) } }
}

pub(in crate::peer_witness) fn run(mut machine: Machine, contract: &PrimaryContract, output: &Path) -> Result<(), String> {
    let mut identities = [Identity(0); 2];
    let mut boundaries = Vec::with_capacity(4);
    let outcome = (|| {
        require(wait::counts(&machine)?.0 == [0, 0] && !machine.traces.armed() && machine.traces.hits.is_empty(),
            "primary observation must arm before either core executes")?;
        let (maps, configured) = bindings(&machine.document)?;
        identities = configured;
        require(u64::try_from(machine.document.execution.quantum) == Ok(QUANTUM) && matches!(machine.document.execution.engine, RunEngine::Unicorn)
            && machine.document.execution.instructions_per_tick.numerator == 24
            && machine.document.execution.instructions_per_tick.denominator == 1, "primary natural-reset scheduling differs")?;
        let committed = ram_decoded(&machine.document, &maps)?;
        observe(&mut machine, contract, &maps, &committed, &mut boundaries)
    })();
    let (status, primary, outcome) = match outcome {
        Ok(Some(receipt)) => (ObservationStatus::Observed, Some(receipt), Ok(())),
        Ok(None) => (ObservationStatus::Absent, None, Err("primary observation absent at finite instruction deadline".to_owned())),
        Err(error) => (ObservationStatus::Refused, None, Err(error)),
    };
    let receipt = PrimaryReceipt { schema: 8, role: contract.role(),
        contract: contract.identity(),
        result: if outcome.is_ok() { WitnessResult::Passed } else { WitnessResult::Failed }, status,
        error: outcome.as_ref().err().map(String::as_str), machine_failure: machine.failure.clone(),
        instructions: Instructions(machine.instructions()), cp14_identities: identities,
        r#final: context_for(&machine, 0)?, boundaries, primary };
    if contract.consumer.is_some() {
        return root0::run(machine, &receipt, output);
    }
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
    require(bytes.len() <= (32 << 20), "primary observation receipt exceeds 32 MiB")?;
    let path = output.join("peer-witness.json");
    std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    println!("{}", path.display());
    outcome
}
