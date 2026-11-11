//! Sparse device-info producers at 0x00f201f8 and 0x00748928, X-M1 1.50.

use xm1_calibration_core::{BiasBank, ChannelIndex, CurveIndex};
use xm1_calibration_core::configuration::CfgKey;
use super::commands::{ByteCount, DebugAddress, PreparedPayload, ProviderError, PublishedRegion};
use super::metadata_calibration::{LimitBank, LimitKind, LimitSide, MetadataCalibration, ModeClass, RegionError};
use super::metadata_selection::{CalibrationChannel, LensSelection, adjust_selection};

pub const DEVICE_INFO_LENGTH: ByteCount = ByteCount(0xe297c);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensPosition(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationMode { Ordinary, Other, Bypass }

impl PublicationMode {
    pub const fn parse(value: u32) -> Self {
        match value { 0 => Self::Ordinary, 6 => Self::Bypass, _ => Self::Other }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileHandle(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileRecordIndex(pub u32);

pub struct ProfileSelectionState<'data> { bytes: &'data [u8; 0x390] }
impl<'data> ProfileSelectionState<'data> {
    pub const fn new(bytes: &'data [u8; 0x390]) -> Self { Self { bytes } }
    pub fn record_index(&self, handle: ProfileHandle) -> ProfileRecordIndex {
        let fallback = load(self.bytes, 8);
        let slot = (handle.0 & 15) as usize * 56;
        let generation = (handle.0 >> 4) & 0xfff;
        let category = (handle.0 >> 18) & 63;
        let valid = handle.0 != u32::MAX
            && generation == load(self.bytes, slot + 12)
            && (category == 0 || category == 63)
            && self.bytes[slot + 24] & 1 != 0;
        let selected = if valid { load(self.bytes, slot + 20) } else { fallback };
        ProfileRecordIndex(if selected <= 1 { selected } else { fallback })
    }
}

pub struct ProfileRecord<'data> { bytes: &'data [u8; 0x2158] }
impl<'data> ProfileRecord<'data> {
    pub const fn new(bytes: &'data [u8; 0x2158]) -> Self { Self { bytes } }
    pub fn resolve(records: &'data [[u8; 0x2158]], state: &ProfileSelectionState<'_>, handle: ProfileHandle) -> Result<Self, MetadataError> {
        let index = state.record_index(handle);
        let bytes = records.get(index.0 as usize).ok_or(MetadataError::ProfileRecordUnavailable { index })?;
        Ok(Self::new(bytes))
    }
    pub fn first_kind(&self) -> u8 { self.bytes[0x47] }
    pub fn second_kind(&self) -> u8 { self.bytes[0x46] }
}

pub struct DeviceInfoSnapshot<'data> {
    pub profile: ProfileRecord<'data>,
    pub channel: CalibrationChannel,
    pub lens_selection: LensSelection,
    pub lens_position: LensPosition,
    pub mode: PublicationMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataError {
    DestinationTooSmall { required: ByteCount },
    Configuration(RegionError),
    Publication(ProviderError),
    ProfileRecordUnavailable { index: ProfileRecordIndex },
    Bypassed,
}
impl From<RegionError> for MetadataError {
    fn from(error: RegionError) -> Self { Self::Configuration(error) }
}

fn word(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn half(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn load(output: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([output[offset], output[offset + 1], output[offset + 2], output[offset + 3]])
}

pub fn produce_calibration(calibration: &mut MetadataCalibration<'_>, output: &mut [u8], channel: CalibrationChannel, selection: LensSelection) -> Result<(), MetadataError> {
    if output.len() < DEVICE_INFO_LENGTH.0 as usize {
        return Err(MetadataError::DestinationTooSmall { required: DEVICE_INFO_LENGTH });
    }
    let channel_index = ChannelIndex(channel.get() as i32);
    let split = calibration.profile().split_count()?.get();
    word(output, 0xb174c, 0x53544c45);
    word(output, 0xb19f0, 0x4c414c45);
    for index in 0..split {
        let curve = CurveIndex(index);
        half(output, 0xb1750 + index as usize * 2, calibration.rounded_word(curve, channel_index)? as u16);
        half(output, 0xb1790 + index as usize * 2, calibration.mapped_value(curve, channel_index)?.0);
        half(output, 0xb17d8 + index as usize * 2, calibration.transformed_value(curve, channel_index)? as u16);
        half(output, 0xb1818 + index as usize * 2, calibration.profile().transform_mapped_word(curve, channel_index)?.0);
        half(output, 0xb18bc + index as usize * 2, calibration.scaled_mapped_word(curve, channel_index)?.0);
    }
    for offset in [0xb1750, 0xb1790, 0xb17d8, 0xb1818, 0xb18bc] {
        half(output, offset + split as usize * 2, 0xfdfd);
    }
    let offset = channel.get() as u32 * 2;
    for (destination, key) in [(0xb187c, 0x3e07cd), (0xb1880, 0x3e07e2), (0xb188c, 0x3e0989)] {
        let value = if channel.get() < 10 { calibration.configuration().read_half(CfgKey(key + offset))? as u32 } else { 0 };
        word(output, destination, value);
    }
    let signed_key = CfgKey(0x3f3d14 + channel.get() as u32 * 22);
    word(output, 0xb17d0, calibration.configuration().read_signed_half(signed_key)? as i32 as u32);
    word(output, 0xb17d4, calibration.configuration().read_signed_half(signed_key)? as i32 as u32);
    word(output, 0xb1858, calibration.configuration().read_half(CfgKey(0x3e0240))? as u32);
    word(output, 0xb185c, calibration.configuration().read_half(CfgKey(0x3e0243))? as u32);
    word(output, 0xb1860, calibration.configuration().read_byte(CfgKey(0x3e0246))? as u32);
    for (destination, key) in [(0xb1864, 0x3e0317), (0xb1868, 0x3e03e0), (0xb186c, 0x3e04a9), (0xb1890, 0x3e09a2), (0xb1894, 0x3e0a6b)] {
        word(output, destination, calibration.configuration().read_signed_half(CfgKey(key + offset))? as i32 as u32);
    }
    for (destination, key) in [(0xb1870, 0x3e0572), (0xb1874, 0x3e063b), (0xb1878, 0x3e0704), (0xb1884, 0x3e07f7), (0xb1888, 0x3e08c0), (0xb18a8, 0x3e0b40), (0xb18ac, 0x3e0c09), (0xb18b0, 0x3e0cd2), (0xb18b4, 0x3e0d9b)] {
        word(output, destination, calibration.configuration().read_half(CfgKey(key + offset))? as u32);
    }
    word(output, 0xb1898, calibration.scaled_limit(CfgKey(0x3e15a6))?);
    word(output, 0xb189c, calibration.scaled_limit(CfgKey(0x3e15a8))?);
    word(output, 0xb18a0, calibration.configuration().read_half(CfgKey(0x3e2640))? as u32);
    word(output, 0xb18a4, calibration.configuration().read_half(CfgKey(0x3e0fdd))? as u32);
    word(output, 0xb18b8, calibration.profile().scaled_channel_curve(channel_index)?.0 as u32);
    let requests = [
        (LimitBank::First, LimitSide::Other, ModeClass::Zero),
        (LimitBank::Second, LimitSide::Other, ModeClass::Zero),
        (LimitBank::First, LimitSide::One, ModeClass::One),
        (LimitBank::First, LimitSide::Other, ModeClass::One),
        (LimitBank::Second, LimitSide::One, ModeClass::One),
        (LimitBank::Second, LimitSide::Other, ModeClass::One),
        (LimitBank::First, LimitSide::One, ModeClass::Two),
        (LimitBank::First, LimitSide::Other, ModeClass::Two),
        (LimitBank::Second, LimitSide::One, ModeClass::Two),
        (LimitBank::Second, LimitSide::Other, ModeClass::Two),
    ];
    for (group, kind) in [LimitKind::OptionalPacked, LimitKind::OptionalWord, LimitKind::OptionalScaled, LimitKind::Packed, LimitKind::Scaled].into_iter().enumerate() {
        for (index, (bank, side, mode)) in requests.iter().copied().enumerate() {
            word(output, 0xb18fc + group * 40 + index * 4, calibration.mode_limit(kind, bank, side, mode)?);
        }
    }
    let first = load(output, 0xb1864) as u16;
    let adjusted = adjust_selection(calibration.profile(), channel, BiasBank::Primary, selection, first as i16)?;
    word(output, 0xb19c4, adjusted as i32 as u32);
    let adjusted = adjust_selection(calibration.profile(), channel, BiasBank::Primary, selection, load(output, 0xb1868) as i16)?;
    word(output, 0xb19c8, adjusted as i32 as u32);
    for (destination, source) in [(0xb19cc, 0xb186c), (0xb19d0, 0xb1870), (0xb19d4, 0xb1874), (0xb19d8, 0xb1878), (0xb19e4, 0xb1884), (0xb19e8, 0xb1888)] {
        let value = (load(output, source) as u16).wrapping_add(first) as i16;
        let adjusted = adjust_selection(calibration.profile(), channel, BiasBank::Primary, selection, value)?;
        word(output, destination, adjusted as i32 as u32);
    }
    for (destination, source) in [(0xb19dc, 0xb187c), (0xb19e0, 0xb1880), (0xb19ec, 0xb188c)] {
        let adjusted = if channel.get() < 10 {
            let value = (load(output, source) as u16).wrapping_add(first) as i16;
            adjust_selection(calibration.profile(), channel, BiasBank::Primary, selection, value)? as i32 as u32
        } else { 0 };
        word(output, destination, adjusted);
    }
    Ok(())
}

pub fn prepare_device_info<'output>(calibration: &mut MetadataCalibration<'_>, output: &'output mut [u8], address: DebugAddress, snapshot: &DeviceInfoSnapshot<'_>) -> Result<PreparedPayload<'output>, MetadataError> {
    if snapshot.mode == PublicationMode::Bypass { return Err(MetadataError::Bypassed); }
    if address.0 == 0 { return Err(MetadataError::Publication(ProviderError::Unavailable)); }
    if output.len() < DEVICE_INFO_LENGTH.0 as usize {
        return Err(MetadataError::DestinationTooSmall { required: DEVICE_INFO_LENGTH });
    }
    address.0.checked_add(DEVICE_INFO_LENGTH.0).ok_or(MetadataError::Publication(ProviderError::Capacity))?;
    produce_calibration(calibration, output, snapshot.channel, snapshot.lens_selection)?;
    word(output, 0, 0x5354434d);
    word(output, 0x5c, snapshot.profile.first_kind() as u32);
    word(output, 0x60, snapshot.profile.second_kind() as u32);
    word(output, 0x7c, snapshot.lens_position.0 as u32);
    word(output, 0x88, snapshot.channel.get() as u32);
    word(output, 0x110, 0x4c41434d);
    if snapshot.profile.first_kind() != 0 || snapshot.mode != PublicationMode::Ordinary {
        word(output, 0x116cc, 0);
    }
    let region = PublishedRegion::adopt(address.0, &output[..DEVICE_INFO_LENGTH.0 as usize]).map_err(MetadataError::Publication)?;
    Ok(PreparedPayload::from_region(region))
}
