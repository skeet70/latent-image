use crate::storage_shutdown_actor::MediumStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BadBlockNumber(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageReadiness { Uninitialized, Initialized }
impl StorageReadiness {
    pub const fn abi(self) -> u32 { match self { Self::Uninitialized => 0, Self::Initialized => 1 } }
}

pub struct StorageMap<'a> {
    pub words: &'a mut [u16; 0x13b],
    pub readiness: StorageReadiness,
    pub bad_block_count: u32,
}

impl StorageMap<'_> {
    pub fn finish_setup(&mut self, count: u32, blocks: impl IntoIterator<Item = BadBlockNumber>) -> MediumStatus {
        self.words[..0x13a].fill(0);
        for block in blocks {
            if block.0 <= 0x13a {
                self.words[block.0 as usize] = 1;
            }
        }
        let mut skipped = 0usize;
        for logical in 0..0x126 {
            while self.words[logical + skipped] != 0 {
                skipped += 1;
                if skipped > 20 {
                    self.bad_block_count = count;
                    return MediumStatus(-255);
                }
            }
            self.words[logical] = (logical + skipped) as u16;
        }
        self.bad_block_count = count;
        self.readiness = StorageReadiness::Initialized;
        MediumStatus(0)
    }
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::{BadBlockNumber, StorageMap, StorageReadiness};
    use crate::application_nand::boot_skip::BootSkipTable;
    use crate::storage_shutdown_actor::MediumStatus;
    extern "C" {
        fn xm1_app_nand_configure(request: *const u32) -> i32;
        fn xm1_app_nand_shutdown();
        fn xm1_app_nand_build_profile_map() -> i32;
        fn xm1_app_nand_read_bad_blocks(request: *mut u32, table: *mut u32) -> i32;
    }
    unsafe fn word(address: u32) -> u32 { unsafe { core::ptr::read_volatile(address as *const u32) } }
    unsafe fn put(address: u32, value: u32) { unsafe { core::ptr::write_volatile(address as *mut u32, value) }; }

    pub unsafe fn finish_setup() -> MediumStatus {
        if unsafe { word(0x89f8a8) } == 0 {
            unsafe { put(0x89f8a8, 1) };
            let table = match unsafe { BootSkipTable::from_boot() } {
                Ok(table) => table,
                Err(_) => return MediumStatus(-255),
            };
            unsafe { put(0x9552f0, table.address().as_mut_ptr() as u32) };
        } else {
            let status = unsafe { xm1_app_nand_configure([0, 1000].as_ptr()) };
            if status != 0 {
                unsafe { put(0x828578, word(word(0x9552f0))) };
                return MediumStatus(status);
            }
            let mut request = [0, 0x13a, 0, 1000];
            let status = unsafe { xm1_app_nand_read_bad_blocks(request.as_mut_ptr(), word(0x9552f0) as *mut u32) };
            unsafe { xm1_app_nand_shutdown() };
            if status != 0 {
                unsafe { put(0x828578, word(word(0x9552f0))) };
                return MediumStatus(status);
            }
        }
        let table = match unsafe { BootSkipTable::from_boot() } {
            Ok(table) => table,
            Err(_) => return MediumStatus(-255),
        };
        let mut map = StorageMap {
            words: unsafe { &mut *(0x828580 as *mut [u16; 0x13b]) },
            readiness: if unsafe { word(0x828574) } == 1 { StorageReadiness::Initialized } else { StorageReadiness::Uninitialized },
            bad_block_count: unsafe { word(0x828578) },
        };
        let status = map.finish_setup(table.iter().len() as u32, table.iter().map(|block| BadBlockNumber(block.index())));
        unsafe { put(0x828578, map.bad_block_count) };
        if status.0 == 0 { unsafe { put(0x828574, 1) }; }
        status
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_storage_shutdown_initialize_medium() -> i32 {
        if unsafe { word(0x828574) } == 1 { return 0; }
        let status = unsafe { xm1_app_nand_configure([0, 1000].as_ptr()) };
        if status != 0 { return status; }
        let _ = unsafe { xm1_app_nand_build_profile_map() };
        unsafe { xm1_app_nand_shutdown() };
        unsafe { finish_setup() }.0
    }
}
