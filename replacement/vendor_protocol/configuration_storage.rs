use crate::{backend::BackendError, startup::WORKSPACE_BYTES};

pub const BLOCK_BYTES: usize = 0x20000;
pub const CONFIGURATION_BLOCKS: usize = 294;
pub const CONFIGURATION_BYTES: usize = CONFIGURATION_BLOCKS * BLOCK_BYTES;
const PREFIX_BLOCKS: usize = 7;
const BAD_BLOCK_LIMIT: u16 = 0x13a;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalBlock(u16);
impl PhysicalBlock {
    pub fn new(value: u16) -> Result<Self, BackendError> {
        if value < 0x400 { Ok(Self(value)) } else { Err(BackendError::Range) }
    }
    pub const fn get(self) -> u16 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlashStatus(pub i32);

pub trait ConfigurationFlash {
    fn bad_blocks(&mut self, output: &mut [PhysicalBlock; 0x13b]) -> Result<usize, FlashStatus>;
    fn read(&mut self, first: PhysicalBlock, output: &mut [u8; BLOCK_BYTES]) -> Result<(), FlashStatus>;
    fn erase(&mut self, first: PhysicalBlock, count: u16) -> Result<(), FlashStatus>;
    fn program(&mut self, first: PhysicalBlock, input: &[u8], replacement_budget: u16) -> Result<bool, FlashStatus>;
    fn delay(&mut self, ticks: u32);
    fn priority(&mut self, priority: u8);
}

pub struct NativeConfigurationFlash { task: u32 }

extern "C" {
    fn xm1_app_nand_configure(request: *const [u32; 2]) -> i32;
    fn xm1_app_nand_shutdown();
    fn xm1_app_nand_read_bad_blocks(request: *mut [u32; 4], output: *mut u32) -> i32;
    fn xm1_app_nand_erase_physical(request: *mut [u32; 5]) -> i32;
    fn xm1_app_nand_write_physical(request: *mut [u32; 8]) -> i32;
    fn xm1_app_nand_read_indexed(request: *mut [u32; 8], use_table: u32) -> i32;
    fn xm1_power_current_task() -> u32;
    fn xm1_power_set_task_phase(task: u32, priority: u32) -> i32;
    fn xm1_power_task_delay(ticks: u32) -> i32;
}

impl NativeConfigurationFlash {
    /// # Safety
    /// The caller grants exclusive NAND-controller ownership until this value is dropped.
    pub unsafe fn configure() -> Result<Self, FlashStatus> {
        let request = [0, 1000];
        let status = unsafe { xm1_app_nand_configure(&request) };
        if status != 0 { return Err(FlashStatus(status)); }
        Ok(Self { task: unsafe { xm1_power_current_task() } })
    }
}

impl Drop for NativeConfigurationFlash {
    fn drop(&mut self) { unsafe { xm1_app_nand_shutdown(); } }
}

impl ConfigurationFlash for NativeConfigurationFlash {
    fn bad_blocks(&mut self, output: &mut [PhysicalBlock; 0x13b]) -> Result<usize, FlashStatus> {
        let mut request = [0, 0x13a, 0, 1000];
        let mut response = [0u32; 0x13b];
        let status = unsafe { xm1_app_nand_read_bad_blocks(&mut request, response.as_mut_ptr()) };
        if status != 0 { return Err(FlashStatus(status)); }
        let count = response[0] as usize;
        let source = response.get(1..count + 1).ok_or(FlashStatus(-255))?;
        for (destination, source) in output.iter_mut().zip(source) {
            *destination = PhysicalBlock::new(u16::try_from(*source).map_err(|_| FlashStatus(-30))?).map_err(|_| FlashStatus(-30))?;
        }
        Ok(count)
    }
    fn read(&mut self, first: PhysicalBlock, output: &mut [u8; BLOCK_BYTES]) -> Result<(), FlashStatus> {
        let destination = u32::try_from(output.as_mut_ptr() as usize).map_err(|_| FlashStatus(-30))?;
        let mut request = [u32::from(first.0), 1, 20, destination, 0, 0, 1000, 0];
        let status = unsafe { xm1_app_nand_read_indexed(&mut request, 0) };
        if status == 0 { Ok(()) } else { Err(FlashStatus(status)) }
    }
    fn erase(&mut self, first: PhysicalBlock, count: u16) -> Result<(), FlashStatus> {
        let mut request = [u32::from(first.0), u32::from(count), 0, 0, 1000];
        let status = unsafe { xm1_app_nand_erase_physical(&mut request) };
        if status == 0 { Ok(()) } else { Err(FlashStatus(status)) }
    }
    fn program(&mut self, first: PhysicalBlock, input: &[u8], replacement_budget: u16) -> Result<bool, FlashStatus> {
        let source = u32::try_from(input.as_ptr() as usize).map_err(|_| FlashStatus(-30))?;
        let mut request = [
            u32::from(first.0), (input.len() / BLOCK_BYTES) as u32,
            u32::from(replacement_budget), source | 0x40000000, 1, 0, 0, 1000,
        ];
        let status = unsafe { xm1_app_nand_write_physical(&mut request) };
        if status == 0 { Ok(request[5] != 0) } else { Err(FlashStatus(status)) }
    }
    fn delay(&mut self, ticks: u32) { unsafe { let _ = xm1_power_task_delay(ticks); } }
    fn priority(&mut self, priority: u8) { unsafe { let _ = xm1_power_set_task_phase(self.task, u32::from(priority)); } }
}

pub struct ConfigurationMap {
    blocks: [PhysicalBlock; CONFIGURATION_BLOCKS],
    bad_count: u16,
}
impl ConfigurationMap {
    pub fn from_bad_blocks(bad: &[PhysicalBlock]) -> Result<Self, FlashStatus> {
        let mut excluded = [false; BAD_BLOCK_LIMIT as usize + 1];
        for block in bad {
            if block.0 <= BAD_BLOCK_LIMIT { excluded[block.0 as usize] = true; }
        }
        let mut blocks = [PhysicalBlock(0); CONFIGURATION_BLOCKS];
        let mut skipped = 0;
        for (logical, physical) in blocks.iter_mut().enumerate() {
            while excluded[logical + skipped] {
                skipped += 1;
                if skipped > 20 { return Err(FlashStatus(-255)); }
            }
            *physical = PhysicalBlock((logical + skipped) as u16);
        }
        Ok(Self { blocks, bad_count: u16::try_from(bad.len()).map_err(|_| FlashStatus(-255))? })
    }
    pub fn physical(&self, logical: u16) -> Result<PhysicalBlock, FlashStatus> {
        self.blocks.get(logical as usize).copied().ok_or(FlashStatus(-30))
    }
    fn refresh(&mut self, flash: &mut impl ConfigurationFlash) -> Result<(), FlashStatus> {
        let mut bad = [PhysicalBlock(0); 0x13b];
        let count = flash.bad_blocks(&mut bad)?;
        let bad = bad.get(..count).ok_or(FlashStatus(-255))?;
        *self = Self::from_bad_blocks(bad)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationIdentity([u8; 20]);
impl ConfigurationIdentity {
    pub const fn new(bytes: [u8; 20]) -> Self { Self(bytes) }
    fn matches(self, input: &[u8]) -> bool {
        self.0.iter().zip(input).take_while(|(expected, actual)| **expected != 0 || **actual != 0).all(|(expected, actual)| expected == actual)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadStatus { Complete, InvalidImage, ChecksumMismatch, StorageFailure, InitializationFailure, CompletionFailure, DriverFailure, PeripheralFailure(i32) }
impl UploadStatus {
    pub const fn code(self) -> u32 {
        match self { Self::Complete => 0, Self::InvalidImage => 1, Self::ChecksumMismatch => 2, Self::StorageFailure => 4, Self::InitializationFailure => 6, Self::CompletionFailure => 7, Self::DriverFailure => 8, Self::PeripheralFailure(status) => status as u32 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WritePolicy { pub erase_passes: u8, pub delay_seconds: u8, pub replacement_limit: u16 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationOffset(u32);
impl ConfigurationOffset {
    pub fn new(value: u32) -> Result<Self, FlashStatus> {
        if value <= 0x60000 { Ok(Self(value)) } else { Err(FlashStatus(-30)) }
    }
    pub const fn get(self) -> u32 { self.0 }
}

pub struct ConfigurationStorage<F> {
    flash: F,
    map: ConfigurationMap,
    identity: ConfigurationIdentity,
    policy: WritePolicy,
}
impl<F: ConfigurationFlash> ConfigurationStorage<F> {
    pub fn mount(mut flash: F, identity: ConfigurationIdentity, policy: WritePolicy) -> Result<Self, FlashStatus> {
        let mut map = ConfigurationMap { blocks: [PhysicalBlock(0); CONFIGURATION_BLOCKS], bad_count: 0 };
        map.refresh(&mut flash)?;
        Ok(Self { flash, map, identity, policy })
    }
    pub fn flash(&self) -> &F { &self.flash }
    pub fn flash_mut(&mut self) -> &mut F { &mut self.flash }
    pub fn map(&self) -> &ConfigurationMap { &self.map }
    pub fn read_configuration(&mut self, offset: ConfigurationOffset, output: &mut [u8], scratch: &mut [u8; BLOCK_BYTES]) -> Result<(), FlashStatus> {
        let end = offset.0.checked_add(u32::try_from(output.len()).map_err(|_| FlashStatus(-30))?).ok_or(FlashStatus(-30))?;
        if end > 0x60000 { return Err(FlashStatus(-30)); }
        let mut position = offset.0 as usize;
        let mut remaining = output;
        while !remaining.is_empty() {
            let logical = PREFIX_BLOCKS + position / BLOCK_BYTES;
            let physical = self.map.physical(logical as u16)?;
            self.flash.read(physical, scratch)?;
            let within = position % BLOCK_BYTES;
            let length = remaining.len().min(BLOCK_BYTES - within);
            remaining[..length].copy_from_slice(&scratch[within..within + length]);
            remaining = &mut remaining[length..];
            position += length;
        }
        Ok(())
    }
    fn erase(&mut self, first: u16, count: u16) -> Result<(), FlashStatus> {
        let physical = self.map.physical(first)?;
        self.flash.erase(physical, count)
    }
    fn prepare(&mut self) -> Result<(), FlashStatus> { self.erase(293, 2) }
    fn write(&mut self, first: u16, input: &[u8]) -> Result<(), FlashStatus> {
        self.flash.priority(10);
        for _ in 0..self.policy.erase_passes.min(10) {
            let _ = self.erase(first, (input.len() / BLOCK_BYTES) as u16);
        }
        if self.policy.erase_passes != 0 && self.policy.delay_seconds != 0 {
            self.flash.delay(u32::from(self.policy.delay_seconds) * 1000);
        }
        self.flash.priority(6);
        let physical = self.map.physical(first)?;
        let budget = self.policy.replacement_limit.wrapping_sub(self.map.bad_count);
        if self.flash.program(physical, input, budget)? { self.map.refresh(&mut self.flash)?; }
        Ok(())
    }
    pub fn upload_fallback(&mut self, workspace: &mut [u8], total_size: u32) -> Result<UploadStatus, BackendError> {
        let workspace = workspace.get_mut(..WORKSPACE_BYTES).ok_or(BackendError::Capacity)?;
        let header = &workspace[..0x200];
        if !self.identity.matches(&header[4..24]) { return Ok(UploadStatus::InvalidImage); }
        let payload_bytes = u32::from_le_bytes(header[0x14..0x18].try_into().unwrap());
        if payload_bytes != total_size.wrapping_sub(0x200) || payload_bytes as usize > CONFIGURATION_BYTES { return Ok(UploadStatus::InvalidImage); }
        let checksum = u32::from_le_bytes(header[0x18..0x1c].try_into().unwrap());
        let sum = workspace[0x200..0x200 + payload_bytes as usize].iter().fold(0u32, |sum, byte| sum.wrapping_add(u32::from(*byte)));
        if checksum != !sum { return Ok(UploadStatus::ChecksumMismatch); }
        let operation = u32::from_le_bytes(workspace[..4].try_into().unwrap());
        let first = match operation { 1 => PREFIX_BLOCKS, 3 => 0, _ => return Ok(UploadStatus::InvalidImage) };
        if self.prepare().is_err() { return Ok(UploadStatus::StorageFailure); }
        let data = &workspace[0x200 + first * BLOCK_BYTES..0x200 + CONFIGURATION_BYTES];
        if self.write(first as u16, data).is_err() {
            let _ = self.prepare();
            return Ok(UploadStatus::StorageFailure);
        }
        workspace.fill(0xff);
        Ok(UploadStatus::Complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Flash { persisted: std::vec::Vec<u8>, first: Option<PhysicalBlock>, fail: bool }
    impl ConfigurationFlash for Flash {
        fn bad_blocks(&mut self, _: &mut [PhysicalBlock; 0x13b]) -> Result<usize, FlashStatus> { Ok(0) }
        fn read(&mut self, _: PhysicalBlock, output: &mut [u8; BLOCK_BYTES]) -> Result<(), FlashStatus> { output.fill(0xff); Ok(()) }
        fn erase(&mut self, _: PhysicalBlock, _: u16) -> Result<(), FlashStatus> { Ok(()) }
        fn program(&mut self, first: PhysicalBlock, input: &[u8], _: u16) -> Result<bool, FlashStatus> {
            if self.fail { return Err(FlashStatus(-255)); }
            self.first = Some(first);
            self.persisted.extend_from_slice(input);
            Ok(false)
        }
        fn delay(&mut self, _: u32) {}
        fn priority(&mut self, _: u8) {}
    }
    fn storage(fail: bool) -> ConfigurationStorage<Flash> {
        ConfigurationStorage::mount(Flash { persisted: std::vec::Vec::new(), first: None, fail }, ConfigurationIdentity::new([0; 20]), WritePolicy { erase_passes: 0, delay_seconds: 0, replacement_limit: 20 }).unwrap()
    }
    fn workspace(operation: u32) -> std::vec::Vec<u8> {
        let mut bytes = std::vec![0xff; WORKSPACE_BYTES];
        bytes[..0x200].fill(0);
        bytes[..4].copy_from_slice(&operation.to_le_bytes());
        bytes[0x14..0x18].copy_from_slice(&(CONFIGURATION_BYTES as u32).to_le_bytes());
        let sum = (CONFIGURATION_BYTES as u32).wrapping_mul(255);
        bytes[0x18..0x1c].copy_from_slice(&(!sum).to_le_bytes());
        bytes
    }
    #[test]
    fn partial_upload_preserves_the_prefix_and_consumes_the_workspace_on_success() {
        let mut storage = storage(false);
        let mut bytes = workspace(1);
        assert_eq!(storage.upload_fallback(&mut bytes, (CONFIGURATION_BYTES + 0x200) as u32), Ok(UploadStatus::Complete));
        assert_eq!(storage.flash().first, Some(PhysicalBlock(7)));
        assert_eq!(storage.flash().persisted.len(), (CONFIGURATION_BLOCKS - PREFIX_BLOCKS) * BLOCK_BYTES);
        assert!(bytes.iter().all(|byte| *byte == 0xff));
    }
    #[test]
    fn failed_write_retains_the_uploaded_image() {
        let mut storage = storage(true);
        let mut bytes = workspace(3);
        assert_eq!(storage.upload_fallback(&mut bytes, (CONFIGURATION_BYTES + 0x200) as u32), Ok(UploadStatus::StorageFailure));
        assert_eq!(&bytes[..4], &3u32.to_le_bytes());
        assert!(storage.flash().persisted.is_empty());
    }
    #[test]
    fn checksum_failure_never_persists_the_image() {
        let mut storage = storage(false);
        let mut bytes = workspace(3);
        bytes[0x200] = 0;
        assert_eq!(storage.upload_fallback(&mut bytes, (CONFIGURATION_BYTES + 0x200) as u32), Ok(UploadStatus::ChecksumMismatch));
        assert!(storage.flash().persisted.is_empty());
    }
    #[test]
    fn configuration_map_skips_bad_blocks_without_using_the_profile_map() {
        let map = ConfigurationMap::from_bad_blocks(&[PhysicalBlock(0), PhysicalBlock(2)]).unwrap();
        assert_eq!(map.physical(0), Ok(PhysicalBlock(1)));
        assert_eq!(map.physical(1), Ok(PhysicalBlock(3)));
        assert_eq!(map.physical(293), Ok(PhysicalBlock(295)));
        let exhausted = core::array::from_fn::<_, 21, _>(|index| PhysicalBlock(index as u16));
        assert!(matches!(ConfigurationMap::from_bad_blocks(&exhausted), Err(FlashStatus(-255))));
    }
}
