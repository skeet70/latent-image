//! The distinct core-1 interface boundary ending before 0xd0ab0.
use super::*;

pub(super) const BODY: (u64, u64) = (0xd0430, 0xd09ac);
const TABLE: (u64, u64) = (0xddaa0, 0xdf0cc);
const TABLE_GUEST: u64 = 0x400d_daa0;
const END: u32 = 0xd0ab0;
const SOURCE: &str = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490";
const BODY_SHA: &str = "60aeb6c0c68717940fef3771f3387540a2c87276d7c947a9f3d276dd51fe850d";
const POOLS: (u64, u64) = (0xebe6c, 0xf190c);

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Role { Retained, Owned }

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "u32")]
struct ScratchBytes(u32);
impl TryFrom<u32> for ScratchBytes {
    type Error = String;
    fn try_from(bytes: u32) -> Result<Self, String> {
        require((8..=1032).contains(&bytes) && bytes % 4 == 0, "initializer scratch extent is not an admitted word-aligned frame")?;
        Ok(Self(bytes))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Contract {
    role: Role,
    entry: Guest,
    rust: (Guest, Guest),
    scratch_bytes: ScratchBytes,
    image_sha256: Sha256Digest,
    symbols_sha256: Option<Sha256Digest>,
    source_sha256: Sha256Digest,
}

impl Contract {
    pub(super) fn owns_body(&self) -> bool { self.role == Role::Owned }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::peer_witness) struct InitializerWire {
    schema: u32,
    prefix: LoaderWire,
    initializer: Contract,
}

impl InitializerWire {
    pub(in crate::peer_witness) fn validate(self) -> Result<LoaderContract, String> {
        require(self.schema == 6, "interface initializer requires schema 6")?;
        let mut prefix = self.prefix.validate()?;
        let init = self.initializer;
        require(prefix.scenario == Scenario::Natural && init.source_sha256 == Sha256Digest::parse(SOURCE)?,
            "initializer requires the natural original-DAT source role")?;
        match init.role {
            Role::Retained => require(prefix.kind == ProgramKind::Retained && init.entry == Guest(BODY.0 as u32)
                && init.rust == (Guest(BODY.0 as u32), Guest(BODY.1 as u32)) && init.scratch_bytes == ScratchBytes(12)
                && init.image_sha256 == Sha256Digest::parse(BODY_SHA)? && init.symbols_sha256.is_none(),
                "retained initializer boundary differs")?,
            Role::Owned => {
                let (start, end) = prefix.image.ok_or("initializer owned image absent")?;
                require(prefix.kind == ProgramKind::Owned && (start.0..end.0).contains(&init.entry.0)
                    && init.entry.0 % 4 == 0 && init.rust.0 .0 >= start.0 && init.rust.1 .0 <= end.0
                    && init.rust.0 .0 < init.rust.1 .0 && init.rust.0 .0 % 4 == 0 && init.rust.1 .0 % 4 == 0
                    && init.symbols_sha256.is_some(),
                    "owned initializer sites or compiler-derived scratch extent differ")?;
            }
        }
        prefix.initializer = Some(init);
        Ok(prefix)
    }
}

#[derive(Serialize)]
struct Context {
    pc: Word, registers: [Word; 13], sp: Word, lr: Word, cpsr: Word, counts: CoreCounts, tcm: TcmWords,
}

fn context(machine: &Machine) -> Result<Context, String> {
    let mut registers = [Word(0); 13];
    for (index, value) in registers.iter_mut().enumerate() {
        *value = register(machine, 1, arm::Register::R(index as u8))?;
    }
    Ok(Context { pc: register(machine, 1, arm::Register::R(15))?, registers,
        sp: register(machine, 1, arm::Register::R(13))?, lr: register(machine, 1, arm::Register::R(14))?,
        cpsr: register(machine, 1, arm::Register::Cpsr)?, counts: wait::counts(machine)?, tcm: tcm_words(machine)? })
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Source { CpuWrite, CpuRead, CpuFetch, HostWrite, HostRead, ModelBusWrite, ModelBusRead }

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ByteOrder { Little, Big }

#[derive(Clone, Copy)]
enum Boundary { Entry, Exit }
impl Boundary {
    fn name(self) -> &'static str {
        match self { Self::Entry => "initializer-entry", Self::Exit => "initializer-exit" }
    }
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "initializer-entry" => Ok(Self::Entry),
            "initializer-exit" => Ok(Self::Exit),
            _ => Err("initializer trace contains an unexpected label".into()),
        }
    }
}
#[derive(Deserialize)]
struct Event {
    source: Source, core: Option<usize>, address: u64, physical: Option<u64>, size: u64,
    value: Option<u64>, pc: Option<u64>, instructions: Option<u64>, byte_order: Option<ByteOrder>,
}
#[derive(Deserialize)]
struct WatchState { violation: Option<String>, records: Vec<Event> }

