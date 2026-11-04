//! Declared boot mutations: the boot-ROM decoder declaration's binding to the ROM and run graph, and the mutation
//! census of one role from reset to the attach-state boundary.
use crate::composed_catalog::{Bank, Core, Declaration};
use crate::run::{self, RunDocument};
use fujimod_emulator::board::observation::{GuardMutation, MutationRow, TcmBank};
use fujimod_emulator_cpu::{CpuMutation, MappingMutation, MutationOrigin};
use serde::{Deserialize, Serialize};

const ROM_BASE: u32 = 0x8000_0000;
/// `str r1,[r0,#8]` with r0 the pool word `0xfff98000` (`reports/bootrom-window.md:51`, `bootrom.dis:2224-2225`).
pub const ROM_SELECT_STORE: (u32, u32) = (0x8000_22a8, 0xe580_1008);
pub const ROM_SELECT_POOL: (u32, u32) = (0x8000_22c4, 0xfff9_8000);

fn rom_word(rom: &[u8], address: u32) -> Option<u32> {
    let offset = usize::try_from(address.checked_sub(ROM_BASE)?).ok()?;
    rom.get(offset..offset + 4).map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")))
}

/// Every decoder declaration must be the boot ROM's selection store on core 0, its ROM words must be the store and
/// pool word, and its decoder's initial choice must be the declaration's previous one.
pub fn check_rom_declarations(document: &RunDocument, rom: &[u8], declared: &[Declaration]) -> Result<(), String> {
    let (_, graph) = run::graph(document)?;
    for declaration in declared {
        let &Declaration::DecoderCommit { decoder, previous, next, core, writer, .. } = declaration else {
            continue;
        };
        if writer != ROM_SELECT_STORE.0 || core != Core::Primary {
            return Err(format!("decoder declaration writer {writer:#x} is not the boot ROM selection store on core 0"));
        }
        for (address, word) in [ROM_SELECT_STORE, ROM_SELECT_POOL] {
            if rom_word(rom, address) != Some(word) {
                return Err(format!("boot ROM word at {address:#x} is not {word:#010x}"));
            }
        }
        let slot = graph.decoders.get(usize::from(decoder)).ok_or("declared decoder is absent from the run graph")?;
        if slot.initial != usize::from(previous) || usize::from(next) >= slot.choices.len() {
            return Err("declared decoder choices do not match the run graph".into());
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    Guest,
    Host,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MappingKind {
    MapBacking,
    MapPhysical,
    ReplacePhysical,
    MapZeroed,
    MapMmio,
    Unmap,
    Protect,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CensusMutation {
    /// `bank` is absent for a coprocessor register other than c9,c1,{0,1}.
    TcmRegion { bank: Option<Bank>, previous: u32, next: u32, origin: Origin },
    DecoderCommit { decoder: usize, previous: usize, next: usize },
    Mapping { operation: MappingKind, address: u64, physical: Option<u64>, bytes: usize, permissions: Option<u32> },
    HostWrite { address: u64, bytes: usize },
    Restore,
    ExecutionPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CensusRow {
    /// The offering core; for a decoder commit, the selecting core.
    pub core: Option<Core>,
    pub mutation: CensusMutation,
    pub pc: Option<u64>,
    pub cpsr: Option<u32>,
    /// PC minus 4 for an A32 guest TCM write; the selecting store for a decoder commit.
    pub writer: Option<u64>,
    pub instructions: Option<u64>,
}

impl CensusRow {
    pub fn of(row: &MutationRow) -> Result<Self, String> {
        let mutation = match row.mutation {
            GuardMutation::Cpu(CpuMutation::Tcm { register, previous, next, origin }) => CensusMutation::TcmRegion {
                bank: [(TcmBank::Data, Bank::Data), (TcmBank::Instruction, Bank::Instruction)].into_iter()
                    .find(|(bank, _)| bank.register() == register).map(|(_, bank)| bank),
                previous,
                next,
                origin: match origin { MutationOrigin::Guest => Origin::Guest, MutationOrigin::Host => Origin::Host },
            },
            GuardMutation::Cpu(CpuMutation::Mapping { operation, address, physical, bytes, permissions }) => CensusMutation::Mapping {
                operation: match operation {
                    MappingMutation::MapBacking => MappingKind::MapBacking,
                    MappingMutation::MapPhysical => MappingKind::MapPhysical,
                    MappingMutation::ReplacePhysical => MappingKind::ReplacePhysical,
                    MappingMutation::MapZeroed => MappingKind::MapZeroed,
                    MappingMutation::MapMmio => MappingKind::MapMmio,
                    MappingMutation::Unmap => MappingKind::Unmap,
                    MappingMutation::Protect => MappingKind::Protect,
                },
                address,
                physical,
                bytes,
                permissions,
            },
            GuardMutation::Cpu(CpuMutation::HostWrite { address, bytes }) => CensusMutation::HostWrite { address, bytes },
            GuardMutation::Cpu(CpuMutation::Restore) | GuardMutation::Restore => CensusMutation::Restore,
            GuardMutation::Cpu(CpuMutation::ExecutionPolicy { .. }) => CensusMutation::ExecutionPolicy,
            GuardMutation::Decoder { decoder, previous, choice, .. } => CensusMutation::DecoderCommit { decoder, previous, next: choice },
        };
        Ok(Self {
            core: row.core.map(Core::from_index).transpose()?,
            mutation,
            pc: row.pc,
            cpsr: row.cpsr,
            writer: row.writer,
            instructions: row.instructions,
        })
    }
}

/// A processor map of a decoder choice in the run graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChoiceMap {
    pub core: Core,
    pub address: u64,
    pub physical: u64,
    pub bytes: usize,
    pub permissions: u32,
}

/// Per decoder, per choice: the processor maps of the run graph.
pub fn choice_maps(document: &RunDocument) -> Result<Vec<Vec<Vec<ChoiceMap>>>, String> {
    let (_, graph) = run::graph(document)?;
    graph.decoders.iter().map(|decoder| decoder.choices.iter().map(|choice| choice.processors.iter().map(|map| Ok(ChoiceMap {
        core: Core::from_index(map.core)?,
        address: map.address,
        physical: map.physical,
        bytes: map.size,
        permissions: map.permissions,
    })).collect()).collect()).collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stream {
    Tcm(Option<Core>, Option<Bank>),
    Decoders,
    Other,
}

fn stream(row: &CensusRow) -> Stream {
    match row.mutation {
        CensusMutation::TcmRegion { bank, origin: Origin::Guest, .. } => Stream::Tcm(row.core, bank),
        CensusMutation::DecoderCommit { .. } | CensusMutation::Mapping { operation: MappingKind::ReplacePhysical, .. } => Stream::Decoders,
        _ => Stream::Other,
    }
}

/// The rows the declarations predict: each TCM write with its writer, and each decoder commit with its writer
/// followed by one `ReplacePhysical` row per processor map of its choice.
fn expected(declared: &[Declaration], maps: &[Vec<Vec<ChoiceMap>>]) -> Result<Vec<CensusRow>, String> {
    let mut rows = Vec::new();
    let row = |core, mutation, writer: Option<u32>| CensusRow { core, mutation, pc: None, cpsr: None, writer: writer.map(u64::from), instructions: None };
    for declaration in declared {
        match *declaration {
            Declaration::TcmRegion { core, bank, previous, next, writer, .. } => rows.push(row(
                Some(core),
                CensusMutation::TcmRegion { bank: Some(bank), previous, next, origin: Origin::Guest },
                Some(writer),
            )),
            Declaration::DecoderCommit { decoder, previous, next, core, writer, .. } => {
                let (decoder, previous, next) = (usize::from(decoder), usize::from(previous), usize::from(next));
                rows.push(row(Some(core), CensusMutation::DecoderCommit { decoder, previous, next }, Some(writer)));
                let choice = maps.get(decoder).and_then(|choices| choices.get(next)).ok_or("declared decoder choice is absent")?;
                rows.extend(choice.iter().map(|map| row(Some(map.core), CensusMutation::Mapping {
                    operation: MappingKind::ReplacePhysical,
                    address: map.address,
                    physical: Some(map.physical),
                    bytes: map.bytes,
                    permissions: Some(map.permissions),
                }, None)));
            }
        }
    }
    Ok(rows)
}

/// The census passes when every row is predicted by the declarations, in order within each TCM register and within
/// the decoder commits, and no other row occurs. Rows compare on core, mutation and writer.
pub fn census_difference(declared: &[Declaration], maps: &[Vec<Vec<ChoiceMap>>], rows: &[CensusRow]) -> Result<Option<String>, String> {
    let expected = expected(declared, maps)?;
    let key = |row: &CensusRow| (row.core, row.mutation, row.writer);
    if let Some(row) = rows.iter().find(|row| stream(row) == Stream::Other) {
        return Ok(Some(format!("undeclared mutation {row:?}")));
    }
    let mut streams: Vec<Stream> = Vec::new();
    for selected in expected.iter().chain(rows).map(stream) {
        if !streams.contains(&selected) {
            streams.push(selected);
        }
    }
    for selected in streams {
        let want: Vec<_> = expected.iter().filter(|row| stream(row) == selected).map(key).collect();
        let got: Vec<_> = rows.iter().filter(|row| stream(row) == selected).map(key).collect();
        if want != got {
            return Ok(Some(format!("rows of {selected:?} are {got:?}, declared {want:?}")));
        }
    }
    Ok(None)
}
