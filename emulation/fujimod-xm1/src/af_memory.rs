use crate::{memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window}, modal_memory::FixedMemory, modal_task::{MemoryError, PhysicalExtent, ValidatedTaskMemory}, provenance::Guest};
use fujimod_emulator::{board::{Board, factory::MemoryMap}, session::Session};
use fujimod_emulator_cpu::arm;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadCore { Primary, Secondary }
impl ReadCore {
    pub fn index(self) -> usize { match self { Self::Primary => 0, Self::Secondary => 1 } }
    pub fn from_index(index: usize) -> Option<Self> { match index { 0 => Some(Self::Primary), 1 => Some(Self::Secondary), _ => None } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadBank { Btcm, Atcm }
impl ReadBank {
    pub fn index(self) -> usize { match self { Self::Btcm => 0, Self::Atcm => 1 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TcmValue(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadPurpose { SharedPhysical, ExecutingCore }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadError { Bounds, Catalogue, Register, InvalidTcm, CrossWindow, Ambiguous, Translation, SharedValidation, PhysicalMismatch, Read }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReadView { Bounds, Catalogue, TcmRegister { core: ReadCore, bank: ReadBank }, ExecutingCore { core: ReadCore }, SharedPhysical }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRefusal { pub error: ReadError, pub view: ReadView, pub observed_tcm: [[Option<TcmValue>; 2]; 2] }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReadProvenance {
    SharedPhysical { physical: Guest, observed_tcm: [[TcmValue; 2]; 2] },
    Tcm { core: ReadCore, bank: ReadBank, register: TcmValue, window_start: u64, window_end: u64, offset: u64, observed_tcm: [[TcmValue; 2]; 2] },
}
impl ReadProvenance {
    pub fn observed_tcm(self) -> [[TcmValue; 2]; 2] {
        match self { Self::SharedPhysical{observed_tcm,..}|Self::Tcm{observed_tcm,..}=>observed_tcm }
    }
}

fn windows(values: [[TcmValue; 2]; 2]) -> Result<[[Option<(u64, u64)>; 2]; 2], ReadError> {
    let mut result = [[None; 2]; 2];
    for core in 0..2 { for bank in 0..2 {
        let value = u32::try_from(values[core][bank].0).map_err(|_| ReadError::InvalidTcm)?;
        result[core][bank] = tcm_window(TcmRegister(value)).map_err(|_| ReadError::InvalidTcm)?;
    }}
    Ok(result)
}

pub fn validate_provenance(provenance: ReadProvenance, purpose: ReadPurpose, core: ReadCore, address: Guest, bytes: u32, maps: &[MemoryMap]) -> Result<(), String> {
    let start = u64::from(address.0);
    let end = start.checked_add(u64::from(bytes)).filter(|end| bytes > 0 && bytes <= 4096 && *end <= 1 << 32).ok_or("AF read bounds differ")?;
    let values = match provenance { ReadProvenance::SharedPhysical { observed_tcm, .. } | ReadProvenance::Tcm { observed_tcm, .. } => observed_tcm };
    let windows = windows(values).map_err(|_| "AF TCM registers differ")?;
    match provenance {
        ReadProvenance::Tcm { core: selected, bank, register, window_start, window_end, offset, .. } => {
            if purpose != ReadPurpose::ExecutingCore || selected != core || register != values[core.index()][bank.index()]
                || windows[core.index()][bank.index()] != Some((window_start, window_end))
                || start < window_start || end > window_end || offset != start - window_start
                || windows[core.index()].iter().flatten().filter(|&&(low, high)| low < end && start < high).count() != 1 {
                return Err("AF TCM provenance differs".into());
            }
        },
        ReadProvenance::SharedPhysical { physical, .. } => {
            let extent = PhysicalExtent::new(address, bytes).map_err(|_| "AF shared extent differs")?;
            if physical != extent.start() { return Err("AF physical provenance differs".into()); }
            for index in 0..2 { for alias in [0, 0x4000_0000] {
                let low = u64::from(physical.0 | alias);
                let high = low + u64::from(bytes);
                if windows[index].iter().flatten().any(|&(base, limit)| base < high && low < limit) { return Err("AF shared span overlaps TCM".into()); }
                let span = static_span(maps, MemoryCore(index), MemoryAddress(low)..MemoryAddress(high))?;
                if span.physical.0 != u64::from(physical.0) { return Err("AF shared translation differs".into()); }
            }}
            if purpose == ReadPurpose::ExecutingCore {
                let span = static_span(maps, MemoryCore(core.index()), MemoryAddress(start)..MemoryAddress(end))?;
                if span.physical.0 != u64::from(physical.0) { return Err("AF selected translation differs".into()); }
            }
        },
    }
    Ok(())
}

pub fn read_cpu_visible(board: &Board, maps: &[MemoryMap], core: ReadCore, address: Guest, output: &mut [u8]) -> Result<ReadProvenance, MemoryRefusal> {
    read_visible(board, maps, core, address, output, ReadPurpose::ExecutingCore)
}

fn read_visible(board: &Board, maps: &[MemoryMap], core: ReadCore, address: Guest, output: &mut [u8], purpose: ReadPurpose) -> Result<ReadProvenance, MemoryRefusal> {
    let mut observed = [[None; 2]; 2];
    let failure = |error, view, observed_tcm| MemoryRefusal { error, view, observed_tcm };
    if board.mappings() != maps || board.instruction_counts().len() != 2 { return Err(failure(ReadError::Catalogue, ReadView::Catalogue, observed)); }
    for index in 0..2 { for bank in [ReadBank::Btcm, ReadBank::Atcm] {
        let selected = ReadCore::from_index(index).unwrap();
        let view = ReadView::TcmRegister { core: selected, bank };
        let cpu = board.cpu(index).map_err(|_| failure(ReadError::Register, view, observed))?;
        let value = cpu.coprocessor(arm::Coprocessor { cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank.index() as u32, secure: 0, wide: false }, None)
            .map_err(|_| failure(ReadError::Register, view, observed))?;
        observed[index][bank.index()] = Some(TcmValue(value));
    }}
    let values = observed.map(|banks| banks.map(|value| value.unwrap()));
    let windows = windows(values).map_err(|error| failure(error, ReadView::ExecutingCore { core }, observed))?;
    let bytes = u32::try_from(output.len()).ok().filter(|bytes| (1..=4096).contains(bytes)).ok_or_else(|| failure(ReadError::Bounds, ReadView::Bounds, observed))?;
    let start = u64::from(address.0);
    let end = start + u64::from(bytes);
    if end > 1 << 32 { return Err(failure(ReadError::Bounds, ReadView::Bounds, observed)); }
    let mut selected = None;
    for bank in [ReadBank::Btcm, ReadBank::Atcm] {
        if let Some((low, high)) = windows[core.index()][bank.index()] {
            if low < end && start < high {
                if selected.is_some() { return Err(failure(ReadError::Ambiguous, ReadView::ExecutingCore { core }, observed)); }
                if start < low || end > high { return Err(failure(ReadError::CrossWindow, ReadView::ExecutingCore { core }, observed)); }
                selected = Some(ReadProvenance::Tcm { core, bank, register: values[core.index()][bank.index()], window_start: low, window_end: high, offset: start - low, observed_tcm: values });
            }
        }
    }
    let provenance = match selected {
        Some(provenance) => provenance,
        None => {
            let extent = PhysicalExtent::new(address, bytes).map_err(|_| failure(ReadError::Translation, ReadView::ExecutingCore { core }, observed))?;
            ReadProvenance::SharedPhysical { physical: extent.start(), observed_tcm: values }
        },
    };
    validate_provenance(provenance, purpose, core, address, bytes, maps)
        .map_err(|_| failure(ReadError::Translation, ReadView::ExecutingCore { core }, observed))?;
    if matches!(provenance, ReadProvenance::SharedPhysical { .. }) {
        read_shared_board(board, maps, address, output).map_err(|error| failure(if error==MemoryError::PhysicalMismatch {ReadError::PhysicalMismatch} else {ReadError::SharedValidation}, ReadView::SharedPhysical, observed))?;
    }
    board.read_memory(core.index(), start, output).map_err(|_| failure(ReadError::Read, ReadView::ExecutingCore { core }, observed))?;
    Ok(provenance)
}

fn read_shared_board(board: &Board, maps: &[MemoryMap], address: Guest, output: &mut [u8]) -> Result<(), MemoryError> {
    let extent = PhysicalExtent::new(address, output.len() as u32).map_err(|_| MemoryError::Unmapped)?;
    let physical = u64::from(extent.start().0);
    board.read_physical(physical, output).map_err(|_| MemoryError::Read)?;
    let mut buffer = [0; 256];
    for core in 0..2 { for alias in [0, 0x4000_0000] { for (index, expected) in output.chunks(256).enumerate() {
        let start = (physical | alias) + (index * 256) as u64;
        let span = static_span(maps, MemoryCore(core), MemoryAddress(start)..MemoryAddress(start + expected.len() as u64)).map_err(|_| MemoryError::Translation)?;
        if span.physical.0 != physical + (index * 256) as u64 { return Err(MemoryError::Translation); }
        let bytes = &mut buffer[..expected.len()];
        board.read_memory(core, start, bytes).map_err(|_| MemoryError::Read)?;
        if bytes != expected { return Err(MemoryError::PhysicalMismatch); }
    }}}
    Ok(())
}

pub(crate) fn read_shared(session: &Session, maps: &[MemoryMap], address: Guest, output: &mut [u8]) -> Result<ReadProvenance, MemoryRefusal> {
    let provenance = read_visible(&session.board, maps, ReadCore::Primary, address, output, ReadPurpose::SharedPhysical)?;
    let observed_tcm=provenance.observed_tcm().map(|banks|banks.map(Some));
    FixedMemory { session, maps }.read(address, output).map_err(|_| MemoryRefusal { error: ReadError::SharedValidation, view: ReadView::SharedPhysical, observed_tcm })?;
    Ok(provenance)
}
