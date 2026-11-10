use crate::{
    memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window},
    modal_lifetime::{LifetimeFailure, LifetimeMemory},
    modal_task::{MemoryError, PhysicalExtent, StackExtent, ValidatedTaskMemory},
    provenance::Guest,
};
use fujimod_emulator::board::{factory::MemoryMap, observation::ObservationContext};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{Cpu, Identity, arm};

pub(crate) struct FixedMemory<'a> {
    pub session: &'a Session,
    pub maps: &'a [MemoryMap],
}

// Board processor topology is immutable.
pub(crate) struct TwoCoreTopology([Identity; 2]);
impl TwoCoreTopology {
    pub(crate) fn new(context: &ObservationContext) -> Result<Self, MemoryError> {
        if context.core_count() != 2 { return Err(MemoryError::Translation); }
        Ok(Self([
            context.cpu(0).map_err(|_| MemoryError::Translation)?.identity(),
            context.cpu(1).map_err(|_| MemoryError::Translation)?.identity(),
        ]))
    }
    pub(crate) fn validate(&self, session: &Session) -> Result<(), MemoryError> {
        self.validate_access(MemoryAccess::Session(session))
    }
    fn validate_access(&self, access: MemoryAccess<'_>) -> Result<(), MemoryError> {
        for (core, expected) in self.0.iter().enumerate() {
            if access.cpu(core)?.identity() != *expected { return Err(MemoryError::Translation); }
        }
        Ok(())
    }
}

pub(crate) struct SampleMemory<'a> {
    pub access: &'a ObservationContext,
    pub topology: &'a TwoCoreTopology,
}

#[derive(Clone, Copy)]
enum MemoryAccess<'a> { Session(&'a Session), Observer(&'a ObservationContext) }
impl<'a> MemoryAccess<'a> {
    fn cpu(self, core: usize) -> Result<&'a dyn Cpu, MemoryError> {
        match self {
            Self::Session(session) => session.board.cpu(core),
            Self::Observer(context) => context.cpu(core),
        }.map_err(|_| MemoryError::Read)
    }
    fn mappings(self) -> &'a [MemoryMap] {
        match self { Self::Session(session) => session.board.mappings(), Self::Observer(context) => context.mappings() }
    }
    fn core_count(self) -> usize {
        match self { Self::Session(session) => session.board.instruction_counts().len(), Self::Observer(context) => context.core_count() }
    }
    fn read_physical(self, address: u64, output: &mut [u8]) -> Result<(), MemoryError> {
        match self {
            Self::Session(session) => session.board.read_physical(address, output),
            Self::Observer(context) => context.read_physical(address, output),
        }.map_err(|_| MemoryError::Read)
    }
    fn read_memory(self, core: usize, address: u64, output: &mut [u8]) -> Result<(), MemoryError> {
        match self {
            Self::Session(session) => session.board.read_memory(core, address, output),
            Self::Observer(context) => context.read_memory(core, address, output),
        }.map_err(|_| MemoryError::Read)
    }
}
struct CheckedMemory<'a> { access: MemoryAccess<'a>, maps: &'a [MemoryMap], topology: Option<&'a TwoCoreTopology> }
impl<'a> FixedMemory<'a> {
    fn checked(&self) -> CheckedMemory<'a> {
        CheckedMemory { access: MemoryAccess::Session(self.session), maps: self.maps, topology: None }
    }
}
impl<'a> SampleMemory<'a> {
    fn checked(&self) -> CheckedMemory<'a> {
        CheckedMemory { access: MemoryAccess::Observer(self.access), maps: self.access.mappings(), topology: Some(self.topology) }
    }
}

impl CheckedMemory<'_> {
    fn validate(&self, extent: PhysicalExtent) -> Result<(), MemoryError> {
        if self.access.mappings() != self.maps { return Err(MemoryError::Translation); }
        match self.topology {
            Some(topology) => topology.validate_access(self.access)?,
            None if self.access.core_count() != 2 => return Err(MemoryError::Translation),
            None => {},
        }
        for core in 0..2 {
            let cpu = self.access.cpu(core)?;
            let mut tcm = [None; 2];
            for (bank, window) in tcm.iter_mut().enumerate() {
                let value = cpu.coprocessor(arm::Coprocessor {
                    cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank as u32,
                    secure: 0, wide: false,
                }, None).map_err(|_| MemoryError::Read)?;
                let value = u32::try_from(value).map_err(|_| MemoryError::ActiveTcm)?;
                *window = tcm_window(TcmRegister(value)).map_err(|_| MemoryError::ActiveTcm)?;
            }
            for alias in [0, 0x4000_0000] {
                let start = u64::from(extent.start().0 | alias);
                let end = start + u64::from(extent.bytes());
                if tcm.iter().flatten().any(|&(low, high)| low < end && start < high) {
                    return Err(MemoryError::ActiveTcm);
                }
                let span = static_span(self.maps, MemoryCore(core), MemoryAddress(start)..MemoryAddress(end))
                    .map_err(|_| MemoryError::Translation)?;
                if span.physical.0 != u64::from(extent.start().0) {
                    return Err(MemoryError::Translation);
                }
            }
        }
        Ok(())
    }

    fn read_fixed(&self, address: Guest, output: &mut [u8]) -> Result<(), MemoryError> {
        let bytes = u32::try_from(output.len()).map_err(|_| MemoryError::Unmapped)?;
        let extent = PhysicalExtent::new(address, bytes).map_err(|_| MemoryError::Unmapped)?;
        self.validate(extent)?;
        self.access.read_physical(u64::from(extent.start().0), output)?;
        let mut observed = [0; 256];
        for core in 0..2 {
            for alias in [0, 0x4000_0000] {
                for (index, expected) in output.chunks(observed.len()).enumerate() {
                    let start = u64::from(extent.start().0 | alias) + (index * observed.len()) as u64;
                    let bytes = &mut observed[..expected.len()];
                    self.access.read_memory(core, start, bytes)?;
                    if bytes != expected { return Err(MemoryError::PhysicalMismatch); }
                }
            }
        }
        Ok(())
    }
}

impl ValidatedTaskMemory for FixedMemory<'_> {
    fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), MemoryError> {
        self.checked().read_fixed(address, output)
    }

    fn validate_stack(&self, extent: StackExtent) -> Result<(), MemoryError> {
        self.checked().validate(extent.physical())
    }
}

impl LifetimeMemory for FixedMemory<'_> {
    fn read(&self, physical: Guest, output: &mut [u8]) -> Result<(), LifetimeFailure> {
        self.checked().read_fixed(physical, output).map_err(|_| LifetimeFailure::Memory)
    }
}

impl ValidatedTaskMemory for SampleMemory<'_> {
    fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), MemoryError> {
        self.checked().read_fixed(address, output)
    }
    fn validate_stack(&self, extent: StackExtent) -> Result<(), MemoryError> {
        self.checked().validate(extent.physical())
    }
}
