use crate::backend::ConfigImages;
use crate::eeprom_profile_medium::{ProfileMedium, ProfileSlot};
use xm1_power_services::storage_shutdown_actor::{MediumStatus, NandBlock};
use xm1_power_services::storage_shutdown_defaults::{DefaultMediaOffset, DefaultStorageIo, IndexedBlock};
use xm1_power_services::storage_shutdown_load::DefaultBank;
use xm1_power_services::storage_shutdown_medium::{BufferAddress, NandBlockTransfer, NandByteTransfer, NandErase, NandSoftware, NandWait, StorageShutdownMedium};

struct SyntheticNand {
    media: std::vec::Vec<u8>,
    initialized: bool,
    configure_status: MediumStatus,
    read_status: MediumStatus,
}
impl SyntheticNand {
    fn new() -> Self {
        let mut media = std::vec![0; 40 * 0x20000];
        for (block, bytes) in media.chunks_mut(0x20000).enumerate() { bytes.fill(0x20 + block as u8); }
        Self { media, initialized: true, configure_status: MediumStatus(0), read_status: MediumStatus(0) }
    }
}
impl NandSoftware for SyntheticNand {
    fn initialized(&self) -> bool { self.initialized }
    fn configure_wait(&mut self, _: NandWait) -> MediumStatus { self.configure_status }
    fn shutdown_if_initialized(&mut self) {}
    fn erase_mapped_blocks(&mut self, _: &mut NandErase) -> MediumStatus { panic!("unexpected erase") }
    fn write_mapped_blocks(&mut self, _: &mut NandBlockTransfer, _: &[u8; 0x40000]) -> MediumStatus { panic!("unexpected write") }
    fn read_mapped_blocks(&mut self, request: &mut NandBlockTransfer, bytes: &mut [u8; 0x40000]) -> MediumStatus {
        if self.read_status.0 == 0 {
            let start = (request.first.0 - 0x20) as usize * 0x20000;
            let length = request.count as usize * 0x20000;
            bytes[..length].copy_from_slice(&self.media[start..start + length]);
        }
        self.read_status
    }
    fn write_mapped_bytes(&mut self, _: &mut NandByteTransfer, _: &[u8; 0xa08]) -> MediumStatus { panic!("unexpected write") }
    fn read_mapped_bytes(&mut self, _: &mut NandByteTransfer, _: &mut [u8; 0xa08]) -> MediumStatus { panic!("unexpected cache read") }
    fn clean_cache(&mut self, _: BufferAddress, _: u32) { panic!("unexpected clean") }
    fn clean_invalidate_all(&mut self) {}
}

struct SyntheticDefaults {
    media: std::vec::Vec<u8>,
    section: Result<DefaultMediaOffset, MediumStatus>,
    cached: usize,
    lock_status: MediumStatus,
    read_status: MediumStatus,
}
impl SyntheticDefaults {
    fn new() -> Self {
        let media = (0..0x80000).map(|index| (index % 251) as u8).collect();
        Self { media, section: Ok(DefaultMediaOffset(0x1e123)), cached: 0, lock_status: MediumStatus(0), read_status: MediumStatus(0) }
    }
}
impl DefaultStorageIo for SyntheticDefaults {
    fn section_offset(&mut self) -> Result<DefaultMediaOffset, MediumStatus> { self.section }
    fn lock_cache(&mut self) -> MediumStatus { self.lock_status }
    fn unlock_cache(&mut self) {}
    fn read_cached(&mut self, first: IndexedBlock) -> MediumStatus {
        self.cached = first.0 as usize * 0x20000;
        self.read_status
    }
    fn copy_cached(&mut self, offset: u32, destination: &mut [u8]) {
        let start = self.cached + offset as usize;
        destination.copy_from_slice(&self.media[start..start + destination.len()]);
    }
    fn read_direct(&mut self, first: IndexedBlock, count: u32, destination: &mut [u8]) -> MediumStatus {
        let start = first.0 as usize * 0x20000;
        let length = count as usize * 0x20000;
        destination.copy_from_slice(&self.media[start..start + length]);
        self.read_status
    }
}

fn medium(nand: SyntheticNand, defaults: SyntheticDefaults) -> ProfileMedium<SyntheticNand, SyntheticDefaults> {
    ProfileMedium::new(StorageShutdownMedium { driver: nand }, defaults, NandBlock(0x3a0))
}

