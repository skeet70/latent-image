//! X-M1 roots 6c17fc, 6c1848, 6c0948, 6c096c and 6c098c at pin 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey, CpuReturn};

extern "C" {
    fn xm1_app_nand_configure(request: *const [u32; 2]) -> i32;
    fn xm1_app_nand_erase(request: *mut [u32; 5]) -> i32;
    fn xm1_app_nand_shutdown();
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfilePhysicalBlock(u16);
impl ProfilePhysicalBlock {
    pub fn from_abi(block: u32) -> Result<Self, BackendError> {
        if (0x380..0x3ec).contains(&block) { Ok(Self(block as u16)) } else { Err(BackendError::Range) }
    }
    const fn logical(self) -> u32 { self.0 as u32 - 0x380 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileEraseMap { header: ProfilePhysicalBlock, entries: ProfilePhysicalBlock }
impl ProfileEraseMap {
    pub fn from_partition_words(header: ProfilePhysicalBlock, entries: ProfilePhysicalBlock) -> Result<Self, BackendError> {
        if entries.logical() + 40 > 0x6c { return Err(BackendError::Range); }
        Ok(Self { header, entries })
    }
}

pub struct EepromMemory<'a> { context: &'a mut [u8; 0xc0], cache: &'a mut [u8; 0xa08] }
impl<'a> EepromMemory<'a> {
    pub fn adopt(context: &'a mut [u8; 0xc0], cache: &'a mut [u8; 0xa08]) -> Self { Self { context, cache } }

    pub fn fallback_state(&self) -> CpuReturn {
        CpuReturn::from_abi(i32::from_le_bytes(self.context[0x3c..0x40].try_into().unwrap()))
    }

    pub fn begin_reset(&mut self) {
        self.context[0x3c..0x40].copy_from_slice(&1u32.to_le_bytes());
        self.cache.fill(0);
        self.cache[4..8].copy_from_slice(&0xabc1_23f9u32.to_le_bytes());
    }

    /// # Safety
    /// The canonical application NAND module, profile map, owned OSAL and semaphores 36/52 are initialized. Calls run in task context with device-memory NAND MMIO. These borrows name the canonical task100 context and profile cache.
    pub unsafe fn clear_profiles(&mut self, map: ProfileEraseMap) -> CpuReturn {
        self.begin_reset();
        if unsafe { erase(map.header.logical(), 1) } != 0 { return CpuReturn::REJECTED; }
        for index in 0..20 {
            if unsafe { erase(map.entries.logical() + index * 2, 2) } != 0 { return CpuReturn::REJECTED; }
        }
        CpuReturn::SUCCESS
    }

    /// # Safety
    /// The `clear_profiles` ownership contract holds.
    pub unsafe fn action110(&mut self, map: ProfileEraseMap, configuration: &mut ConfigImages<'_>) -> Result<ActionCompletion, BackendError> {
        let result = unsafe { self.clear_profiles(map) };
        ConfigurationKey::from_abi(0x81).write(configuration, u8::from(result != CpuReturn::SUCCESS))?;
        Ok(if result == CpuReturn::SUCCESS { ActionCompletion::service_success() } else { ActionCompletion::service_rejected() })
    }
}

unsafe fn erase(first: u32, count: u32) -> i32 {
    let configuration = [0, 1000];
    let status = unsafe { xm1_app_nand_configure(&configuration) };
    if status != 0 { return status; }
    let mut request = [first, count, 0, 1000, 0];
    let status = unsafe { xm1_app_nand_erase(&mut request) };
    unsafe { xm1_app_nand_shutdown() };
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_reset_publishes_fallback_and_reinitializes_the_existing_cache() {
        let mut context = [0xa5; 0xc0];
        let mut cache = [0x5a; 0xa08];
        let mut memory = EepromMemory::adopt(&mut context, &mut cache);
        memory.begin_reset();
        assert_eq!(memory.fallback_state().get(), 1);
        assert_eq!(&context[..0x3c], &[0xa5; 0x3c]);
        assert_eq!(&context[0x40..], &[0xa5; 0x80]);
        assert_eq!(&cache[..4], &[0; 4]);
        assert_eq!(&cache[4..8], &0xabc1_23f9u32.to_le_bytes());
        assert!(cache[8..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn fallback_query_returns_the_existing_word_without_boolean_normalization() {
        let mut context = [0; 0xc0];
        context[0x3c..0x40].copy_from_slice(&(-17i32).to_le_bytes());
        let mut cache = [0; 0xa08];
        assert_eq!(EepromMemory::adopt(&mut context, &mut cache).fallback_state().get(), -17);
    }

    #[test]
    fn erase_windows_are_bounded_by_the_owned_profile_map() {
        assert_eq!(ProfilePhysicalBlock::from_abi(0x37f), Err(BackendError::Range));
        assert_eq!(ProfilePhysicalBlock::from_abi(0x3ec), Err(BackendError::Range));
        let header = ProfilePhysicalBlock::from_abi(0x380).unwrap();
        let last_window = ProfilePhysicalBlock::from_abi(0x3c4).unwrap();
        assert!(ProfileEraseMap::from_partition_words(header, last_window).is_ok());
        let overflow = ProfilePhysicalBlock::from_abi(0x3c5).unwrap();
        assert_eq!(ProfileEraseMap::from_partition_words(header, overflow), Err(BackendError::Range));
    }
}
