use crate::{backend::{BackendError, ConfigImages}, configuration_actions_low::ConfigurationKey};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileId(u8);
impl ProfileId {
    pub fn parse(value: u32) -> Result<Self, BackendError> { if value < 36 { Ok(Self(value as u8)) } else { Err(BackendError::Range) } }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameBounds { pub top: u32, pub left: u32, pub height: u32, pub width: u32 }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameGeometry { pub size: u32, pub pitch: u32, pub width: u32, pub height: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat { Unpacked16, Packed12 }
impl PixelFormat { pub const fn from_configuration(value: u8) -> Self { if value == 1 { Self::Packed12 } else { Self::Unpacked16 } } }

pub struct ProfileTables<'a> { entries: &'a [u8; 720], classes: &'a [u8; 816], configured: &'a [u8; 272], expanded: &'a [u8; 272] }
impl<'a> ProfileTables<'a> {
    pub fn adopt(entries: &'a [u8; 720], classes: &'a [u8; 816], configured: &'a [u8; 272], expanded: &'a [u8; 272]) -> Result<Self, BackendError> {
        let tables = Self { entries, classes, configured, expanded };
        for index in 0..36 { if tables.word(tables.entry(ProfileId(index)), 4) >= 17 { return Err(BackendError::Range); } }
        Ok(tables)
    }
    fn word(&self, bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) }
    fn half(&self, bytes: &[u8], offset: usize) -> u32 { u32::from(u16::from_le_bytes([bytes[offset], bytes[offset + 1]])) }
    fn entry(&self, profile: ProfileId) -> &[u8] { let start = usize::from(profile.0) * 20; &self.entries[start..start + 20] }
    fn classification(&self, profile: ProfileId) -> usize { self.word(self.entry(profile), 4) as usize }
    fn class(&self, profile: ProfileId) -> &[u8] { let start = self.classification(profile) * 48; &self.classes[start..start + 48] }
    pub fn valid(&self, profile: ProfileId) -> bool { self.word(self.entry(profile), 0) != 0 }
    pub fn kind(&self, profile: ProfileId) -> u8 { self.entry(profile)[8] }
    pub fn attributes(&self, profile: ProfileId) -> [u8; 2] { let class = self.class(profile); [class[0x29], class[0x2a]] }
    pub fn first_flag(&self, profile: ProfileId) -> u8 { self.class(profile)[0x26] }
    pub fn reduction(&self, profile: ProfileId) -> u8 { self.class(profile)[0x28] }
    pub fn extended(&self, profile: ProfileId) -> bool { (9..16).contains(&self.classification(profile)) }
    pub fn full(&self, profile: ProfileId) -> FrameBounds { let class = self.class(profile); FrameBounds { top: 0, left: 0, height: self.half(class, 2), width: self.half(class, 0) } }
    pub fn first_multiplier(&self, profile: ProfileId) -> u32 { if self.first_flag(profile) != 0 { 2 } else { match self.reduction(profile) { 1 => 2, 2 => 4, _ => 1 } } }
    pub fn second_multiplier(&self, profile: ProfileId) -> u32 { if self.class(profile)[0x27] != 0 { 2 } else { 1 } }
    fn keyed_bounds(&self, profile: ProfileId, bytes: &[u8], configuration: &ConfigImages<'_>) -> Result<FrameBounds, BackendError> {
        let row = self.classification(profile) * 16;
        let read = |offset| -> Result<u32, BackendError> {
            let key = self.word(bytes, row + offset);
            if key == 0 { return Ok(1); }
            let high = configuration.byte(key).ok_or(BackendError::Capacity)?;
            let low = configuration.byte(key.wrapping_add(1)).ok_or(BackendError::Capacity)?;
            Ok(u32::from(u16::from_be_bytes([high, low])))
        };
        Ok(FrameBounds { top: read(0)?, left: read(4)?, height: read(8)?, width: read(12)? })
    }
    pub fn configured(&self, profile: ProfileId, configuration: &ConfigImages<'_>) -> Result<FrameBounds, BackendError> { self.keyed_bounds(profile, self.configured, configuration) }
    pub fn relative(&self, profile: ProfileId, relative: bool, configuration: &ConfigImages<'_>) -> Result<FrameBounds, BackendError> {
        let mut bounds = self.keyed_bounds(profile, self.expanded, configuration)?;
        if relative {
            let configured = self.configured(profile, configuration)?;
            let first = self.first_multiplier(profile); let second = self.second_multiplier(profile);
            bounds.top = bounds.top.saturating_sub(configured.top) / first;
            bounds.left = bounds.left.saturating_sub(configured.left) / second;
            bounds.width /= first; bounds.height /= second;
        }
        Ok(bounds)
    }
    pub fn geometry(&self, profile: ProfileId, format: PixelFormat, configuration: &ConfigImages<'_>) -> Result<FrameGeometry, BackendError> {
        if !self.valid(profile) { return Err(BackendError::Range); }
        let mut bounds = self.full(profile);
        let crop = ConfigurationKey::from_abi(0xc6).read(configuration)?;
        if self.kind(profile) != 0 && crop != 1 {
            let mut configured = self.configured(profile, configuration)?;
            let shift = if self.first_flag(profile) != 0 { 1 } else { match self.reduction(profile) { 1 => 1, 2 => 2, _ => 0 } };
            configured.top >>= shift; configured.width >>= shift;
            if crop == 0 { bounds.width = configured.top.wrapping_add(configured.width); }
            if crop == 2 {
                bounds.height = if self.extended(profile) { configured.left.wrapping_sub(1) } else { bounds.height.saturating_sub(1) };
                bounds.width = configured.width;
            }
        }
        Ok(FrameGeometry::from_bounds(bounds, format))
    }
}
impl FrameGeometry {
    pub fn from_bounds(bounds: FrameBounds, format: PixelFormat) -> Self {
        let pitch = match format { PixelFormat::Packed12 => (bounds.width.wrapping_add(255) & 0xff00).wrapping_mul(3) >> 1, PixelFormat::Unpacked16 => (bounds.width.wrapping_add(63) & 0xffc0) << 1 };
        Self { size: bounds.height.wrapping_mul(pitch), pitch, width: bounds.width, height: bounds.height }
    }
}
#[cfg(target_arch = "arm")]
pub unsafe fn native_tables() -> Result<ProfileTables<'static>, BackendError> {
    ProfileTables::adopt(unsafe { &*(0x00ef_6de4 as *const [u8; 720]) }, unsafe { &*(0x00ef_6aa4 as *const [u8; 816]) }, unsafe { &*(0x00ef_6994 as *const [u8; 272]) }, unsafe { &*(0x00ef_71bc as *const [u8; 272]) })
}
