//! X-M1 1.50 capture comparisons, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Installed dcd85c/dcd874/dccf64/dcfe14/e677bc return zero.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::capture_settings_predicates_runtime::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureSettings(pub GuestAddress);

impl CaptureSettings {
    pub fn word<const OFFSET: u32>(&self, memory: &mut impl GuestAddressSpace) -> Result<u32, MemoryError> {
        const { assert!(OFFSET % 4 == 0 && OFFSET + 4 <= 0x3e0); }
        memory.read_u32(self.0.checked_add(OFFSET)?)
    }
    pub fn half<const OFFSET: u32>(&self, memory: &mut impl GuestAddressSpace) -> Result<u16, MemoryError> {
        const { assert!(OFFSET % 2 == 0 && OFFSET + 2 <= 0x3e0); }
        memory.read_u16(self.0.checked_add(OFFSET)?)
    }
    pub fn byte<const OFFSET: u32>(&self, memory: &mut impl GuestAddressSpace) -> Result<u8, MemoryError> {
        const { assert!(OFFSET < 0x3e0); }
        memory.read_u8(self.0.checked_add(OFFSET)?)
    }
    fn selected_pair(&self, memory: &mut impl GuestAddressSpace) -> Result<(u32, u32), MemoryError> {
        Ok(match self.word::<0x2c>(memory)? {
            8 => (self.word::<0x3c>(memory)?, self.word::<0x40>(memory)?),
            9 => (self.word::<0x44>(memory)?, self.word::<0x48>(memory)?),
            _ => (self.word::<0x34>(memory)?, self.word::<0x38>(memory)?),
        })
    }
    fn kind_word<const OFFSET: u32>(&self, memory: &mut impl GuestAddressSpace) -> Result<u32, MemoryError> {
        const { assert!(OFFSET % 4 == 0 && OFFSET + 8 <= 0x3e0); }
        let index = u32::from(self.word::<0>(memory)? == 44);
        memory.read_u32(self.0.checked_add(OFFSET + index * 4)?)
    }
    fn output_kind(&self, memory: &mut impl GuestAddressSpace) -> Result<bool, MemoryError> {
        Ok(self.word::<0>(memory)? != 44 || self.word::<0x4c>(memory)? == 0)
    }
}

fn special_kind(kind: u32) -> bool { matches!(kind, 25 | 26 | 29) }
fn kind_changed(before: CaptureSettings, after: CaptureSettings, memory: &mut impl GuestAddressSpace) -> Result<bool, MemoryError> {
    Ok(before.word::<0>(memory)? != after.word::<0>(memory)? || before.word::<4>(memory)? != after.word::<4>(memory)?)
}
fn mode_count_changed(before: CaptureSettings, after: CaptureSettings, memory: &mut impl GuestAddressSpace) -> Result<bool, MemoryError> {
    let old = before.half::<0x124>(memory)?;
    let new = after.half::<0x124>(memory)?;
    if old != new && old != 0 && new != 0 { return Ok(true); }
    let old_kind = before.word::<0>(memory)?;
    let new_kind = after.word::<0>(memory)?;
    Ok((old != 0 && special_kind(new_kind)) || (new != 0 && special_kind(old_kind))
        || (old_kind != new_kind && special_kind(old_kind) && special_kind(new_kind)))
}

macro_rules! predicates {
    ($($variant:ident = $address:literal => |$before:ident, $after:ident, $context:ident| $body:block),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(u32)]
        pub enum SettingsDifference { $($variant = $address),+ }
        impl TryFrom<u32> for SettingsDifference {
            type Error = ();
            fn try_from(address: u32) -> Result<Self, Self::Error> {
                match address { $($address => Ok(Self::$variant),)+ _ => Err(()) }
            }
        }
        pub fn evaluate<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, predicate: SettingsDifference, before: GuestAddress, after: GuestAddress) -> Result<bool, CapturePredicateError> {
            let before = CaptureSettings(before);
            let after = CaptureSettings(after);
            match predicate { $(SettingsDifference::$variant => {
                let $before = before; let $after = after; let $context = context; $body
            }),+ }
        }
    };
}

