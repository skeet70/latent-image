#![cfg(target_arch = "arm")]

use crate::storage_shutdown_actor::MediumStatus;
use crate::storage_shutdown_medium::{BufferAddress, NandBlockTransfer, NandByteTransfer, NandErase, NandSoftware, NandWait, StorageShutdownMedium};

extern "C" {
    fn xm1_app_nand_configure(request: *const u32) -> i32;
    fn xm1_app_nand_shutdown();
    fn xm1_app_nand_erase(request: *mut u32) -> i32;
    fn xm1_app_nand_write_blocks(request: *mut u32) -> i32;
    fn xm1_app_nand_read_blocks(request: *mut u32) -> i32;
    fn xm1_app_nand_write_bytes(request: *mut u32) -> i32;
    fn xm1_app_nand_read_bytes(request: *mut u32) -> i32;
    fn xm1_power_dcache_clean_range(address: u32, length: u32);
    fn xm1_power_dcache_clean_invalidate_all();
}

pub struct NativeNand;
pub fn medium() -> StorageShutdownMedium<NativeNand> { StorageShutdownMedium { driver: NativeNand } }

impl NandSoftware for NativeNand {
    fn initialized(&self) -> bool { unsafe { core::ptr::read_volatile(0x828574 as *const u32) == 1 } }
    fn configure_wait(&mut self, request: NandWait) -> MediumStatus {
        MediumStatus(unsafe { xm1_app_nand_configure([request.mode, request.timeout].as_ptr()) })
    }
    fn shutdown_if_initialized(&mut self) { unsafe { xm1_app_nand_shutdown() }; }
    fn erase_mapped_blocks(&mut self, request: &mut NandErase) -> MediumStatus {
        let mut words = [request.first.0, request.count, 0, request.timeout, request.slot.0];
        MediumStatus(unsafe { xm1_app_nand_erase(words.as_mut_ptr()) })
    }
    fn write_mapped_blocks(&mut self, request: &mut NandBlockTransfer, _bytes: &[u8; 0x40000]) -> MediumStatus {
        let mut words = [request.first.0, request.count, request.buffer.0, 1, 0, request.timeout, request.slot.0];
        MediumStatus(unsafe { xm1_app_nand_write_blocks(words.as_mut_ptr()) })
    }
    fn read_mapped_blocks(&mut self, request: &mut NandBlockTransfer, _bytes: &mut [u8; 0x40000]) -> MediumStatus {
        let mut words = [request.first.0, request.count, request.buffer.0, 0, request.timeout, request.slot.0];
        MediumStatus(unsafe { xm1_app_nand_read_blocks(words.as_mut_ptr()) })
    }
    fn write_mapped_bytes(&mut self, request: &mut NandByteTransfer, _bytes: &[u8; 0xa08]) -> MediumStatus {
        let mut words = [request.first.0, request.offset, request.length, request.buffer.0, 1, 0, request.timeout, request.slot.0];
        MediumStatus(unsafe { xm1_app_nand_write_bytes(words.as_mut_ptr()) })
    }
    fn read_mapped_bytes(&mut self, request: &mut NandByteTransfer, _bytes: &mut [u8; 0xa08]) -> MediumStatus {
        let mut words = [request.first.0, request.offset, request.length, request.buffer.0, 0, request.timeout, request.slot.0];
        MediumStatus(unsafe { xm1_app_nand_read_bytes(words.as_mut_ptr()) })
    }
    fn clean_cache(&mut self, address: BufferAddress, bytes: u32) { unsafe { xm1_power_dcache_clean_range(address.0, bytes) }; }
    fn clean_invalidate_all(&mut self) { unsafe { xm1_power_dcache_clean_invalidate_all() }; }
}
