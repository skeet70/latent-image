#![forbid(unsafe_code)]

//! The controller's parameter record and status snapshot, which display controllers copy out
//! (0xed8af4, 0xed8b10), change and hand back (0xed8b2c, 0xed8b6c).

use crate::profile::ProfileHandle;

trait Field: Copy {
    const BYTES: usize;
    fn read(bytes: &[u8]) -> Self;
    fn write(self, bytes: &mut [u8]);
}

macro_rules! scalar {
    ($($ty:ty),*) => {$(
        impl Field for $ty {
            const BYTES: usize = core::mem::size_of::<$ty>();

            fn read(bytes: &[u8]) -> Self { <$ty>::from_le_bytes(core::array::from_fn(|index| bytes[index])) }

            fn write(self, bytes: &mut [u8]) { bytes[..Self::BYTES].copy_from_slice(&self.to_le_bytes()) }
        }
    )*};
}

scalar!(u8, i8, u16, i16, u32);

impl<T: Field, const N: usize> Field for [T; N] {
    const BYTES: usize = T::BYTES * N;

    fn read(bytes: &[u8]) -> Self { core::array::from_fn(|index| T::read(&bytes[index * T::BYTES..])) }

    fn write(self, bytes: &mut [u8]) {
        for (index, value) in self.into_iter().enumerate() {
            value.write(&mut bytes[index * T::BYTES..]);
        }
    }
}

macro_rules! record {
    ($name:ident, $size:expr, { $($(#[$doc:meta])* $get:ident, $set:ident: $ty:ty = $at:expr;)* }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct $name {
            bytes: [u8; $size],
        }

        impl $name {
            pub const BYTES: usize = $size;

            pub const fn from_bytes(bytes: [u8; $size]) -> Self { Self { bytes } }

            pub const fn bytes(&self) -> &[u8; $size] { &self.bytes }

            $(
                $(#[$doc])*
                pub fn $get(&self) -> $ty { <$ty as Field>::read(&self.bytes[$at..]) }

                pub fn $set(&mut self, value: $ty) { value.write(&mut self.bytes[$at..]) }
            )*
        }
    };
}

record!(Parameters, 0x48, {
    profile_word, set_profile_word: u32 = 0x00;
    /// Scales the status triplet into the white balance (0xee5ec0).
    limit, set_limit: i16 = 0x04;
    secondary_limit, set_secondary_limit: i16 = 0x06;
    /// Stored collapsed (0x746834).
    index, set_index: u32 = 0x08;
    index_scale, set_index_scale: u32 = 0x0c;
    area, set_area: [u32; 3] = 0x10;
    mode_shift, set_mode_shift: i16 = 0x1c;
    /// The crop's height and width ratio.
    format_ratio, set_format_ratio: [u16; 2] = 0x1e;
    quad_mode, set_quad_mode: u32 = 0x24;
    /// The two thresholds a kind-zero profile's scan selection encodes (0xef743c).
    scan_thresholds, set_scan_thresholds: [i16; 2] = 0x28;
    /// The refresh does not take it (0xee1bc0).
    output, set_output: u32 = 0x2c;
    /// Taken only for profiles of the extended classes.
    extended_pair, set_extended_pair: [u32; 2] = 0x30;
    mode, set_mode: u32 = 0x38;
    mode_first, set_mode_first: u32 = 0x3c;
    mode_second, set_mode_second: u32 = 0x40;
    mode_byte, set_mode_byte: i8 = 0x44;
});

impl Parameters {
    pub fn profile(&self) -> ProfileHandle { ProfileHandle(self.profile_word()) }

    pub fn set_profile(&mut self, profile: ProfileHandle) { self.set_profile_word(profile.0) }
}

record!(StatusSnapshot, 200, {
    area, set_area: [u32; 3] = 0x00;
    output_mode, set_output_mode: u32 = 0x0c;
    processing_option, set_processing_option: u32 = 0x10;
    /// 1 enables the scan geometry.
    geometry_enabled, set_geometry_enabled: u32 = 0x14;
    geometry_kind, set_geometry_kind: u32 = 0x18;
    /// The output dimensions, height then width.
    dimensions, set_dimensions: [u32; 2] = 0x1c;
    primary_pair, set_primary_pair: [u32; 2] = 0x2c;
    secondary_pair, set_secondary_pair: [u32; 2] = 0x34;
    triplet, set_triplet: [u16; 3] = 0x3c;
    /// Read only by stock outside the layer, as are `at_4c`, `at_50`, `at_70`, `at_74`, `at_78`,
    /// `at_a0` and `at_ac`.
    at_44, set_at_44: u32 = 0x44;
    /// A status code the record code table maps for the pipeline (0xeeedac).
    record_code, set_record_code: u32 = 0x48;
    at_4c, set_at_4c: u32 = 0x4c;
    at_50, set_at_50: u32 = 0x50;
    /// 2 sets the scan toggle.
    toggle, set_toggle: u32 = 0x54;
    format, set_format: u32 = 0x5c;
    /// The scan rate selection of profiles whose kind is not zero.
    scan_selection, set_scan_selection: u32 = 0x60;
    query_mode, set_query_mode: u32 = 0x64;
    /// Primary then secondary.
    record_kinds, set_record_kinds: [u32; 2] = 0x68;
    at_70, set_at_70: u8 = 0x70;
    at_74, set_at_74: u32 = 0x74;
    at_78, set_at_78: i16 = 0x78;
    /// The refresh does not take it (0xee2adc).
    range, set_range: u32 = 0x7c;
    tracking_armed, set_tracking_armed: u32 = 0x8c;
    tracked_index, set_tracked_index: u32 = 0x90;
    scan_profile, set_scan_profile: u32 = 0x94;
    tracking_held, set_tracking_held: u32 = 0x98;
    record_kind, set_record_kind: u32 = 0x9c;
    at_a0, set_at_a0: u8 = 0xa0;
    rate, set_rate: u16 = 0xa2;
    rate_companion, set_rate_companion: u32 = 0xa4;
    processing_level, set_processing_level: u32 = 0xa8;
    at_ac, set_at_ac: u32 = 0xac;
    first_of, set_first_of: u32 = 0xb0;
    split, set_split: u32 = 0xb4;
    /// Zero takes the first split.
    split_second, set_split_second: u32 = 0xb8;
    multiple, set_multiple: u8 = 0xbc;
    scan_index, set_scan_index: u32 = 0xc0;
    scan_limit, set_scan_limit: u16 = 0xc4;
});
