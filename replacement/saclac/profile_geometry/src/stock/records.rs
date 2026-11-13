#![forbid(unsafe_code)]

//! The controller's parameter record and status snapshot. Each accessor is a field the
//! controller compares on refresh (0xee1bc0, 0xee2adc).

use crate::profile::ProfileHandle;

fn field<const N: usize>(bytes: &[u8], at: usize) -> [u8; N] { core::array::from_fn(|index| bytes[at + index]) }

macro_rules! record {
    ($name:ident, $size:expr, { $($get:ident, $set:ident: $ty:ty = $at:expr;)* }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct $name {
            bytes: [u8; $size],
        }

        impl $name {
            pub const BYTES: usize = $size;

            pub const fn from_bytes(bytes: [u8; $size]) -> Self { Self { bytes } }

            pub const fn bytes(&self) -> &[u8; $size] { &self.bytes }

            $(
                pub fn $get(&self) -> $ty { <$ty>::from_le_bytes(field(&self.bytes, $at)) }

                pub fn $set(&mut self, value: $ty) {
                    self.bytes[$at..$at + core::mem::size_of::<$ty>()].copy_from_slice(&value.to_le_bytes());
                }
            )*
        }
    };
}

record!(Parameters, 0x48, {
    profile_word, set_profile_word: u32 = 0x00;
    at_04, set_at_04: i16 = 0x04;
    at_06, set_at_06: i16 = 0x06;
    at_08, set_at_08: u32 = 0x08;
    at_0c, set_at_0c: u32 = 0x0c;
    at_10, set_at_10: u32 = 0x10;
    at_14, set_at_14: u32 = 0x14;
    at_18, set_at_18: u32 = 0x18;
    at_1c, set_at_1c: i16 = 0x1c;
    at_1e, set_at_1e: u16 = 0x1e;
    at_20, set_at_20: u16 = 0x20;
    at_24, set_at_24: u32 = 0x24;
    at_28, set_at_28: i16 = 0x28;
    at_2a, set_at_2a: i16 = 0x2a;
    at_30, set_at_30: u32 = 0x30;
    at_34, set_at_34: u32 = 0x34;
    at_38, set_at_38: u32 = 0x38;
    at_3c, set_at_3c: u32 = 0x3c;
    at_40, set_at_40: u32 = 0x40;
    at_44, set_at_44: i8 = 0x44;
});

impl Parameters {
    pub fn profile(&self) -> ProfileHandle { ProfileHandle(self.profile_word()) }

    pub fn set_profile(&mut self, profile: ProfileHandle) { self.set_profile_word(profile.0) }
}

record!(StatusSnapshot, 200, {
    at_00, set_at_00: u32 = 0x00;
    at_04, set_at_04: u32 = 0x04;
    at_08, set_at_08: u32 = 0x08;
    at_0c, set_at_0c: u32 = 0x0c;
    at_10, set_at_10: u32 = 0x10;
    at_14, set_at_14: u32 = 0x14;
    at_18, set_at_18: u32 = 0x18;
    at_1c, set_at_1c: u32 = 0x1c;
    at_20, set_at_20: u32 = 0x20;
    at_2c, set_at_2c: u32 = 0x2c;
    at_30, set_at_30: u32 = 0x30;
    at_34, set_at_34: u32 = 0x34;
    at_38, set_at_38: u32 = 0x38;
    at_3c, set_at_3c: u32 = 0x3c;
    at_40, set_at_40: u16 = 0x40;
    at_44, set_at_44: u32 = 0x44;
    at_48, set_at_48: u32 = 0x48;
    at_4c, set_at_4c: u32 = 0x4c;
    at_50, set_at_50: u32 = 0x50;
    at_54, set_at_54: u32 = 0x54;
    at_5c, set_at_5c: u32 = 0x5c;
    at_60, set_at_60: u32 = 0x60;
    at_64, set_at_64: u32 = 0x64;
    at_68, set_at_68: u32 = 0x68;
    at_6c, set_at_6c: u32 = 0x6c;
    at_70, set_at_70: u8 = 0x70;
    at_74, set_at_74: u32 = 0x74;
    at_78, set_at_78: i16 = 0x78;
    at_8c, set_at_8c: u32 = 0x8c;
    at_90, set_at_90: u32 = 0x90;
    at_94, set_at_94: u32 = 0x94;
    at_98, set_at_98: u32 = 0x98;
    at_9c, set_at_9c: u32 = 0x9c;
    at_a0, set_at_a0: u8 = 0xa0;
    at_a2, set_at_a2: u16 = 0xa2;
    at_a4, set_at_a4: u32 = 0xa4;
    at_a8, set_at_a8: u32 = 0xa8;
    at_ac, set_at_ac: u32 = 0xac;
    at_b0, set_at_b0: u32 = 0xb0;
    at_b4, set_at_b4: u32 = 0xb4;
    at_b8, set_at_b8: u32 = 0xb8;
    at_bc, set_at_bc: u8 = 0xbc;
    at_c0, set_at_c0: u32 = 0xc0;
    at_c4, set_at_c4: u16 = 0xc4;
});
