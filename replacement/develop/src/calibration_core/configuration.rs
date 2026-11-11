pub const CONFIGURATION_RAM_SIZE: usize = 0x420000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CfgKey(pub u32);

impl CfgKey {
    pub const fn wrapping_add(self, bytes: u32) -> Self {
        Self(self.0.wrapping_add(bytes))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ConfigAddress(pub u32);

impl ConfigAddress {
    pub const fn offset(self, key: CfgKey) -> Self {
        Self(self.0.wrapping_add(key.0))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationError<E> {
    Backend(E),
    RegionAddressOverflow { base: ConfigAddress, length: usize },
    OutOfBounds { address: ConfigAddress, length: usize },
    PhysicalBorrowUnavailable { address: ConfigAddress, length: usize },
}

/// Bounds-checked reads take `&mut self`.
pub trait ConfigurationMemory {
    type Error;
    fn read_byte(&mut self, address: ConfigAddress) -> Result<u8, ConfigurationError<Self::Error>>;
    fn read_half(&mut self, address: ConfigAddress) -> Result<u16, ConfigurationError<Self::Error>>;
    fn read_word(&mut self, address: ConfigAddress) -> Result<u32, ConfigurationError<Self::Error>>;
}

#[derive(Clone, Copy)]
enum ConfigurationBacking<'data> {
    Borrowed(&'data [u8]),
    #[cfg(target_arch = "arm")]
    Physical { address: ConfigAddress, length: usize },
}

#[derive(Clone, Copy)]
pub struct ConfigurationRegion<'data> {
    base: ConfigAddress,
    backing: ConfigurationBacking<'data>,
}

impl<'data> ConfigurationRegion<'data> {
    pub fn new(base: ConfigAddress, bytes: &'data [u8]) -> Result<Self, ConfigurationError<core::convert::Infallible>> {
        Self::validate_extent(base, bytes.len())?;
        Ok(Self { base, backing: ConfigurationBacking::Borrowed(bytes) })
    }

    fn validate_extent(base: ConfigAddress, length: usize) -> Result<(), ConfigurationError<core::convert::Infallible>> {
        match (base.0 as u64).checked_add(length as u64) {
            Some(end) if end <= (1_u64 << 32) => Ok(()),
            _ => Err(ConfigurationError::RegionAddressOverflow { base, length }),
        }
    }

    pub const fn base(&self) -> ConfigAddress { self.base }

    pub const fn len(&self) -> usize {
        match self.backing {
            ConfigurationBacking::Borrowed(bytes) => bytes.len(),
            #[cfg(target_arch = "arm")]
            ConfigurationBacking::Physical { length, .. } => length,
        }
    }

    pub const fn is_empty(&self) -> bool { self.len() == 0 }

    fn offset(&self, address: ConfigAddress, length: usize) -> Result<usize, ConfigurationError<core::convert::Infallible>> {
        let offset = address.0.checked_sub(self.base.0).map(|offset| offset as usize);
        match offset {
            Some(offset) if offset.checked_add(length).is_some_and(|end| end <= self.len()) => Ok(offset),
            _ => Err(ConfigurationError::OutOfBounds { address, length }),
        }
    }

    /// # Safety
    /// `offset` must be less than this region's length.
    unsafe fn byte_unchecked(&self, offset: usize) -> u8 {
        match self.backing {
            ConfigurationBacking::Borrowed(bytes) => unsafe { *bytes.get_unchecked(offset) },
            #[cfg(target_arch = "arm")]
            ConfigurationBacking::Physical { address, .. } => unsafe {
                core::ptr::read_volatile((address.0 as *const u8).add(offset))
            },
        }
    }

    /// # Safety
    /// `offset + 2` must not exceed this region's length.
    unsafe fn half_unchecked(&self, offset: usize) -> u16 {
        match self.backing {
            ConfigurationBacking::Borrowed(bytes) => decode_half(unsafe { *bytes.get_unchecked(offset) }, unsafe { *bytes.get_unchecked(offset + 1) }),
            #[cfg(target_arch = "arm")]
            ConfigurationBacking::Physical { address, .. } => {
                let value: u32;
                let at = address.0.wrapping_add(offset as u32);
                unsafe { core::arch::asm!("ldrh {value}, [{address}]", value = lateout(reg) value, address = in(reg) at, options(nostack, readonly, preserves_flags)) };
                value as u16
            }
        }
    }

    /// # Safety
    /// `offset + 4` must not exceed this region's length.
    unsafe fn word_unchecked(&self, offset: usize) -> u32 {
        match self.backing {
            ConfigurationBacking::Borrowed(bytes) => u32::from_le_bytes(unsafe {
                [*bytes.get_unchecked(offset), *bytes.get_unchecked(offset + 1), *bytes.get_unchecked(offset + 2), *bytes.get_unchecked(offset + 3)]
            }),
            #[cfg(target_arch = "arm")]
            ConfigurationBacking::Physical { address, .. } => {
                let value: u32;
                let at = address.0.wrapping_add(offset as u32);
                unsafe { core::arch::asm!("ldr {value}, [{address}]", value = lateout(reg) value, address = in(reg) at, options(nostack, readonly, preserves_flags)) };
                value
            }
        }
    }

    pub fn read_bytes(&mut self, address: ConfigAddress, length: usize) -> Result<&'data [u8], ConfigurationError<core::convert::Infallible>> {
        let offset = self.offset(address, length)?;
        match self.backing {
            ConfigurationBacking::Borrowed(bytes) => Ok(&bytes[offset..offset + length]),
            #[cfg(target_arch = "arm")]
            ConfigurationBacking::Physical { .. } => Err(ConfigurationError::PhysicalBorrowUnavailable { address, length }),
        }
    }
}

#[cfg(target_arch = "arm")]
impl ConfigurationRegion<'static> {
    /// # Safety
    /// The complete range must remain readable physical RAM for the region's lifetime.
    pub(crate) unsafe fn physical(base: ConfigAddress, length: usize) -> Result<Self, ConfigurationError<core::convert::Infallible>> {
        Self::validate_extent(base, length)?;
        Ok(Self { base, backing: ConfigurationBacking::Physical { address: base, length } })
    }
}

impl ConfigurationMemory for ConfigurationRegion<'_> {
    type Error = core::convert::Infallible;

