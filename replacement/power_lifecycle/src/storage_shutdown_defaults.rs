use crate::storage_shutdown_actor::MediumStatus;
use crate::storage_shutdown_load::DefaultBank;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DefaultMediaOffset(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexedBlock(pub u32);

pub trait DefaultStorageIo {
    fn section_offset(&mut self) -> Result<DefaultMediaOffset, MediumStatus>;
    fn lock_cache(&mut self) -> MediumStatus;
    fn unlock_cache(&mut self);
    fn read_cached(&mut self, first: IndexedBlock) -> MediumStatus;
    fn copy_cached(&mut self, offset: u32, destination: &mut [u8]);
    fn read_direct(&mut self, first: IndexedBlock, count: u32, destination: &mut [u8]) -> MediumStatus;
}

pub fn read_defaults(io: &mut impl DefaultStorageIo, bank: DefaultBank, destination: &mut [u8]) -> MediumStatus {
    let source = match io.section_offset() { Ok(source) => source.0, Err(status) => return status };
    let mut offset = source.wrapping_add(match bank { DefaultBank::Primary => 0x15e00, DefaultBank::Extended => 0 });
    let mut destination = destination;
    let within = offset & 0x1ffff;
    if within != 0 {
        if io.lock_cache().0 != 0 { return MediumStatus(-255); }
        let count = core::cmp::min(0x20000 - within, destination.len() as u32) as usize;
        let status = io.read_cached(IndexedBlock(offset >> 17));
        io.copy_cached(within, &mut destination[..count]);
        io.unlock_cache();
        if status.0 != 0 { return status; }
        offset = (offset & 0xfffe0000).wrapping_add(0x20000);
        destination = &mut destination[count..];
    }
    let whole = destination.len() >> 17;
    if whole != 0 {
        let count = whole << 17;
        let status = io.read_direct(IndexedBlock(offset >> 17), whole as u32, &mut destination[..count]);
        if status.0 != 0 { return status; }
        offset = offset.wrapping_add(count as u32);
        destination = &mut destination[count..];
    }
    if destination.is_empty() { return MediumStatus(0); }
    if io.lock_cache().0 != 0 { return MediumStatus(-255); }
    let status = io.read_cached(IndexedBlock(offset >> 17));
    io.copy_cached(0, destination);
    io.unlock_cache();
    status
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::*;
    use crate::application_nand::boot_format::{section_metadata, MetadataSection};
    extern "C" {
        fn xm1_app_nand_configure(request: *const u32) -> i32;
        fn xm1_app_nand_shutdown();
        fn xm1_app_nand_read_indexed(request: *mut u32, callback: u32) -> i32;
        fn xm1_power_dcache_clean_invalidate_all();
        fn xm1_power_semaphore_wait(id: i32, timeout: i32) -> i32;
        fn xm1_power_semaphore_signal(id: i32) -> i32;
    }
    unsafe fn word(address: u32) -> u32 { unsafe { core::ptr::read_volatile(address as *const u32) } }

    unsafe fn indexed(first: IndexedBlock, count: u32, destination: *mut u8) -> MediumStatus {
        if unsafe { word(0x828574) } != 1 { return MediumStatus(-255); }
        if first.0 >= 0x400 { return MediumStatus(-30); }
        let status = unsafe { xm1_app_nand_configure([0, 1000].as_ptr()) };
        if status != 0 { return MediumStatus(status); }
        let mut request = core::mem::MaybeUninit::<[u32; 8]>::uninit();
        let pointer = request.as_mut_ptr().cast::<u32>();
        unsafe {
            pointer.add(0).write(u32::from(core::ptr::read_volatile((0x828580 + first.0 * 2) as *const u16)));
            pointer.add(1).write(count);
            pointer.add(2).write(word(0x7c8ca0).wrapping_sub(word(0x828578)));
            pointer.add(3).write(destination as u32 | 0x40000000);
            pointer.add(5).write(0);
            pointer.add(6).write(1000);
            pointer.add(7).write(0);
            xm1_power_dcache_clean_invalidate_all();
        }
        let status = unsafe { xm1_app_nand_read_indexed(pointer, 0) };
        unsafe { xm1_app_nand_shutdown() };
        MediumStatus(status)
    }

    pub struct NativeDefaults;
    impl DefaultStorageIo for NativeDefaults {
        fn section_offset(&mut self) -> Result<DefaultMediaOffset, MediumStatus> {
            unsafe { section_metadata(MetadataSection::LensDefaults) }
                .map(|metadata| DefaultMediaOffset(metadata.source_offset.bytes()))
                .map_err(|_| MediumStatus(-255))
        }
        fn lock_cache(&mut self) -> MediumStatus { MediumStatus(unsafe { xm1_power_semaphore_wait(38, -1) }) }
        fn unlock_cache(&mut self) { let _ = unsafe { xm1_power_semaphore_signal(38) }; }
        fn read_cached(&mut self, first: IndexedBlock) -> MediumStatus { unsafe { indexed(first, 1, 0x91a760 as *mut u8) } }
        fn copy_cached(&mut self, offset: u32, destination: &mut [u8]) {
            for (index, byte) in destination.iter_mut().enumerate() {
                *byte = unsafe { core::ptr::read_volatile((0x91a760 + offset + index as u32) as *const u8) };
            }
        }
        fn read_direct(&mut self, first: IndexedBlock, count: u32, destination: &mut [u8]) -> MediumStatus {
            unsafe { indexed(first, count, destination.as_mut_ptr()) }
        }
    }
}

#[cfg(test)]
#[path = "storage_shutdown_defaults_tests.rs"]
mod tests;
