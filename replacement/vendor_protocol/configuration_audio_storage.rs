use crate::{backend::{BackendError, ConfigImages}, configuration_actions_low::{ActionCompletion, ConfigurationKey}, configuration_storage::{ConfigurationFlash, ConfigurationOffset, ConfigurationStorage, FlashStatus, BLOCK_BYTES}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationPage(u8);
impl ConfigurationPage { pub const fn from_sample(value: u8) -> Self { Self(value) } }

pub fn copy_configuration_page<F: ConfigurationFlash>(storage: &mut ConfigurationStorage<F>, page: ConfigurationPage, destination: &mut [u8], scratch: &mut [u8; BLOCK_BYTES]) -> Result<ActionCompletion, FlashStatus> {
    if destination.len() < 0x206 { return Err(FlashStatus(-30)); }
    if page.0 <= 31 {
        let offset = usize::from(page.0) << 8;
        let output = destination.get_mut(offset..offset + 256).ok_or(FlashStatus(-30))?;
        let _ = storage.read_configuration(ConfigurationOffset::new(offset as u32)?, output, scratch);
    } else if page.0 == 255 {
        let output = destination.get_mut(..0x42000).ok_or(FlashStatus(-30))?;
        let _ = storage.read_configuration(ConfigurationOffset::new(0)?, output, scratch);
        destination[0x1e8] = 1;
        destination[0x1e9] = 0x78;
    }
    destination[0x204] = 1;
    destination[0x205] = 0x78;
    Ok(ActionCompletion::service_success())
}

pub fn action28_copy<F: ConfigurationFlash>(key: ConfigurationKey, configuration: &ConfigImages<'_>, storage: &mut ConfigurationStorage<F>,
    destination: &mut [u8], scratch: &mut [u8; BLOCK_BYTES]) -> Result<ActionCompletion, BackendError> {
    let page = ConfigurationPage::from_sample(key.read(configuration)?);
    copy_configuration_page(storage, page, destination, scratch).map_err(|_| BackendError::Capacity)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlushStatus(i32);
impl FlushStatus {
    pub const fn code(self) -> i32 { self.0 }
    pub const fn configuration_byte(self) -> u8 { if self.0 == 0 { 0 } else { 1 } }
}

pub fn publish_flush(key: ConfigurationKey, status: FlushStatus, configuration: &mut ConfigImages<'_>) -> Result<ActionCompletion, BackendError> {
    key.write(configuration, status.configuration_byte())?;
    Ok(ActionCompletion::service_success())
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::FlushStatus;
    extern "C" {
        fn xm1_app_nand_configure(request: *const [u32; 2]) -> i32;
        fn xm1_app_nand_write_blocks(request: *mut [u32; 7]) -> i32;
        fn xm1_app_nand_shutdown();
        fn xm1_power_dcache_clean_range(address: u32, length: u32);
    }

    // The shared application NAND owner holds the profile map and lifecycle lock.
    unsafe fn write_profile_blocks(first: u32, count: u32, input: *const u8) -> i32 {
        let status = unsafe { xm1_app_nand_configure(&[0, 1000]) };
        if status != 0 { return status; }
        let address = input as usize as u32;
        let mut request = [first, count, address | 0x40000000, 1, 0, 1000, 0];
        unsafe { xm1_power_dcache_clean_range(address & !0x40000000, count << 17); }
        let status = unsafe { xm1_app_nand_write_blocks(&mut request) };
        unsafe { xm1_app_nand_shutdown(); }
        status
    }

    pub fn flush_configuration(image: &[u8]) -> FlushStatus {
        if image.len() < 0x420000 { return FlushStatus(-30); }
        if unsafe { core::ptr::read_volatile(0x828574 as *const u32) } != 1 { return FlushStatus(-255); }
        FlushStatus(unsafe { write_profile_blocks(41, 33, image.as_ptr()) })
    }

    pub fn action29_flush(key: crate::configuration_actions_low::ConfigurationKey, configuration: &mut crate::backend::ConfigImages<'_>, image: &[u8]) -> Result<crate::configuration_actions_low::ActionCompletion, crate::backend::BackendError> {
        super::publish_flush(key, flush_configuration(image), configuration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration_storage::{ConfigurationIdentity, PhysicalBlock, WritePolicy};
    struct Flash { fail_reads: bool }
    impl ConfigurationFlash for Flash {
        fn bad_blocks(&mut self, _: &mut [PhysicalBlock; 0x13b]) -> Result<usize, FlashStatus> { Ok(0) }
        fn read(&mut self, first: PhysicalBlock, output: &mut [u8; BLOCK_BYTES]) -> Result<(), FlashStatus> {
            if self.fail_reads { return Err(FlashStatus(-7)); }
            output.fill(first.get() as u8);
            Ok(())
        }
        fn erase(&mut self, _: PhysicalBlock, _: u16) -> Result<(), FlashStatus> { Ok(()) }
        fn program(&mut self, _: PhysicalBlock, _: &[u8], _: u16) -> Result<bool, FlashStatus> { Ok(false) }
        fn delay(&mut self, _: u32) {}
        fn priority(&mut self, _: u8) {}
    }
    fn storage(fail_reads: bool) -> ConfigurationStorage<Flash> {
        ConfigurationStorage::mount(Flash { fail_reads }, ConfigurationIdentity::new([0; 20]), WritePolicy { erase_passes: 0, delay_seconds: 0, replacement_limit: 0 }).unwrap()
    }
    #[test]
    fn selected_page_reads_the_configuration_mapping_without_replacing_other_pages() {
        let mut storage = storage(false);
        let mut destination = [0xff; 0x1000];
        let mut scratch = [0; BLOCK_BYTES];
        assert_eq!(copy_configuration_page(&mut storage, ConfigurationPage::from_sample(3), &mut destination, &mut scratch), Ok(ActionCompletion::service_success()));
        assert!(destination[0x300..0x400].iter().all(|byte| *byte == 7));
        assert_eq!(&destination[..4], &[0xff; 4]);
        assert_eq!(&destination[0x204..0x206], &[1, 0x78]);
    }
    #[test]
    fn copy_read_failure_still_publishes_the_source_overlay() {
        let mut storage = storage(true);
        let mut destination = [0xaa; 0x42000];
        let mut scratch = [0; BLOCK_BYTES];
        assert_eq!(copy_configuration_page(&mut storage, ConfigurationPage::from_sample(255), &mut destination, &mut scratch), Ok(ActionCompletion::service_success()));
        assert_eq!(&destination[0x1e8..0x1ea], &[1, 0x78]);
        assert_eq!(&destination[0x204..0x206], &[1, 0x78]);
        assert_eq!(destination[0], 0xaa);
    }
    #[test]
    fn flush_failure_is_a_result_byte_not_a_service_rejection() {
        for (status, expected) in [(0, 0), (-255, 1), (-30, 1)] {
            let mut bytes = [0xff; 4];
            let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
            assert_eq!(publish_flush(ConfigurationKey::from_abi(2), FlushStatus(status), &mut configuration), Ok(ActionCompletion::service_success()));
            assert_eq!(configuration.byte(2), Some(expected));
        }
    }
}