    fn read_byte(&mut self, address: ConfigAddress) -> Result<u8, ConfigurationError<Self::Error>> {
        let offset = self.offset(address, 1)?;
        Ok(unsafe { self.byte_unchecked(offset) })
    }

    fn read_half(&mut self, address: ConfigAddress) -> Result<u16, ConfigurationError<Self::Error>> {
        let offset = self.offset(address, 2)?;
        Ok(unsafe { self.half_unchecked(offset) })
    }

    fn read_word(&mut self, address: ConfigAddress) -> Result<u32, ConfigurationError<Self::Error>> {
        let offset = self.offset(address, 4)?;
        Ok(unsafe { self.word_unchecked(offset) })
    }
}

pub struct Configuration<'data, M: ConfigurationMemory = ConfigurationRegion<'data>> {
    memory: M,
    blob: ConfigAddress,
    borrowed: core::marker::PhantomData<&'data ()>,
}

impl<'data, M: ConfigurationMemory> Configuration<'data, M> {
    pub const fn new(memory: M, blob: ConfigAddress) -> Self { Self { memory, blob, borrowed: core::marker::PhantomData } }

    pub const fn memory(&self) -> &M { &self.memory }
    pub fn memory_mut(&mut self) -> &mut M { &mut self.memory }
    pub const fn blob_address(&self) -> ConfigAddress { self.blob }

    pub const fn pointer(&self, key: CfgKey) -> ConfigAddress { self.blob.offset(key) }

    pub fn read_byte(&mut self, key: CfgKey) -> Result<u8, ConfigurationError<M::Error>> {
        self.memory.read_byte(self.blob.offset(key))
    }

    pub fn read_half(&mut self, key: CfgKey) -> Result<u16, ConfigurationError<M::Error>> {
        let low = self.read_byte(key)?;
        let high = self.read_byte(key.wrapping_add(1))?;
        Ok(decode_half(low, high))
    }