#[test]
fn selected_profiles_read_both_blocks_into_only_the_profile_window() {
    for (slot, first, second) in [(0, 0x20, 0x21), (3, 0x26, 0x27), (19, 0x46, 0x47)] {
        let mut bytes = std::vec![0xff; 0x420000];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        assert_eq!(medium(SyntheticNand::new(), SyntheticDefaults::new()).read_profile(ProfileSlot::new(slot).unwrap(), &mut configuration), MediumStatus(0));
        assert!(configuration.encoded_bytes()[..0x3e0000].iter().all(|byte| *byte == 0xff));
        assert!(configuration.encoded_bytes()[0x3e0000..0x400000].iter().all(|byte| *byte == first));
        assert!(configuration.encoded_bytes()[0x400000..].iter().all(|byte| *byte == second));
    }
    assert_eq!(ProfileSlot::new(20), None);
    assert_eq!(ProfileSlot::new(255), None);
}

#[test]
fn default_banks_read_nonzero_section_offsets_across_source_block_boundaries() {
    for (bank, destination, source, length) in [
        (DefaultBank::Primary, 0x3e0000, 0x33f23, 0x2000),
        (DefaultBank::Extended, 0x3e2000, 0x1e123, 0x15e00),
    ] {
        let defaults = SyntheticDefaults::new();
        let expected = defaults.media[source..source + length].to_vec();
        let mut bytes = std::vec![0xff; 0x420000];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        assert_eq!(medium(SyntheticNand::new(), defaults).read_defaults(bank, &mut configuration), MediumStatus(0));
        assert_eq!(&configuration.encoded_bytes()[destination..destination + length], &expected);
        assert!(configuration.encoded_bytes()[..destination].iter().all(|byte| *byte == 0xff));
        assert!(configuration.encoded_bytes()[destination + length..].iter().all(|byte| *byte == 0xff));
    }
}

#[test]
fn profile_initialization_configuration_and_read_failures_preserve_status_and_destination() {
    for (initialized, configure, read, expected) in [(false, 0, 0, -255), (true, -17, 0, -17), (true, 0, -30, -30)] {
        let mut nand = SyntheticNand::new();
        nand.initialized = initialized;
        nand.configure_status = MediumStatus(configure);
        nand.read_status = MediumStatus(read);
        let mut bytes = std::vec![0xff; 0x420000];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        assert_eq!(medium(nand, SyntheticDefaults::new()).read_profile(ProfileSlot::new(0).unwrap(), &mut configuration), MediumStatus(expected));
        assert!(configuration.encoded_bytes().iter().all(|byte| *byte == 0xff));
    }
}

#[test]
fn default_metadata_lock_and_read_failures_are_not_successful_loads() {
    for (metadata, lock, read, expected) in [(-30, 0, 0, -30), (0, -17, 0, -255), (0, 0, -41, -41)] {
        let mut defaults = SyntheticDefaults::new();
        if metadata != 0 { defaults.section = Err(MediumStatus(metadata)); }
        defaults.lock_status = MediumStatus(lock);
        defaults.read_status = MediumStatus(read);
        let mut bytes = std::vec![0xff; 0x420000];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        assert_eq!(medium(SyntheticNand::new(), defaults).read_defaults(DefaultBank::Primary, &mut configuration), MediumStatus(expected));
        assert!(configuration.encoded_bytes()[..0x3e0000].iter().all(|byte| *byte == 0xff));
        assert!(configuration.encoded_bytes()[0x3e2000..].iter().all(|byte| *byte == 0xff));
    }
}

#[test]
fn incomplete_configuration_rejects_profile_and_defaults_without_partial_updates() {
    let mut bytes = std::vec![0xff; 0x3e1000];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut medium = medium(SyntheticNand::new(), SyntheticDefaults::new());
    assert_eq!(medium.read_profile(ProfileSlot::new(0).unwrap(), &mut configuration), MediumStatus(-255));
    assert_eq!(medium.read_defaults(DefaultBank::Primary, &mut configuration), MediumStatus(-255));
    assert_eq!(medium.read_defaults(DefaultBank::Extended, &mut configuration), MediumStatus(-255));
    assert!(configuration.encoded_bytes().iter().all(|byte| *byte == 0xff));
}