#[derive(Serialize)]
struct Publication { guest: Guest, physical: Address, value: Word, pc: Guest, instructions: Instructions }
#[derive(Serialize)]
struct AliasBinding { core: usize, guest: Address, physical: Address, size: u64, sha256: Sha256Digest }
#[derive(Serialize)]
struct Decoder { choices: Vec<usize>, pending: Vec<Option<usize>> }

#[derive(Serialize)]
pub(super) struct Receipt {
    contract: ContractIdentity,
    before: Context, entered: Context, after: Context,
    frame_before: Views, frame_after: Views,
    table_before: Views, table_after: Views,
    tail_before: Views, tail_after: Views,
    pools_before: Views, pools_after: Views,
    decoder: Decoder, aliases: Vec<AliasBinding>,
    publications: Vec<Publication>,
    watch: Value,
}

#[derive(Serialize)]
struct ContractIdentity {
    role: Role, entry: Guest, rust: (Guest, Guest), scratch_bytes: ScratchBytes,
    source_sha256: Sha256Digest, image_sha256: Sha256Digest, symbols_sha256: Option<Sha256Digest>,
    body_sha256: Sha256Digest,
}

fn decoder(machine: &Machine) -> Result<Decoder, String> {
    let observed: BoardState = serde_json::from_value(machine.session.board.state()).map_err(|error| error.to_string())?;
    let decoder = observed.memory_decoders;
    require(decoder.choices == [1] && decoder.pending == [None] && decoder.failure.is_none(),
        "initializer RAM decoder is not committed")?;
    Ok(Decoder { choices: decoder.choices, pending: decoder.pending })
}

