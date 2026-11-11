//! X-M1 1.50 scan-bound state at 0x016caf00, pinned x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::{helper_69f8fc, helper_69fec8, helper_69ff68};

#[derive(Clone, Copy)]
pub(crate) struct SettingKey(pub u32);

pub(crate) fn byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: SettingKey) -> Result<u32, MemoryError> { helper_69f8fc(context, key.0) }
pub(crate) fn half<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: SettingKey) -> Result<u32, MemoryError> { helper_69fec8(context, key.0) }
pub(crate) fn signed_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: SettingKey) -> Result<i32, MemoryError> { helper_69ff68(context, key.0) }

#[derive(Clone, Copy)]
pub(crate) enum RateFamily { First, Second, Third, Fourth, Fifth, Sixth }
impl RateFamily {
    pub(crate) fn parse(mode: u32) -> Option<Self> {
        match mode {
            0x12..=0x14 => Some(Self::First),
            0x15..=0x17 => Some(Self::Second),
            0x19..=0x1b => Some(Self::Third),
            0x22..=0x24 => Some(Self::Fourth),
            0x25..=0x27 => Some(Self::Fifth),
            0x28..=0x2a => Some(Self::Sixth),
            _ => None,
        }
    }
    pub(crate) fn ordinal(self) -> u32 {
        match self { Self::First => 0, Self::Second => 2, Self::Third => 1, Self::Fourth => 3, Self::Fifth => 4, Self::Sixth => 5 }
    }
}

#[repr(C)]
pub(crate) struct BoundsState {
    pub selection: u32,
    pub flags04: u32,
    pub previous: u32,
    reserved0c: [u8; 0x14],
    pub enabled20: u32,
    pub enabled24: u32,
    pub distance: i16,
    pub position: i16,
    pub alternate_position: i16,
    reserved2e: u16,
    pub response: i16,
    pub baseline_response: i16,
    pub response_status: u32,
    reserved38: u32,
    pub category: u32,
    pub sample_count: u32,
    pub quality: i16,
    reserved46: u16,
    pub lower_quality: i16,
    pub upper_quality: i16,
    pub elapsed: u32,
    reserved50: [u8; 8],
    pub motion: u32,
    pub upper_bound: i16,
    pub lower_bound: i16,
    pub alternate_upper: i16,
    pub alternate_lower: i16,
    reserved64: [u8; 0x28],
    pub blocked: u32,
    reserved90: [u8; 0x58],
    pub previous_grid: [i16; 256],
    reserved2e8: u16,
    pub reference_position: i16,
    reserved2ec: u32,
    pub reference_class: u32,
    pub reference_kind: u32,
    pub reference_static: u32,
    pub current_grid_address: u32,
    pub first_grid: [i16; 256],
    pub first_difference: u16,
    reserved502: u16,
    pub current_position: i16,
    pub first_position: i16,
    pub first_position_difference: u16,
    reserved50a: u16,
    reserved50c: u32,
    pub current_kind: u32,
    pub static_status: u32,
    pub static_detection: u32,
    pub static_count: u8,
    reserved51d: [u8; 0x1b],
    pub second_previous_grid: [i16; 256],
    pub second_reference_position: i16,
    reserved73a: [u8; 6],
    pub second_reference_class: u32,
    pub second_reference_kind: u32,
    reserved748: [u8; 8],
    pub second_grid: [i16; 256],
    pub second_difference: u16,
    reserved952: [u8; 4],
    pub second_position: i16,
    pub second_position_difference: u16,
    reserved95a: [u8; 0x236],
    pub tick: u16,
    reservedb92: [u8; 0x214],
    pub baseline_grid: [i16; 256],
    reservedfa6: [u8; 0xa3a],
    pub quality_filter: u32,
    pub distance_filter: u32,
}

pub(crate) fn state(offset: u32) -> GuestAddress { GuestAddress::new(0x016c_af00u32.wrapping_add(offset)) }

pub(crate) fn weight<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<i32, MemoryError> {
    Ok(match byte(context, SettingKey(0x75c3))? {
        1 => 1,
        2 => (context.read_u8(GuestAddress::new(0x00f5_68c0).checked_add(index)?)? == 0) as i32,
        _ => {
            let key = 0x64bc + u32::from(context.read_u8(GuestAddress::new(0x00f5_e220).checked_add(index)?)?);
            byte(context, SettingKey(key))? as i32
        }
    })
}

pub(crate) fn divide(numerator: i32, denominator: i32) -> i32 {
    let numerator = numerator as u32;
    let denominator = denominator as u32;
    if denominator == 0 { if numerator == 0 { 0 } else { -1 } }
    else { (numerator / denominator) as i32 }
}
