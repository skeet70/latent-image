use crate::{
    memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window},
    modal_lifetime::{LifetimeFailure, LifetimeMemory},
    modal_task::{MemoryError, PhysicalExtent, StackExtent, ValidatedTaskMemory},
    provenance::Guest,
};
use fujimod_emulator::{board::factory::MemoryMap, session::Session};
use fujimod_emulator_cpu::arm;

pub(crate) struct FixedMemory<'a> {
    pub session: &'a Session,
    pub maps: &'a [MemoryMap],
}

impl FixedMemory<'_> {
    fn validate(&self, extent: PhysicalExtent) -> Result<(), MemoryError> {
        if self.session.board.mappings() != self.maps
            || self.session.board.instruction_counts().len() != 2 {
            return Err(MemoryError::Translation);
        }
        for core in 0..2 {
            let cpu = self.session.board.cpu(core).map_err(|_| MemoryError::Read)?;
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
        self.session.board.read_physical(u64::from(extent.start().0), output)
            .map_err(|_| MemoryError::Read)?;
        let mut observed = [0; 256];
        for core in 0..2 {
            for alias in [0, 0x4000_0000] {
                for (index, expected) in output.chunks(observed.len()).enumerate() {
                    let start = u64::from(extent.start().0 | alias) + (index * observed.len()) as u64;
                    let bytes = &mut observed[..expected.len()];
                    self.session.board.read_memory(core, start, bytes).map_err(|_| MemoryError::Read)?;
                    if bytes != expected { return Err(MemoryError::PhysicalMismatch); }
                }
            }
        }
        Ok(())
    }
}

impl ValidatedTaskMemory for FixedMemory<'_> {
    fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), MemoryError> {
        self.read_fixed(address, output)
    }

    fn validate_stack(&self, extent: StackExtent) -> Result<(), MemoryError> {
        self.validate(extent.physical())
    }
}

impl LifetimeMemory for FixedMemory<'_> {
    fn read(&self, physical: Guest, output: &mut [u8]) -> Result<(), LifetimeFailure> {
        self.read_fixed(physical, output).map_err(|_| LifetimeFailure::Memory)
    }
}