fn equal_views(views: &Views) -> Result<(), String> {
    require(views.r#virtual == views.physical, "initializer virtual and physical committed bytes differ")
}

fn aliases(machine: &Machine, runner: &Runner<'_>, tcm: &TcmWords) -> Result<Vec<AliasBinding>, String> {
    let mut guarded = runner.guarded();
    guarded.push(ZERO);
    tcm_disjoint_views(&runner.committed, tcm, &guarded)?;
    let mut result = Vec::new();
    for core in 0..2 {
        require(machine.session.board.cpu(core)?.rprofile_identity(None)? == [0x4003, 0x5003][core],
            "initializer CP14 identity changed")?;
        for (start, end) in [RECORD, LOADER_B, ZERO] {
            let physical = machine.read(start, (end - start) as usize)?;
            for guest in [start, ZERO_GUEST + start] {
                require(runner.physical_of(core, guest, end - start)? == start, "initializer resource alias mapping differs")?;
                require(wait::bytes(machine, core, guest, physical.len())? == physical, "initializer resource alias bytes differ")?;
                result.push(AliasBinding { core, guest: Address(guest), physical: Address(start), size: end - start,
                    sha256: Sha256Digest::of(&physical) });
            }
        }
    }
    Ok(result)
}

fn expected(machine: &Machine) -> Result<Vec<(u64, u32)>, String> {
    let body = machine.read(BODY.0, (BODY.1 - BODY.0) as usize)?;
    require(Sha256Digest::of(&body) == Sha256Digest::parse(BODY_SHA)?, "initializer original leaf identity differs")?;
    let word = |offset| u32::from_le_bytes([body[offset], body[offset + 1], body[offset + 2], body[offset + 3]]);
    let immediate = |word: u32| ((word >> 4) & 0xf000) | (word & 0xfff);
    let mut expected = Vec::with_capacity(1419);
    for slot in 0..94 {
        let offset = 0x10 + slot * 12;
        let (low, high, store) = (word(offset), word(offset + 4), word(offset + 8));
        require(low & 0xfff0_f000 == 0xe300_2000 && high & 0xfff0_f000 == 0xe340_2000
            && store == 0xe583_2000 + slot as u32 * 4, "initializer service publication encoding differs")?;
        expected.push((TABLE.0 + slot as u64 * 4, immediate(low) | (immediate(high) << 16)));
    }
    for (index, target) in [0xd12bc, 0xd12cc, 0xd12e8, 0xd1304, 0x400f3420].into_iter().enumerate() {
        expected.push((TABLE.0 + 0x1618 + index as u64 * 4, target));
    }
    for index in 0..120_u32 {
        let record = TABLE.0 + 0x178 + u64::from(index) * 44;
        expected.push((record, index + 1));
        for field in 0..8_u32 {
            expected.push((record + 4 + u64::from(field) * 4, 0x400e_be6c + index * 40 + field * 4));
        }
        expected.push((record + 36, 0x400e_fecc + index * 56 + 24));
        expected.push((record + 40, 0x400e_fecc + index * 56 + 28));
    }
    Ok(expected)
}

pub(super) fn observe(machine: &mut Machine, runner: &Runner<'_>, contract: &Contract) -> Result<Receipt, String> {
    let before = context(machine)?;
    require(before.pc.0 == EXIT && before.sp.0 == FRAME as u32
        && before.registers[11].0 == 0x400f341c && before.registers[4].0 == ARGUMENT
        && before.cpsr.0 & 0x0100_02ff == 0xd3, "initializer did not inherit the admitted core-1 caller frame")?;
    let original_prefix = runner.stops.iter().find(|stop| stop.label == Label::PrefixEntry).ok_or("initializer prefix entry absent")?;
    let frame_before = runner.views(machine, 1, FRAME, 16)?;
    equal_views(&frame_before)?;
    let frame_words: Vec<u32> = frame_before.physical.chunks_exact(4).map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).collect();
    require(frame_words == [0x400f3420, ARGUMENT, original_prefix.registers[11].0, 0xd00c0], "initializer caller frame differs")?;
    let decoder_before = decoder(machine)?;
    let mut bindings = aliases(machine, runner, &before.tcm)?;
    let expected = expected(machine)?;
    let loader = machine.read(LOADER_B.0, (LOADER_B.1 - LOADER_B.0) as usize)?;
    let call = u32::from_le_bytes([loader[0xaac], loader[0xaad], loader[0xaae], loader[0xaaf]]);
    let displacement = (contract.entry.0 as i64) - (EXIT as i64 + 8);
    require(call == (0xeb00_0000 | (((displacement >> 2) as u32) & 0x00ff_ffff)), "initializer BL does not reach the selected entry")?;
    let (rust_start, rust_end) = (contract.rust.0 .0, contract.rust.1 .0);
    if contract.role == Role::Owned {
        let (start, end) = runner.contract.image.ok_or("initializer image absent")?;
        require(Sha256Digest::of(&machine.read(u64::from(start.0), (end.0 - start.0) as usize)?) == contract.image_sha256,
            "initializer source-bound image identity differs")?;
    }
    let table_before = runner.views(machine, 1, TABLE_GUEST, (TABLE.1 - TABLE.0) as usize)?;
    let tail_before = runner.views(machine, 1, TABLE_GUEST + TABLE.1 - TABLE.0, 16)?;
    let pools_before = runner.views(machine, 1, ZERO_GUEST + POOLS.0, (POOLS.1 - POOLS.0) as usize)?;
    for views in [&table_before, &tail_before, &pools_before] { equal_views(views)?; }
    let scratch = (FRAME - u64::from(contract.scratch_bytes.0), FRAME);
    let physical = |span: (u64, u64)| PhysicalRange::new(span.0, span.1).map(ScopeSpan::Physical);
    let mut confinement = vec![
        ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Write,
            rule: Confinement::Allow(vec![physical(TABLE)?, physical((scratch.0 - ZERO_GUEST, scratch.1 - ZERO_GUEST))?]) },
        ConfinementRule { cores: CoreSet::new(&[0])?, access: Access::Write,
            rule: Confinement::Forbid(vec![physical(ZERO)?, physical(RECORD)?, physical(LOADER_B)?]) },
    ];
    let mut forbidden_reads = vec![physical(TABLE)?];
    if contract.role == Role::Owned {
        confinement.push(ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Fetch,
            rule: Confinement::Forbid(std::iter::once(BODY).chain(REPLACED).map(physical).collect::<Result<_, _>>()?) });
        forbidden_reads.extend([physical(BODY)?, physical(ENTRY_POOL)?]);
    }
    confinement.push(ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Read, rule: Confinement::Forbid(forbidden_reads) });
    let mut watch = machine.session.board.memory_watch_begin_scoped(ScopedWatch {
        classes: vec![WatchRange { start: TABLE.0, end: TABLE.1, kinds: WatchKinds::WRITES },
            WatchRange { start: ZERO.0, end: ZERO.1, kinds: WatchKinds::HOST_WRITE.union(WatchKinds::MODEL_BUS_WRITE) },
            WatchRange { start: RECORD.0, end: RECORD.1, kinds: WatchKinds::WRITES },
            WatchRange { start: LOADER_B.0, end: LOADER_B.1, kinds: WatchKinds::WRITES }],
        limit: WATCH_LIMIT, confinement, counters: Vec::new(), coverage: Vec::new(),
    }, runner.maps.clone())?;
    let outcome = (|| {
        machine.traces.arm(&machine.session, BTreeMap::from([
            (contract.entry.0, Boundary::Entry.name().into()), (END, Boundary::Exit.name().into()),
        ]))?;
        machine.traces.stop_labels = [Boundary::Entry.name().into(), Boundary::Exit.name().into()].into_iter().collect();
        let mut entered = None;
        let after = loop {
            require(machine.instructions() < BUDGET, "initializer instruction budget exhausted")?;
            let previous = machine.traces.hits.len();
            machine.run(BUDGET - machine.instructions());
            machine.require_running()?;
            let hits = machine.traces.hits[previous..].to_vec();
            require(!hits.is_empty(), "initializer boundary not reached")?;
            let mut exit = None;
            for hit in hits {
                if hit.core != 1 { continue; }
                match Boundary::parse(&hit.site)? {
                    Boundary::Entry => {
                        require(entered.is_none() && hit_address(&hit)? == contract.entry.0, "initializer entry repeated or misplaced")?;
                        let entry = context(machine)?;
                        require(entry.pc.0 == contract.entry.0 && entry.counts.0[1] == before.counts.0[1] + 1 && entry.counts.0[0] >= before.counts.0[0]
                            && entry.registers == before.registers && entry.sp == before.sp && entry.lr.0 == END
                            && entry.cpsr == before.cpsr && entry.tcm == before.tcm,
                            "initializer entry is not the unchanged context after its core-1 BL")?;
                        entered = Some(entry);
                        machine.traces.remove_site(&machine.session, contract.entry)?;
                    }
                    Boundary::Exit => {
                        require(hit_address(&hit)? == END && entered.is_some(), "initializer return precedes entry")?;
                        exit = Some(context(machine)?);
                    }
                }
            }
            if let Some(after) = exit { break after; }
        };
        let entered = entered.ok_or("initializer entry absent")?;
        require(after.pc.0 == END && after.sp == before.sp && after.lr.0 == END && after.registers[4..12] == before.registers[4..12]
            && after.registers[..4] == [Word(0x400ebe6c), Word(0x400ed120), Word(0x400f18f0), Word(0x400def40)]
            && after.registers[12].0 == 120 && after.cpsr.0 == ((before.cpsr.0 & 0x0fff_ffff) | 0x6000_0000)
            && after.tcm == before.tcm, "initializer exact return frame, scratch outputs or CPSR differ")?;
        decoder(machine)?;
        bindings.extend(aliases(machine, runner, &after.tcm)?);
        let frame_after = runner.views(machine, 1, FRAME, 16)?;
        let table_after = runner.views(machine, 1, TABLE_GUEST, (TABLE.1 - TABLE.0) as usize)?;
        let tail_after = runner.views(machine, 1, TABLE_GUEST + TABLE.1 - TABLE.0, 16)?;
        let pools_after = runner.views(machine, 1, ZERO_GUEST + POOLS.0, (POOLS.1 - POOLS.0) as usize)?;
        for views in [&frame_after, &table_after, &tail_after, &pools_after] { equal_views(views)?; }
        require(frame_after.physical == frame_before.physical && tail_after.physical == tail_before.physical
            && pools_after.physical == pools_before.physical, "initializer changed its caller frame, tail guard or retained pools")?;
        let state = watch.state();
        let observed: WatchState = serde_json::from_value(state.clone()).map_err(|error| error.to_string())?;
        require(observed.violation.is_none() && observed.records.len() == expected.len(), "initializer store count or confinement differs")?;
        let mut publications = Vec::with_capacity(expected.len());
        let mut last_instruction: u64 = entered.counts.0.iter().sum();
        for (event, &(physical, value)) in observed.records.iter().zip(&expected) {
            let pc = event.pc.ok_or("initializer store lacks PC attribution")?;
            let instructions = event.instructions.ok_or("initializer store lacks instruction attribution")?;
            require(matches!(event.source, Source::CpuWrite) && event.core == Some(1) && event.physical == Some(physical)
                && event.address == ZERO_GUEST + physical && event.size == 4 && event.value == Some(u64::from(value))
                && event.byte_order == Some(ByteOrder::Little) && instructions >= last_instruction
                && instructions < after.counts.0.iter().sum() && pc >= u64::from(rust_start) && pc < u64::from(rust_end),
                "initializer publication order, value, origin or attribution differs")?;
            let offset = (physical - TABLE.0) as usize;
            require(table_after.physical[offset..offset + 4] == value.to_le_bytes(), "initializer committed word readback differs")?;
            last_instruction = instructions;
            publications.push(Publication { guest: Guest(event.address as u32), physical: Address(physical), value: Word(value),
                pc: Guest(pc as u32), instructions: Instructions(instructions) });
        }
        require(record_immutable_sha256(machine.read(RECORD.0, RECORD_BYTES)?.as_slice().try_into().map_err(|_| "initializer record extent")?)
            == runner.contract.record_immutable && Sha256Digest::of(&machine.read(LOADER_B.0, (LOADER_B.1 - LOADER_B.0) as usize)?) == runner.contract.loader,
            "initializer changed source-bound record or loader identity")?;
        Ok(Receipt { contract: ContractIdentity { role: contract.role, entry: contract.entry, rust: contract.rust, scratch_bytes: contract.scratch_bytes,
            source_sha256: contract.source_sha256, image_sha256: contract.image_sha256, symbols_sha256: contract.symbols_sha256,
            body_sha256: Sha256Digest::parse(BODY_SHA)? }, before, entered, after, frame_before, frame_after, table_before, table_after,
            tail_before, tail_after, pools_before, pools_after, decoder: decoder_before, aliases: bindings, publications, watch: state })
    })();
    let end = machine.session.board.memory_watch_end_scoped(&mut watch);
    match outcome { Ok(receipt) => end.map(|()| receipt), Err(error) => { let _ = end; Err(error) } }
}