    pub fn read_word(&mut self, key: CfgKey) -> Result<u32, ConfigurationError<M::Error>> {
        self.memory.read_word(self.blob.offset(key))
    }

    pub fn read_signed_byte(&mut self, key: CfgKey) -> Result<i8, ConfigurationError<M::Error>> {
        self.read_byte(key).map(|value| value as i8)
    }

    pub fn read_signed_half(&mut self, key: CfgKey) -> Result<i16, ConfigurationError<M::Error>> {
        self.read_half(key).map(|value| value as i16)
    }
}

pub const fn decode_half(low: u8, high: u8) -> u16 {
    u16::from_le_bytes([low, high])
}

#[cfg(test)]
mod tests {
    use super::{CfgKey, ConfigAddress, Configuration, ConfigurationError, ConfigurationMemory, ConfigurationRegion};
    use core::convert::Infallible;

    #[test]
    fn reads_supplied_configuration_values_and_signed_bits() {
        let bytes = [0x34, 0xf2, 0x7f, 0x80, 0x01];
        let mut configuration = Configuration::new(ConfigurationRegion::new(ConfigAddress(0x100), &bytes).unwrap(), ConfigAddress(0x100));
        assert_eq!(configuration.read_byte(CfgKey(0)), Ok(0x34));
        assert_eq!(configuration.read_half(CfgKey(1)), Ok(0x7ff2));
        assert_eq!(configuration.read_word(CfgKey(0)), Ok(0x807ff234));
        assert_eq!(configuration.read_signed_byte(CfgKey(3)), Ok(-128));
        assert_eq!(configuration.read_signed_half(CfgKey(2)), Ok(-32641));
        assert_eq!(configuration.pointer(CfgKey(3)), ConfigAddress(0x103));
    }

    #[test]
    fn reads_past_the_supplied_region_are_refused() {
        let bytes = [0x5a];
        let mut configuration = Configuration::new(ConfigurationRegion::new(ConfigAddress(0), &bytes).unwrap(), ConfigAddress(0));
        assert_eq!(configuration.read_half(CfgKey(0)), Err(ConfigurationError::OutOfBounds { address: ConfigAddress(1), length: 1 }));
        assert_eq!(configuration.read_word(CfgKey(0)), Err(ConfigurationError::OutOfBounds { address: ConfigAddress(0), length: 4 }));
    }

    #[test]
    fn a_region_that_wraps_the_address_space_is_refused() {
        let bytes = [0u8; 4];
        assert_eq!(
            ConfigurationRegion::new(ConfigAddress(0xffff_fffe), &bytes).err(),
            Some(ConfigurationError::RegionAddressOverflow { base: ConfigAddress(0xffff_fffe), length: 4 }),
        );
    }

    struct Failing;
    impl ConfigurationMemory for Failing {
        type Error = u8;
        fn read_byte(&mut self, _address: ConfigAddress) -> Result<u8, ConfigurationError<u8>> { Err(ConfigurationError::Backend(7)) }
        fn read_half(&mut self, _address: ConfigAddress) -> Result<u16, ConfigurationError<u8>> { Err(ConfigurationError::Backend(7)) }
        fn read_word(&mut self, _address: ConfigAddress) -> Result<u32, ConfigurationError<u8>> { Err(ConfigurationError::Backend(7)) }
    }

    #[test]
    fn a_backend_error_reaches_the_caller_unchanged() {
        let mut configuration = Configuration::<Failing>::new(Failing, ConfigAddress(0));
        assert_eq!(configuration.read_byte(CfgKey(0)), Err(ConfigurationError::Backend(7)));
        assert_eq!(configuration.read_half(CfgKey(0)), Err(ConfigurationError::Backend(7)));
        let borrowed: Result<u8, ConfigurationError<Infallible>> = ConfigurationRegion::new(ConfigAddress(0), &[1]).unwrap().read_byte(ConfigAddress(0));
        assert_eq!(borrowed, Ok(1));
    }
}
