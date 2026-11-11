use crate::storage_shutdown_actor::{LensProfileMedium, MediumStatus, NandBlock};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NandTransferMode { Read, Write }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NandWaitMode { Poll }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NandHandleSlot(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NandWait {
    pub mode: u32,
    pub timeout: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NandErase {
    pub first: NandBlock,
    pub count: u32,
    pub wait_mode: NandWaitMode,
    pub timeout: u32,
    pub slot: NandHandleSlot,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NandByteTransfer {
    pub first: NandBlock,
    pub offset: u32,
    pub length: u32,
    pub buffer: BufferAddress,
    pub mode: NandTransferMode,
    pub wait_mode: NandWaitMode,
    pub timeout: u32,
    pub slot: NandHandleSlot,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NandBlockTransfer {
    pub first: NandBlock,
    pub count: u32,
    pub buffer: BufferAddress,
    pub mode: NandTransferMode,
    pub wait_mode: NandWaitMode,
    pub timeout: u32,
    pub slot: NandHandleSlot,
}

pub trait NandSoftware {
    fn initialized(&self) -> bool;
    fn configure_wait(&mut self, request: NandWait) -> MediumStatus;
    fn shutdown_if_initialized(&mut self);
    fn erase_mapped_blocks(&mut self, request: &mut NandErase) -> MediumStatus;
    fn write_mapped_blocks(&mut self, request: &mut NandBlockTransfer, bytes: &[u8; 0x40000]) -> MediumStatus;
    fn read_mapped_blocks(&mut self, request: &mut NandBlockTransfer, bytes: &mut [u8; 0x40000]) -> MediumStatus;
    fn write_mapped_bytes(&mut self, request: &mut NandByteTransfer, bytes: &[u8; 0xa08]) -> MediumStatus;
    fn read_mapped_bytes(&mut self, request: &mut NandByteTransfer, bytes: &mut [u8; 0xa08]) -> MediumStatus;
    fn clean_cache(&mut self, address: BufferAddress, bytes: u32);
    fn clean_invalidate_all(&mut self);
}

pub struct StorageShutdownMedium<D> {
    pub driver: D,
}
impl<D: NandSoftware> StorageShutdownMedium<D> {
    fn configure(&mut self) -> MediumStatus {
        if !self.driver.initialized() { return MediumStatus(-255); }
        self.driver.configure_wait(NandWait { mode: 0, timeout: 1000 })
    }

    pub fn read_profile(&mut self, first: NandBlock, configuration: &mut [u8; 0x40000]) -> MediumStatus {
        let status = self.configure();
        if status.0 != 0 { return status; }
        let mut request = NandBlockTransfer {
            first, count: 2, buffer: BufferAddress(configuration.as_mut_ptr() as usize as u32 | 0x40000000),
            mode: NandTransferMode::Read, wait_mode: NandWaitMode::Poll, timeout: 1000, slot: NandHandleSlot(0),
        };
        self.driver.clean_invalidate_all();
        let status = self.driver.read_mapped_blocks(&mut request, configuration);
        self.driver.shutdown_if_initialized();
        status
    }
}

impl<D: NandSoftware> LensProfileMedium for StorageShutdownMedium<D> {
    fn read_cache(&mut self, block: NandBlock, destination: &mut [u8; 0xa08]) -> MediumStatus {
        if !self.driver.initialized() { return MediumStatus(-255); }
        let mut request = NandByteTransfer {
            first: block, offset: 0, length: 0xa08,
            buffer: BufferAddress(destination.as_mut_ptr() as usize as u32 | 0x40000000),
            mode: NandTransferMode::Read, wait_mode: NandWaitMode::Poll, timeout: 1000, slot: NandHandleSlot(0),
        };
        self.driver.clean_invalidate_all();
        self.driver.read_mapped_bytes(&mut request, destination)
    }

    fn erase_blocks(&mut self, first: NandBlock, count: u32) -> MediumStatus {
        let status = self.configure();
        if status.0 != 0 { return status; }
        let mut request = NandErase { first, count, wait_mode: NandWaitMode::Poll, timeout: 1000, slot: NandHandleSlot(0) };
        let status = self.driver.erase_mapped_blocks(&mut request);
        self.driver.shutdown_if_initialized();
        status
    }

    fn write_profile(&mut self, first: NandBlock, configuration: &[u8; 0x40000]) -> MediumStatus {
        let status = self.configure();
        if status.0 != 0 { return status; }
        let address = configuration.as_ptr() as usize as u32;
        let mut request = NandBlockTransfer {
            first, count: 2, buffer: BufferAddress(address | 0x40000000),
            mode: NandTransferMode::Write, wait_mode: NandWaitMode::Poll, timeout: 1000, slot: NandHandleSlot(0),
        };
        self.driver.clean_cache(BufferAddress(address & !0x40000000), 0x40000);
        let status = self.driver.write_mapped_blocks(&mut request, configuration);
        self.driver.shutdown_if_initialized();
        status
    }

    fn write_cache(&mut self, first: NandBlock, cache: &[u8; 0xa08]) -> MediumStatus {
        let status = self.configure();
        if status.0 != 0 { return status; }
        let address = cache.as_ptr() as usize as u32;
        let mut request = NandByteTransfer {
            first, offset: 0, length: 0xa08, buffer: BufferAddress(address | 0x40000000),
            mode: NandTransferMode::Write, wait_mode: NandWaitMode::Poll, timeout: 1000, slot: NandHandleSlot(0),
        };
        self.driver.clean_cache(BufferAddress(address & !0x40000000), 0xa08);
        let status = self.driver.write_mapped_bytes(&mut request, cache);
        self.driver.shutdown_if_initialized();
        status
    }
}
