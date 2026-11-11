#![no_std]

pub mod publication;
#[cfg(all(target_arch = "arm", not(feature = "library")))]
mod abi;

pub const CACHE_SIZE: usize = 0x420000;
pub const FIXED_SIZE: usize = 0x42000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationOffset(usize);
impl CalibrationOffset {
    pub const fn parse(value: usize) -> Option<Self> {
        if value < CACHE_SIZE { Some(Self(value)) } else { None }
    }
    pub const fn byte_offset(self) -> usize { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationValue(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignedCalibrationValue(pub i32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationByte(pub u8);

pub struct InitializedCalibrationBase(core::ptr::NonNull<u8>);
impl InitializedCalibrationBase {
    /// # Safety
    /// The base addresses an initialized, readable CACHE_SIZE-byte calibration cache for every read through this value.
    pub unsafe fn from_initialized_base(base: core::ptr::NonNull<u8>) -> Self { Self(base) }
    pub fn read_byte(&self, offset: CalibrationOffset) -> CalibrationByte {
        CalibrationByte(unsafe { core::ptr::read_volatile(self.0.as_ptr().add(offset.byte_offset())) })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadPath { Primary, Fixed, Defaults }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReloadWindow { Page(CalibrationPage), Fixed, StampOnly }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationPage(u8);
impl ReloadWindow {
    pub fn parse(value: u32) -> Self {
        match value { 0..=31 => Self::Page(CalibrationPage(value as u8)), 255 => Self::Fixed, _ => Self::StampOnly }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServicePersonality(u8);
impl ServicePersonality {
    pub fn enabled(self) -> bool { self.0 != 0 }
    pub fn is_one(self) -> bool { self.0 == 1 }
    pub fn is_two(self) -> bool { self.0 == 2 }
    pub fn raw(self) -> u8 { self.0 }
}

pub trait CalibrationStorage {
    fn read_primary(&mut self, destination: &mut [u8]);
    fn read_backup(&mut self, destination: &mut [u8]);
    fn read_fixed(&mut self, offset: CalibrationOffset, destination: &mut [u8]);
}

pub struct Defaults<'a>(&'a [u8]);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceError { MissingSentinel, InvalidLength }
impl<'a> Defaults<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ResourceError> {
        if bytes.len() % 4 != 0 { return Err(ResourceError::InvalidLength); }
        if !bytes.chunks_exact(4).take(CACHE_SIZE).any(|row| u16::from_le_bytes([row[0], row[1]]) == 0xffff) {
            return Err(ResourceError::MissingSentinel);
        }
        Ok(Self(bytes))
    }
}

pub struct Calibration<'a> { bytes: &'a mut [u8; CACHE_SIZE] }
impl<'a> Calibration<'a> {
    pub fn borrow(bytes: &'a mut [u8; CACHE_SIZE]) -> Self { Self { bytes } }
    pub fn initialize(&mut self, storage: &mut impl CalibrationStorage, defaults: &Defaults<'_>) -> LoadPath {
        storage.read_primary(&mut self.bytes[..0x76800]);
        storage.read_backup(&mut self.bytes[0x400000..0x40fd00]);
        let path = if self.signature() { LoadPath::Primary } else {
            self.bytes.fill(0);
            storage.read_fixed(CalibrationOffset(0), &mut self.bytes[..FIXED_SIZE]);
            if self.signature() { LoadPath::Fixed } else {
                self.load_defaults(defaults);
                return LoadPath::Defaults;
            }
        };
        self.stamp();
        for (offset, value) in [(0x4069,2),(0x406a,0x50),(0x406b,2),(0x406c,0x40),(0x406d,2),(0x406e,0x50),(0xd8,0x80)] {
            self.bytes[offset] = value;
        }
        path
    }
    pub fn load_defaults(&mut self, defaults: &Defaults<'_>) {
        self.bytes.fill(0);
        for row in defaults.0.chunks_exact(4).take(CACHE_SIZE) {
            let offset = u16::from_le_bytes([row[0], row[1]]);
            if offset == 0xffff { break; }
            self.bytes[offset as usize] = row[2];
        }
        self.stamp();
    }
    fn signature(&self) -> bool { self.bytes[0x202..0x204] == [0x55,0xaa] }
    fn stamp(&mut self) { self.bytes[0x204] = 1; self.bytes[0x205] = 0x78; }
    pub fn reload(&mut self, storage: &mut impl CalibrationStorage, window: ReloadWindow) {
        match window {
            ReloadWindow::Page(page) => {
                let offset = usize::from(page.0) * 256;
                storage.read_fixed(CalibrationOffset(offset), &mut self.bytes[offset..offset+256]);
            }
            ReloadWindow::Fixed => {
                storage.read_fixed(CalibrationOffset(0), &mut self.bytes[..FIXED_SIZE]);
                self.bytes[0x1e8] = 1; self.bytes[0x1e9] = 0x78;
            }
            ReloadWindow::StampOnly => {}
        }
        self.stamp();
    }
    pub fn read(&self, offset: CalibrationOffset, width: Width) -> Option<CalibrationValue> {
        let source = self.bytes.get(offset.0..offset.0.checked_add(width.bytes())?)?;
        let mut result = 0;
        for (shift, byte) in source.iter().enumerate() { result |= u32::from(*byte) << (shift * 8); }
        Some(CalibrationValue(result))
    }
    pub fn signed_byte(&self, offset: CalibrationOffset) -> SignedCalibrationValue { SignedCalibrationValue(i32::from(self.bytes[offset.0] as i8)) }
    pub fn signed_halfword(&self, offset: CalibrationOffset) -> Option<SignedCalibrationValue> {
        self.read(offset, Width::Halfword).map(|v| SignedCalibrationValue(i32::from(v.0 as i16)))
    }
    pub fn byte_extended_halfword(&self, offset: CalibrationOffset) -> SignedCalibrationValue { SignedCalibrationValue(i32::from(self.bytes[offset.0])) }
    pub fn set_byte(&mut self, offset: CalibrationOffset, value: CalibrationByte) { self.bytes[offset.0] = value.0; }
    pub fn personality(&self) -> ServicePersonality { ServicePersonality(self.bytes[0xf7]) }
    pub fn bytes(&self) -> &[u8] { self.bytes }
    pub fn region_tail(&self, region: CalibrationRegion) -> &[u8] { &self.bytes[region.offset()..] }
    pub fn copy(&self, offset: CalibrationOffset, destination: &mut [u8]) -> Result<(), ReadError> {
        let end = offset.0.checked_add(destination.len()).ok_or(ReadError::OutsideCache)?;
        let source = self.bytes.get(offset.0..end).ok_or(ReadError::OutsideCache)?;
        destination.copy_from_slice(source);
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width { Byte, Halfword, ThreeBytes, Word }
impl Width { pub const fn bytes(self) -> usize { match self { Self::Byte => 1, Self::Halfword => 2, Self::ThreeBytes => 3, Self::Word => 4 } } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError { OutsideCache }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibrationRegion { First, Second, Third, Fourth, Fifth, Sixth, Seventh, Eighth, Default, Resource }
impl CalibrationRegion {
    pub fn parse(value: u32) -> Self {
        match value { 1 => Self::First, 2 => Self::Second, 3 => Self::Third, 4 => Self::Fourth, 5 => Self::Fifth, 6 => Self::Sixth, 7 => Self::Seventh, 8 => Self::Eighth, _ => Self::Default }
    }
    pub const fn offset(self) -> usize {
        match self { Self::First => 0x11c840, Self::Second => 0x2bb8e0, Self::Third => 0x3b4940, Self::Fourth => 0x3b8ba8, Self::Fifth => 0x3c31b0, Self::Sixth => 0x76500, Self::Seventh => 0x76600, Self::Eighth => 0x76700, Self::Default => 0x76800, Self::Resource => 0x51000 }
    }
}

#[cfg(all(target_arch = "arm", not(feature = "library")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop { core::hint::spin_loop(); } }

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