predicates! {
    PredicateDE29A8 = 0xde29a8 => |before, after, context| { Ok(selection_active(context, before)? && !selection_active(context, after)?) },
    PredicateDE29DC = 0xde29dc => |before, after, context| { Ok(if selection_active(context, before)? { selection_active(context, after)? && output_state(context)? != 2 } else { selection_active(context, after)? }) },
    PredicateDE2A40 = 0xde2a40 => |before, after, context| { Ok(before.word::<0x4c>(context)? == 0 && after.word::<0x4c>(context)? != 0) },
    PredicateDE2A70 = 0xde2a70 => |before, after, context| { Ok(if before.word::<0x4c>(context)? == 0 { after.word::<0x4c>(context)? == 0 && output_state(context)? != 3 } else { after.word::<0x4c>(context)? == 0 }) },
    PredicateDE2AC8 = 0xde2ac8 => |before, after, context| { Ok(before.word::<0x2c>(context)? != after.word::<0x2c>(context)? || (configuration_mode(context)? == 2 && after.word::<0x2c>(context)? == 8)) },
    PredicateDE2B10 = 0xde2b10 => |before, after, context| { Ok(before.selected_pair(context)? != after.selected_pair(context)?) },
    PredicateDE2B84 = 0xde2b84 => |before, after, context| { Ok(before.word::<0x68>(context)? != after.word::<0x68>(context)?) },
    PredicateDE2BA8 = 0xde2ba8 => |before, after, context| { Ok(before.word::<0x24>(context)? != after.word::<0x24>(context)?) },
    PredicateDE2BCC = 0xde2bcc => |before, after, context| { Ok(before.word::<0x6c>(context)? != after.word::<0x6c>(context)?) },
    PredicateDE2BF0 = 0xde2bf0 => |before, after, context| { Ok(before.kind_word::<0x58>(context)? != after.kind_word::<0x58>(context)?) },
    PredicateDE2C34 = 0xde2c34 => |before, after, context| { Ok(before.word::<0x7c>(context)? != after.word::<0x7c>(context)?) },
    PredicateDE2C58 = 0xde2c58 => |before, after, context| { Ok(before.word::<0x7c>(context)? != after.word::<0x7c>(context)? && before.word::<0x7c>(context)? < 25 && after.word::<0x7c>(context)?.wrapping_sub(25) < 4) },
    PredicateDE2CA0 = 0xde2ca0 => |before, after, context| { Ok(before.word::<0x80>(context)? != after.word::<0x80>(context)?) },
    PredicateDE2CC4 = 0xde2cc4 => |before, after, context| {
        let mode = before.word::<0xc>(context)?;
        Ok(adjust_mode(before.word::<0x84>(context)?, mode) != adjust_mode(after.word::<0x84>(context)?, mode))
    },
    PredicateDE2CFC = 0xde2cfc => |before, after, context| { Ok(before.word::<0x88>(context)? != after.word::<0x88>(context)?) },
    PredicateDE2D20 = 0xde2d20 => |before, after, context| { Ok(before.word::<0x8c>(context)? != after.word::<0x8c>(context)?) },
    PredicateDE2D44 = 0xde2d44 => |before, after, context| { Ok(before.word::<0x9c>(context)? != after.word::<0x9c>(context)?) },
    PredicateDE2D68 = 0xde2d68 => |before, after, context| { Ok(before.byte::<0x98>(context)? != after.byte::<0x98>(context)?) },
    PredicateDE2D8C = 0xde2d8c => |before, after, context| { Ok(after.word::<0x4c>(context)? == 2 && before.byte::<0xb6>(context)? != after.byte::<0xb6>(context)?) },
    PredicateDE2DF0 = 0xde2df0 => |before, after, context| { Ok(after.word::<0x4c>(context)? == 2 && before.byte::<0xb7>(context)? != after.byte::<0xb7>(context)?) },
    PredicateDE2E54 = 0xde2e54 => |before, after, context| { Ok(before.word::<0x90>(context)? != after.word::<0x90>(context)?) },
    PredicateDE2E78 = 0xde2e78 => |before, after, context| { Ok(before.word::<0x130>(context)? != after.word::<0x130>(context)?) },
    PredicateDE2E9C = 0xde2e9c => |before, after, context| { Ok(before.word::<0x15c>(context)? != after.word::<0x15c>(context)?) },
    PredicateDE2EC0 = 0xde2ec0 => |before, after, context| { Ok(mode_count_changed(before, after, context)?) },
    PredicateDE2F74 = 0xde2f74 => |before, after, context| { Ok(before.half::<0x124>(context)? == 0 && !special_kind(before.word::<0>(context)?) && (after.half::<0x124>(context)? != 0 || special_kind(after.word::<0>(context)?))) },
    PredicateDE300C = 0xde300c => |before, after, context| { Ok(after.half::<0x124>(context)? == 0 && !special_kind(after.word::<0>(context)?) && (before.half::<0x124>(context)? != 0 || special_kind(before.word::<0>(context)?))) },
    PredicateDE30A8 = 0xde30a8 => |before, after, context| { Ok(before.word::<0x74>(context)? != after.word::<0x74>(context)?) },
    PredicateDE30CC = 0xde30cc => |before, after, context| { Ok(before.word::<0x78>(context)? != after.word::<0x78>(context)?) },
    PredicateDE30F0 = 0xde30f0 => |before, after, context| { Ok(before.word::<0x28>(context)? != after.word::<0x28>(context)?) },
    PredicateDE3114 = 0xde3114 => |before, after, context| { Ok(before.word::<0x28>(context)? != after.word::<0x28>(context)? && before.word::<0xf8>(context)? == 1 && before.word::<0xa4>(context)? != 1) },
    PredicateDE3158 = 0xde3158 => |before, after, context| { Ok(before.word::<0xe8>(context)? != after.word::<0xe8>(context)?) },
    PredicateDE317C = 0xde317c => |before, after, context| { Ok(before.word::<0xe8>(context)? != after.word::<0xe8>(context)? && after.word::<0xe8>(context)? == 1) },
    PredicateDE31B0 = 0xde31b0 => |before, after, context| { Ok(before.word::<0xe8>(context)? != after.word::<0xe8>(context)? && before.word::<0xe8>(context)? == 1) },
    PredicateDE31E4 = 0xde31e4 => |before, after, context| { Ok(before.word::<0xf8>(context)? != after.word::<0xf8>(context)?) },
    PredicateDE3208 = 0xde3208 => |before, after, context| { Ok(before.word::<0xf8>(context)? != after.word::<0xf8>(context)? && after.word::<0xf8>(context)? == 1 && after.word::<0xa4>(context)? != 1) },
    PredicateDE3248 = 0xde3248 => |before, after, context| { Ok(before.word::<0xf8>(context)? != after.word::<0xf8>(context)? && before.word::<0xf8>(context)? == 1 && before.word::<0xa4>(context)? != 1) },
    PredicateDE3288 = 0xde3288 => |before, after, context| { Ok(before.word::<0xf0>(context)? != after.word::<0xf0>(context)?) },
    PredicateDE32AC = 0xde32ac => |before, after, context| { Ok(before.word::<0xf0>(context)? != after.word::<0xf0>(context)? && after.word::<0xf0>(context)? == 1) },
    PredicateDE32E0 = 0xde32e0 => |before, after, context| { Ok(before.word::<0xf0>(context)? != after.word::<0xf0>(context)? && before.word::<0xf0>(context)? == 1) },
    PredicateDE3314 = 0xde3314 => |before, after, context| { Ok((before.word::<0>(context)? == 25 && after.word::<0>(context)? != 25) || (before.word::<0>(context)? == 26 && after.word::<0>(context)? != 26) || (!matches!(before.word::<0>(context)?, 25 | 26) && matches!(after.word::<0>(context)?, 25 | 26))) },
    PredicateDE3390 = 0xde3390 => |before, after, context| { Ok(before.word::<0>(context)? != 29 && after.word::<0>(context)? == 29) },
    PredicateDE33C0 = 0xde33c0 => |before, after, context| { Ok(before.word::<0x304>(context)? != after.word::<0x304>(context)?) },
    PredicateDE33E4 = 0xde33e4 => |before, after, context| { Ok(false) },
    PredicateDE341C = 0xde341c => |before, after, context| { Ok(false) },
    PredicateDE3450 = 0xde3450 => |before, after, context| { Ok(false) },
    PredicateDE34CC = 0xde34cc => |before, after, context| { Ok(false) },
    PredicateDE3504 = 0xde3504 => |before, after, context| { Ok(false) },
    PredicateDE3538 = 0xde3538 => |before, after, context| { Ok(false) },
    PredicateDE35A0 = 0xde35a0 => |before, after, context| { Ok(before.word::<0x320>(context)? != after.word::<0x320>(context)?) },
    PredicateDE35C4 = 0xde35c4 => |before, after, context| { Ok(before.word::<0xf4>(context)? != after.word::<0xf4>(context)?) },
    PredicateDE35E8 = 0xde35e8 => |before, after, context| { Ok(before.word::<0xf4>(context)? != after.word::<0xf4>(context)? && after.word::<0xf4>(context)? == 1) },
    PredicateDE361C = 0xde361c => |before, after, context| { Ok(before.word::<0xf4>(context)? != after.word::<0xf4>(context)? && before.word::<0xf4>(context)? == 1) },
    PredicateDE367C = 0xde367c => |before, after, context| { Ok(before.word::<0xfc>(context)? == 0 && after.word::<0xfc>(context)? == 2 && !output_disabled(context)?) },
    PredicateDE36A8 = 0xde36a8 => |before, after, context| { Ok(before.word::<0xfc>(context)? == 0 && after.word::<0xfc>(context)? == 2 && output_disabled(context)?) },
    PredicateDE3704 = 0xde3704 => |before, after, context| { Ok(before.word::<0xfc>(context)? == 2 && after.word::<0xfc>(context)? == 0 && !output_disabled(context)?) },
    PredicateDE3738 = 0xde3738 => |before, after, context| { Ok(before.word::<0xfc>(context)? == 2 && after.word::<0xfc>(context)? == 0 && output_disabled(context)?) },
    PredicateDE3770 = 0xde3770 => |before, after, context| { Ok(before.word::<0xfc>(context)? == 1 && after.word::<0xfc>(context)? == 2) },
    PredicateDE379C = 0xde379c => |before, after, context| { Ok(before.word::<0xfc>(context)? == 2 && after.word::<0xfc>(context)? == 1) },
    PredicateDE37C8 = 0xde37c8 => |before, after, context| { Ok(before.word::<0xfc>(context)? == 1 && after.word::<0xfc>(context)? == 0 && !output_disabled(context)?) },
    PredicateDE37FC = 0xde37fc => |before, after, context| { Ok(before.word::<0xfc>(context)? == 1 && after.word::<0xfc>(context)? == 0 && output_disabled(context)?) },
    PredicateDE3834 = 0xde3834 => |before, after, context| { Ok(before.word::<0xfc>(context)? == 0 && after.word::<0xfc>(context)? == 1 && !output_disabled(context)?) },
    PredicateDE3868 = 0xde3868 => |before, after, context| { Ok(before.word::<0xfc>(context)? == 0 && after.word::<0xfc>(context)? == 1 && output_disabled(context)?) },
    PredicateDE38A0 = 0xde38a0 => |before, after, context| { Ok(before.word::<0x398>(context)? != after.word::<0x398>(context)?) },
    PredicateDE38C4 = 0xde38c4 => |before, after, context| { Ok(before.word::<0x398>(context)? == 0 && after.word::<0x398>(context)? == 1) },
    PredicateDE38F0 = 0xde38f0 => |before, after, context| { Ok(before.word::<0x398>(context)? == 1 && after.word::<0x398>(context)? == 0) },
    PredicateDE391C = 0xde391c => |before, after, context| { Ok(before.word::<0xc8>(context)? != after.word::<0xc8>(context)?) },
    PredicateDE3940 = 0xde3940 => |before, after, context| { Ok(!matches!(before.word::<0xc8>(context)?, 5 | 7) && matches!(after.word::<0xc8>(context)?, 5 | 7)) },
    PredicateDE397C = 0xde397c => |before, after, context| { Ok(matches!(before.word::<0xc8>(context)?, 5 | 7) && !matches!(after.word::<0xc8>(context)?, 5 | 7)) },
    PredicateDE39B8 = 0xde39b8 => |before, after, context| { Ok(matches!((before.word::<0xfc>(context)?, after.word::<0xfc>(context)?), (0, 2) | (1, 2)) && before.word::<0xe8>(context)? == 1) },
    PredicateDE3A04 = 0xde3a04 => |before, after, context| { Ok(matches!((before.word::<0xfc>(context)?, after.word::<0xfc>(context)?), (0, 2) | (1, 2)) && after.word::<0xe8>(context)? == 1) },
    PredicateDE3A50 = 0xde3a50 => |before, after, context| { Ok(matches!((before.word::<0xfc>(context)?, after.word::<0xfc>(context)?), (2, 0) | (2, 1)) && before.word::<0xe8>(context)? == 1) },
    PredicateDE3A9C = 0xde3a9c => |before, after, context| { Ok(matches!((before.word::<0xfc>(context)?, after.word::<0xfc>(context)?), (2, 0) | (2, 1)) && after.word::<0xe8>(context)? == 1) },
    PredicateDE3B0C = 0xde3b0c => |before, after, context| { Ok(before.word::<0x100>(context)? != after.word::<0x100>(context)? && output_class(context)? == 0xa3) },
    PredicateDE3B44 = 0xde3b44 => |before, after, context| { Ok(before.word::<0x100>(context)? != after.word::<0x100>(context)? && output_class(context)? == 0x97) },
    PredicateDE3B88 = 0xde3b88 => |before, after, context| { Ok(before.word::<0x100>(context)? != after.word::<0x100>(context)? && (before.word::<0x104>(context)? == 0xd5 || after.word::<0x104>(context)? == 0xd5) && output_class(context)? == 0x97) },
    PredicateDE3BF0 = 0xde3bf0 => |before, after, context| { Ok(before.word::<0x104>(context)? != after.word::<0x104>(context)? && after.word::<0x100>(context)? == 1) },
    PredicateDE3C24 = 0xde3c24 => |before, after, context| { Ok(before.word::<0x60>(context)? != after.word::<0x60>(context)?) },
    PredicateDE3C48 = 0xde3c48 => |before, after, context| { Ok(before.word::<0x64>(context)? != after.word::<0x64>(context)?) },
    PredicateDE3C6C = 0xde3c6c => |before, after, context| { Ok(before.word::<0xc0>(context)? != after.word::<0xc0>(context)? && !matches!((before.word::<0xc0>(context)?, after.word::<0xc0>(context)?), (0x97, 0x98) | (0x98, 0x97))) },
    PredicateDE3CBC = 0xde3cbc => |before, after, context| { Ok(before.word::<0x3b0>(context)? != after.word::<0x3b0>(context)?) },
    PredicateDE3CE0 = 0xde3ce0 => |before, after, context| { Ok((before.word::<0x3b0>(context)? == 1) != (after.word::<0x3b0>(context)? == 1)) },
    PredicateDE3D20 = 0xde3d20 => |before, after, context| { Ok((before.word::<0x3b0>(context)? == 2) != (after.word::<0x3b0>(context)? == 2)) },
    PredicateDE3D60 = 0xde3d60 => |before, after, context| { Ok(before.word::<0x0>(context)? != after.word::<0x0>(context)? || before.word::<0x4>(context)? != after.word::<0x4>(context)?) },
    PredicateDE3D94 = 0xde3d94 => |before, after, context| { Ok(before.word::<0x28>(context)? != after.word::<0x28>(context)? && after.word::<0xf8>(context)? == 1 && after.word::<0xa4>(context)? != 1 && before.word::<0>(context)? == after.word::<0>(context)? && before.word::<4>(context)? == after.word::<4>(context)?) },
    PredicateDE3DEC = 0xde3dec => |before, after, context| { Ok(false) },
    PredicateDE3E18 = 0xde3e18 => |before, after, context| { Ok(kind_changed(before, after, context)? && before.word::<0xf8>(context)? == 1 && before.word::<0xa4>(context)? != 1) },
    PredicateDE3E54 = 0xde3e54 => |before, after, context| { Ok(kind_changed(before, after, context)? && after.word::<0xf8>(context)? == 1 && after.word::<0xa4>(context)? != 1) },
    PredicateDE3E90 = 0xde3e90 => |before, after, context| { Ok(kind_changed(before, after, context)? && before.word::<0xf0>(context)? == 1) },
    PredicateDE3EC0 = 0xde3ec0 => |before, after, context| { Ok(kind_changed(before, after, context)? && before.word::<0xf4>(context)? == 1) },
    PredicateDE3EF0 = 0xde3ef0 => |before, after, context| { Ok(before.word::<0x8>(context)? == 0 && after.word::<0x8>(context)? == 1) },
    PredicateDE3F24 = 0xde3f24 => |before, after, context| { Ok((before.word::<0>(context)? == 29) != (after.word::<0>(context)? == 29)) },
    PredicateDE3F64 = 0xde3f64 => |before, after, context| { Ok((before.word::<0>(context)? == 11) != (after.word::<0>(context)? == 11)) },
    PredicateDE3FA4 = 0xde3fa4 => |before, after, context| { Ok(before.word::<0>(context)? != after.word::<0>(context)? && after.word::<0>(context)? == 44) },
    PredicateDE3FD8 = 0xde3fd8 => |before, after, context| { Ok(before.word::<0>(context)? != 44 && after.word::<0>(context)? == 44 && (before.word::<0xe8>(context)? == 1)) },
    PredicateDE4008 = 0xde4008 => |before, after, context| { Ok(before.word::<0>(context)? != 44 && after.word::<0>(context)? == 44 && (before.word::<0xc8>(context)? != 7 && after.word::<0xc8>(context)? != 7)) },
    PredicateDE4040 = 0xde4040 => |before, after, context| { Ok(before.word::<0>(context)? != 44 && after.word::<0>(context)? == 44 && (before.word::<0xc8>(context)? == 7 || after.word::<0xc8>(context)? == 7)) },
    PredicateDE407C = 0xde407c => |before, after, context| { Ok(before.word::<0>(context)? != 44 && after.word::<0>(context)? == 44 && (before.word::<0x4c>(context)? == 3)) },
    PredicateDE40AC = 0xde40ac => |before, after, context| { Ok(false) },
    PredicateDE40C4 = 0xde40c4 => |before, after, context| { Ok(before.word::<0>(context)? == 44 && after.word::<0>(context)? != 44) },
    PredicateDE4100 = 0xde4100 => |before, after, context| { Ok(before.word::<0>(context)? == 44 && after.word::<0>(context)? != 44 && (before.word::<0xe8>(context)? == 1)) },
    PredicateDE4130 = 0xde4130 => |before, after, context| { Ok(before.word::<0>(context)? == 44 && after.word::<0>(context)? != 44 && (before.word::<0xc8>(context)? != 7 && after.word::<0xc8>(context)? != 7)) },
    PredicateDE4168 = 0xde4168 => |before, after, context| { Ok(before.word::<0>(context)? == 44 && after.word::<0>(context)? != 44 && (before.word::<0xc8>(context)? == 7 || after.word::<0xc8>(context)? == 7)) },
    PredicateDE41A4 = 0xde41a4 => |before, after, context| { Ok(before.word::<0>(context)? == 44 && after.word::<0>(context)? != 44 && (after.word::<0x4c>(context)? == 3)) },
    PredicateDE41D4 = 0xde41d4 => |before, after, context| { Ok(before.word::<0>(context)? == 44 && after.word::<0>(context)? != 44 && (after.word::<0x4c>(context)? == 0)) },
    PredicateDE4200 = 0xde4200 => |before, after, context| { Ok(before.word::<0x14>(context)? != after.word::<0x14>(context)? || before.word::<0x20>(context)? != after.word::<0x20>(context)?) },
    PredicateDE4234 = 0xde4234 => |before, after, context| { Ok(matches!(before.word::<0>(context)?,44|74) && !matches!(after.word::<0>(context)?,44|74)) },
    PredicateDE426C = 0xde426c => |before, after, context| { Ok(!matches!(before.word::<0>(context)?,44|74) && matches!(after.word::<0>(context)?,44|74)) },
    PredicateDE42A0 = 0xde42a0 => |before, after, context| { Ok(!(matches!(before.word::<0>(context)?,44|74) != matches!(after.word::<0>(context)?,44|74))) },
    PredicateDE42FC = 0xde42fc => |before, after, context| { Ok(before.word::<0x18>(context)? != after.word::<0x18>(context)?) },
    PredicateDE4320 = 0xde4320 => |before, after, context| { Ok(before.word::<0x1c>(context)? != after.word::<0x1c>(context)?) },
    PredicateDE4344 = 0xde4344 => |before, after, context| { Ok(before.word::<0xc>(context)? != after.word::<0xc>(context)?) },
    PredicateDE4368 = 0xde4368 => |before, after, context| { Ok(false) },
    PredicateDE4380 = 0xde4380 => |before, after, context| { Ok(before.kind_word::<0x50>(context)? != after.kind_word::<0x50>(context)?) },
    PredicateDE43C4 = 0xde43c4 => |before, after, context| { Ok(before.word::<0x10>(context)? != after.word::<0x10>(context)?) },
    PredicateDE43E8 = 0xde43e8 => |before, after, context| { Ok(before.word::<0x4c>(context)? != after.word::<0x4c>(context)?) },
    PredicateDE440C = 0xde440c => |before, after, context| { Ok(before.word::<0x114>(context)? != after.word::<0x114>(context)?) },
    PredicateDE4430 = 0xde4430 => |before, after, context| { Ok(before.word::<0x144>(context)? != after.word::<0x144>(context)?) },
    PredicateDE4454 = 0xde4454 => |before, after, context| { Ok(if before.word::<0x144>(context)? == 1 { after.word::<0x144>(context)? == 1 && auxiliary_state(context)? != 1 && auxiliary_pending(context)? != 0 } else { after.word::<0x144>(context)? == 1 }) },
    PredicateDE44C8 = 0xde44c8 => |before, after, context| { Ok(before.word::<0x144>(context)? != after.word::<0x144>(context)? && before.word::<0x144>(context)? == 1) },
    PredicateDE4518 = 0xde4518 => |before, after, context| {
        let first = GuestAddress::new(before.word::<0x174>(context)?);
        let second = GuestAddress::new(after.word::<0x174>(context)?);
        Ok(bytes_differ(context, first, second, 0x788)?)
    },
    PredicateDE453C = 0xde453c => |before, after, context| { Ok(before.word::<0x2dc>(context)? != after.word::<0x2dc>(context)?) },
    PredicateDE4560 = 0xde4560 => |before, after, context| { Ok(before.word::<0x2dc>(context)? == 1 && after.word::<0x2dc>(context)? == 0) },
    PredicateDE459C = 0xde459c => |before, after, context| { Ok(before.word::<0x2dc>(context)? == 0 && after.word::<0x2dc>(context)? == 1) },
    PredicateDE45DC = 0xde45dc => |before, after, context| { Ok(before.word::<0x2e0>(context)? != after.word::<0x2e0>(context)?) },
    PredicateDE4600 = 0xde4600 => |before, after, context| { Ok(before.word::<0x128>(context)? != after.word::<0x128>(context)?) },
    PredicateDE4624 = 0xde4624 => |before, after, context| { Ok(before.word::<0x15c>(context)? == 1 && after.word::<0x15c>(context)? == 0 && capture_flags(context)? & 3 != 0 && capture_mode(context)? == 3) },
    PredicateDE467C = 0xde467c => |before, after, context| { Ok(before.word::<0x14c>(context)? != after.word::<0x14c>(context)?) },
    PredicateDE46C4 = 0xde46c4 => |before, after, context| { Ok(before.byte::<0xb5>(context)? != after.byte::<0xb5>(context)?) },
    PredicateDE47E0 = 0xde47e0 => |before, after, context| { Ok(matches!(after.word::<0xc>(context)?,1|3|7) && before.word::<0x2fc>(context)? != after.word::<0x2fc>(context)?) },
    PredicateDE4820 = 0xde4820 => |before, after, context| { Ok(matches!(after.word::<0xc>(context)?,1|3|7) && before.word::<0x2f8>(context)? != after.word::<0x2f8>(context)?) },
    PredicateDE4860 = 0xde4860 => |before, after, context| { Ok(after.word::<0xc>(context)? == 8 && before.word::<0x70>(context)? != after.word::<0x70>(context)?) },
    PredicateDE4894 = 0xde4894 => |before, after, context| { Ok(before.word::<0x310>(context)? != after.word::<0x310>(context)?) },
    PredicateDE48B8 = 0xde48b8 => |before, after, context| { Ok(before.word::<0x308>(context)? != after.word::<0x308>(context)?) },
    PredicateDE48DC = 0xde48dc => |before, after, context| { Ok(before.word::<0xb8>(context)? != after.word::<0xb8>(context)?) },
    PredicateDE4900 = 0xde4900 => |before, after, context| { Ok(before.word::<0x318>(context)? != after.word::<0x318>(context)?) },
    PredicateDE4924 = 0xde4924 => |before, after, context| { Ok(before.word::<0x164>(context)? != after.word::<0x164>(context)?) },
    PredicateDE4948 = 0xde4948 => |before, after, context| { Ok(before.word::<0x30c>(context)? != after.word::<0x30c>(context)?) },
    PredicateDE496C = 0xde496c => |before, after, context| { Ok(before.word::<0x31c>(context)? != after.word::<0x31c>(context)?) },
    PredicateDE4990 = 0xde4990 => |before, after, context| { Ok(after.word::<0x2c>(context)? == 10 && before.word::<0x30>(context)? != after.word::<0x30>(context)?) },
    PredicateDE49C4 = 0xde49c4 => |before, after, context| { Ok(before.word::<0xa0>(context)? != after.word::<0xa0>(context)?) },
    PredicateDE49E8 = 0xde49e8 => |before, after, context| { Ok(false) },
    PredicateDE4A00 = 0xde4a00 => |before, after, context| { Ok((before.word::<0x334>(context)? == 0 && after.word::<0x334>(context)? == 1) || (!matches!(before.word::<0>(context)?,35|36) && matches!(after.word::<0>(context)?,35|36) && after.word::<0x334>(context)? == 1)) },
    PredicateDE4A68 = 0xde4a68 => |before, after, context| { Ok((before.word::<0x334>(context)? == 1 && after.word::<0x334>(context)? == 0) || (matches!(before.word::<0>(context)?,35|36) && !matches!(after.word::<0>(context)?,35|36) && before.word::<0x334>(context)? == 1)) },
    PredicateDE4ACC = 0xde4acc => |before, after, context| { Ok(false) },
    PredicateDE4AE4 = 0xde4ae4 => |before, after, context| { Ok(before.byte::<0x338>(context)? != after.byte::<0x338>(context)?) },
    PredicateDE4B08 = 0xde4b08 => |before, after, context| { Ok(before.byte::<0x339>(context)? != after.byte::<0x339>(context)?) },
    PredicateDE4B34 = 0xde4b34 => |before, after, context| { Ok(before.byte::<0x33a>(context)? != after.byte::<0x33a>(context)?) },
    PredicateDE4B60 = 0xde4b60 => |before, after, context| { Ok(before.byte::<0x338>(context)? != after.byte::<0x338>(context)?) },
    PredicateDE4B84 = 0xde4b84 => |before, after, context| { Ok(before.word::<0x33c>(context)? != after.word::<0x33c>(context)?) },
    PredicateDE4BA8 = 0xde4ba8 => |before, after, context| { Ok(before.word::<0xf8>(context)? != after.word::<0xf8>(context)? && after.word::<0xf8>(context)? == 1 && after.word::<0xa4>(context)? != 0 && after.word::<0x4c>(context)? != 0) },
    PredicateDE4C00 = 0xde4c00 => |before, after, context| { Ok(before.word::<0xf8>(context)? != after.word::<0xf8>(context)? && before.word::<0xf8>(context)? == 1 && before.word::<0xa4>(context)? != 0 && before.word::<0x4c>(context)? != 0) },
    PredicateDE4C58 = 0xde4c58 => |before, after, context| { Ok(kind_changed(before, after, context)? && before.word::<0xf8>(context)? == 1 && before.word::<0xa4>(context)? != 0 && before.word::<0x4c>(context)? != 0) },
    PredicateDE4CB0 = 0xde4cb0 => |before, after, context| { Ok(kind_changed(before, after, context)? && after.word::<0xf8>(context)? == 1 && after.word::<0xa4>(context)? != 0 && after.word::<0x4c>(context)? != 0) },
    PredicateDE4D08 = 0xde4d08 => |before, after, context| { Ok(before.word::<0x4c>(context)? != after.word::<0x4c>(context)? && before.word::<0xf8>(context)? == 1 && before.word::<0xa4>(context)? != 0 && before.word::<0x4c>(context)? != 0) },
    PredicateDE4D60 = 0xde4d60 => |before, after, context| { Ok(before.word::<0x4c>(context)? != after.word::<0x4c>(context)? && after.word::<0xf8>(context)? == 1 && after.word::<0xa4>(context)? != 0 && after.word::<0x4c>(context)? != 0) },
    PredicateDE4DB8 = 0xde4db8 => |before, after, context| { Ok((before.word::<0>(context)? == 43) != (after.word::<0>(context)? == 43)) },
    PredicateDE4E10 = 0xde4e10 => |before, after, context| { Ok(before.word::<0x12c>(context)? != after.word::<0x12c>(context)?) },
    PredicateDE4E34 = 0xde4e34 => |before, after, context| { Ok(matches!(before.word::<0x13c>(context)?,5|6) != matches!(after.word::<0x13c>(context)?,5|6)) },
    PredicateDE4E8C = 0xde4e8c => |before, after, context| { Ok(before.word::<0xe0>(context)? != after.word::<0xe0>(context)?) },
    PredicateDE4EB0 = 0xde4eb0 => |before, after, context| { Ok(false) },
    PredicateDE4EC8 = 0xde4ec8 => |before, after, context| { Ok(before.word::<0x344>(context)? != after.word::<0x344>(context)?) },
    PredicateDE4EEC = 0xde4eec => |before, after, context| { Ok(before.word::<0x350>(context)? != after.word::<0x350>(context)?) },
    PredicateDE4F10 = 0xde4f10 => |before, after, context| { Ok(before.word::<0x348>(context)? != after.word::<0x348>(context)?) },
    PredicateDE4F34 = 0xde4f34 => |before, after, context| { Ok(before.word::<0x354>(context)? == 0 && after.word::<0x354>(context)? == 1) },
    PredicateDE4F60 = 0xde4f60 => |before, after, context| { Ok(before.word::<0x354>(context)? == 1 && after.word::<0x354>(context)? == 0) },
    PredicateDE4F8C = 0xde4f8c => |before, after, context| { Ok(before.word::<0x354>(context)? == 1 && after.word::<0x354>(context)? == 0 && after.word::<0xfc>(context)? != 2) },
    PredicateDE4FBC = 0xde4fbc => |before, after, context| { Ok(before.word::<0x354>(context)? == 1 && after.word::<0x354>(context)? == 0 && after.word::<0xfc>(context)? == 2) },
    PredicateDE4FF0 = 0xde4ff0 => |before, after, context| { Ok(false) },
    PredicateDE5008 = 0xde5008 => |before, after, context| { Ok(before.word::<0x354>(context)? == 1 && after.word::<0x354>(context)? == 0 && after.word::<0x144>(context)? == 1) },
    PredicateDE503C = 0xde503c => |before, after, context| { Ok(before.word::<0x378>(context)? != after.word::<0x378>(context)? || before.word::<0x37c>(context)? != after.word::<0x37c>(context)?) },
    PredicateDE5074 = 0xde5074 => |before, after, context| { Ok(before.word::<0x380>(context)? != after.word::<0x380>(context)?) },
    PredicateDE5098 = 0xde5098 => |before, after, context| { Ok(before.word::<0x8>(context)? != after.word::<0x8>(context)?) },
    PredicateDE50BC = 0xde50bc => |before, after, context| { Ok(before.word::<0x314>(context)? != after.word::<0x314>(context)?) },
    PredicateDE50E0 = 0xde50e0 => |before, after, context| { Ok(before.word::<0x314>(context)? != after.word::<0x314>(context)? && (after.word::<0xfc>(context)? != 2 || after.word::<0xd4>(context)? != 1 || after.word::<0x4c>(context)? == 0)) },
    PredicateDE5134 = 0xde5134 => |before, after, context| { Ok(before.word::<0x314>(context)? != after.word::<0x314>(context)?) },
    PredicateDE5168 = 0xde5168 => |before, after, context| { Ok(before.word::<0x390>(context)? != after.word::<0x390>(context)?) },
    PredicateDE518C = 0xde518c => |before, after, context| { Ok(before.word::<0x394>(context)? != after.word::<0x394>(context)? && after.word::<0x390>(context)? == 2) },
    PredicateDE51C4 = 0xde51c4 => |before, after, context| { Ok(before.word::<0x38c>(context)? != after.word::<0x38c>(context)?) },
    PredicateDE51E8 = 0xde51e8 => |before, after, context| { Ok(before.word::<0x3b4>(context)? != after.word::<0x3b4>(context)?) },
    PredicateDE520C = 0xde520c => |before, after, context| { Ok(!before.output_kind(context)? && after.output_kind(context)?) },
    PredicateDE5244 = 0xde5244 => |before, after, context| { Ok(before.output_kind(context)? && !after.output_kind(context)?) },
    PredicateDE5278 = 0xde5278 => |before, after, context| { Ok(before.word::<0x39c>(context)? != after.word::<0x39c>(context)?) },
    PredicateDE529C = 0xde529c => |before, after, context| { Ok(before.word::<0x3a0>(context)? != after.word::<0x3a0>(context)?) },
    PredicateDE52C0 = 0xde52c0 => |before, after, context| { Ok(bytes_differ(context, before.0.checked_add(0x3a4)?, after.0.checked_add(0x3a4)?, 8)?) },
    PredicateDE5324 = 0xde5324 => |before, after, context| { Ok(before.byte::<0x3ac>(context)? != after.byte::<0x3ac>(context)?) },
    PredicateDE5350 = 0xde5350 => |before, after, context| { Ok(before.word::<0x3d8>(context)? != after.word::<0x3d8>(context)?) },
    PredicateDE5374 = 0xde5374 => |before, after, context| { Ok(before.word::<0x3dc>(context)? != after.word::<0x3dc>(context)?) },
}
