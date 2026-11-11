use super::shared::{Address32, ByteOffset};
use core::ptr::{read_volatile, write_volatile};

const WRITER: u32 = 0x14e9d50;
const SETTINGS: u32 = 0x14e9e3c;
const IDENTITY: u32 = 0x14e9d98;
const DEBUG: u32 = 0x14e9ea0;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct MetadataBlock(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct DirectoryCode(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct WriterMode(pub u16);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct WriterFormat(pub u16);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ByteOrder(pub u16);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TagId(pub u16);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RuleAddress(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SavedPass(pub u32);

#[derive(Clone, Copy)]
#[repr(u32)]
enum DirectoryKind { Primary, Exif, Maker, ImageRecord, Interoperability, Gps, Thumbnail, MpfIndex, MpfAttributes }
impl DirectoryKind {
    fn parse(code: DirectoryCode) -> Option<Self> {
        Some(match code.0 { 0 => Self::Primary, 1 => Self::Exif, 2 => Self::Maker, 3 => Self::ImageRecord, 4 => Self::Interoperability, 5 => Self::Gps, 6 => Self::Thumbnail, 7 => Self::MpfIndex, 8 => Self::MpfAttributes, _ => return None })
    }
    fn table(self) -> Address32 {
        Address32(match self { Self::Primary => 0x11d09dc, Self::Exif => 0x11d041c, Self::Maker => 0x11d0fbc, Self::ImageRecord => 0x11d23c4, Self::Interoperability => 0x11d099c, Self::Gps => 0x11d0c1c, Self::Thumbnail => 0x11d0ebc, Self::MpfIndex => 0x11d0e5c, Self::MpfAttributes => 0x11d2324 })
    }
    fn count(self) -> u32 {
        match self { Self::Primary | Self::Gps => 18, Self::Exif => 44, Self::Maker => 154, Self::ImageRecord => 9, Self::Interoperability => 2, Self::Thumbnail => 8, Self::MpfIndex => 3, Self::MpfAttributes => 5 }
    }
}

unsafe fn encoded_word(address: u32) -> u32 {
    unsafe {
        u32::from_le_bytes([Address32(address).byte(), Address32(address.wrapping_add(1)).byte(), Address32(address.wrapping_add(2)).byte(), Address32(address.wrapping_add(3)).byte()])
    }
}
unsafe fn encoded_half(address: u32) -> u16 {
    unsafe { u16::from_le_bytes([Address32(address).byte(), Address32(address.wrapping_add(1)).byte()]) }
}

unsafe fn word(base: u32, offset: u32) -> u32 { unsafe { Address32(base.wrapping_add(offset)).word() } }
unsafe fn half(base: u32, offset: u32) -> u16 { unsafe { Address32(base.wrapping_add(offset)).half() } }
unsafe fn store(base: u32, offset: u32, value: u32) { unsafe { Address32(base.wrapping_add(offset)).write_word(value); } }
unsafe fn store_half(base: u32, offset: u32, value: u16) { unsafe { Address32(base.wrapping_add(offset)).write_half(value); } }
unsafe fn copy(destination: u32, source: u32, length: u32) {
    unsafe { for index in 0..length { let value = Address32(source.wrapping_add(index)).byte(); Address32(destination.wrapping_add(index)).write_byte(value); } }
}
unsafe fn copy_local<T>(destination: u32, value: &T, length: u32) { unsafe { copy(destination, value as *const T as u32, length); } }
unsafe fn string_length(source: u32) -> u32 {
    unsafe { let mut length = 0u32; while Address32(source.wrapping_add(length)).byte() != 0 { length = length.wrapping_add(1); } length }
}
unsafe fn copy_string(destination: u32, source: u32) { unsafe { copy(destination, source, string_length(source).wrapping_add(1)); } }

unsafe extern "C" {
    fn xm1_pipeline_image_write_tiff_value(rule: Address32, source: Address32, destination: Address32, count: u32, base: Address32, cursor: *mut u32) -> u32;
    fn xm1_pipeline_image_allocate_tiff_value(size: ByteOffset, cursor: *mut u32) -> u32;
    fn xm1_pipeline_image_tiff_type_size(kind: u32) -> u32;
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_exif_writer(mode: WriterMode, order: ByteOrder, format: WriterFormat, buffer: Address32, option: u16) {
    unsafe {
        store_half(SETTINGS, 0, mode.0);
        store_half(SETTINGS, 2, order.0);
        store_half(SETTINGS, 4, format.0);
        store(SETTINGS, 8, buffer.0);
        store_half(SETTINGS, 12, option);
        for offset in [0, 4, 8, 12, 16, 24, 32, 36, 40, 44, 48, 52, 56, 60, 64] { store(WRITER, offset, 0); }
        store_half(WRITER, 68, 0);
        store_half(WRITER, 70, 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_soi(destination: Address32) -> u32 {
    unsafe { copy(destination.0, 0x11c2ae8, 2); }
    2
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_tiff_header(destination: Address32) -> u32 {
    unsafe {
        store(WRITER, 0, destination.0);
        let marker = if half(SETTINGS, 2) == 0 { 0x11c2aec } else { 0x11c2af0 };
        copy(destination.0, marker, 2);
        copy_local(destination.0.wrapping_add(2), &42u16, 2);
        copy_local(destination.0.wrapping_add(4), &8u32, 4);
    }
    8
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_app1_header(destination: Address32) -> u32 {
    unsafe {
        copy(destination.0, 0x11c2af4, 2);
        store(WRITER, 44, destination.0.wrapping_add(2));
        copy_local(destination.0.wrapping_add(2), &0u16, 2);
        copy(destination.0.wrapping_add(4), 0x11c2af8, 6);
    }
    10
}

unsafe fn write_directory(kind: DirectoryKind, source: MetadataBlock, destination: Address32, mode_three_pass: bool, base: Address32) -> u32 {
    unsafe {
        match half(SETTINGS, 4) {
            0 => xm1_pipeline_image_write_filtered_directory(DirectoryCode(kind as u32), source, destination, base),
            1 => xm1_pipeline_image_write_saved_directory(source, destination, DirectoryCode(kind as u32), SavedPass((mode_three_pass && half(SETTINGS, 0) == 3) as u32)),
            2 => xm1_pipeline_image_write_saved_directory(source, destination, DirectoryCode(kind as u32), SavedPass(1)),
            _ => 0,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_primary_ifd(source: MetadataBlock, destination: Address32) -> u32 {
    unsafe {
        if half(SETTINGS, 0) > 3 { return 0; }
        if half(SETTINGS, 4) == 0 && half(SETTINGS, 0) == 3 { store(SETTINGS, 8, destination.0); store(WRITER, 0, destination.0); }
        write_directory(DirectoryKind::Primary, source, destination, false, Address32(word(WRITER, 0)))
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_exif_ifd(source: MetadataBlock, destination: Address32) -> u32 {
    unsafe {
        if half(SETTINGS, 0) > 3 { return 0; }
        let offset = destination.0.wrapping_sub(word(WRITER, 0));
        copy_local(word(WRITER, 4), &offset, 4);
        write_directory(DirectoryKind::Exif, source, destination, true, Address32(word(WRITER, 0)))
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_gps_ifd(source: MetadataBlock, destination: Address32) -> u32 {
    unsafe {
        if half(WRITER, 70) == 0 || half(SETTINGS, 0) > 3 { return 0; }
        let offset = destination.0.wrapping_sub(word(WRITER, 0));
        copy_local(word(WRITER, 12), &offset, 4);
        write_directory(DirectoryKind::Gps, source, destination, false, Address32(word(WRITER, 0)))
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_thumbnail_ifd(source: MetadataBlock, destination: Address32) -> u32 {
    unsafe {
        if half(SETTINGS, 0) > 2 { return 0; }
        let offset = destination.0.wrapping_sub(word(WRITER, 0));
        copy_local(word(WRITER, 40), &offset, 4);
        let length = write_directory(DirectoryKind::Thumbnail, source, destination, false, Address32(word(WRITER, 0)));
        let offset = destination.0.wrapping_add(length).wrapping_sub(word(WRITER, 0));
        copy_local(word(WRITER, 36), &offset, 4);
        length
    }
}

#[repr(C)]
pub struct TiffEntry { tag: u16, kind: u16, count: u32, value: u32 }
#[repr(C)]
pub struct ValueLayout { width: u16, indirect: u16 }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_interoperability_ifd(source: MetadataBlock, destination: Address32) -> u32 {
    unsafe {
        let version = word(0x11c2b00, 0);
        let offset = destination.0.wrapping_sub(word(WRITER, 0));
        copy_local(word(WRITER, 8), &offset, 4);
        let mut index = TiffEntry { tag: 1, kind: 2, count: 4, value: 0 };
        let text = if half(source.0, 0x28a) == 0xffff { 0x11c2b18 } else { 0x14e9e08 };
        copy_string(&mut index.value as *mut u32 as u32, text);
        let version = TiffEntry { tag: 2, kind: 7, count: 4, value: version };
        copy_local(destination.0, &2u16, 2);
        copy_local(destination.0.wrapping_add(2), &index, 12);
        copy_local(destination.0.wrapping_add(14), &version, 12);
        copy_local(destination.0.wrapping_add(26), &0u32, 4);
    }
    30
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_maker_signature(destination: Address32) -> u32 {
    unsafe {
        let mut signature = [0u8; 8];
        copy(signature.as_mut_ptr() as u32, 0x11d0404, 8);
        store(WRITER, 28, destination.0);
        copy(destination.0, signature.as_ptr() as u32, 8);
        copy_local(destination.0.wrapping_add(8), &12u32, 4);
    }
    12
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_makernote(source: MetadataBlock, destination: Address32) -> u32 {
    unsafe {
        if half(WRITER, 68) == 0 || half(SETTINGS, 0) > 3 { return 0; }
        let offset = destination.0.wrapping_sub(word(WRITER, 0));
        copy_local(word(WRITER, 24), &offset, 4);
        let swapped = half(SETTINGS, 2) == 1;
        if swapped { store_half(SETTINGS, 2, 0); }
        let mut size = xm1_pipeline_image_write_maker_signature(destination);
        size = size.wrapping_add(write_directory(DirectoryKind::Maker, source, Address32(destination.0.wrapping_add(size)), true, Address32(word(WRITER, 28))));
        if swapped { store_half(SETTINGS, 2, 1); }
        copy_local(word(WRITER, 20), &size, 4);
        size
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_exif_header(output: Address32, primary: MetadataBlock, secondary: MetadataBlock, maker: MetadataBlock, gps: MetadataBlock, thumbnail: MetadataBlock, format: WriterFormat, option: u32) -> u32 {
    unsafe {
        xm1_pipeline_image_initialize_exif_writer(WriterMode(0), ByteOrder(0), format, Address32(0), option as u16);
        let mut size = xm1_pipeline_image_write_soi(output);
        let length = xm1_pipeline_image_write_app1_header(Address32(output.0.wrapping_add(size)));
        size = size.wrapping_add(length);
        let length = xm1_pipeline_image_write_tiff_header(Address32(output.0.wrapping_add(size)));
        size = size.wrapping_add(length);
        let length = xm1_pipeline_image_write_primary_ifd(primary, Address32(output.0.wrapping_add(size)));
        size = size.wrapping_add(length);
        let length = xm1_pipeline_image_write_exif_ifd(secondary, Address32(output.0.wrapping_add(size)));
        size = size.wrapping_add(length);
        let length = xm1_pipeline_image_write_makernote(maker, Address32(output.0.wrapping_add(size)));
        size = size.wrapping_add(length);
        let length = xm1_pipeline_image_write_interoperability_ifd(secondary, Address32(output.0.wrapping_add(size)));
        size = size.wrapping_add(length);
        if option == 1 { let length = xm1_pipeline_image_write_gps_ifd(gps, Address32(output.0.wrapping_add(size))); size = size.wrapping_add(length); }
        xm1_pipeline_image_write_thumbnail_ifd(thumbnail, Address32(output.0.wrapping_add(size))).wrapping_add(size)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_select_ifd_table(code: DirectoryCode) -> Address32 {
    DirectoryKind::parse(code).map_or(Address32(0), DirectoryKind::table)
}

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_ifd_table_count(code: DirectoryCode) -> u32 {
    DirectoryKind::parse(code).map_or(0, DirectoryKind::count)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_tiff_value_capacity(code: DirectoryCode, rule: RuleAddress, source: MetadataBlock) -> u32 {
    unsafe {
        let tag = half(rule.0, 0);
        let limit = half(rule.0, 6) as u32;
        match DirectoryKind::parse(code) {
            Some(DirectoryKind::Primary) => match tag { 0x110 => string_length(IDENTITY + 16).wrapping_add(1), 0x131 => string_length(IDENTITY + 48).wrapping_add(1), 0x10f => string_length(IDENTITY).wrapping_add(1), _ => limit },
            Some(DirectoryKind::Maker) => {
                let count = match tag {
                    0x1027 => (half(source.0, 0xca) as u32).wrapping_mul(3),
                    0x1428 => (half(source.0, 0x112) as u32).wrapping_mul(2),
                    0x4009 => (half(source.0, 0x20a) as u32).wrapping_mul(4),
                    0x400a | 0x400b => half(source.0, 0x20a) as u32,
                    0x4103 => half(source.0, 0xe03c) as u32,
                    0x4104 => (half(source.0, 0x222) as u32).wrapping_mul(4),
                    0x4106 => half(source.0, 0xe03e) as u32,
                    0x4101 | 0x4102 | 0x4105 | 0x4107 | 0x4180 | 0x4181 => half(source.0, 0x222) as u32,
                    0x4203 => half(source.0, 0xe040) as u32,
                    0x4204 => (half(source.0, 0x58a) as u32).wrapping_mul(4),
                    0x4206 => half(source.0, 0xe042) as u32,
                    0x4201 | 0x4202 | 0x4205 | 0x4207 | 0x4208 | 0x4280 | 0x4281 | 0x4283 | 0x4284 => half(source.0, 0x58a) as u32,
                    0x4282 => 8,
                    _ => limit,
                };
                count.min(limit)
            }
            Some(DirectoryKind::Gps) => match tag { 8 => string_length(source.0.wrapping_add(96)).wrapping_add(1), 18 => string_length(source.0.wrapping_add(136)).wrapping_add(1), _ => limit },
            Some(DirectoryKind::ImageRecord) => match tag { 0x1001 => half(source.0, 8) as u32, 0x1110..=0x1112 => half(source.0, 60) as u32, _ => limit },
            _ => limit,
        }
    }
}

unsafe fn capture_tag_available(tag: TagId, source: MetadataBlock, filtered: bool) -> bool {
    unsafe {
        let tag = tag.0;
        let captured = Address32(source.0.wrapping_add(0x1e8)).byte() != 0;
        if !captured {
            if matches!(tag, 0x1408..=0x140a | 0x140c | 0x4100 | 0x4200) { return false; }
            if !filtered && matches!(tag, 0x4002..=0x4007) { return false; }
        }
        if (!captured || half(source.0, 0x222) == 0) && tag.wrapping_sub(0x4000) < 2 { return false; }
        if !captured && filtered && tag.wrapping_sub(0x4002) < 6 { return false; }
        if let 0x4002..=0x4007 = tag {
            if half(source.0, 0xe044 + (tag as u32 - 0x4002) * 2) == 0 { return false; }
        }
        if !captured || half(source.0, 0x20a) == 0xffff {
            if tag == 0x4008 { return false; }
        }
        if !captured || matches!(half(source.0, 0x20a), 0 | 0xffff) {
            if tag.wrapping_sub(0x4009) < 3 { return false; }
        }
        if !captured || half(source.0, 0x222) == 0 {
            if matches!(tag, 0x4101..=0x4107 | 0x4180 | 0x4181) { return false; }
        }
        if tag == 0x4106 && half(source.0, 0xe03e) == 0 { return false; }
        if !captured || half(source.0, 0x58a) == 0 {
            if matches!(tag, 0x4201..=0x4208 | 0x4280..=0x4284) { return false; }
        }
        if tag == 0x4206 && half(source.0, 0xe042) == 0 { return false; }
        if Address32(source.0.wrapping_add(0x1f2)).byte() == 0 && matches!(tag, 0x4282 | 0x140c) { return false; }
        if tag == 0x4282 && word(source.0, 0x1d38) != 1 { return false; }
        if half(source.0, 0x1f0) & 1 == 0 && matches!(tag, 0x4000..=0x400b | 0x4282..=0x4284) { return false; }
        true
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_saved_tag_available(tag: TagId, source: MetadataBlock) -> u32 {
    unsafe {
        if !capture_tag_available(tag, source, false) { return 0; }
        (match tag.0 {
            0x1420 => half(source.0, 0xf8) == 1,
            0x1423 => half(source.0, 0x106) != 0xffff,
            0xb211 => word(source.0, 0xe870) == 1,
            0xb212 => word(source.0, 0xe874) == 1,
            0xb213 => word(source.0, 0xe878) == 1,
            0xb214 => word(source.0, 0xe87c) == 1,
            0x1437 => word(source.0, 0xe8e8) == 1,
            _ => true,
        }) as u32
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_should_write_tag(code: DirectoryCode, tag: TagId, source: MetadataBlock) -> u32 {
    unsafe {
        let tag_value = tag.0;
        match DirectoryKind::parse(code) {
            Some(DirectoryKind::Primary) => return match tag_value { 0x13e => (word(source.0, 0xf4) != 0) as u32, 0x13f => (word(source.0, 0x104) != 0) as u32, 0x211 => (word(source.0, 0x134) != 0) as u32, _ => 1 },
            Some(DirectoryKind::Exif) => return match tag_value { 0xa40a => (half(source.0, 0x2b4) != 0xffff) as u32, 0xa500 => (word(source.0, 0x33c) != 0) as u32, _ => 1 },
            Some(DirectoryKind::ImageRecord) => {
                if word(source.0, 0) != 1 && matches!(tag_value, 0x100 | 0x1000 | 0x1001 | 0x1020 | 0x1021) { return 0; }
                if word(source.0, 56) == 1 && !matches!(half(source.0, 60), 0 | 0xffff) { return 1; }
                return (!matches!(tag_value, 0x1100 | 0x1110..=0x1112)) as u32;
            }
            Some(DirectoryKind::Gps) => return (match tag_value {
                8 => Address32(source.0.wrapping_add(96)).byte() != 0,
                9 => Address32(source.0.wrapping_add(99)).byte() != 0,
                10 => Address32(source.0.wrapping_add(101)).byte() != 0,
                11 => word(source.0, 104) != 0 || word(source.0, 108) != 0,
                16 => Address32(source.0.wrapping_add(124)).byte() != 0,
                17 => word(source.0, 128) != 1 || word(source.0, 132) != 0,
                _ => true,
            }) as u32,
            Some(DirectoryKind::MpfAttributes) => return (tag_value != 0xb000 || word(source.0, 28) != 0) as u32,
            Some(DirectoryKind::Maker) => {}
            _ => return 1,
        }
        let initial = match tag_value {
            0x1210 => half(source.0, 0x40) != 0xffff && word(source.0, 0xd8) != 0,
            0x1004 => half(source.0, 0x42) != 0xffff,
            0x1003 => half(source.0, 0x40) != 0xffff,
            0x1001 => half(source.0, 0x3c) != 0xffff,
            0x1005 => half(source.0, 0x44) != 0,
            0x100a => word(source.0, 0x48) != 0,
            0x1401 => word(source.0, 0xec) != 0,
            0x1402 => half(source.0, 0xf2) != 0xffff,
            0x1403 => half(source.0, 0xf4) != 0xffff,
            0x1431 => word(source.0, 0x134) != u32::MAX,
            _ => true,
        };
        if !initial || !capture_tag_available(tag, source, true) { return 0; }
        let available = match tag_value {
            0x1201 => word(source.0, 0xd4) != u32::MAX,
            0x1026 => half(source.0, 0x78) != 0xffff,
            0x1033 => half(source.0, 0xa2) != 0xffff,
            0x1034 => half(source.0, 0xa4) != 0xffff,
            0x1035 => half(source.0, 0xa6) != 0xffff,
            0x1040 => word(source.0, 0xa8) != 0xffff,
            0x1041 => word(source.0, 0xac) != 0xffff,
            0x1042 => word(source.0, 0xb0) != 0,
            0x1043 => word(source.0, 0xb8) != 0,
            0x140b => half(source.0, 0xf6) != 0xffff,
            0x1422 => word(source.0, 0xfc) != 0,
            0x1424 => half(source.0, 0x108) != 0xffff,
            0x100b => half(source.0, 0x54) != 0xffff,
            0x100e => word(source.0, 0x5c) == 1,
            0x100d => Address32(source.0.wrapping_add(0x58)).byte() != 0,
            0x1425 | 0x1427 | 0x1428 => word(source.0, 0x10c) == 1,
            0x1150..=0x1152 => word(source.0, 0xc4) != u32::MAX,
            0x1027 => word(source.0, 0xc4) == 2,
            0x1153 => half(source.0, 0xcc) != 0xffff,
            0x1154 => half(source.0, 0xce) != 0xffff,
            0x1028 => half(source.0, 0x92) != 0xffff,
            _ => true,
        };
        if !available { return 0; }
        if half(source.0, 0x92).wrapping_sub(1) > 0xfffd && tag_value.wrapping_sub(0x1029) < 2 { return 0; }
        if word(source.0, 0x94) == 0 && tag_value == 0x102a { return 0; }
        let available = match tag_value {
            0x1044 => word(source.0, 0xe8a4) != u32::MAX,
            0x1045 => word(source.0, 0xe8f0) != u32::MAX,
            0x1440 => word(source.0, 0xe8f4) != u32::MAX,
            0x1441 => word(source.0, 0xe8f8) != 0x80000000,
            0x1436 => half(source.0, 0xe8a8) != 0xffff,
            _ => true,
        };
        if !available { return 0; }
        if Address32(source.0.wrapping_add(0xe050)).byte() == 70 && matches!(tag_value, 0x4500 | 0x4510 | 0x4520..=0x4537) { return 0; }
        (match tag_value {
            0xb211 => word(source.0, 0xe870) == 1,
            0xb212 => word(source.0, 0xe874) == 1,
            0xb213 => word(source.0, 0xe878) == 1,
            0xb214 => word(source.0, 0xe87c) == 1,
            0x1432 => half(source.0, 0xe894) != 0xffff,
            0x1433..=0x1435 => half(source.0, 0xe894).wrapping_sub(1) <= 0xfffd,
            0x1437 => word(source.0, 0xe8e8) == 1,
            0x1020 => half(source.0, 0x6e) != 0xffff,
            _ => true,
        }) as u32
    }
}

#[repr(C)]
pub struct SavedParser { context: Address32, file: Address32, workspace: Address32, auxiliary: Address32, reserved: Address32, state: Address32, tail: Address32 }

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SavedBank(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TransferDirection(pub i32);

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_transfer_saved_parser(parser: *mut SavedParser, bank: SavedBank, direction: TransferDirection) -> i8 {
    unsafe {
        let (state_slot, file_slot) = match bank.0 { 0 => (0x14e9e50, 0x14e9e58), 1 => (0x14e9e54, 0x14e9e4c), _ => return -1 };
        if parser.is_null() { return -1; }
        match direction.0 {
            0 => {
                for index in 0..28 { write_volatile((parser as *mut u8).add(index), 0); }
                write_volatile(&mut (*parser).file, Address32(word(file_slot, 0)));
                write_volatile(&mut (*parser).state, Address32(word(state_slot, 0)));
            }
            1 => {
                store(file_slot, 0, read_volatile(&(*parser).file).0);
                store(state_slot, 0, read_volatile(&(*parser).state).0);
            }
            _ => {}
        }
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_saved_directory_slots(code: DirectoryCode, parser: *const SavedParser) -> Address32 {
    unsafe {
        let offset = match code.0 as u8 { 0 => 0xffc, 1 => 0x1c4c, 2 => 0x250c, 5 => 0xa444, 6 => 0xa6f8, _ => return Address32(0) };
        Address32(read_volatile(&(*parser).state).0.wrapping_add(offset))
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_saved_directory_values(code: DirectoryCode, parser: *const SavedParser) -> Address32 {
    unsafe {
        let offset = match code.0 as u8 { 0 => 0x10d4, 1 => 0x1e5c, 2 => 0x2c44, 5 => 0xa51c, 6 => 0xa758, _ => return Address32(0) };
        Address32(read_volatile(&(*parser).state).0.wrapping_add(offset))
    }
}

unsafe fn debug_directory(code: DirectoryCode, source: MetadataBlock, destination: Address32, base: Address32) {
    unsafe {
        store(DEBUG, 0, code.0);
        store(DEBUG, 4, source.0);
        store(DEBUG, 8, destination.0);
        store(DEBUG, 72, base.0);
        for offset in [12, 16, 20, 24, 28, 32] { store(DEBUG, offset, source.0); }
    }
}

unsafe fn copy_identity(source: MetadataBlock) {
    unsafe {
        copy(source.0, IDENTITY, 16);
        copy(source.0.wrapping_add(16), IDENTITY + 16, 32);
        copy(source.0.wrapping_add(48), IDENTITY + 48, 64);
    }
}

unsafe fn rule_enabled(rule: RuleAddress) -> bool {
    unsafe {
        let flags = half(rule.0, 2);
        if flags & 16 != 0 && half(SETTINGS, 12) == 1 { return true; }
        let mask = half(0x11d230c, (half(SETTINGS, 0) as u32) * 2);
        flags & mask == mask
    }
}

fn value_layout(entry: &TiffEntry) -> ValueLayout {
    let (width, indirect) = match entry.kind {
        1 | 2 | 7 => (1, (entry.count > 4) as u16),
        3 => (2, (entry.count > 2) as u16),
        4 | 9 => (4, (entry.count > 1) as u16),
        5 | 10 => (8, 1),
        _ => (0, 0),
    };
    ValueLayout { width, indirect }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_get_tiff_value_layout(entry: *const TiffEntry, layout: *mut ValueLayout) {
    unsafe { write_volatile(layout, value_layout(&*entry)); }
}

unsafe fn capture_slot(value: Address32, result: *mut TiffEntry, offset: u32) {
    unsafe {
        store(WRITER, offset, value.0.wrapping_add(8));
        write_volatile(&mut (*result).value, 0);
    }
}

unsafe fn callback_value(source: MetadataBlock, destination: Address32, layout: *const ValueLayout, result: *mut TiffEntry, offset: u32) {
    unsafe {
        let count = read_volatile(&(*result).count);
        let size = count.wrapping_mul(read_volatile(&(*layout).width) as u32);
        let output = if size <= 4 { write_volatile(&mut (*result).value, 0); Address32(&mut (*result).value as *mut u32 as u32) } else { destination };
        copy(output.0, source.0.wrapping_add(offset), size);
    }
}

unsafe fn invoke_rule_callback(callback: u32, source: MetadataBlock, entry: Address32, destination: Address32, layout: *mut ValueLayout, result: *mut TiffEntry) {
    unsafe {
        match callback {
            0x11cecb8 => capture_slot(entry, result, 36),
            0x11cece8 => capture_slot(entry, result, 32),
            0x11ced18 => capture_slot(entry, result, 4),
            0x11ced48 => { store(WRITER, 12, entry.0.wrapping_add(8)); store_half(WRITER, 70, 1); write_volatile(&mut (*result).value, 0); }
            0x11ced80 => write_volatile(&mut (*result).value, 1),
            0x11ceda0 => { store(WRITER, 20, entry.0.wrapping_add(4)); store(WRITER, 24, entry.0.wrapping_add(8)); store_half(WRITER, 68, 1); write_volatile(&mut (*result).value, 0); }
            0x11cede0 => capture_slot(entry, result, 8),
            0x11cee10 => {
                let packed = if half(SETTINGS, 4) == 1 { 0 } else { (half(source.0, 0x76) as u32).wrapping_shl(16).wrapping_add(half(source.0, 0x74) as u32) };
                write_volatile(&mut (*result).value, packed);
            }
            0x11cee4c => {
                let value = if half(SETTINGS, 4) <= 1 { word(source.0, 0x134) } else { 0 };
                write_volatile(&mut (*result).value, value);
            }
            0x11cefa8 => callback_value(source, destination, layout, result, 0x100),
            0x11ceff0 => callback_value(source, destination, layout, result, 0x7a),
            0x11cf038 => {
                if Address32(source.0.wrapping_add(4)).byte() == 0 {
                    let mut text = [0u8; 48];
                    copy(text.as_mut_ptr() as u32, IDENTITY + 116, 4);
                    copy(text.as_mut_ptr().add(4) as u32, IDENTITY + 120, 12);
                    text[16] = 32;
                    copy(text.as_mut_ptr().add(17) as u32, IDENTITY + 132, 30);
                    text[47] = 0;
                    copy(destination.0, text.as_ptr() as u32, read_volatile(&(*result).count).wrapping_mul(read_volatile(&(*layout).width) as u32));
                } else { copy(destination.0, source.0.wrapping_add(4), 48); }
            }
            0x11cf3cc => {
                let text = source.0.wrapping_add(0xb2);
                let count = string_length(text).wrapping_add(1);
                write_volatile(&mut (*result).count, count);
                if count > 4 {
                    copy(destination.0, text, count.wrapping_mul(read_volatile(&(*layout).width) as u32));
                    if count & 1 != 0 { Address32(destination.0.wrapping_add(count)).write_byte(0); }
                    write_volatile(&mut (*layout).indirect, 1);
                } else {
                    write_volatile(&mut (*result).value, 0);
                    copy(&mut (*result).value as *mut u32 as u32, text, count);
                    write_volatile(&mut (*layout).indirect, 0);
                }
            }
            0x11cf82c => { store(WRITER, 16, entry.0.wrapping_add(8)); xm1_pipeline_image_write_printim(MetadataBlock(source.0.wrapping_add(0x15c)), destination); }
            0x11cfd58 => {
                let mut size = (half(source.0, 0x58a) as u32).wrapping_mul(8);
                let mut cursor = destination.0.wrapping_add(size);
                let mut index = 0u16;
                while index < half(source.0, 0x58a) {
                    let record = source.0.wrapping_add((index as u32).wrapping_mul(832));
                    let (offset, length) = if word(record, 0x1d3c) == 1 || word(record, 0x1d74) == 1 {
                        let length = xm1_pipeline_image_write_filtered_directory(DirectoryCode(3), MetadataBlock(record.wrapping_add(0x1d3c)), Address32(cursor), destination);
                        let offset = cursor.wrapping_sub(destination.0);
                        size = size.wrapping_add(length);
                        cursor = cursor.wrapping_add(length);
                        (offset, length)
                    } else { (0, 0) };
                    let item = destination.0.wrapping_add((index as u32).wrapping_mul(8));
                    copy_local(item, &offset, 4);
                    copy_local(item.wrapping_add(4), &length, 4);
                    index = index.wrapping_add(1);
                }
                write_volatile(&mut (*result).count, size);
            }
            _ => core::hint::unreachable_unchecked(),
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_printim(source: MetadataBlock, destination: Address32) -> u32 {
    unsafe {
        let mut header = [0u8; 16];
        copy_string(header.as_mut_ptr() as u32, 0x11d22fc);
        copy_string(header.as_mut_ptr().add(8) as u32, 0x11d2304);
        copy_local(header.as_mut_ptr().add(12) as u32, &0u16, 2);
        copy_local(header.as_mut_ptr().add(14) as u32, &3u16, 2);
        copy(destination.0, header.as_ptr() as u32, 16);
        let record = write_short_record(destination.0.wrapping_add(16), TagId(2), Address32(source.0).byte() as u32);
        let (cursor, size) = if word(source.0, 4) == 0 { (record, 22u32) } else { (write_short_record(record, TagId(3), 0), 28u32) };
        let cursor = write_short_record(cursor, TagId(0x101), Address32(source.0.wrapping_add(76)).byte() as u32);
        if word(source.0, 4) == 0 { size.wrapping_add(6) }
        else {
            copy_local(record.wrapping_add(2), &cursor.wrapping_sub(destination.0), 4);
            copy(cursor, source.0.wrapping_add(4), 72);
            size.wrapping_add(78)
        }
    }
}

unsafe fn write_short_record(destination: u32, tag: TagId, value: u32) -> u32 {
    unsafe {
        let mut record = [0u8; 6];
        copy_local(record.as_mut_ptr() as u32, &tag.0, 2);
        copy_local(record.as_mut_ptr().add(2) as u32, &value, 4);
        copy(destination, record.as_ptr() as u32, 6);
        destination.wrapping_add(6)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_filtered_directory(code: DirectoryCode, source: MetadataBlock, destination: Address32, base: Address32) -> u32 {
    unsafe {
        debug_directory(code, source, destination, base);
        let Some(kind) = DirectoryKind::parse(DirectoryCode(code.0 as u8 as u32)) else { return 0 };
        let table = kind.table().0;
        let length = kind.count();
        if code.0 == 0 { copy_identity(source); }
        let mut count = 0u16;
        for index in 0..length {
            let rule = RuleAddress(table.wrapping_add(index * 32));
            if rule_enabled(rule) && xm1_pipeline_image_should_write_tag(code, TagId(half(rule.0, 0)), source) != 0 { count = count.wrapping_add(1); }
        }
        let mut values = destination.0.wrapping_add((count as u32).wrapping_mul(12)).wrapping_add(6);
        copy_local(destination.0, &count, 2);
        let mut cursor = destination.0.wrapping_add(2);
        let mut size = 2u32;
        for index in 0..length {
            let rule = RuleAddress(table.wrapping_add(index * 32));
            if !rule_enabled(rule) { continue; }
            let tag = TagId(half(rule.0, 0));
            if xm1_pipeline_image_should_write_tag(code, tag, source) == 0 { continue; }
            let mut entry = TiffEntry { tag: tag.0, kind: half(rule.0, 4), count: xm1_pipeline_image_tiff_value_capacity(code, rule, source), value: 0 };
            let callback = word(rule.0, 8);
            if callback == 0 {
                size = size.wrapping_add(xm1_pipeline_image_write_tiff_value(Address32(rule.0), Address32(source.0.wrapping_add(word(rule.0, 28))), Address32(cursor), entry.count, base, &mut values));
            } else {
                let mut layout = value_layout(&entry);
                invoke_rule_callback(callback, source, Address32(cursor), Address32(values), &mut layout, &mut entry);
                if layout.indirect == 1 {
                    entry.value = values.wrapping_sub(base.0);
                    size = size.wrapping_add(xm1_pipeline_image_allocate_tiff_value(ByteOffset(entry.count.wrapping_mul(layout.width as u32)), &mut values));
                }
                copy_local(cursor, &entry, 12);
                size = size.wrapping_add(12);
            }
            cursor = cursor.wrapping_add(12);
        }
        if code.0 == 0 {
            store(WRITER, 40, cursor);
            store(source.0, 0x154, cursor);
            store(source.0, 0x158, 0);
            copy(cursor, source.0.wrapping_add(0x158), 4);
        } else if code.0 == 7 { store(WRITER, 64, cursor); }
        else { copy_local(cursor, &0u32, 4); }
        size.wrapping_add(4)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_saved_directory(source: MetadataBlock, destination: Address32, code: DirectoryCode, pass: SavedPass) -> u32 {
    unsafe {
        let mut parser = core::mem::MaybeUninit::<SavedParser>::uninit();
        xm1_pipeline_image_transfer_saved_parser(parser.as_mut_ptr(), SavedBank(0), TransferDirection(0));
        let parser = parser.assume_init();
        let mut count = 0u16;
        let supported = code.0 <= 6 && (1u32 << code.0) & 0x67 != 0;
        if supported {
            let table = xm1_pipeline_image_select_ifd_table(code).0;
            let length = xm1_pipeline_image_ifd_table_count(code);
            let slots = xm1_pipeline_image_saved_directory_slots(code, &parser).0;
            if slots != 0 && table != 0 {
                for index in 0..length {
                    let rule = RuleAddress(table.wrapping_add(index * 32));
                    if code.0 == 2 && pass.0 == 0 && xm1_pipeline_image_saved_tag_available(TagId(half(rule.0, 0)), source) == 0 { continue; }
                    let flags = word(rule.0, if pass.0 == 0 { 20 } else { 24 });
                    let mode = half(SETTINGS, 0) as u32;
                    let present = word(0x11d24e4, mode * 4);
                    let alternate = word(0x11d040c, mode * 4);
                    let defaults = word(0x11d2314, mode * 4);
                    if (word(slots, index * 12) != 0 && (flags & present == present || flags & alternate == alternate)) || flags & defaults == defaults { count = count.wrapping_add(1); }
                }
            }
        }
        copy_local(destination.0, &count, 2);
        let first = destination.0.wrapping_add(2);
        let mut cursor = first;
        let mut values = destination.0.wrapping_add(6).wrapping_add((count as u32).wrapping_mul(12));
        let table = xm1_pipeline_image_select_ifd_table(code).0;
        let length = xm1_pipeline_image_ifd_table_count(code);
        let slots = xm1_pipeline_image_saved_directory_slots(code, &parser).0;
        let payload = xm1_pipeline_image_saved_directory_values(code, &parser).0;
        if code.0 == 0 { copy_identity(source); }
        let base = Address32(word(WRITER, if code.0 == 2 { 28 } else { 0 }));
        debug_directory(code, source, Address32(cursor), base);
        for index in 0..length {
            let rule = RuleAddress(table.wrapping_add(index * 32));
            let slot = slots.wrapping_add(index * 12);
            let tag = TagId(half(rule.0, 0));
            if code.0 == 2 && pass.0 == 0 && xm1_pipeline_image_saved_tag_available(tag, source) == 0 { continue; }
            let flags = word(rule.0, if pass.0 == 0 { 20 } else { 24 });
            let mode = half(SETTINGS, 0) as u32;
            let present = word(0x11d24e4, mode * 4);
            let alternate = word(0x11d040c, mode * 4);
            if flags & present != present && flags & alternate != alternate { continue; }
            let saved_count = word(slot, 0);
            if saved_count != 0 && flags & present == present {
                let mut entry = TiffEntry { tag: tag.0, kind: half(slot, 8), count: saved_count, value: 0 };
                let size = entry.count.wrapping_mul(xm1_pipeline_image_tiff_type_size(entry.kind as u32));
                if size <= 4 { copy(&mut entry.value as *mut u32 as u32, payload.wrapping_add(word(slot, 4)), 4); }
                else {
                    entry.value = values.wrapping_sub(base.0);
                    copy(values, payload.wrapping_add(word(slot, 4)), size);
                    if size & 1 != 0 { Address32(values.wrapping_add(size)).write_byte(0); }
                    values = values.wrapping_add(size).wrapping_add(size & 1);
                }
                copy_local(cursor, &entry, 12);
                cursor = cursor.wrapping_add(12);
            } else {
                let defaults = word(0x11d2314, mode * 4);
                if !((saved_count != 0 && flags & alternate == alternate) || (saved_count == 0 && flags & defaults == defaults)) { continue; }
                let mut entry = TiffEntry { tag: tag.0, kind: half(rule.0, 4), count: xm1_pipeline_image_tiff_value_capacity(code, rule, source), value: 0 };
                let callback = word(rule.0, 8);
                if callback == 0 {
                    xm1_pipeline_image_write_tiff_value(Address32(rule.0), Address32(source.0.wrapping_add(word(rule.0, 28))), Address32(cursor), entry.count, base, &mut values);
                } else {
                    let mut layout = value_layout(&entry);
                    invoke_rule_callback(callback, source, Address32(cursor), Address32(values), &mut layout, &mut entry);
                    if layout.indirect == 1 {
                        entry.value = values.wrapping_sub(base.0);
                        xm1_pipeline_image_allocate_tiff_value(ByteOffset(entry.count.wrapping_mul(layout.width as u32)), &mut values);
                    }
                    copy_local(cursor, &entry, 12);
                }
                cursor = cursor.wrapping_add(12);
            }
        }
        if code.0 == 0 { store(WRITER, 40, cursor); }
        copy_local(cursor, &0u32, 4);
        values.wrapping_add(2).wrapping_sub(first)
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CaptureJob(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct DirectoryIndex(pub i32);
#[repr(C)]
pub struct FoundTag { count: u32, data: Address32, kind: u16 }

unsafe fn clear_found(output: *mut FoundTag) {
    unsafe {
        write_volatile(&mut (*output).kind, 0);
        write_volatile(&mut (*output).count, 0);
        write_volatile(&mut (*output).data, Address32(0));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_find_directory_tag(base: Address32, size: ByteOffset, index: DirectoryIndex, tag: u32, output: *mut FoundTag, bank: SavedBank) -> i8 {
    unsafe {
        let mut parser = core::mem::MaybeUninit::<SavedParser>::uninit();
        xm1_pipeline_image_transfer_saved_parser(parser.as_mut_ptr(), bank, TransferDirection(0));
        clear_found(output);
        let descriptor = base.0.wrapping_add((index.0 as u32).wrapping_mul(8));
        let offset = encoded_word(descriptor);
        let length = encoded_word(descriptor.wrapping_add(4));
        if offset == 0 || length == 0 || length.wrapping_add(offset) > size.0 { return -1; }
        let directory = base.0.wrapping_add(offset);
        let count = encoded_half(directory) as u32;
        if count == 0 || count.wrapping_mul(12).wrapping_add(6) > length { return -1; }
        let mut entry = directory.wrapping_add(2);
        let mut position = 0u32;
        while position < count {
            if encoded_half(entry) as u32 == tag { break; }
            entry = entry.wrapping_add(12);
            position = position.wrapping_add(1);
        }
        if position >= count { return -1; }
        let kind = encoded_half(entry.wrapping_add(2));
        let elements = encoded_word(entry.wrapping_add(4));
        let value_address = entry.wrapping_add(8);
        let value = encoded_word(value_address);
        if elements == 0 { return -1; }
        let table = DirectoryKind::ImageRecord.table().0;
        let mut rule = table;
        let mut position = 0u32;
        while half(rule, 0) as u32 != tag {
            position = position.wrapping_add(1);
            if position == DirectoryKind::ImageRecord.count() { return -1; }
            rule = rule.wrapping_add(32);
        }
        if kind != half(rule, 4) { return -1; }
        let bytes = elements.wrapping_mul(xm1_pipeline_image_tiff_type_size(kind as u32));
        let address = if bytes > 4 {
            if size.0 < bytes.wrapping_add(value) { return -1; }
            Address32(base.0.wrapping_add(value))
        } else { Address32(value_address) };
        write_volatile(&mut (*output).kind, kind);
        write_volatile(&mut (*output).count, elements);
        write_volatile(&mut (*output).data, address);
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_find_primary_directory_tag(base: Address32, size: ByteOffset, index: DirectoryIndex, tag: u32, output: *mut FoundTag) -> i8 {
    unsafe { xm1_pipeline_image_find_directory_tag(base, size, index, tag, output, SavedBank(0)) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_find_saved_tag(code: DirectoryCode, tag: TagId, output: *mut FoundTag, bank: SavedBank) -> i8 {
    unsafe {
        let mut parser = core::mem::MaybeUninit::<SavedParser>::uninit();
        xm1_pipeline_image_transfer_saved_parser(parser.as_mut_ptr(), bank, TransferDirection(0));
        let parser = parser.assume_init();
        let code = DirectoryCode(code.0 as u8 as u32);
        let count = xm1_pipeline_image_ifd_table_count(code);
        let mut slot = xm1_pipeline_image_saved_directory_slots(code, &parser).0;
        let base = xm1_pipeline_image_saved_directory_values(code, &parser).0;
        let mut rule = xm1_pipeline_image_select_ifd_table(code).0;
        clear_found(output);
        let mut index = 0u32;
        while index < count {
            if half(rule, 0) == tag.0 { break; }
            rule = rule.wrapping_add(32);
            slot = slot.wrapping_add(12);
            index = index.wrapping_add(1);
        }
        if half(rule, 0) != tag.0 || word(slot, 0) == 0 { return -1; }
        write_volatile(&mut (*output).kind, half(slot, 8));
        write_volatile(&mut (*output).count, word(slot, 0));
        write_volatile(&mut (*output).data, Address32(base.wrapping_add(word(slot, 4))));
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_find_primary_saved_tag(code: DirectoryCode, tag: TagId, output: *mut FoundTag) -> i8 {
    unsafe { xm1_pipeline_image_find_saved_tag(code, tag, output, SavedBank(0)) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_populate_special_metadata(job: CaptureJob, metadata: MetadataBlock) {
    unsafe {
        let saved = word(job.0, 0x19b0);
        let mut found = FoundTag { count: 0, data: Address32(0), kind: 0 };
        if word(saved, 0x680) == 0 { store(metadata.0, 0x1d38, 0); }
        else {
            store(metadata.0, 0x1d38, 1);
            let mut records = FoundTag { count: 0, data: Address32(0), kind: 0 };
            xm1_pipeline_image_find_primary_saved_tag(DirectoryCode(2), TagId(0x4282), &mut records);
            let mut selections = word(saved, 0x654);
            let mut row = 0u16;
            let mut record_index = 0u16;
            while row < half(saved, 0x650) {
                let selection = half(selections, 0);
                selections = selections.wrapping_add(2);
                let row_address = metadata.0.wrapping_add((row as u32).wrapping_mul(832));
                if (selection as i16) < 0 {
                    store(row_address, 0x1d3c, 1);
                    loop {
                        let status = xm1_pipeline_image_find_primary_directory_tag(records.data, ByteOffset(records.count), DirectoryIndex(record_index as i32), 0x100, &mut found);
                        if status == 0 && encoded_word(found.data.0) != u32::MAX { break; }
                        record_index = record_index.wrapping_add(1);
                    }
                    copy(row_address.wrapping_add(0x1d40), found.data.0, 4);
                    if xm1_pipeline_image_find_primary_directory_tag(records.data, ByteOffset(records.count), DirectoryIndex(record_index as i32), 0x1000, &mut found) == 0 { copy(row_address.wrapping_add(0x1d44), found.data.0, 2); }
                    if xm1_pipeline_image_find_primary_directory_tag(records.data, ByteOffset(records.count), DirectoryIndex(record_index as i32), 0x1001, &mut found) == 0 { copy(row_address.wrapping_add(0x1d46), found.data.0, half(row_address, 0x1d44) as u32); }
                    if xm1_pipeline_image_find_primary_directory_tag(records.data, ByteOffset(records.count), DirectoryIndex(record_index as i32), 0x1020, &mut found) == 0 { copy(row_address.wrapping_add(0x1d68), found.data.0, 4); }
                    if xm1_pipeline_image_find_primary_directory_tag(records.data, ByteOffset(records.count), DirectoryIndex(record_index as i32), 0x1021, &mut found) == 0 { copy(row_address.wrapping_add(0x1d6c), found.data.0, 8); }
                    record_index = record_index.wrapping_add(1);
                } else { store(row_address, 0x1d3c, 0); }
                store_half(metadata.0, (row as u32).wrapping_add(708).wrapping_mul(2).wrapping_add(4), half(selections.wrapping_sub(2), 0));
                row = row.wrapping_add(1);
            }
        }
        xm1_pipeline_image_find_primary_saved_tag(DirectoryCode(2), TagId(0x1434), &mut found);
        copy(metadata.0.wrapping_add(0xe898), found.data.0, 8);
        if xm1_pipeline_image_find_primary_saved_tag(DirectoryCode(2), TagId(0x1020), &mut found) != 0 { store_half(metadata.0, 0x6e, 0xffff); }
        else { copy(metadata.0.wrapping_add(0x6e), found.data.0, 2); }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConfigurationKey(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ExposureSelection(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SensitivitySelection(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SensitivityValue(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SensitivityDelta(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ExposureMode(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ExposureRounding(pub i32);

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: ConfigurationKey) -> u32;
    fn xm1_buffers_configuration_half(key: ConfigurationKey) -> u16;
    fn xm1_develop_lookup_width(selection: u32, aspect: u32) -> u32;
    fn xm1_develop_lookup_height(selection: u32, aspect: u32) -> u32;
    fn __aeabi_uidiv(numerator: u32, denominator: u32) -> u32;
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_exposure_value(selection: ExposureSelection, mode: ExposureMode, rounding: ExposureRounding) -> i32 {
    unsafe {
        if mode.0 == 0 {
            return match selection.0 {
                0 => 0,
                -1 => word(0x9134d0, 0) as i32,
                13..=64 => word(0x816bb8, (selection.0 as u32).wrapping_mul(8)) as i32,
                value if value <= 64 => -3000,
                _ => 400000,
            };
        }
        let value = selection.0;
        let index = if value < -1400 { -140 } else if value <= 1600 { value / 10 } else { 160 };
        let table = if (rounding.0 as u32).wrapping_sub(1) <= 1 { 0x8165c8 } else { 0x81690c };
        let mut value = half(table, ((index + 140) as u32).wrapping_mul(2)) as i16 as i32;
        if (index as u32).wrapping_add(99) > 198 { value = value.wrapping_mul(100); }
        value.wrapping_mul(10)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_sensitivity_iso(value: SensitivityValue) -> u16 {
    unsafe {
        let index = (value.0 / 10).clamp(30, 130);
        half(0x8167e8, index.wrapping_mul(2))
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_sensitivity_delta(delta: SensitivityDelta) -> i32 {
    unsafe { half(0x8168f6, delta.0.wrapping_mul(2)) as i16 as i32 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_sensitivity_parameter(selection: SensitivitySelection) -> i32 {
    unsafe {
        let (key, increment) = match selection.0 {
            1 => (0x1902, 0),
            2 => (0x1904, 0),
            3..=17 => (0x1906 + ((selection.0 - 3) / 3) * 2, match (selection.0 - 3) % 3 { 0 => 0, 1 => 33, _ => 67 }),
            18 => (0x1910, 0),
            19 => (0x1912, 0),
            20 => (0x1914, 0),
            _ => return 0,
        };
        xm1_buffers_configuration_half(ConfigurationKey(key)).wrapping_add(increment) as i16 as i32
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_sensitivity_base(selection: SensitivitySelection) -> i32 {
    unsafe {
        let parameter = match selection.0 { 0..=5 => 0, 6..=7 => 1, 8..=9 => 2, 10..=25 => selection.0 - 7, 27 => 19, 28 => 20, _ => return 0 };
        xm1_pipeline_image_metadata_sensitivity_parameter(SensitivitySelection(parameter))
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_current_sensitivity(selection: SensitivitySelection) -> i32 {
    unsafe { xm1_pipeline_image_metadata_sensitivity_base(selection) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_sensitivity_threshold(value: SensitivityValue) -> u32 {
    unsafe {
        let mut row = 0x816e20;
        let mut index = 1u32;
        loop {
            let limit = (xm1_buffers_configuration_half(ConfigurationKey(word(row, 16))) as u32).wrapping_add(word(row, 20));
            if value.0 < limit { break; }
            index = index.wrapping_add(1);
            row += 16;
            if index == 20 { break; }
        }
        word(0x816e18, index.wrapping_mul(16))
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_apply_sensitivity_delta(selection: SensitivitySelection, delta: SensitivityDelta) -> u32 {
    unsafe {
        let base = xm1_pipeline_image_metadata_sensitivity_base(selection);
        let offset = xm1_pipeline_image_metadata_sensitivity_delta(delta);
        xm1_pipeline_image_metadata_sensitivity_threshold(SensitivityValue(base.wrapping_add(offset) as u16 as u32))
    }
}

#[derive(Clone, Copy)]
enum SensorAxis { Horizontal, Vertical }

unsafe fn metadata_sensor_extent(job: CaptureJob, axis: SensorAxis) -> u32 {
    unsafe {
        let flags = word(job.0, 0x49c);
        let multiplier = if flags & 0x100 != 0 { 2 } else if flags & 0x200 != 0 { 3 } else if flags & 0x4000 == 0 { 1 } else { 2 };
        let extent = word(job.0, match axis { SensorAxis::Horizontal => 0x430, SensorAxis::Vertical => 0x434 }).wrapping_mul(multiplier);
        let (selection, aspect) = if word(job.0, 0x1678) == 0 { (word(job.0, 0x644), word(job.0, 0x4f0)) } else { (33, 1) };
        let output = match axis { SensorAxis::Horizontal => xm1_develop_lookup_width(selection, aspect), SensorAxis::Vertical => xm1_develop_lookup_height(selection, aspect) };
        let crop = match word(job.0, 0x4b4) {
            26 => xm1_buffers_configuration_byte(ConfigurationKey(0x49d0)),
            25 => xm1_buffers_configuration_byte(ConfigurationKey(0x48a2)),
            _ => half(job.0, 0x6c2) as u32,
        };
        let crop = crop.wrapping_mul(xm1_buffers_configuration_byte(ConfigurationKey(0x254)));
        let adjustment = match axis {
            SensorAxis::Horizontal => if word(job.0, 0x63c) == 0 { crop } else { ((crop as i32) / 2) as u32 },
            SensorAxis::Vertical => {
                let product = if word(job.0, 0x63c) == 0 { crop.wrapping_mul(2) } else { crop.wrapping_add(crop >> 31) & !1 };
                ((product as i32) / 3) as u32
            }
        };
        let scale = __aeabi_uidiv(extent.wrapping_sub(adjustment).wrapping_shl(12), output);
        output.wrapping_mul(scale) >> 12
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_sensor_width(job: CaptureJob, _reserved: u32) -> u32 {
    unsafe { metadata_sensor_extent(job, SensorAxis::Horizontal) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_sensor_height(job: CaptureJob) -> u32 {
    unsafe { metadata_sensor_extent(job, SensorAxis::Vertical) }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PixelExtent(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct QualitySlot(pub i32);

unsafe extern "C" {
    fn xm1_pipeline_image_copy_capture_date(job: CaptureJob, metadata: Address32);
    fn xm1_pipeline_image_copy_exif_creation_date(job: CaptureJob, metadata: Address32, primary: Address32);
    fn xm1_pipeline_image_copy_exif_digitization_date(job: CaptureJob, metadata: Address32, primary: Address32);
    fn xm1_pipeline_image_store_exposure_compensation(job: CaptureJob, metadata: Address32);
    fn xm1_pipeline_image_rotate_metadata_orientation(selection: u32, orientation: u32) -> u32;
    fn xm1_pipeline_image_populate_location_metadata(job: CaptureJob, metadata: Address32);
    fn xm1_pipeline_image_metadata_quality_selection(job: CaptureJob) -> u32;
    fn xm1_pipeline_image_get_metadata_quality(kind: u32, width: PixelExtent, height: PixelExtent, slot: QualitySlot, reserved: u32, output: *mut Address32) -> u32;
    fn xm1_pipeline_image_compression_band(selection: u32) -> u32;
}

unsafe fn fill(destination: u32, length: u32, value: u8) {
    unsafe { for index in 0..length { Address32(destination.wrapping_add(index)).write_byte(value); } }
}
unsafe fn words(destination: u32, offset: u32, values: &[u32]) {
    unsafe { for (index, value) in values.iter().enumerate() { store(destination, offset.wrapping_add((index as u32).wrapping_mul(4)), *value); } }
}
fn orientation_tag(value: u32) -> u16 { match value { 2 => 6, 3 => 3, 4 => 8, _ => 1 } }
fn adjustable_capture_mode(value: u32) -> bool { matches!(value, 2..=4 | 29 | 31 | 33..=36) }
fn portrait_scene(value: u32) -> bool { matches!(value, 5 | 7 | 0x30..=0x39 | 0x41 | 0x42 | 0x44 | 0x45 | 0x47 | 0x48) }
fn action_scene(value: u32) -> bool { matches!(value, 8 | 12 | 13 | 16 | 17 | 19 | 0x40 | 0x43 | 0x46) }

unsafe fn output_flash_tag(job: CaptureJob) -> (u16, bool) {
    unsafe {
        let capture = word(job.0, 0x4b4);
        if (capture == 15 && half(job.0, 0x1680) == 1) || capture.wrapping_sub(25) < 2 || word(job.0, 0x710) & 0x400 != 0 { return (16, false); }
        let selection = word(job.0, 0x67c);
        let control = word(job.0, 0x4e0);
        if control == 1 { return (9, false); }
        if control == 0 && word(job.0, 0x4d4) == 1 && !matches!(selection, 7 | 10) { return (16, true); }
        if control == 2 && selection == 7 { return (9, true); }
        match selection {
            0 => (if word(job.0, 0x714) == 0 { 25 } else { 24 }, true),
            1 => (if word(job.0, 0x714) == 0 { 89 } else { 24 }, true),
            2 => (9, true),
            3 => (73, true),
            4 => (if matches!(word(job.0, 0x4d0), 2..=4) { 9 } else { 25 }, true),
            5 => (if matches!(word(job.0, 0x4d0), 2 | 3 | 5) { 73 } else { 89 }, true),
            6 => (if word(job.0, 0x4d4) == 1 || word(job.0, 0x4d0) == 6 { 16 } else { 24 }, true),
            7 | 10 => (9, false),
            _ => (9, true),
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_populate_output_metadata(job: CaptureJob, primary: MetadataBlock, auxiliary: MetadataBlock, secondary: MetadataBlock, maker: MetadataBlock, location: MetadataBlock, paired: u32) {
    unsafe {
        xm1_pipeline_image_copy_capture_date(job, Address32(primary.0));
        let capture = word(job.0, 0x4b4);
        let orientation = if capture == 30 { 1 } else if capture == 29 {
            let rotated = xm1_pipeline_image_rotate_metadata_orientation(word(job.0, 0x1604), word(job.0, 0x5d0));
            if rotated == 3 { 1 } else { orientation_tag(rotated) }
        } else { orientation_tag(word(job.0, 0x1604)) };
        store_half(primary.0, 0x84, orientation);
        if word(job.0, 0x5f0) == 1 { words(primary.0, 0xf4, &[313,1000,329,1000]); } else { fill(primary.0.wrapping_add(0xf4), 16, 0); }
        if word(job.0, 0x5f0) == 1 { words(primary.0, 0x104, &[64,100,33,100,21,100,71,100,15,100,6,100]); } else { fill(primary.0.wrapping_add(0x104), 48, 0); }
        if word(job.0, 0x5f0) == 1 { words(primary.0, 0x134, &[299,1000,587,1000,114,1000]); } else { fill(primary.0.wrapping_add(0x134), 24, 0); }
        if word(job.0, 0x5f0) == 1 { words(primary.0, 0x160, &[6098,10000,2053,10000,1492,10000,3111,10000,6257,10000,632,10000,194,10000,609,10000,7448,10000]); }
        else { words(primary.0, 0x160, &[4361,10000,3851,10000,1431,10000,2224,10000,7169,10000,606,10000,139,10000,971,10000,7141,10000]); }
        Address32(primary.0.wrapping_add(0x15c)).write_byte(1);
        Address32(primary.0.wrapping_add(0x1a8)).write_byte(0);
        let orientation = if word(job.0, 0x4b4) == 29 {
            let rotated = xm1_pipeline_image_rotate_metadata_orientation(word(job.0, 0x1604), word(job.0, 0x5d0));
            if rotated == 3 { 1 } else { orientation_tag(rotated) }
        } else { orientation_tag(word(job.0, 0x1604)) };
        store_half(auxiliary.0, 42, orientation);
        store(secondary.0, 0x15c, half(job.0, 0x73a) as i16 as i32 as u32);
        store(secondary.0, 0x160, 100);
        store(secondary.0, 0x14c, word(job.0, 0x648));
        store(secondary.0, 0x150, 100);
        xm1_pipeline_image_store_exposure_compensation(job, Address32(secondary.0));
        let width = if word(job.0, 0x1678) == 0 { word(job.0, 0x1770) } else { xm1_develop_lookup_width(33, 1) };
        store(secondary.0, 0x28c, width);
        let height = if word(job.0, 0x1678) == 0 { word(job.0, 0x176c) } else { xm1_develop_lookup_height(33, 1) };
        store(secondary.0, 0x290, height);
        if word(job.0, 0x62c) == 0 { store(secondary.0, 8, half(job.0, 0x658) as u32); store(secondary.0, 12, 100); }
        else { store(secondary.0, 8, 0); store(secondary.0, 12, 0); }
        let mut quality = Address32(0);
        let (width, height) = if word(job.0, 0x1678) == 0 { (word(job.0, 0x1770), word(job.0, 0x176c)) } else { (xm1_develop_lookup_width(33, 1), xm1_develop_lookup_height(33, 1)) };
        let kind = if matches!(word(job.0, 0x508), 4 | 7..=10) { 3 } else { 1 };
        let selected = xm1_pipeline_image_metadata_quality_selection(job);
        let slot = match selected { 2 => 2, 3 => match word(job.0, 0x514) { 1 => 0, 0 => 1, _ => 2 }, 1 => 0, _ => 1 };
        let sensitivity = if word(job.0, 0x4a8) == 8 { xm1_pipeline_image_metadata_apply_sensitivity_delta(SensitivitySelection(word(job.0, 0x66c)), SensitivityDelta(word(job.0, 0x1d3c))) } else { word(job.0, 0x66c) };
        let band = xm1_pipeline_image_compression_band(sensitivity);
        if xm1_pipeline_image_get_metadata_quality(kind, PixelExtent(width & 0xffff), PixelExtent(height & 0xffff), QualitySlot(slot), band, &mut quality) == 0 && quality.0 != 0 {
            store(secondary.0, 0x144, quality.byte() as u32);
            store(secondary.0, 0x148, Address32(quality.0.wrapping_add(1)).byte() as u32);
        }
        if word(job.0, 0x62c) == 0 {
            let rounded = (word(job.0, 0x654).wrapping_mul(10) / 10).wrapping_add(5);
            store(secondary.0, 0x154, (rounded / 10).wrapping_mul(10));
            store(secondary.0, 0x158, 100);
        } else { store(secondary.0, 0x154, 0); store(secondary.0, 0x158, 0); }
        if word(job.0, 0x62c) == 0 { store(secondary.0, 0x16c, word(job.0, 0x3254)); store(secondary.0, 0x170, 100); }
        else { store(secondary.0, 0x16c, 0); store(secondary.0, 0x170, 0); }
        store(secondary.0, 0x17c, if word(job.0, 0x62c) == 0 { word(job.0, 0x1760) } else { word(job.0, 0x628).wrapping_mul(100) });
        store(secondary.0, 0x180, 100);
        let width = if word(job.0, 0x1678) != 0 { xm1_develop_lookup_width(33, 1) } else { word(job.0, if word(job.0, 0x4f0) == 3 { 0x176c } else { 0x1770 }) };
        let density = if word(job.0, 0x4b4) == 29 { 988 }
        else if word(job.0, 0x27ac) == 1 && word(job.0, 0x600) == 1 { 0 }
        else {
            let extent = if word(job.0, 0x1678) == 0 && word(job.0, 0x4f0) == 3 { xm1_pipeline_image_metadata_sensor_height(job) } else { xm1_pipeline_image_metadata_sensor_width(job, 0) };
            __aeabi_uidiv(width.wrapping_mul(2092), extent)
        };
        store(secondary.0, 0x294, density);
        store(secondary.0, 0x298, 1);
        store(secondary.0, 0x29c, word(secondary.0, 0x294));
        store(secondary.0, 0x2a0, 1);
        let mode = word(job.0, 0x4b8);
        let rounding = if mode == 2 { 0 } else if mode == 3 { (word(job.0, 0x4c0) == u32::MAX) as i32 } else {
            match if mode == 1 { 35 } else { word(job.0, 0x4b4) } {
                12 | 13 => (word(job.0, 0x57c) != 1) as i32,
                18 | 34 => 0,
                36 => (word(job.0, 0x4c0) == u32::MAX) as i32,
                _ => 1,
            }
        };
        let exposure = xm1_pipeline_image_metadata_exposure_value(ExposureSelection(word(job.0, 0x648) as i32), ExposureMode(1), ExposureRounding(rounding));
        if exposure < 0 { store(secondary.0, 0, (exposure.wrapping_neg() / 10) as u32); store(secondary.0, 4, 10); }
        else { store(secondary.0, 0, 10); store(secondary.0, 4, (exposure / 10) as u32); }
        let program = match word(job.0, 0x4b4) {
            1 => { let scene = word(job.0, 0x638); if portrait_scene(scene) { 7 } else if action_scene(scene) { 8 } else { 2 } }
            5 | 6 | 24 => 7, 8..=10 | 12 | 13 => 8, 11 => 6, 18 | 36 => 1, 34 => 4, 35 => 3, _ => 2,
        };
        store_half(secondary.0, 16, program);
        let metering = if adjustable_capture_mode(word(job.0, 0x4b4)) { match word(job.0, 0x4e4) { 0 => 5, 1 => 3, _ => 1 } } else { 5 };
        store_half(secondary.0, 0x174, metering);
        let (flash, adjust) = output_flash_tag(job);
        store_half(secondary.0, 0x178, flash);
        if adjust && half(secondary.0, 0x178) & 1 != 0 && word(job.0, 0x718) == 3 { store_half(secondary.0, 0x178, half(secondary.0, 0x178) & !1); }
        store_half(secondary.0, 0x28a, if word(job.0, 0x5f0) == 1 { 0xffff } else { 1 });
        store_half(secondary.0, 0x2a4, 3);
        store_half(secondary.0, 0x2a6, 2);
        let sensitivity = word(job.0, 0x4bc);
        if word(job.0, 0x4a8) == 8 && sensitivity > 5 {
            let base = xm1_pipeline_image_metadata_current_sensitivity(SensitivitySelection(sensitivity));
            let delta = xm1_pipeline_image_metadata_sensitivity_delta(SensitivityDelta(word(job.0, 0x1d3c)));
            store_half(secondary.0, 18, xm1_pipeline_image_metadata_sensitivity_iso(SensitivityValue(base.wrapping_add(delta) as u16 as u32)));
        } else {
            let iso = match sensitivity {
                0..=5 => {
                    let base = half(job.0, 0x732) as u32;
                    let value = if word(job.0, 0x4a8) == 8 { (base as i32).wrapping_add(xm1_pipeline_image_metadata_sensitivity_delta(SensitivityDelta(word(job.0, 0x1d3c)))) as u16 as u32 } else { base };
                    xm1_pipeline_image_metadata_sensitivity_iso(SensitivityValue(value))
                }
                6 => 64, 7 => 80, 8 => 100, 9 => 160, 10 => 200, 11 => 250, 12 => 320, 13 => 400, 14 => 500, 15 => 640, 16 => 800, 17 => 1000, 18 => 1250, 19 => 1600, 20 => 2000, 21 => 2500, 22 => 3200, 23 => 4000, 24 => 5000, 25 => 6400, 26 => 10000, 27 => 12800, _ => 25600,
            };
            store_half(secondary.0, 18, iso);
            if word(job.0, 0x4a8) != 8 && word(job.0, 0x4bc) == 1 && half(secondary.0, 18) > 400 { store_half(secondary.0, 18, 400); }
        }
        store_half(secondary.0, 0x2b8, (word(job.0, 0x62c) == 0) as u16);
        let white_balance = if word(job.0, 0x718) == 3 { match word(job.0, 0x4f4) { 1 => 1, 2 => 11, 3 => 12, 4 => 13, 5 => 14, 6 => 3, _ => 0 } } else { 0 };
        store_half(secondary.0, 0x176, white_balance);
        store_half(secondary.0, 0x2ae, if word(job.0, 0x4a8) == 4 { 2 } else { (word(job.0, 0x4b4) == 36) as u16 });
        let white_balance = word(job.0, 0x4f4);
        let custom = if adjustable_capture_mode(word(job.0, 0x4b4)) { white_balance.wrapping_sub(8) < 3 || (word(job.0, 0x718) == 3 && !matches!(white_balance, 0 | 7)) } else { false };
        store_half(secondary.0, 0x2b0, custom as u16);
        let scene = match word(job.0, 0x4b4) {
            1 => { let scene = word(job.0, 0x638); if portrait_scene(scene) { 2 } else if matches!(scene, 12 | 13) { 3 } else if action_scene(scene) { 1 } else { 0 } }
            5 | 6 | 24 => 2, 8..=10 => 1, 12 | 13 => 3, _ => 0,
        };
        store_half(secondary.0, 0x2b2, scene);
        let capture = word(job.0, 0x4b4);
        let contrast = if capture == 23 { 2 } else if matches!(capture, 2..=4 | 31 | 33..=36) {
            match word(job.0, 0x500) { 0 | 1 => 1, 3 | 4 => 2, _ => 0 }
        } else if matches!(word(job.0, 0x508), 1 | 3 | 11 | 12) { 0xffff } else { 0 };
        store_half(secondary.0, 0x2b4, contrast);
        store_half(secondary.0, 0x2b6, 0);
        let rendered = !matches!(word(job.0, 0x4b4), 38 | 40) && matches!(word(job.0, 0x508), 4 | 5 | 7..=10);
        store_half(secondary.0, 0x2ac, rendered as u16);
        xm1_pipeline_image_copy_exif_creation_date(job, Address32(secondary.0), Address32(primary.0));
        xm1_pipeline_image_copy_exif_digitization_date(job, Address32(secondary.0), Address32(primary.0));
        if word(job.0, 0x5f0) == 1 { store(secondary.0, 0x33c, 22); store(secondary.0, 0x340, 10); } else { fill(secondary.0.wrapping_add(0x33c), 8, 0); }
        let focal = if word(job.0, 0x62c) == 0 { word(job.0, 0x1760).wrapping_mul(15) / 100 } else { word(job.0, 0x628).wrapping_mul(15) };
        store_half(secondary.0, 0x284, (focal.wrapping_add(5) / 10) as u16);
        if word(job.0, 0x62c) == 0 {
            store(secondary.0, 0x2bc, word(job.0, 0x17d4)); store(secondary.0, 0x2c0, 100);
            store(secondary.0, 0x2cc, word(job.0, 0x17dc)); store(secondary.0, 0x2d0, 100);
            let (long, aperture) = if word(job.0, 0x17d4) == word(job.0, 0x17d8) { (word(job.0, 0x17d4), 0x17dc) } else { (word(job.0, 0x17d8), 0x17e0) };
            store(secondary.0, 0x2c4, long); store(secondary.0, 0x2c8, 100);
            store(secondary.0, 0x2d4, word(job.0, aperture)); store(secondary.0, 0x2d8, 100);
        } else {
            store(secondary.0, 0x2bc, word(job.0, 0x628).wrapping_mul(100)); store(secondary.0, 0x2c0, 100);
            store(secondary.0, 0x2c4, word(job.0, 0x628).wrapping_mul(100)); store(secondary.0, 0x2c8, 100);
            for offset in [0x2cc, 0x2d0, 0x2d4, 0x2d8] { store(secondary.0, offset, 0); }
        }
        for (target, origin, length) in [(0x2dc, 0x1778, 14), (0x2ea, 0x1786, 64), (0x32a, 0x17c6, 12)] {
            fill(secondary.0.wrapping_add(target), length, 0);
            copy(secondary.0.wrapping_add(target), job.0.wrapping_add(origin), length);
        }
        xm1_pipeline_image_populate_maker_metadata(job, maker, paired, 0, 0);
        xm1_pipeline_image_populate_location_metadata(job, Address32(location.0));
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct FaceRecords(pub u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RectangleWords(pub u32);

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_transform_metadata_rectangle(job: CaptureJob, width: PixelExtent, height: PixelExtent, rectangle: RectangleWords) {
    unsafe {
        let selection = word(job.0, 0x1758);
        let full_width = xm1_develop_lookup_width(selection, 1);
        let full_height = xm1_develop_lookup_height(selection, 1);
        let aspect = word(job.0, 0x4f0);
        let crop_width = xm1_develop_lookup_width(selection, aspect);
        let crop_height = xm1_develop_lookup_height(selection, aspect);
        let horizontal = full_width.wrapping_sub(crop_width) >> 1;
        let vertical = full_height.wrapping_sub(crop_height) >> 1;
        let right = horizontal.wrapping_add(word(rectangle.0, 4));
        let top = vertical.wrapping_add(word(rectangle.0, 8));
        let bottom = vertical.wrapping_add(word(rectangle.0, 12));
        let left = horizontal.wrapping_add(word(rectangle.0, 0));
        store(rectangle.0, 0, __aeabi_uidiv(width.0.wrapping_mul(left), full_width));
        store(rectangle.0, 4, __aeabi_uidiv(width.0.wrapping_mul(right), full_width));
        store(rectangle.0, 8, __aeabi_uidiv(height.0.wrapping_mul(top), full_height));
        store(rectangle.0, 12, __aeabi_uidiv(height.0.wrapping_mul(bottom), full_height));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_transform_metadata_faces(job: CaptureJob, records: FaceRecords) {
    unsafe {
        let width = xm1_develop_lookup_width(33, 1);
        let height = xm1_develop_lookup_height(33, 1);
        let original_width = word(job.0, 0x490);
        let difference = (half(job.0, 0x6c2) as u32).wrapping_sub(half(job.0, 0x6c4) as u32);
        let corrected_width = xm1_buffers_configuration_byte(ConfigurationKey(0x254)).wrapping_mul(difference).wrapping_add(original_width);
        let original_height = word(job.0, 0x494);
        let difference = (half(job.0, 0x6c4) as u32).wrapping_sub(half(job.0, 0x6c2) as u32);
        let correction = xm1_buffers_configuration_byte(ConfigurationKey(0x254)).wrapping_mul(difference).wrapping_mul(2) as i32 / 3;
        let corrected_height = original_height.wrapping_sub(correction as u32);
        let horizontal = word(job.0, 0x490).wrapping_sub(word(job.0, 0x460)) as i32 / 2;
        let vertical = word(job.0, 0x494).wrapping_sub(word(job.0, 0x464)) as i32 / 2;
        let (extra_horizontal, extra_vertical) = if word(job.0, 0x70c) == 1 {
            let difference = (half(job.0, 0x6c4) as u32).wrapping_sub(half(job.0, 0x6c2) as u32);
            let horizontal = xm1_buffers_configuration_byte(ConfigurationKey(0x254)).wrapping_mul(difference) as i32 / 2;
            let difference = (half(job.0, 0x6c4) as u32).wrapping_sub(half(job.0, 0x6c2) as u32);
            let vertical = xm1_buffers_configuration_byte(ConfigurationKey(0x254)).wrapping_mul(difference).wrapping_mul(2) as i32 / 6;
            (horizontal, vertical)
        } else { (0, 0) };
        let mut index = 0u32;
        while index < word(records.0, 0) {
            let record = records.0.wrapping_add(index.wrapping_mul(0xb8));
            let right = word(record, 0x10).wrapping_add(horizontal as u32).wrapping_add(extra_horizontal as u32);
            let top = word(record, 0x14).wrapping_add(vertical as u32).wrapping_add(extra_vertical as u32);
            let bottom = word(record, 0x18).wrapping_add(vertical as u32).wrapping_add(extra_vertical as u32);
            let left = word(record, 0xc).wrapping_add(horizontal as u32).wrapping_add(extra_horizontal as u32);
            store(record, 0xc, __aeabi_uidiv(width.wrapping_mul(left), corrected_width));
            store(record, 0x10, __aeabi_uidiv(width.wrapping_mul(right), corrected_width));
            store(record, 0x14, __aeabi_uidiv(height.wrapping_mul(top), corrected_height));
            store(record, 0x18, __aeabi_uidiv(height.wrapping_mul(bottom), corrected_height));
            index = index.wrapping_add(1);
        }
    }
}

fn maker_manual_mode(mode: u32) -> bool { matches!(mode, 2..=4 | 33..=36) }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_primary_mode(job: CaptureJob) -> u32 {
    unsafe {
        let mode = word(job.0, 0x4b4);
        if mode == 23 { return 4; }
        if maker_manual_mode(mode) || mode == 31 {
            match word(job.0, 0x500) { 0 => 2, 1 => 0x82, 3 => 0x84, 4 => 4, _ => 3 }
        } else if matches!(word(job.0, 0x508), 1 | 3 | 11 | 12) { 0x8000 } else { 3 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_secondary_mode(job: CaptureJob) -> u32 {
    unsafe {
        let mode = word(job.0, 0x4b4);
        if !maker_manual_mode(mode) && !matches!(mode, 29 | 31 | 44) { return 0; }
        match word(job.0, 0x4f4) { 1 => 0x100, 2 => 0x200, 3 => 0x300, 4 => 0x301, 5 => 0x302, 6 => 0x400, 7 => 0x600, 8 => 0xf00, 9 => 0xf01, 10 => 0xff0, _ => 0 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_color_mode(job: CaptureJob) -> u32 {
    unsafe {
        match word(job.0, 0x4b4) {
            5 | 6 => 1, 8..=10 => 2, 11 => 4, 12 => 0x17, 13 => 5,
            14 => 7, 15 => 0xf, 16 => 0x10, 17 => 0x11, 18 => 0x12,
            19 => 10, 20 => 0x13, 21 => 0xd, 22 => 0xc, 23 => 0xe,
            24 => 0x14, 25 => 0x18, 26 => 0x19, 27 => 0x1b, 28 => 0x1c,
            29 => 0x16, 32 => 0x40, 33 => 6, 34 => 0x200, 35 => 0x100,
            36 => 0x300, 47 => 8, 58 => 0xb, 62 => 0x15, _ => 0,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_processing_mode(job: CaptureJob) -> u32 {
    unsafe {
        let selection = word(job.0, 0x508);
        match selection {
            0 | 1 | 3 | 11 | 12 => {
                let mode = word(job.0, 0x4b4);
                if maker_manual_mode(mode) || matches!(mode, 29 | 31 | 44) {
                    match word(job.0, 0x504) { 2 => 0x100, 3 => 0x80, 4 => 0x400, 5 => 0x180, _ => 0 }
                } else if matches!(mode, 0 | 14 | 15 | 25 | 26 | 30 | 32 | 39 | 41 | 42) {
                    if matches!(selection, 1 | 3) { 0x8000 } else { 0 }
                } else { 0xffff }
            }
            4 | 7 => 0x300, 5 => 0x310, 8 => 0x301, 9 => 0x302, 10 => 0x303, _ => 0,
        }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct BalanceAxis(pub u32);

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_kelvin(job: CaptureJob) -> u32 {
    unsafe {
        if word(job.0, 0x4f4) != 10 { return 0; }
        match word(job.0, 0x5fc) {
            0 => 2500, 1 => 2550, 2 => 2650, 3 => 2700, 4 => 2800,
            5 => 2850, 6 => 2950, 7 => 3000, 8 => 3100, 9 => 3200,
            10 => 3300, 11 => 3400, 12 => 3600, 13 => 3700, 14 => 3800,
            15 => 4000, 16 => 4200, 17 => 4300, 18 => 4500, 19 => 4800,
            20 => 5000, 21 => 5300, 22 => 5600, 23 => 5900, 24 => 6300,
            25 => 6700, 26 => 7100, 27 => 7700, 28 => 8300, 29 => 9100,
            30 => 10000, _ => 0,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_tone_scale(job: CaptureJob) -> u32 {
    unsafe {
        let mode = word(job.0, 0x4b4);
        if maker_manual_mode(mode) || mode == 31 { 0x100 } else { 0x80 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_saturation(job: CaptureJob) -> u32 {
    unsafe { match word(job.0, 0x524) { 0 => 0x200, 1 => 0x280, 3 => 0x180, 4 => 0x100, _ => 0 } }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_balance_adjustment(job: CaptureJob, axis: BalanceAxis) -> i32 {
    unsafe {
        let value = word(job.0, axis.0.wrapping_add(0x13e).wrapping_mul(4)) as i32;
        if (-9..=9).contains(&value) { value * 20 } else { 0 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_flash_processing(job: CaptureJob) -> u32 {
    unsafe {
        let mut selection = word(job.0, 0x4d0);
        let control = word(job.0, 0x4e0);
        if control == 1 { selection = 7; }
        else if control == 0 {
            if word(job.0, 0x4d4) == 1 && !matches!(selection, 7 | 10) { selection = 6; }
        } else if control == 2 && selection == 7 { selection = 2; }
        match word(job.0, 0x4b4) {
            25 | 26 | 30 | 44 => 0,
            1 => (word(job.0, 0x67c).wrapping_sub(4) < 2) as u32,
            _ => if word(job.0, 0x680) == 1 { 0 } else { (selection.wrapping_sub(4) < 2) as u32 },
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_processing_profile(job: CaptureJob) -> u32 {
    unsafe {
        let mode = word(job.0, 0x4b4);
        if !maker_manual_mode(mode) && !matches!(mode, 0 | 14 | 15 | 25 | 26 | 29 | 30 | 31 | 44) { return 0xffff; }
        match word(job.0, 0x508) { 1 => 0x120, 3 => 0x200, 4 | 5 | 7..=10 => 0xffff, 11 => 0x500, 12 => 0x501, _ => 0 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_store_maker_exposure_bias(job: CaptureJob, metadata: MetadataBlock) {
    unsafe {
        if word(job.0, 0x4b4) == 44 { store(metadata.0, 0x64, 0); }
        else {
            let exposure = word(job.0, 0x4dc) as i32;
            let absolute = if exposure < 0 { exposure.wrapping_neg() } else { exposure };
            let scaled = absolute.wrapping_mul(1000) / 3;
            let rounded = ((scaled.wrapping_mul(10) / 10).wrapping_add(5)) / 10;
            store(metadata.0, 0x64, if exposure < 0 { rounded.wrapping_neg() } else { rounded } as u32);
        }
        store(metadata.0, 0x68, 100);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_store_maker_flash_mode(job: CaptureJob, metadata: MetadataBlock) {
    unsafe {
        let mut selection = word(job.0, 0x4d0);
        match word(job.0, 0x4e0) {
            1 => selection = 7,
            0 => if word(job.0, 0x4d4) == 1 && !matches!(selection, 7 | 10) { selection = 6; },
            2 => if selection == 7 { selection = 2; },
            _ => {}
        }
        let mode = word(job.0, 0x4b4);
        let value = if mode == 44 || (mode == 15 && half(job.0, 0x1680) == 1) || matches!(mode, 14 | 25 | 26 | 30 | 38 | 40..=42) { 2 }
            else if selection == 6 { 2 }
            else if word(job.0, 0x680) == 1 { 4 }
            else { match selection { 1 | 3 | 5 => 3, 2 | 4 => 1, 7 | 10 => 16, 8 => 4, _ => 0 } };
        store_half(metadata.0, 0x6c, value);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_pack_metadata_rectangle(destination: Address32, source: RectangleWords) {
    unsafe {
        store_half(destination.0, 0, word(source.0, 0) as u16);
        store_half(destination.0, 2, word(source.0, 8) as u16);
        store_half(destination.0, 4, word(source.0, 4) as u16);
        store_half(destination.0, 6, word(source.0, 12) as u16);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_pack_paired_metadata_rectangle(destination: Address32, source: RectangleWords) {
    unsafe { xm1_pipeline_image_pack_metadata_rectangle(destination, source); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_pack_metadata_direction(byte_destination: Address32, half_destination: Address32, source: RectangleWords) {
    unsafe {
        let direction = word(source.0, 0x10);
        let direction = if matches!(direction, 1 | 2) { direction << 4 } else { 0 };
        let selection = word(source.0, 0x14);
        let value = direction | if (1..=11).contains(&selection) { selection } else { 0 };
        if byte_destination.0 != 0 { Address32(byte_destination.0).write_byte(value as u8); }
        if half_destination.0 != 0 { store_half(half_destination.0, 0, value as u16); }
    }
}

unsafe extern "C" {
    fn xm1_pipeline_image_processed_width(profile: u32, aspect: u32, mode: u32) -> u16;
    fn xm1_pipeline_image_processed_height(profile: u32, aspect: u32, mode: u32) -> u16;
    fn xm1_pipeline_image_normalize_faces(frame: *mut u8, faces: *mut u8);
}

unsafe fn maker_extent(job: CaptureJob, axis: SensorAxis) -> u32 {
    unsafe {
        if word(job.0, 0x1678) == 0 {
            word(job.0, match axis { SensorAxis::Horizontal => 0x1770, SensorAxis::Vertical => 0x176c })
        } else {
            match axis { SensorAxis::Horizontal => xm1_develop_lookup_width(33, 1), SensorAxis::Vertical => xm1_develop_lookup_height(33, 1) }
        }
    }
}

unsafe fn maker_face_flags(job: CaptureJob, address: u32) {
    unsafe {
        let value = Address32(address).byte();
        Address32(address).write_byte((value & !1) | (word(job.0, 0x5a0) == 1) as u8);
        let enabled = word(job.0, 0x5a0) == 1 && word(job.0, 0x544) != 7;
        let value = Address32(address).byte();
        Address32(address).write_byte((value & !2) | ((enabled as u8) << 1));
        let value = Address32(address).byte();
        Address32(address).write_byte((value & !0xc) | (((word(job.0, 0x5ac) == 1) as u8) << 2));
    }
}

unsafe fn maker_focus_rectangle(job: CaptureJob) {
    unsafe {
        copy(0x15b53bc, job.0.wrapping_add(0x16a0), 0xb8);
        if word(job.0, 0x1678) != 0 {
            let width = xm1_develop_lookup_width(33, 1);
            let height = xm1_develop_lookup_height(33, 1);
            xm1_pipeline_image_transform_metadata_rectangle(job, PixelExtent(width), PixelExtent(height), RectangleWords(0x15b53bc));
        } else if word(job.0, 0x78) == 11 {
            xm1_pipeline_image_transform_metadata_rectangle(job, PixelExtent(word(job.0, 0x1770)), PixelExtent(word(job.0, 0x176c)), RectangleWords(0x15b53bc));
        }
    }
}

unsafe fn maker_focus_center(job: CaptureJob, metadata: MetadataBlock, axis: SensorAxis) {
    unsafe {
        let (destination, position, adjustment, rectangle) = match axis {
            SensorAxis::Horizontal => (0x74, 0x540, 0x698, 0),
            SensorAxis::Vertical => (0x76, 0x541, 0x699, 8),
        };
        let value = if word(job.0, 0x4b4) == 29 { 0xffff } else {
            let extent = maker_extent(job, axis);
            if word(job.0, 0x534) == 0 {
                let (profile, aspect) = if word(job.0, 0x1678) == 0 { (word(job.0, 0x644), word(job.0, 0x4f0)) } else { (33, 1) };
                let mode = Address32(job.0.wrapping_add(position)).byte() as i8 as i32 as u32;
                match axis {
                    SensorAxis::Horizontal => xm1_pipeline_image_processed_width(profile, aspect, mode),
                    SensorAxis::Vertical => xm1_pipeline_image_processed_height(profile, aspect, mode),
                }
            } else if word(job.0, 0x534) == 7 || word(job.0, 0x74c) == 0 || word(job.0, 0x5a0) != 1 || word(job.0, 0x169c) == 0 || word(job.0, 0x2734) != 1 || word(job.0, 0x16b8) != 0 {
                let adjustment = Address32(job.0.wrapping_add(adjustment)).byte() as i8 as i32;
                let extent_signed = extent as i32;
                let product = match axis {
                    SensorAxis::Horizontal => extent_signed.wrapping_mul(adjustment),
                    SensorAxis::Vertical => extent_signed.wrapping_neg().wrapping_mul(adjustment),
                };
                ((product.wrapping_mul(116) / 1000).wrapping_add(extent_signed / 2)) as u16
            } else {
                maker_focus_rectangle(job);
                let sum = word(0x15b53bc, rectangle).wrapping_add(word(0x15b53bc, rectangle + 4)) as i32;
                ((sum.wrapping_mul(50) / 10).wrapping_add(5) / 10) as u16
            }
        };
        store_half(metadata.0, destination, value);
    }
}

unsafe fn maker_normalize_records(job: CaptureJob, source: u32) {
    unsafe {
        copy(0x15b5474, source, 0x740);
        if word(job.0, 0x1678) == 0 {
            xm1_pipeline_image_normalize_faces(job.0 as *mut u8, 0x15b5474 as *mut u8);
        } else {
            xm1_pipeline_image_transform_metadata_faces(job, FaceRecords(0x15b5474));
        }
    }
}

unsafe fn maker_scene_components(job: CaptureJob, metadata: MetadataBlock) {
    unsafe {
        if word(job.0, 0x4b4) != 1 {
            store(metadata.0, 0x10c, 0);
            store_half(metadata.0, 0x110, 0);
            store_half(metadata.0, 0x112, 0xffff);
            return;
        }
        store(metadata.0, 0x10c, 1);
        let scene = word(job.0, 0x638);
        let category = match scene {
            5 | 0x30 | 0x34 | 0x36 | 0x38 | 0x41 | 0x44 | 0x47 => 0x100,
            7 | 0x31 | 0x35 | 0x37 | 0x39 | 0x42 | 0x45 | 0x48 => 0x105,
            8 | 0x10 | 0x11 | 0x13 | 0x40 | 0x43 | 0x46 => 0x200,
            0xc | 0xd => 0x300, 0x32 | 0x33 => 0x103, 0x49 => 0x400, _ => 0,
        };
        store_half(metadata.0, 0x110, category);
        let count = match scene {
            7 | 0x10 | 0x11 | 0x13 | 0x30 | 0x32 | 0x33 | 0x40 | 0x43 => 2,
            0x31 | 0x34 | 0x36 | 0x38 | 0x41 | 0x44 | 0x46 => 3,
            0x35 | 0x37 | 0x39 | 0x42 | 0x45 | 0x47 => 4, 0x48 => 5, _ => 1,
        };
        let count = count + ((word(job.0, 0x710) & 0x2000 != 0 && word(job.0, 0x638) != 0) as u16);
        let count = count + ((word(job.0, 0x710) & 0x400 != 0) as u16);
        store_half(metadata.0, 0x112, count);
        fill(metadata.0.wrapping_add(0x114), 32, 0);
        let components: &[u16] = match word(job.0, 0x638) {
            5 => &[0x10], 7 => &[0x10,0x50], 8 => &[0x20], 0xc => &[0x31], 0xd => &[0x30],
            0x10 => &[0x20,0x60], 0x11 => &[0x20,0x80], 0x13 => &[0x20,0x70],
            0x30 => &[0x10,0x20], 0x31 => &[0x10,0x20,0x50], 0x32 => &[0x10,0x31], 0x33 => &[0x10,0x30],
            0x34 => &[0x10,0x20,0x60], 0x35 => &[0x10,0x20,0x50,0x60],
            0x36 => &[0x10,0x20,0x80], 0x37 => &[0x10,0x20,0x50,0x80],
            0x38 => &[0x10,0x20,0x70], 0x39 => &[0x10,0x20,0x50,0x70],
            0x40 => &[0x20,0x90], 0x41 => &[0x10,0x20,0x90], 0x42 => &[0x10,0x20,0x50,0x90],
            0x43 => &[0x20,0xa0], 0x44 => &[0x10,0x20,0xa0], 0x45 => &[0x10,0x20,0x50,0xa0],
            0x46 => &[0x20,0x90,0xa0], 0x47 => &[0x10,0x20,0x90,0xa0],
            0x48 => &[0x10,0x20,0x50,0x90,0xa0], 0x49 => &[0x40], _ => &[0],
        };
        for (index, value) in components.iter().enumerate() { store_half(metadata.0, 0x114 + index as u32 * 2, *value); }
        let index = (half(metadata.0, 0x112) as u32).wrapping_sub(1);
        let flags = word(job.0, 0x710);
        if flags & 0x2000 == 0 {
            if flags & 0x400 != 0 { store_half(metadata.0, 0x114u32.wrapping_add(index.wrapping_mul(2)), 0x8000); }
        } else if flags & 0x400 == 0 {
            let offset = if word(job.0, 0x638) == 0 { 0x114 } else { 0x114u32.wrapping_add(index.wrapping_mul(2)) };
            store_half(metadata.0, offset, 0xb0);
        } else if word(job.0, 0x638) == 0 {
            store_half(metadata.0, 0x114, 0xb0); store_half(metadata.0, 0x116, 0x8000);
        } else {
            store_half(metadata.0, 0x112u32.wrapping_add(index.wrapping_mul(2)), 0xb0);
            store_half(metadata.0, 0x114u32.wrapping_add(index.wrapping_mul(2)), 0x8000);
        }
    }
}

unsafe fn maker_face_records(job: CaptureJob, metadata: MetadataBlock) {
    unsafe {
        let special = matches!(word(job.0, 0x4b4), 27 | 28);
        fill(0x15b5304, 0xb8, 0);
        maker_normalize_records(job, job.0.wrapping_add(0xe8c));
        if word(job.0, 0xe8c) != 0 {
            let source = if word(job.0, 0x1698) == 1 { 0x15b5480 } else { 0x15b5304 };
            xm1_pipeline_image_pack_metadata_rectangle(Address32(metadata.0.wrapping_add(0x20c)), RectangleWords(source));
            if word(job.0, 0xe8c) > 1 {
                let source = if word(job.0, 0x1698) == 1 { 0x15b5538 } else { 0x15b5304 };
                xm1_pipeline_image_pack_metadata_rectangle(Address32(metadata.0.wrapping_add(0x214)), RectangleWords(source));
            }
            store_half(metadata.0, 0x21c, if word(job.0, 0x1698) == 1 { half(job.0, 0xeb4) } else { 0 });
            if word(job.0, 0xe8c) > 1 { store_half(metadata.0, 0x21e, if word(job.0, 0x1698) == 1 { half(job.0, 0xf6c) } else { 0 }); }
            if word(job.0, 0x1698) == 1 {
                xm1_pipeline_image_pack_metadata_direction(Address32(metadata.0.wrapping_add(0x220)), Address32(0), RectangleWords(job.0.wrapping_add(0xe98)));
            } else { Address32(metadata.0.wrapping_add(0x220)).write_byte(0); }
            if word(job.0, 0xe8c) > 1 {
                if word(job.0, 0x1698) == 1 {
                    xm1_pipeline_image_pack_metadata_direction(Address32(metadata.0.wrapping_add(0x221)), Address32(0), RectangleWords(job.0.wrapping_add(0xf50)));
                } else { Address32(metadata.0.wrapping_add(0x221)).write_byte(0); }
            }
        }
        store_half(metadata.0, 0x58a, word(job.0, 0x74c) as u16);
        store_half(metadata.0, 0x222, if special { 0 } else { half(metadata.0, 0x58a) });
        let mut index = 0u32;
        while index < word(job.0, 0x74c) && index < 60 {
            let code = match word(job.0, 0x4b4) { 27 => 0x41, 28 => 0x81, _ => 1 };
            store_half(metadata.0, 0x58c + index * 2, code);
            if word(job.0, 0x7dc + index * 0xb8) == 1 {
                store_half(metadata.0, 0x58c + index * 2, half(metadata.0, 0x58c + index * 2) | 0x8000);
            }
            if !matches!(word(job.0, 0x4b4), 27 | 28) { Address32(metadata.0.wrapping_add(0x224 + index)).write_byte(half(metadata.0, 0x58c + index * 2) as u8); }
            index += 1;
        }
        let mut index = 0u32;
        while index < word(job.0, 0x74c) && index < 60 {
            Address32(metadata.0.wrapping_add(0x604 + index)).write_byte(4);
            if !matches!(word(job.0, 0x4b4), 27 | 28) { Address32(metadata.0.wrapping_add(0x22e + index)).write_byte(4); }
            index += 1;
        }
        maker_normalize_records(job, job.0.wrapping_add(0x74c));
        let mut index = 0u32;
        let mut components = 0u8;
        while index < word(0x15b5474, 0) && index < 60 {
            xm1_pipeline_image_pack_metadata_rectangle(Address32(metadata.0.wrapping_add(0x640 + index * 8)), RectangleWords(0x15b5480 + index * 0xb8));
            if !matches!(word(job.0, 0x4b4), 27 | 28) { copy(metadata.0.wrapping_add(0x238 + index * 8), metadata.0.wrapping_add(0x640 + index * 8), 8); }
            components = components.wrapping_add(4);
            index += 1;
        }
        store_half(metadata.0, 0xe040, components as u16);
        store_half(metadata.0, 0xe03c, components as u16);
        copy(0x15b5474, job.0.wrapping_add(0x74c), 0x740);
        let width = maker_extent(job, SensorAxis::Horizontal);
        let height = maker_extent(job, SensorAxis::Vertical);
        if word(job.0, 0x1678) == 0 { xm1_pipeline_image_normalize_faces(job.0 as *mut u8, 0x15b5474 as *mut u8); }
        else { xm1_pipeline_image_transform_metadata_faces(job, FaceRecords(0x15b5474)); }
        let count = word(0x15b5474, 0);
        for index in 0..count.min(60) {
            let source = 0x15b5474 + index * 0xb8;
            for (target, origin, divisor) in [(0,0xc,width),(2,0x14,height),(4,0x10,width),(6,0x18,height)] {
                let value = __aeabi_uidiv(word(source, origin).wrapping_shl(16), divisor);
                store_half(metadata.0, 0x820 + index * 8 + target, value as u16);
            }
            if !matches!(word(job.0, 0x4b4), 27 | 28) {
                for offset in [0,2,4,6] { store_half(metadata.0, 0x288 + index * 8 + offset, half(metadata.0, 0x820 + index * 8 + offset)); }
            }
        }
        let mut index = 0u32;
        while index < word(job.0, 0x74c) && index < 60 {
            store_half(metadata.0, 0xa00 + index * 2, 0);
            if !matches!(word(job.0, 0x4b4), 27 | 28) { store_half(metadata.0, 0x2d8 + index * 2, 0); }
            index += 1;
        }
        fill(metadata.0.wrapping_add(0xa78), 0xf00, 0);
        store_half(metadata.0, 0xe042, 0);
        if !matches!(word(job.0, 0x4b4), 27 | 28) {
            fill(metadata.0.wrapping_add(0x2ec), 0x280, 0);
            store_half(metadata.0, 0xe03e, half(metadata.0, 0xe042));
        }
        let mut index = 0u32;
        while index < word(job.0, 0x74c) && index < 60 {
            store(metadata.0, 0x1978 + index * 4, 0);
            if matches!(half(metadata.0, 0x58c + index * 2) & 0x7fff, 1 | 0x41 | 0x81) {
                maker_face_flags(job, metadata.0.wrapping_add(0x1978 + index * 4));
                let address = Address32(metadata.0.wrapping_add(0x1978 + index * 4));
                address.write_byte(address.byte() & !0x10);
            }
            index += 1;
        }
        let mut index = 0u32;
        while index < word(job.0, 0x74c) && index < 60 {
            let presence = word(job.0, 0x770 + index * 0xb8);
            store_half(metadata.0, 0x1a68 + index * 2, if presence == 0 { half(job.0, 0x774 + index * 0xb8) } else { 0 });
            if !matches!(word(job.0, 0x4b4), 27 | 28) {
                store_half(metadata.0, 0x56c + index * 2, if word(job.0, 0x770 + index * 0xb8) == 1 { 0 } else { half(metadata.0, 0x1a68 + index * 2) });
            }
            index += 1;
        }
        let mut index = 0u32;
        while index < word(job.0, 0x74c) && index < 60 {
            if word(job.0, 0x770 + index * 0xb8) == 0 {
                xm1_pipeline_image_pack_metadata_direction(Address32(0), Address32(metadata.0.wrapping_add(0x1ae0 + index * 2)), RectangleWords(job.0.wrapping_add(0x758 + index * 0xb8)));
            } else { store_half(metadata.0, 0x1ae0 + index * 2, 0); }
            if !matches!(word(job.0, 0x4b4), 27 | 28) {
                if word(job.0, 0x770 + index * 0xb8) == 1 { Address32(metadata.0.wrapping_add(0x580 + index)).write_byte(0); }
                else { xm1_pipeline_image_pack_metadata_direction(Address32(metadata.0.wrapping_add(0x580 + index)), Address32(0), RectangleWords(job.0.wrapping_add(0x758 + index * 0xb8))); }
            }
            index += 1;
        }
        store(metadata.0, 0x1d38, 0);
        let mut index = 0u32;
        while index < word(job.0, 0x74c) && index < 60 {
            store(metadata.0, 0x1b58 + index * 4, 0);
            let value = (word(job.0, 0x770 + index * 0xb8) == 1) as u8;
            let address = Address32(metadata.0.wrapping_add(0x1b58 + index * 4));
            address.write_byte((address.byte() & !1) | value);
            store(metadata.0, 0x1c48 + index * 4, word(metadata.0, 0x1b58 + index * 4));
            index += 1;
        }
    }
}

unsafe fn maker_detection_metadata(job: CaptureJob, metadata: MetadataBlock) {
    unsafe {
        if word(job.0, 0x598) == 1 {
            store_half(metadata.0, 0xe894, 1);
            store_half(metadata.0, 0xe896, (word(job.0, 0x1670) != 0) as u16);
            if half(metadata.0, 0xe894) == 1 {
                if half(metadata.0, 0xe896) == 0 {
                    store(metadata.0, 0xe898, u32::MAX); store(metadata.0, 0xe89c, u32::MAX);
                } else {
                    if word(job.0, 0x1770) < word(job.0, 0x176c) { loop { core::hint::spin_loop(); } }
                    let size = word(job.0, 0x1770).wrapping_mul(half(job.0, 0x1674) as u32).wrapping_mul(half(job.0, 0x1676) as u32).wrapping_add(500) / 1000;
                    store(metadata.0, 0xe898, size); store(metadata.0, 0xe89c, word(job.0, 0x1770));
                }
                if half(metadata.0, 0xe894) == 1 {
                    store_half(metadata.0, 0xe8a0, if half(metadata.0, 0xe896) == 0 || word(job.0, 0x1670) < 2 { 0xffff } else { (word(job.0, 0x1670) == 2) as u16 });
                }
            }
        } else { store_half(metadata.0, 0xe894, 0); }
        store_half(metadata.0, 0xe8a8, (word(job.0, 0x604) == 1) as u16);
        store(metadata.0, 0xe8e8, 1); store(metadata.0, 0xe8ec, 0);
        if word(job.0, 0x614) != 0 {
            let address = Address32(metadata.0.wrapping_add(0xe8ef));
            address.write_byte((address.byte() & 15) | ((word(job.0, 0x618) & 15) << 4) as u8);
            address.write_byte((address.byte() & 0xf0) | matches!(word(job.0, 0x618), 0 | 2) as u8);
            let address = Address32(metadata.0.wrapping_add(0xe8ec));
            if word(job.0, 0x614) == 1 { address.write_byte((address.byte() & !1) | 2); }
            else if word(job.0, 0x614) == 2 { address.write_byte(address.byte() | 3); }
        }
        store(metadata.0, 0xe8f4, u32::MAX); store(metadata.0, 0xe8f8, 0x80000000);
        if word(job.0, 0x74c) != 0 {
            Address32(metadata.0.wrapping_add(0x1f6)).write_byte(half(job.0, 0x15cc) as u8);
            if word(job.0, 0x74c) != 0 { Address32(metadata.0.wrapping_add(0x1f7)).write_byte(half(job.0, 0x15ce) as u8); }
        }
        for (presence, target, origin, extra) in [(0xe044,0x1f8,0x15d0,false),(0xe046,0x1fa,0x15d2,true),(0xe048,0x1fc,0x15d4,true)] {
            store_half(metadata.0, presence, 0);
            if word(job.0, 0x534) != 7 && word(job.0, 0x74c) != 0 && word(job.0, 0x5a0) == 1 && !matches!(word(job.0, 0x4b4), 27 | 28) && word(job.0, 0x169c) != 0 && (!extra || word(job.0, 0x2730) == 1) {
                store_half(metadata.0, presence, word(job.0, 0x169c) as u16);
                if presence == 0xe048 { Address32(metadata.0.wrapping_add(target)).write_byte(half(job.0, origin) as u8); }
                else { store_half(metadata.0, target, half(job.0, origin)); }
            }
        }
        for presence in [0xe04a,0xe04c,0xe04e] {
            store_half(metadata.0, presence, 0);
            if word(job.0, 0x534) != 7 && word(job.0, 0x74c) != 0 && word(job.0, 0x5a0) == 1 && word(job.0, 0x169c) != 0 && word(job.0, 0x2734) == 1 && word(job.0, 0x16b8) == 0 {
                store_half(metadata.0, presence, word(job.0, 0x169c) as u16);
                match presence {
                    0xe04a => { maker_focus_rectangle(job); xm1_pipeline_image_pack_metadata_rectangle(Address32(metadata.0.wrapping_add(0x1fe)), RectangleWords(0x15b53bc)); }
                    0xe04c => store_half(metadata.0, 0x206, half(job.0, 0x16bc)),
                    _ => xm1_pipeline_image_pack_metadata_direction(Address32(metadata.0.wrapping_add(0x208)), Address32(0), RectangleWords(job.0.wrapping_add(0x16a0))),
                }
            }
        }
        let count = if matches!(word(job.0, 0x4b4), 27 | 28) || word(job.0, 0x4d0) == 7 || word(job.0, 0x4e0) != 0 || word(job.0, 0x718) == 3 || word(job.0, 0x1698) != 1 { 0xffff }
            else { word(job.0, 0xe8c).min(2) as u16 };
        store_half(metadata.0, 0x20a, count);
        maker_face_records(job, metadata);
    }
}

#[derive(Clone, Copy)]
enum PairedFaceField { Code, Components, Rectangle, NormalizedRectangle, Reserved, Flags, Confidence, Direction, SavedPresence, Selection }

unsafe fn maker_append_face_pass(job: CaptureJob, metadata: MetadataBlock, field: PairedFaceField, start: u8, width: u32, height: u32) -> u8 {
    unsafe {
        let components = matches!(field, PairedFaceField::Rectangle | PairedFaceField::NormalizedRectangle);
        let mut destination = if components { start.wrapping_mul(4) } else { start };
        let limit = if components { 239 } else { 59 };
        if destination > limit { return destination; }
        let mut source_index = 0u8;
        while (source_index as u32) < word(job.0, 0x74c) {
            for (presence, origin, code) in [(0x798,0x784,2),(0x7b8,0x7a4,3)] {
                let record = job.0.wrapping_add((source_index as u32).wrapping_mul(0xb8));
                if word(record, presence) != 1 { continue; }
                let index = destination as u32;
                match field {
                    PairedFaceField::Code => store_half(metadata.0, 0x58c + index * 2, code),
                    PairedFaceField::Components => Address32(metadata.0.wrapping_add(0x604 + index)).write_byte(4),
                    PairedFaceField::Rectangle => xm1_pipeline_image_pack_paired_metadata_rectangle(Address32(metadata.0.wrapping_add(0x640 + index * 2)), RectangleWords(record.wrapping_add(origin))),
                    PairedFaceField::NormalizedRectangle => {
                        for (component, offset, divisor) in [(0,0,width),(1,8,height),(2,4,width),(3,12,height)] {
                            let component = destination.wrapping_add(component) as u32;
                            let value = __aeabi_uidiv(word(record, origin + offset).wrapping_shl(16), divisor);
                            store_half(metadata.0, 0x820 + component * 2, value as u16);
                        }
                    }
                    PairedFaceField::Reserved => store_half(metadata.0, 0xa00 + index * 2, 0),
                    PairedFaceField::Flags => store(metadata.0, 0x1978 + index * 4, 0),
                    PairedFaceField::Confidence => store_half(metadata.0, 0x1a68 + index * 2, half(record, origin + 0x1c)),
                    PairedFaceField::Direction => {
                        store_half(metadata.0, 0x1ae0 + index * 2, 0);
                        let address = Address32(metadata.0.wrapping_add(0x1ae0 + index * 2));
                        address.write_byte((address.byte() & !1) | (word(record, origin + 0x18) & 1) as u8);
                        address.write_byte((address.byte() & !2) | ((word(record, presence) & 1) << 1) as u8);
                        address.write_byte((address.byte() & !4) | ((word(record, origin + 0x10) & 1) << 2) as u8);
                        Address32(metadata.0.wrapping_add(0x1ae1 + index * 2)).write_byte(source_index);
                    }
                    PairedFaceField::SavedPresence => store(metadata.0, 0x1d3c + index * 0x340, 0),
                    PairedFaceField::Selection => { store(metadata.0, 0x1b58 + index * 4, 0); store(metadata.0, 0x1c48 + index * 4, 0); }
                }
                destination = destination.wrapping_add(if components { 4 } else { 1 });
            }
            if destination > limit { break; }
            source_index = source_index.wrapping_add(1);
        }
        destination
    }
}

unsafe fn maker_append_faces(job: CaptureJob, metadata: MetadataBlock, paired: u32) {
    unsafe {
        if word(job.0, 0x534) == 7 {
            if word(job.0, 0x169c) == 0 { return; }
            let index = word(job.0, 0x74c) as u8 as u32;
            store_half(metadata.0, 0x58a, half(metadata.0, 0x58a).wrapping_add(1));
            store_half(metadata.0, 0x58c + index * 2, 0x1000);
            Address32(metadata.0.wrapping_add(0x604 + index)).write_byte(4);
            store_half(metadata.0, 0xe040, ((index + 1) * 4) as u16);
            for (component, origin) in [(0,0x16a0),(2,0x16a8),(4,0x16a4),(6,0x16ac)] { store_half(metadata.0, 0x640 + index * 8 + component, word(job.0, origin) as u16); }
            let width = maker_extent(job, SensorAxis::Horizontal);
            let height = maker_extent(job, SensorAxis::Vertical);
            for (component, divisor) in [(0,width),(2,height),(4,width),(6,height)] {
                let coordinate = half(metadata.0, 0x640 + index * 8 + component) as i16 as i32 as u32;
                store_half(metadata.0, 0x820 + index * 8 + component, __aeabi_uidiv(coordinate.wrapping_shl(16), divisor) as u16);
            }
            store_half(metadata.0, 0xa00 + index * 2, 0);
            fill(metadata.0.wrapping_add(0xa78), 0xf00, 0);
            store_half(metadata.0, 0xe042, 0);
            store(metadata.0, 0x1978 + index * 4, 0);
            store_half(metadata.0, 0x1a68 + index * 2, 0);
            store_half(metadata.0, 0x1ae0 + index * 2, 0);
            store(metadata.0, 0x1d38, 0);
            store(metadata.0, 0x1b58 + index * 4, 0);
            store(metadata.0, 0x1c48 + index * 4, 0);
            return;
        }
        if paired != 1 || word(job.0, 0x74c) == 0 { return; }
        let mut count = 0u16;
        let mut any = false;
        let mut index = 0u8;
        while (index as u32) < word(job.0, 0x74c) {
            if word(job.0, 0x798 + index as u32 * 0xb8) == 1 { count = count.wrapping_add(1); any = true; }
            if word(job.0, 0x7b8 + index as u32 * 0xb8) == 1 { count = count.wrapping_add(1); any = true; }
            index = index.wrapping_add(1);
        }
        store_half(metadata.0, 0x58a, half(metadata.0, 0x58a).wrapping_add(count));
        if !any { return; }
        let start = word(job.0, 0x74c) as u8;
        maker_append_face_pass(job, metadata, PairedFaceField::Code, start, 0, 0);
        maker_append_face_pass(job, metadata, PairedFaceField::Components, start, 0, 0);
        let components = maker_append_face_pass(job, metadata, PairedFaceField::Rectangle, start, 0, 0);
        store_half(metadata.0, 0xe040, components as u16);
        let width = maker_extent(job, SensorAxis::Horizontal);
        let height = maker_extent(job, SensorAxis::Vertical);
        maker_append_face_pass(job, metadata, PairedFaceField::NormalizedRectangle, start, width, height);
        for field in [PairedFaceField::Reserved,PairedFaceField::Flags,PairedFaceField::Confidence,PairedFaceField::Direction,PairedFaceField::SavedPresence,PairedFaceField::Selection] {
            maker_append_face_pass(job, metadata, field, start, 0, 0);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_populate_maker_metadata(job: CaptureJob, metadata: MetadataBlock, paired: u32, _reserved: u32, _option: u32) {
    unsafe {
        xm1_pipeline_image_store_maker_exposure_bias(job, metadata);
        fill(metadata.0.wrapping_add(4), 48, 0);
        store_half(metadata.0, 0x3c, xm1_pipeline_image_metadata_primary_mode(job) as u16);
        store_half(metadata.0, 0x3e, xm1_pipeline_image_metadata_secondary_mode(job) as u16);
        xm1_pipeline_image_store_maker_flash_mode(job, metadata);
        store_half(metadata.0, 0x40, xm1_pipeline_image_metadata_processing_mode(job) as u16);
        store_half(metadata.0, 0x42, 0xffff);
        store_half(metadata.0, 0x44, xm1_pipeline_image_metadata_kelvin(job) as u16);
        store(metadata.0, 0x4c, xm1_pipeline_image_metadata_balance_adjustment(job, BalanceAxis(0)) as u32);
        store(metadata.0, 0x50, xm1_pipeline_image_metadata_balance_adjustment(job, BalanceAxis(1)) as u32);
        store(metadata.0, 0x48, maker_manual_mode(word(job.0, 0x4b4)) as u32);
        store_half(metadata.0, 0x54, xm1_pipeline_image_metadata_tone_scale(job) as u16);
        if maker_manual_mode(word(job.0, 0x4b4)) || word(job.0, 0x4b4) == 31 {
            store_half(metadata.0, 0x60, xm1_pipeline_image_metadata_saturation(job) as u16);
            store(metadata.0, 0x5c, 1);
        } else { store(metadata.0, 0x5c, 0); }
        let focus = if word(job.0, 0x325c) == 1 { 0xffff }
            else if word(job.0, 0x4b4) == 44 { 0 }
            else if word(job.0, 0x53c) != 0 || (word(job.0, 0x4b4) == 1 && word(job.0, 0x638) == 0x49) { 1 } else { 0 };
        store_half(metadata.0, 0x6e, focus);
        store_half(metadata.0, 0x70, (word(job.0, 0x534) == 0) as u16);
        store_half(metadata.0, 0x92, (word(job.0, 0x534) == 7) as u16);
        if word(job.0, 0x534) == 7 {
            store(metadata.0, 0x94, (word(job.0, 0x1764) != 0) as u32);
            if word(job.0, 0x534) == 7 && word(job.0, 0x1764) == 1 { store(metadata.0, 0x98, word(job.0, 0x1600)); }
        }
        store_half(metadata.0, 0x9c, xm1_pipeline_image_metadata_flash_processing(job) as u16);
        store_half(metadata.0, 0x9e, xm1_pipeline_image_metadata_color_mode(job) as u16);
        store_half(metadata.0, 0xa0, half(job.0, 0x1682));
        store_half(metadata.0, 0xa2, 0xffff); store_half(metadata.0, 0xa4, 0xffff);
        store_half(metadata.0, 0xa6, match word(job.0, 0x4b4) { 30 | 42 => 1, 38 => 0x10, 39 => 0x11, 40 => 0x12, 41 => 0, _ => 0xffff });
        for (target, origin) in [(0xa8,0x5f8),(0xac,0x5f4)] {
            store(metadata.0, target, match word(job.0, origin) { 0 => (-32i32) as u32, 1 => (-16i32) as u32, 3 => 16, 4 => 32, _ => 0 });
        }
        store(metadata.0, 0xb0, 1); store_half(metadata.0, 0xb4, half(job.0, 0x166c));
        store(metadata.0, 0xb8, 1); store_half(metadata.0, 0xbc, half(job.0, 0x166a));
        store(metadata.0, 0xe8a4, u32::MAX); store(metadata.0, 0xe8f0, u32::MAX);
        let capture = match word(job.0, 0x4b4) {
            44 => 0, 15 | 38..=40 => 2, 29..=31 | 41 | 42 => 0,
            _ => match word(job.0, 0x4a8) { 2 | 10 => 2, 1 | 3..=9 => 1, _ => 0 },
        };
        store_half(metadata.0, 0xbe, capture);
        store_half(metadata.0, 0xd0, 0);
        store(metadata.0, 0xd4, match word(job.0, 0x530) { 1..=5 => word(job.0, 0x530) << 16, 6 => 0x60001, 7 => 0x60003, 8 => 0x60004, 9 => 0x60005, 10 => 0x60006, 11 => 0x60002, 12 => 0x90000, 13 => 0x70000, _ => u32::MAX });
        store(metadata.0, 0xd8, 0); store_half(metadata.0, 0xea, 1);
        let processing = xm1_pipeline_image_metadata_processing_profile(job);
        store_half(metadata.0, 0xf0, processing as u16); store(metadata.0, 0xec, (processing != 0xffff) as u32);
        let mode = word(job.0, 0x4b4);
        let eligible = maker_manual_mode(mode) || matches!(mode, 0 | 1 | 29 | 31 | 32);
        store_half(metadata.0, 0xf2, if eligible { (word(job.0, 0x520) != 0) as u16 } else { 0xffff });
        let mode = word(job.0, 0x4b4);
        let eligible = maker_manual_mode(mode) || matches!(mode, 0 | 1 | 29 | 31 | 32);
        let range = if eligible && half(metadata.0, 0xf2) == 1 { match word(job.0, 0x520) { 1 => 100, 2 => 200, 3 => 400, 4 => 800, 5 => 1600, _ => 0xffff } } else { 0xffff };
        store_half(metadata.0, 0xf4, range);
        store_half(metadata.0, 0x72, matches!(word(job.0, 0x534), 2 | 3 | 5 | 6) as u16);
        maker_focus_center(job, metadata, SensorAxis::Horizontal);
        maker_focus_center(job, metadata, SensorAxis::Vertical);
        store_half(metadata.0, 0xc0, half(job.0, 0x1680));
        store_half(metadata.0, 0xde, (word(job.0, 0x4b4) != 44 && word(job.0, 0x688) != 0) as u16);
        store_half(metadata.0, 0xe2, (word(job.0, 0x4b4) != 44 && word(job.0, 0x684) & 0x18 != 0) as u16);
        store_half(metadata.0, 0xe0, (word(job.0, 0x4b4) != 44 && word(job.0, 0x68c) != 0) as u16);
        let diagnostic = if xm1_buffers_configuration_byte(ConfigurationKey(0x22e)) & 4 == 0 { false }
            else { word(0x4089ea9c, 0) == 2 && word(0x4089ea9c, 4) == 0xf0 && word(0x4089ea9c, 8) != 0 };
        store_half(metadata.0, 0xe4, diagnostic as u16);
        store_half(metadata.0, 0xe6, word(job.0, 0x690) as u16); store_half(metadata.0, 0xe8, 0);
        let quality = if word(job.0, 0x4b4) == 44 { 0xeb02d0 } else {
            match xm1_pipeline_image_metadata_quality_selection(job) {
                0 => 0xeb02d0, 1 => 0xeb02d8,
                3 => match word(job.0, 0x514) { 0 => 0xeb02d0, 1 => 0xeb02d8, _ => 0xeb02e0 },
                _ => 0xeb02e0,
            }
        };
        copy_string(metadata.0.wrapping_add(0x34), quality);
        if word(job.0, 0x4b4) == 24 { copy(metadata.0.wrapping_add(0x58), 0xeb02e8, 4); }
        else { fill(metadata.0.wrapping_add(0x58), 4, 0); }
        copy(metadata.0.wrapping_add(0x1e8), 0xeb02f0, 4); copy(metadata.0.wrapping_add(0x1ec), 0xeb02e8, 4);
        store_half(metadata.0, 0x1f0, 0); maker_face_flags(job, metadata.0.wrapping_add(0x1f0));
        let enabled = word(job.0, 0x5a0) == 1 &&
            ((xm1_buffers_configuration_byte(ConfigurationKey(0x51d7)) == 1 && !matches!(word(job.0, 0x4b4), 27 | 28)) ||
             (xm1_buffers_configuration_byte(ConfigurationKey(0x51d8)) == 1 && word(job.0, 0x4b4) == 27) ||
             (xm1_buffers_configuration_byte(ConfigurationKey(0x51d9)) == 1 && word(job.0, 0x4b4) == 28));
        let flags = Address32(metadata.0.wrapping_add(0x1f0));
        flags.write_byte((flags.byte() & !0x10) | ((enabled as u8) << 4));
        fill(metadata.0.wrapping_add(0x1f2), 4, 0);
        let mode = word(job.0, 0x4b4);
        let range = if (maker_manual_mode(mode) || matches!(mode, 0 | 1 | 29 | 31 | 32)) && half(metadata.0, 0xf2) != 1 {
            match word(job.0, 0x6c8) { 2 => 200, 3 => 400, 4 => 800, _ => 100 }
        } else { 0xffff };
        store_half(metadata.0, 0xf6, range);
        store(metadata.0, 0xfc, 1);
        let (stabilized, selection) = if word(job.0, 0x62c) == 0 && word(job.0, 0x3258) == 1 {
            (1, match word(job.0, 0x594) { 0 => 0, 2 => 2, _ => 1 })
        } else { (0, 0) };
        store_half(metadata.0, 0x100, stabilized); store_half(metadata.0, 0x102, selection); store_half(metadata.0, 0x104, 0);
        store_half(metadata.0, 0x78, match word(job.0, 0x175c) { 1 => 0x10, 2 => 0x20, 3 => 0x30, 4 => 0x40, _ => 0 });
        let mode = word(job.0, 0x4b4);
        let rendering = if maker_manual_mode(mode) || matches!(mode, 0 | 14 | 15 | 25 | 26 | 29 | 30 | 31 | 44) {
            match word(job.0, 0x508) { 1 => 0x1120, 3 => 0x1200, 4 | 5 | 7..=12 => 0xffff, _ => 0x1000 }
        } else { 0xffff };
        store_half(metadata.0, 0x108, rendering);
        maker_scene_components(job, metadata);
        fill(metadata.0.wrapping_add(0x138), 0x81, 0);
        let processed = if word(job.0, 0x78) == 11 { let value = word(job.0, 0x19a8); if matches!(value, 0 | u32::MAX) { 0 } else { value } } else { 0 };
        store(metadata.0, 0x134, processed);
        maker_detection_metadata(job, metadata);
        maker_append_faces(job, metadata, paired);
        maker_tail(job, metadata);
    }
}

unsafe fn maker_tail(job: CaptureJob, metadata: MetadataBlock) {
    unsafe {
        copy(metadata.0.wrapping_add(0xe050), 0xeb02c8, 4);
        store_half(metadata.0, 0xe054, 0);
        Address32(metadata.0.wrapping_add(0xe056)).write_byte(0);
        store(metadata.0, 0xe058, 0);
        Address32(metadata.0.wrapping_add(0xe05c)).write_byte(0);
        Address32(metadata.0.wrapping_add(0xe05d)).write_byte(0);
        store(metadata.0, 0xe060, 0);
        Address32(metadata.0.wrapping_add(0xe064)).write_byte(0);
        for offset in [0xe068,0xe06c,0xe070,0xe074,0xe078,0xe07c] { store(metadata.0, offset, 0); }
        Address32(metadata.0.wrapping_add(0xe080)).write_byte(0);
        for index in 0..6 { store(metadata.0, 0xe084 + index * 4, (index + 1) * 10); }
        store(metadata.0, 0xe09c, 0);
        for offset in [0xe0a0,0xe230,0xe2f8,0xe3c0,0xe0a0,0xe230,0xe2f8,0xe3c0] { Address32(metadata.0.wrapping_add(offset)).write_byte(0); }
        store_half(metadata.0, 0xe8ac, 0);
        Address32(metadata.0.wrapping_add(0xe8ae)).write_byte(0);
        for index in 0..6 { store(metadata.0, 0xe8b0 + index * 4, (index + 1) * 10); }
        Address32(metadata.0.wrapping_add(0xe8c8)).write_byte(0);
        for index in 0..6 { store(metadata.0, 0xe8cc + index * 4, (index + 1) * 10); }
        store_half(metadata.0, 0xe8e4, 0);
        let mode = word(job.0, 0x4b4);
        let kind = if mode == 25 || word(job.0, 0x710) & 0x400 != 0 { 1 }
            else if mode == 26 { 2 } else if mode == 29 { if word(job.0, 0x279c) == 0 { 0x20 } else { 0x40 } } else { u32::MAX };
        store(metadata.0, 0xc4, kind);
        let mode = word(job.0, 0x4b4);
        if matches!(mode, 25 | 26 | 29) || word(job.0, 0x710) & 0x400 != 0 { store_half(metadata.0, 0xc8, word(job.0, 0x2fec) as u16); }
        let mode = word(job.0, 0x4b4);
        if matches!(mode, 25 | 26) || word(job.0, 0x710) & 0x400 != 0 {
            store_half(metadata.0, 0xca, if word(job.0, 0x276c) == 1 { word(job.0, 0x2ff0) as u16 } else { 1 });
        } else if mode == 29 { store_half(metadata.0, 0xca, word(job.0, 0x2ff0) as u16); }
        store_half(metadata.0, 0xcc, if word(job.0, 0x4b4) == 29 { match word(job.0, 0x5cc) { 1 => 180, 2 => 240, 3 | 4 => 360, _ => 120 } } else { 0xffff });
        store_half(metadata.0, 0xce, if word(job.0, 0x4b4) == 29 { match word(job.0, 0x5d0) { 2 => 2, 3 => 1, 1 => 4, _ => 3 } } else { 0xffff });
        let width = maker_extent(job, SensorAxis::Horizontal);
        let height = maker_extent(job, SensorAxis::Vertical);
        fill(metadata.0.wrapping_add(0x7a), 24, 0);
        if word(job.0, 0x4b4) == 26 && word(job.0, 0x2ff0) != 0 {
            let mut index = 0u32;
            while index < word(job.0, 0x2ff0) {
                let selection = word(job.0, 0x3080u32.wrapping_add(index.wrapping_mul(4)));
                if selection != 0 {
                    let coordinate = word(job.0, selection.wrapping_add(0xdb).wrapping_mul(8));
                    let scale = xm1_buffers_configuration_byte(ConfigurationKey(0x920c));
                    let horizontal = width.wrapping_mul(scale.wrapping_mul(coordinate).wrapping_add(700)) as i32 / 1400;
                    store_half(metadata.0, 0x7au32.wrapping_add(index.wrapping_mul(6)), horizontal as u16);
                    let coordinate = word(job.0, selection.wrapping_add(0xdb).wrapping_mul(8).wrapping_add(4));
                    let scale = xm1_buffers_configuration_byte(ConfigurationKey(0x920d));
                    let vertical = height.wrapping_mul(700u32.wrapping_sub(coordinate.wrapping_mul(scale))) as i32 / 1400;
                    store_half(metadata.0, 0x7cu32.wrapping_add(index.wrapping_mul(6)), vertical as u16);
                    store_half(metadata.0, 0x7eu32.wrapping_add(index.wrapping_mul(6)), word(job.0, selection.wrapping_sub(1).wrapping_mul(4).wrapping_add(0x6cc)) as u16);
                }
                index = index.wrapping_add(1);
            }
        }
        store(metadata.0, 0x1bc, if word(job.0, 0x62c) == 0 { word(job.0, 0x17d4) } else { word(job.0, 0x628).wrapping_mul(100) });
        store(metadata.0, 0x1c0, 100);
        if word(job.0, 0x62c) == 0 { store(metadata.0, 0x1c4, word(job.0, 0x17d8)); store(metadata.0, 0x1c8, 100); }
        else { store(metadata.0, 0x1c4, word(job.0, 0x628).wrapping_mul(100)); store(metadata.0, 0x1c8, 100); }
        if word(job.0, 0x62c) == 0 { store(metadata.0, 0x1cc, word(job.0, 0x17dc)); store(metadata.0, 0x1d0, 100); }
        else { store(metadata.0, 0x1cc, 0); store(metadata.0, 0x1d0, 0); }
        if word(job.0, 0x62c) == 0 {
            let aperture = if word(job.0, 0x17d8) == word(job.0, 0x17d4) { 0x17dc } else { 0x17e0 };
            store(metadata.0, 0x1d4, word(job.0, aperture)); store(metadata.0, 0x1d8, 100);
        } else { store(metadata.0, 0x1d4, 0); store(metadata.0, 0x1d8, 0); }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ProfileValue(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RateGridMode(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PackedTriplet(pub u32);

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_profile_alternate_bank(profile: ProfileValue) -> u32 {
    unsafe {
        let setting = xm1_buffers_configuration_byte(ConfigurationKey(0x1c1d)).wrapping_mul(5);
        let threshold = ((setting.wrapping_shl(17) as i32) >> 17).wrapping_mul(2);
        (profile.0 >= threshold) as u32
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_profile_value_index(value: i32) -> u32 {
    unsafe {
        if value == 2500 { return 0; }
        for index in 1..31 {
            if word(0x818dc0, index * 4) as i32 == value { return index; }
        }
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_export_profile_value(value: i32, profile: ProfileValue, output: PackedTriplet) {
    unsafe {
        let index = xm1_pipeline_image_profile_value_index(value);
        store_half(output.0, 0, xm1_buffers_configuration_half(ConfigurationKey(0x7cb6)));
        let alternate = xm1_pipeline_image_profile_alternate_bank(profile) != 0;
        let offset = index * 2 + if alternate { 124 } else { 0 };
        store_half(output.0, 2, half(0x9189a0, offset));
        store_half(output.0, 4, half(0x9189a0, offset + 62));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_profile_adjusted_limits(value: i32, profile: ProfileValue, output: PackedTriplet) {
    unsafe { xm1_pipeline_image_export_profile_value(value, profile, output); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_export_profile_mode(mode: RateGridMode, profile: ProfileValue, output: PackedTriplet) {
    unsafe {
        let index = match mode.0 {
            1 => 1, 2 | 4 => 2, 3 => 3, 5 => 4, 6 => 5, 7 => 6, 8 => 7, 9 => 8, 10 => 9,
            13 => 11, 14 => 12, 15 => 13, 16 => 10, 18 => 0, 19 => 14, 20 => 15, 21 => 16,
            22 => 17, 23 => 18, 25..=60 => mode.0 - 6, _ => 4,
        };
        let alternate = xm1_pipeline_image_profile_alternate_bank(profile) != 0;
        store_half(output.0, 0, xm1_buffers_configuration_half(ConfigurationKey(0x8e42)));
        let offset = index * 2 + if alternate { 220 } else { 0 };
        store_half(output.0, 2, half(0x940b00, offset));
        store_half(output.0, 4, half(0x940b00, offset + 110));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_rate_grid_weights(mode: RateGridMode, profile: ProfileValue, output: PackedTriplet) {
    unsafe {
        if xm1_buffers_configuration_byte(ConfigurationKey(0x8e40)) != 0 {
            xm1_pipeline_image_export_profile_mode(mode, profile, output);
            return;
        }
        let alternate = xm1_pipeline_image_profile_alternate_bank(profile) != 0;
        let key = match mode.0 {
            1 => 0x1c26, 2 | 4 | 27..=60 => 0x1c2c, 3 => 0x1c32,
            5 => 0x1c38, 6 => 0x1c3e, 7 => 0x1c44, 8 => 0x1c4a, 9 => 0x1c50, 10 => 0x1c56,
            13 => 0x1c62, 14 => 0x1c68, 15 => 0x1c6e, 16 => 0x1c5c, 18 => 0x1c20,
            19 => 0x1c78, 20 => 0x1c7e, 21 => 0x1c84, 22 => 0x1c8a, 23 => 0x1c90,
            25 => 0x1d10, 26 => 0x1d16, _ => 0x1c38,
        };
        let key = key + if alternate { if matches!(mode.0, 25 | 26) { 12 } else { 120 } } else { 0 };
        for index in 0..3 { store_half(output.0, index * 2, xm1_buffers_configuration_half(ConfigurationKey(key + index * 2))); }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SplineCount(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct FloatSamples(pub u32);

unsafe fn sample(base: FloatSamples, index: u32) -> f32 { unsafe { f32::from_bits(word(base.0, index.wrapping_mul(4))) } }
unsafe fn put_sample(base: FloatSamples, index: u32, value: f32) { unsafe { store(base.0, index.wrapping_mul(4), value.to_bits()); } }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_build_metadata_spline(count: SplineCount, knots: FloatSamples, values: FloatSamples, constant: FloatSamples, linear: FloatSamples, quadratic: FloatSamples, cubic: FloatSamples) -> i32 {
    unsafe {
        if (count.0 as u32).wrapping_sub(2) > 18 { loop { core::hint::spin_loop(); } }
        let count = count.0 as u32;
        for row in 0..count - 1 {
            if sample(knots, row) > sample(knots, row + 1) { loop { core::hint::spin_loop(); } }
        }
        for row in 0..count { put_sample(constant, row, sample(values, row)); }
        let matrix = FloatSamples(0x911b74);
        let inverse = FloatSamples(0x9114e4);
        let right_hand_side = FloatSamples(0x911b24);
        for row in 0..count {
            for column in 0..count {
                put_sample(matrix, row * count + column, 0.0);
                put_sample(inverse, row * count + column, 0.0);
                if row == column { put_sample(inverse, row * count + column, 1.0); }
            }
        }
        for row in 0..count {
            if row == 0 {
                let span = sample(knots, 1) - sample(knots, 0);
                put_sample(matrix, 0, span + span);
                put_sample(matrix, 1, sample(knots, 1) - sample(knots, 0));
            } else if row == count - 1 {
                put_sample(matrix, row * count + row - 1, sample(knots, row) - sample(knots, row - 1));
                let span = sample(knots, row) - sample(knots, row - 1);
                put_sample(matrix, row * count + row, span + span);
            } else {
                put_sample(matrix, row * count + row - 1, sample(knots, row + 1) - sample(knots, row));
                let center = sample(knots, row);
                let next = sample(knots, row + 1) - center;
                let previous = center - sample(knots, row - 1);
                let diagonal = next + previous;
                put_sample(matrix, row * count + row, diagonal + diagonal);
                put_sample(matrix, row * count + row + 1, sample(knots, row) - sample(knots, row - 1));
            }
        }
        for row in 0..count {
            let value = if row == 0 { (sample(values, 1) - sample(values, 0)) * 3.0 }
            else if row == count - 1 { (sample(values, row) - sample(values, row - 1)) * 3.0 }
            else {
                let center = sample(knots, row);
                let previous_span = center - sample(knots, row - 1);
                let center_value = sample(values, row);
                let next_span = sample(knots, row + 1) - center;
                let first = (previous_span * (sample(values, row + 1) - center_value)) / next_span;
                let second = (next_span * (center_value - sample(values, row - 1))) / previous_span;
                (first + second) * 3.0
            };
            put_sample(right_hand_side, row, value);
        }
        for pivot in 0..count {
            let diagonal = sample(matrix, pivot * count + pivot);
            if diagonal == 0.0 { return -1; }
            for column in 0..count {
                let position = pivot * count + column;
                put_sample(matrix, position, sample(matrix, position) / diagonal);
                put_sample(inverse, position, sample(inverse, position) / diagonal);
            }
            for row in 0..count {
                if row == pivot { continue; }
                let factor = sample(matrix, row * count + pivot) / sample(matrix, pivot * count + pivot);
                for column in 0..count {
                    let product = factor * sample(matrix, pivot * count + column);
                    let position = row * count + column;
                    put_sample(matrix, position, sample(matrix, position) - product);
                    let product = factor * sample(inverse, pivot * count + column);
                    put_sample(inverse, position, sample(inverse, position) - product);
                }
            }
        }
        for row in 0..count {
            put_sample(linear, row, 0.0);
            for column in 0..count {
                let product = sample(inverse, row * count + column) * sample(right_hand_side, column);
                put_sample(linear, row, sample(linear, row) + product);
            }
        }
        for row in 0..count - 1 {
            let span = sample(knots, row + 1) - sample(knots, row);
            let term = sample(linear, row) * -2.0 - sample(linear, row + 1);
            let difference = (sample(values, row + 1) - sample(values, row)) * 3.0;
            let term = term + difference / span;
            put_sample(quadratic, row, term / span);
            let term = sample(linear, row) + sample(linear, row + 1);
            let difference = sample(values, row + 1) - sample(values, row);
            let difference = (difference + difference) / span;
            let term = term - difference;
            put_sample(cubic, row, term / (span * span));
        }
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_evaluate_metadata_spline(count: SplineCount, knots: FloatSamples, value: f32, constant: FloatSamples, linear: FloatSamples, quadratic: FloatSamples, cubic: FloatSamples) -> f32 {
    unsafe {
        let mut selected = 0u32;
        let mut index = 0u32;
        while index != count.0 as u32 {
            if sample(knots, index) < value { selected = index; }
            index = index.wrapping_add(1);
        }
        let delta = value - sample(knots, selected);
        let result = delta * sample(linear, selected);
        let result = result + sample(constant, selected);
        let term = delta * sample(quadratic, selected);
        let term = term * delta;
        let result = result + term;
        let term = delta * sample(cubic, selected);
        let term = term * delta;
        let term = term * delta;
        result + term
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct BalanceSource(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct BalancePreference(pub u32);

unsafe fn configuration_high_first(key: ConfigurationKey) -> u16 {
    unsafe {
        let high = xm1_buffers_configuration_byte(ConfigurationKey(key.0.wrapping_add(1)));
        let low = xm1_buffers_configuration_byte(key);
        (low | high << 8) as u16
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_select_metadata_balance(source: BalanceSource, preference: BalancePreference, first: Address32, second: Address32, third: Address32) {
    unsafe {
        let setting = xm1_buffers_configuration_byte(ConfigurationKey(0xa902));
        if setting == 2 && source.0 == 1 {
            for (output, key) in [(first,0xa903),(second,0xa905),(third,0xa907)] {
                if output.0 != 0 { store_half(output.0, 0, configuration_high_first(ConfigurationKey(key))); }
            }
        } else {
            let selection = if setting != 2 && source.0 == 1 { (preference.0 != 0) as u32 }
                else if preference.0 == 0 { 2 } else { 3 };
            let base = 0x409c2944 + selection * 6;
            store_half(first.0, 0, half(base, 0));
            store_half(second.0, 0, half(base, 2));
            store_half(third.0, 0, half(base, 4));
        }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConfigurationChannel(pub u32);

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_threshold_band(channel: ConfigurationChannel, value: ProfileValue) -> u32 {
    unsafe {
        let row = 0x829ad8u32.wrapping_add(channel.0.wrapping_mul(16));
        let first = xm1_buffers_configuration_half(ConfigurationKey(word(row, 0))) as i32;
        let second = xm1_buffers_configuration_half(ConfigurationKey(word(row, 4))) as i32;
        let third = xm1_buffers_configuration_half(ConfigurationKey(word(row, 8))) as i32;
        let fourth = xm1_buffers_configuration_half(ConfigurationKey(word(row, 12))) as i32;
        if first >= value.0 { 0 } else if value.0 <= second { 1 } else if value.0 <= third { 2 } else if value.0 <= fourth { 3 } else { 4 }
    }
}

unsafe extern "C" {
    fn xm1_pipeline_image_configuration_signed_half(key: ConfigurationKey) -> i32;
    fn xm1_pipeline_image_configuration_pointer(key: ConfigurationKey) -> Address32;
    fn xm1_pipeline_sensor_select_calibration(mode: u32, channel: u32, level: i32, out_mode: *mut u8, out_channel: *mut u8, out_level: *mut u8);
    fn xm1_pipeline_sensor_primary_geometry(stage: u32, output: u32, operation: u32, selection: u32, secondary: i32) -> u32;
    fn xm1_pipeline_image_build_distortion(stage: *mut u8, configuration: *const u8, output: *mut u8);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_sensor_band(job: CaptureJob) -> u32 {
    unsafe {
        let value = Address32(job.0.wrapping_add(0x6c0)).byte() as i32 - 1;
        let mut cursor = 0xe5859cu32;
        let mut key = 0x3e3a00u32;
        let mut band = 0u32;
        loop {
            if value <= xm1_buffers_configuration_byte(ConfigurationKey(key)) as i32 { return band; }
            band = band.wrapping_add(1);
            cursor = cursor.wrapping_add(4);
            key = word(cursor, 0);
            if key == 0 { return band; }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_level_band(job: CaptureJob) -> u32 {
    unsafe {
        let channel_offset = xm1_pipeline_image_metadata_sensor_band(job).wrapping_mul(4);
        let mut cursor = 0xe58590u32;
        let mut key = 0x3e3a10u32;
        let mut band = 0u32;
        loop {
            let threshold = xm1_pipeline_image_configuration_signed_half(ConfigurationKey(key.wrapping_add(channel_offset))) as u16 as i32;
            if half(job.0, 0x2264) as i16 as i32 <= threshold { return band; }
            band = band.wrapping_add(1);
            cursor = cursor.wrapping_add(4);
            key = word(cursor, 0);
            if key == 0 { return band; }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_distortion_enabled(job: CaptureJob) -> u32 {
    unsafe {
        if word(job.0, 0x4b4) == 29 { return 0; }
        if word(job.0, 0x78) == 11 {
            let source = word(job.0, 0x1884);
            if half(source, 0xc0) == 0 { return 0; }
            return (half(source, 0xc2) != 0) as u32;
        }
        let band = xm1_pipeline_image_metadata_sensor_band(job);
        if word(job.0, 0x3240) == 1 {
            if xm1_buffers_configuration_byte(ConfigurationKey(0xc0)) & 0x80 == 0 { return 0; }
            if word(job.0, 0x3244) == 0 { return 0; }
        } else {
            if word(job.0, 0x62c) == 1 { return 0; }
            if xm1_buffers_configuration_byte(ConfigurationKey(0xc0)) & 0x80 == 0 { return 0; }
            let flags = xm1_buffers_configuration_byte(ConfigurationKey(0x3e3a4cu32.wrapping_add(band >> 3)));
            if (flags >> (band & 7)) & 1 == 0 { return 0; }
        }
        if xm1_buffers_configuration_byte(ConfigurationKey(0x4011c1)) == 0 { return 0; }
        let selected = word(job.0, 0x272c);
        if selected <= 1 { 1 - selected } else { 0 }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SnapshotSelection(pub u32);

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_snapshot_band(selection: SnapshotSelection) -> u32 {
    unsafe {
        let mut band = 0u32;
        while band != 9 {
            let threshold = xm1_buffers_configuration_byte(ConfigurationKey(word(0x829a38, band * 4)));
            if threshold >= selection.0 { break; }
            band += 1;
        }
        band
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_cached_snapshot_value(selection: SnapshotSelection) -> u32 {
    unsafe {
        if selection.0 > 30 { return 0x40; }
        let offset = selection.0 * 8;
        if word(0x95ada0, offset + 4) == 0 {
            let value = xm1_buffers_configuration_byte(ConfigurationKey(word(0x829a5c, selection.0 * 4)));
            Address32(0x95ada0 + offset).write_byte(value as u8);
            store(0x95ada0, offset + 4, 1);
        }
        Address32(0x95ada0 + offset).byte() as u32
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_cached_snapshot_band(selection: SnapshotSelection) -> u32 {
    unsafe {
        let band = xm1_pipeline_image_metadata_snapshot_band(selection);
        let offset = (band + 31) * 8;
        if word(0x95ada0, band * 8 + 0xfc) == 0 {
            let value = xm1_buffers_configuration_byte(ConfigurationKey(word(0x829a10, band * 4)));
            Address32(0x95ada0 + offset).write_byte(value as u8);
            store(0x95ada0, offset + 4, 1);
        }
        Address32(0x95ada0 + offset).byte() as u32
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_snapshot_projection(kind: SnapshotSelection, selection: SnapshotSelection, destination: Address32) {
    unsafe {
        if destination.0 == 0 { return; }
        let band = xm1_pipeline_image_metadata_snapshot_band(selection);
        let source = 0x8179a0u32.wrapping_add(kind.0.wrapping_sub(1).wrapping_mul(440)).wrapping_add(band.wrapping_mul(44));
        for index in 0..11 {
            let value = xm1_buffers_configuration_byte(ConfigurationKey(word(source, index * 4)));
            Address32(destination.0.wrapping_add(index)).write_byte(value as u8);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_export_metadata_snapshot(first: SnapshotSelection, second: SnapshotSelection, destination: Address32) {
    unsafe {
        Address32(destination.0).write_byte(xm1_pipeline_image_metadata_cached_snapshot_value(first) as u8);
        Address32(destination.0.wrapping_add(1)).write_byte(xm1_pipeline_image_metadata_cached_snapshot_band(second) as u8);
        let mut projection = core::mem::MaybeUninit::<[u8; 11]>::uninit();
        for kind in 1..6 {
            let projection_address = projection.as_mut_ptr() as *mut u8;
            xm1_pipeline_image_metadata_snapshot_projection(SnapshotSelection(kind), second, Address32(projection_address as u32));
            for index in 0..11 { Address32(destination.0.wrapping_add(2 + (kind - 1) * 11 + index)).write_byte(read_volatile(projection_address.add(index as usize))); }
        }
    }
}

unsafe extern "C" {
    fn xm1_pipeline_sensor_collect_settings(stage: u32, descriptor: u32, packet: u32, operation: u32, option: u32);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_metadata_preview(job: CaptureJob, destination: Address32) {
    unsafe { xm1_pipeline_sensor_collect_settings(job.0, destination.0, 0x1568ffc, 14, 0); }
}

unsafe extern "C" {
    fn xm1_pipeline_image_finalize_noise_tables(job: Address32, output: Address32, descriptor: Address32);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_populate_sensor_snapshot(job: CaptureJob, destination: Address32) {
    unsafe {
        if xm1_pipeline_image_metadata_distortion_enabled(job) != 0 {
            let width = word(job.0, 0x430);
            let configuration = 0x1568fac;
            store(configuration, 0x18, width);
            let height = word(job.0, 0x434);
            store(configuration, 0x1c, height); store(configuration, 4, 0);
            store(configuration, 0x38, width); store(configuration, 0x3c, height);
            store(configuration, 8, 0x1000); store(configuration, 12, 0x1000);
            store(configuration, 0x20, width.wrapping_sub(1)); store(configuration, 0x24, height.wrapping_sub(1));
            store(configuration, 0x14, width); store(configuration, 0x2c, width);
            store(configuration, 0x30, 0); store(configuration, 0x34, 0);
            xm1_pipeline_image_build_distortion(job.0 as *mut u8, configuration as *const u8, 0x156acd4 as *mut u8);
            store_half(destination.0, 0xc0, word(0x156acd4, 4) as u16);
            store_half(destination.0, 0xc2, word(0x156acd4, 8) as u16);
            store_half(destination.0, 0xc4, half(0x156acd4, 12));
            store_half(destination.0, 0xc6, half(0x156acd4, 14));
            Address32(destination.0.wrapping_add(0xc8)).write_byte(half(0x156acd4, 16) as u8);
            Address32(destination.0.wrapping_add(0xc9)).write_byte(half(0x156acd4, 18) as u8);
            store_half(destination.0, 0xca, word(0x156acd4, 20) as u16);
            store_half(destination.0, 0xcc, word(0x156acd4, 24) as u16);
            Address32(destination.0.wrapping_add(0xbe)).write_byte(word(0x156acd4, 0) as u8);
            for index in 0..31 { store(destination.0, 0xd0 + index * 4, word(0x156acd4, 0x1c + index * 4)); }
        } else {
            for offset in [0xc0,0xc2,0xc4,0xc6] { store_half(destination.0, offset, 0); }
            Address32(destination.0.wrapping_add(0xc8)).write_byte(0); Address32(destination.0.wrapping_add(0xc9)).write_byte(0);
            store_half(destination.0, 0xca, 0); store_half(destination.0, 0xcc, 0);
            Address32(destination.0.wrapping_add(0xbe)).write_byte(0);
            for index in 0..31 { store(destination.0, 0xd0 + index * 4, 0); }
        }
        xm1_pipeline_image_export_metadata_snapshot(SnapshotSelection(Address32(job.0.wrapping_add(0x2258)).byte() as u32), SnapshotSelection(Address32(job.0.wrapping_add(0x2220)).byte() as u32), Address32(0x156973c));
        Address32(destination.0.wrapping_add(0x14c)).write_byte(Address32(0x156973c).byte());
        Address32(destination.0.wrapping_add(0x14d)).write_byte(Address32(0x156973d).byte());
        copy(destination.0.wrapping_add(0x14e), 0x156973e, 55);
        let active = (word(job.0, 0x4ac) != 1) as u32;
        fill(destination.0.wrapping_add(10), 56, 0);
        let primary = if word(job.0, 0x2728) == 0 && active != 0 {
            xm1_pipeline_sensor_primary_geometry(job.0, 0x15699cc, 3, 0, 0) == 1
        } else { false };
        if primary {
            Address32(destination.0).write_byte(1);
            store_half(destination.0, 2, word(0x15699cc, 0) as u16);
            store_half(destination.0, 4, word(0x15699cc, 4) as u16);
            Address32(destination.0.wrapping_add(6)).write_byte(word(0x15699cc, 8) as u8);
            Address32(destination.0.wrapping_add(7)).write_byte(word(0x15699cc, 12) as u8);
            store_half(destination.0, 8, word(0x15699cc, 16) as u16);
            for index in 0..28 { store_half(destination.0, 10 + index * 2, word(0x15699cc, 20 + index * 4) as u16); }
        } else {
            Address32(destination.0).write_byte(0); store_half(destination.0, 2, 0); store_half(destination.0, 4, 0);
            Address32(destination.0.wrapping_add(6)).write_byte(0); Address32(destination.0.wrapping_add(7)).write_byte(0); store_half(destination.0, 8, 0);
        }
        fill(destination.0.wrapping_add(0x4e), 56, 0); fill(destination.0.wrapping_add(0x86), 56, 0);
        xm1_pipeline_image_prepare_metadata_preview(job, Address32(0x156af00));
        xm1_pipeline_image_finalize_noise_tables(Address32(job.0), Address32(0x1569a74), Address32(0x156af00));
        if active & (word(0x1569a74, 0) >> 3) != 0 {
            for (target, origin) in [(0x42,0x1a6),(0x43,0x1a7),(0x44,0x1a8)] { Address32(destination.0.wrapping_add(target)).write_byte(Address32(0x1569a74 + origin).byte()); }
            store_half(destination.0, 0x48, half(0x1569a74, 0x1aa)); store_half(destination.0, 0x46, half(0x1569a74, 0x1ac));
            Address32(destination.0.wrapping_add(0x4a)).write_byte(half(0x1569a74, 0x1b2) as u8);
            Address32(destination.0.wrapping_add(0x4b)).write_byte(half(0x1569a74, 0x1b0) as u8);
            store_half(destination.0, 0x4c, half(0x1569a74, 0x1ae));
            for index in 0..28 { store_half(destination.0, 0x4e + index * 2, half(0x1569a74, 0x1b4 + index * 2)); }
            for index in 0..28 { store_half(destination.0, 0x86 + index * 2, half(0x1569a74, 0x1ec + index * 2)); }
        } else {
            for offset in [0x42,0x43,0x44] { Address32(destination.0.wrapping_add(offset)).write_byte(0); }
            store_half(destination.0, 0x48, 0); store_half(destination.0, 0x46, 0);
            Address32(destination.0.wrapping_add(0x4a)).write_byte(0); Address32(destination.0.wrapping_add(0x4b)).write_byte(0); store_half(destination.0, 0x4c, 0);
        }
    }
}

unsafe fn copy_group_two(destination: u32, source: u32) {
    unsafe {
        let first = word(source, 0); let second = word(source, 4);
        store(destination, 0, first); store(destination, 4, second);
    }
}
unsafe fn copy_group_three(destination: u32, source: u32) {
    unsafe {
        let first = word(source, 0); let second = word(source, 4); let third = word(source, 8);
        store(destination, 0, first); store(destination, 4, second); store(destination, 8, third);
    }
}
unsafe fn copy_group_four(destination: u32, source: u32) {
    unsafe {
        let first = word(source, 0); let second = word(source, 4); let third = word(source, 8); let fourth = word(source, 12);
        store(destination, 0, first); store(destination, 4, second); store(destination, 8, third); store(destination, 12, fourth);
    }
}

unsafe fn metadata_curve_knots(knots: FloatSamples) {
    unsafe {
        copy_group_four(knots.0, 0xec0010);
        copy_group_four(knots.0.wrapping_add(16), 0xec0020);
        store(knots.0, 32, word(0xec0030, 0));
    }
}

unsafe fn metadata_curve_coefficients(knots: FloatSamples, values: FloatSamples) {
    unsafe {
        xm1_pipeline_image_build_metadata_spline(SplineCount(9), knots, values, FloatSamples(0x41717078), FloatSamples(0x4171709c), FloatSamples(0x417170c0), FloatSamples(0x417170e0));
    }
}

unsafe fn metadata_curve_sample(knots: FloatSamples, index: u32) -> i32 {
    unsafe {
        let position = index as f32 / 10.0f32;
        xm1_pipeline_image_evaluate_metadata_spline(SplineCount(9), knots, position, FloatSamples(0x41717078), FloatSamples(0x4171709c), FloatSamples(0x417170c0), FloatSamples(0x417170e0)) as i32
    }
}

unsafe fn metadata_configuration_offset(key: ConfigurationKey, offset: u32) -> u32 {
    unsafe {
        let base = xm1_pipeline_image_configuration_pointer(key).0;
        let origin = xm1_pipeline_image_configuration_pointer(ConfigurationKey(0)).0;
        base.wrapping_add(offset).wrapping_sub(origin)
    }
}

unsafe fn populate_raf_curves(job: CaptureJob, destination: MetadataBlock, mirror: MetadataBlock) {
    unsafe {
        let mut knots = core::mem::MaybeUninit::<[u32; 9]>::uninit();
        let knots = FloatSamples(knots.as_mut_ptr() as u32);
        metadata_curve_knots(knots);
        let mut enabled = word(job.0, 0x4ac) != 1;
        store(destination.0, 0x1b8, configuration_high_first(ConfigurationKey(0x4d9e)) as u32); store(destination.0, 0x1bc, 11);
        for index in 0..11 { store(destination.0, 0x1c0 + index * 8, index); store(destination.0, 0x1c4 + index * 8, 10); }
        let sensor = xm1_pipeline_image_metadata_sensor_band(job);
        let level = xm1_pipeline_image_metadata_level_band(job);
        let key = if word(job.0, 0x3240) == 1 {
            match word(job.0, 0x3244) {
                value @ 1..=6 => metadata_configuration_offset(ConfigurationKey(0x33d0), (6 - value) * 16),
                _ => { enabled = false; metadata_configuration_offset(ConfigurationKey(0x33d0), 0) }
            }
        } else {
            metadata_configuration_offset(ConfigurationKey(0x3e3a55), level.wrapping_mul(15).wrapping_add(sensor).wrapping_mul(16))
        };
        store(0x41717054, 0, 0);
        for index in 0..8 {
            let value = xm1_pipeline_image_configuration_signed_half(ConfigurationKey(key.wrapping_add(index * 2))) as f64;
            let value = value * f64::from_bits(0x4034000000000000);
            let value = value * f64::from_bits(0x408f400000000000);
            let value = value * f64::from_bits(0x3f40000000000000);
            store(0x41717054, 4 + index * 4, (value as f32).to_bits());
        }
        metadata_curve_coefficients(knots, FloatSamples(0x41717054));
        for index in 0..11 {
            store(destination.0, 0x218 + index * 8, if enabled { metadata_curve_sample(knots, index) as u32 } else { 0 });
            store(destination.0, 0x21c + index * 8, 1000);
        }
        if mirror.0 != 0 { copy(mirror.0.wrapping_add(0x1b8), destination.0.wrapping_add(0x1b8), 0xb8); }
        let mut triplet = core::mem::MaybeUninit::<[u16; 3]>::uninit();
        let triplet_address = triplet.as_mut_ptr() as u32;
        for (mode, offset, count) in [(10,0x270,17),(25,0x280,21)] {
            xm1_pipeline_image_rate_grid_weights(RateGridMode(mode), ProfileValue(half(job.0, 0x736) as i16 as i32), PackedTriplet(triplet_address));
            for index in 0..3 { store(destination.0, offset + index * 4, half(triplet_address, index * 2) as u32); }
            store(destination.0, offset + 12, count);
        }
        if mirror.0 != 0 { copy_group_four(mirror.0.wrapping_add(0x270), destination.0.wrapping_add(0x270)); copy_group_four(mirror.0.wrapping_add(0x280), destination.0.wrapping_add(0x280)); }
        for (source, target) in [(0x71c,0x290),(0x2230,0x29c)] {
            for index in 0..3 { store(destination.0, target + index * 4, half(job.0, source + index * 2) as u32); }
            if mirror.0 != 0 { copy_group_three(mirror.0.wrapping_add(target), destination.0.wrapping_add(target)); }
        }
        metadata_curve_knots(knots);
        let enabled = word(job.0, 0x4ac) != 1;
        store(destination.0, 0x2a8, configuration_high_first(ConfigurationKey(0x4d9e)) as u32); store(destination.0, 0x2ac, 10);
        for index in 0..10 { store(destination.0, 0x2b0 + index * 8, index + 1); store(destination.0, 0x2b4 + index * 8, 10); }
        xm1_pipeline_image_prepare_metadata_preview(job, Address32(0x15b7e30));
        let channel = Address32(0x15b7f9c).byte() as u32;
        let band = xm1_pipeline_image_metadata_threshold_band(ConfigurationChannel(channel), ProfileValue(word(0x15b7e30, 0x230) as i32));
        let key = metadata_configuration_offset(ConfigurationKey(0x3e3dd0), band.wrapping_mul(15).wrapping_add(channel).wrapping_mul(16));
        store(0x41717054, 0xac, 0); store(0x41717054, 0xd0, 0);
        for index in 0..8 {
            let first = xm1_buffers_configuration_byte(ConfigurationKey(key.wrapping_add(index))) as u8 as i8 as f64;
            let first = first * f64::from_bits(0x412e848000000000);
            let first = first * f64::from_bits(0x3f00000000000000);
            store(0x41717054, 0xd4 + index * 4, (first as f32).to_bits());
            let second = xm1_buffers_configuration_byte(ConfigurationKey(key.wrapping_add(index).wrapping_add(8))) as u8 as i8 as f64;
            let second = second * f64::from_bits(0x412e848000000000);
            let second = second * f64::from_bits(0x3f00000000000000);
            store(0x41717054, 0xb0 + index * 4, (second as f32).to_bits());
        }
        for (samples, offset) in [(0x41717124,0x300),(0x41717100,0x350)] {
            metadata_curve_coefficients(knots, FloatSamples(samples));
            for index in 1..11 {
                store(destination.0, offset + (index - 1) * 8, if enabled { metadata_curve_sample(knots, index) as u32 } else { 0 });
                store(destination.0, offset + 4 + (index - 1) * 8, 1000000);
            }
        }
        if mirror.0 != 0 { copy(mirror.0.wrapping_add(0x2a8), destination.0.wrapping_add(0x2a8), 0xf8); }
        metadata_curve_knots(knots);
        let mut mode = 0u8; let mut channel = 0u8; let mut level = 0u8;
        let mut enabled = word(job.0, 0x4ac) != 1;
        store(destination.0, 0x3a0, configuration_high_first(ConfigurationKey(0x4d9e)) as u32); store(destination.0, 0x3a4, 11);
        for index in 0..11 { store(destination.0, 0x3a8 + index * 8, index); store(destination.0, 0x3ac + index * 8, 10); }
        xm1_pipeline_sensor_select_calibration(Address32(job.0.wrapping_add(0x6c0)).byte().wrapping_sub(1) as u32, Address32(job.0.wrapping_add(0x660)).byte() as u32, half(job.0, 0x2264) as i16 as i32, &mut mode, &mut channel, &mut level);
        if word(job.0, 0x3240) == 1 {
            let selection = Address32(job.0.wrapping_add(0x3250)).byte() as i8;
            let row = match selection { 1..=5 => Some(5 - selection as u32), -5..=-1 => Some((4 - selection as i32) as u32), _ => None };
            let offset = match row { Some(index) => index * 9, None => { enabled = false; 0 } };
            let numerator = metadata_configuration_offset(ConfigurationKey(0x4400), offset);
            let denominator = metadata_configuration_offset(ConfigurationKey(0x4401), offset);
            store(0x41717054, 0xf4, 0x461c4000);
            for index in 0..8 {
                let numerator_value = xm1_buffers_configuration_byte(ConfigurationKey(denominator.wrapping_add(index)));
                let denominator_value = xm1_buffers_configuration_byte(ConfigurationKey(numerator));
                let value = numerator_value as f64 * f64::from_bits(0x4059000000000000);
                let value = value * f64::from_bits(0x4059000000000000);
                let value = value / denominator_value as f64;
                store(0x41717054, 0xf8 + index * 4, (value as f32).to_bits());
            }
        } else {
            let offset = (level as u32).wrapping_mul(90).wrapping_add((channel as u32).wrapping_mul(15)).wrapping_add(mode as u32).wrapping_mul(8);
            let key = metadata_configuration_offset(ConfigurationKey(0x3e3170), offset);
            store(0x41717054, 0xf4, 0x461c4000);
            for index in 0..8 {
                let value = xm1_buffers_configuration_byte(ConfigurationKey(key.wrapping_add(index))) as f64;
                let value = value * f64::from_bits(0x4059000000000000);
                let value = value * f64::from_bits(0x4059000000000000);
                let value = value / f64::from_bits(0x406fe00000000000);
                store(0x41717054, 0xf8 + index * 4, (value as f32).to_bits());
            }
        }
        metadata_curve_coefficients(knots, FloatSamples(0x41717148));
        for index in 0..11 {
            store(destination.0, 0x400 + index * 8, if enabled { metadata_curve_sample(knots, index) as u32 } else { 10000 });
            store(destination.0, 0x404 + index * 8, 100);
        }
        store(destination.0, 0x458, 0);
        if mirror.0 != 0 { store(mirror.0, 0x458, 0); }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RafCaptureRecord(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RafExtension(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct OutputContainer(pub u32);

unsafe extern "C" {
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_develop_append_stream_text(task: u32, text: u32);
}

unsafe fn invalid_raf_record_size() -> ! {
    unsafe { xm1_develop_append_stream_text(xm1_pipeline_image_current_task() & 0xff, 0xec0078); }
    loop { core::hint::spin_loop(); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_populate_raf_metadata(job: CaptureJob, container: OutputContainer, record: RafCaptureRecord, mirror: RafCaptureRecord, extension: RafExtension, extension_mirror: RafExtension) {
    unsafe {
        let key = if word(job.0, 0x5f0) == 0 {
            match word(job.0, 0x63c) { 0 => 0x4d44, 2 => 0x4d54, _ => 0x4d4c }
        } else {
            match word(job.0, 0x63c) { 0 => 0x4d5c, 2 => 0x4d6c, _ => 0x4d64 }
        };
        for index in 0..8 { Address32(container.0.wrapping_add(0x60 + index)).write_byte(xm1_buffers_configuration_byte(ConfigurationKey(key + index)) as u8); }
        for (offset, first, second) in [(0,0x4d20,0x4d22),(4,0x4d24,0x4d26),(8,0x4d28,0x4d2a),(12,0x4d78,0x4d7a),(16,0x4d86,0x4d84)] {
            store_half(record.0, offset, configuration_high_first(ConfigurationKey(first)));
            store_half(record.0, offset + 2, configuration_high_first(ConfigurationKey(second)));
            if mirror.0 != 0 { store(mirror.0, offset, word(record.0, offset)); }
        }
        for (offset, key) in [(20,0x4d43),(21,0x4d42),(22,0x4d41),(23,0x4d40)] {
            Address32(record.0.wrapping_add(offset)).write_byte(xm1_buffers_configuration_byte(ConfigurationKey(key)) as u8);
        }
        if mirror.0 != 0 { store(mirror.0, 20, word(record.0, 20)); }
        for index in 0..36 { Address32(record.0.wrapping_add(0x3b - index)).write_byte(xm1_buffers_configuration_byte(ConfigurationKey(0x50d0 + index)) as u8); }
        if mirror.0 != 0 {
            copy_group_four(mirror.0.wrapping_add(24), record.0.wrapping_add(24));
            copy_group_four(mirror.0.wrapping_add(40), record.0.wrapping_add(40));
            store(mirror.0, 56, word(record.0, 56));
        }
        store_half(record.0, 60, 12); store_half(record.0, 62, 36);
        if mirror.0 != 0 { store(mirror.0, 60, word(record.0, 60)); }
        let exposure = half(job.0, 0x164e).wrapping_add(half(job.0, 0x164c)).wrapping_add(half(job.0, 0x1650));
        let exposure = exposure.wrapping_sub(configuration_high_first(ConfigurationKey(0x4d91)));
        store_half(record.0, 64, exposure.wrapping_neg()); store_half(record.0, 66, 100);
        if mirror.0 != 0 {
            store_half(mirror.0, 64, word(job.0, 0x648).wrapping_sub(word(job.0, 0x64c)).wrapping_sub(exposure as u32) as u16);
            store_half(mirror.0, 66, 100);
        }
        store(job.0, 0x19ac, 0x23a8);
        let mut weights = core::mem::MaybeUninit::<[u16; 3]>::uninit();
        let weights_address = weights.as_mut_ptr() as u32;
        for mode in 5..11 {
            xm1_pipeline_image_rate_grid_weights(RateGridMode(mode), ProfileValue(half(job.0, 0x736) as i16 as i32), PackedTriplet(weights_address));
            for index in 0..3 { store_half(job.0, 0x1894 + (mode - 5) * 6 + index * 2, half(weights_address, index * 2)); }
        }
        xm1_pipeline_image_select_metadata_balance(BalanceSource(1), BalancePreference(0), Address32(weights_address + 2), Address32(weights_address), Address32(weights_address + 4));
        if half(weights_address, 0) == 0 || half(weights_address, 2) == 0 || half(weights_address, 4) == 0 {
            xm1_pipeline_image_rate_grid_weights(RateGridMode(5), ProfileValue(half(job.0, 0x736) as i16 as i32), PackedTriplet(weights_address));
        }
        for index in 0..3 { store_half(job.0, 0x18b8 + index * 2, half(weights_address, index * 2)); }
        for index in 0..3 { store_half(job.0, 0x18be + index * 2, half(job.0, 0x1664 + index * 2)); }
        const TEMPERATURES: [i32; 31] = [2500,2550,2650,2700,2800,2850,2950,3000,3100,3200,3300,3400,3600,3700,3800,4000,4200,4300,4500,4800,5000,5300,5600,5900,6300,6700,7100,7700,8300,9100,10000];
        for (index, temperature) in TEMPERATURES.iter().enumerate() {
            xm1_pipeline_image_profile_adjusted_limits(*temperature, ProfileValue(half(job.0, 0x736) as i16 as i32), PackedTriplet(weights_address));
            for component in 0..3 { store_half(job.0, 0x18c4 + index as u32 * 6 + component * 2, half(weights_address, component * 2)); }
        }
        Address32(job.0.wrapping_add(0x188c)).write_byte(xm1_buffers_configuration_byte(ConfigurationKey(0x17936)) as u8);
        copy_local(record.0.wrapping_add(68), &0x14d0000u32, 4);
        let mut final_offset = 0u32;
        let mut final_size = 0u32;
        for index in 0..10 {
            let descriptor = 0xec01c0 + index * 4;
            let offsets = 0xec004c + index * 4;
            let size = half(descriptor, 2) as u32;
            let source = half(offsets, 0) as u32;
            let destination = half(offsets, 2) as u32;
            let present = half(descriptor, 0);
            if size != 0 && present != 0 { copy(record.0.wrapping_add(72).wrapping_add(destination), job.0.wrapping_add(0x424).wrapping_add(source), size); }
            final_offset = destination; final_size = size;
        }
        if final_offset.wrapping_add(final_size) != 0x23a4 { invalid_raf_record_size(); }
        xm1_pipeline_image_populate_sensor_snapshot(job, Address32(record.0.wrapping_add(0x52f4)));
        if mirror.0 != 0 { copy(mirror.0.wrapping_add(68), record.0.wrapping_add(68), 0x55c0); }
        store_half(extension.0, 0, 0x4949); store_half(extension.0, 2, 42); store(extension.0, 4, 8);
        if extension_mirror.0 != 0 { copy_group_two(extension_mirror.0, extension.0); }
        store_half(extension.0, 8, 1); store_half(extension.0, 12, 0xf000);
        store(extension.0, 16, 13); store(extension.0, 20, 1); store(extension.0, 24, 26); store(extension.0, 28, 0);
        if extension_mirror.0 != 0 {
            copy_group_four(extension_mirror.0.wrapping_add(8), extension.0.wrapping_add(8));
            copy_group_two(extension_mirror.0.wrapping_add(24), extension.0.wrapping_add(24));
        }
        store_half(extension.0, 32, 16);
        let mut cursor = 224u32;
        for index in 0..16 {
            let rule = 0xec00c0 + index * 16;
            let destination = extension.0.wrapping_add(36 + index * 16);
            let tag = half(rule, 0);
            store_half(destination, 0, tag);
            store(destination, 4, word(rule, 4)); store(destination, 8, word(rule, 8));
            let value = match tag {
                0xf001 => configuration_high_first(ConfigurationKey(0x4d22)) as u32,
                0xf002 => configuration_high_first(ConfigurationKey(0x4d20)) as u32,
                0xf003 => 12, 0xf004 => 4,
                0xf005 => {
                    let denominator = word(extension.0, 0x60);
                    let second = word(extension.0, 0x50);
                    let first = word(extension.0, 0x30);
                    __aeabi_uidiv(first.wrapping_mul(second) >> 3, if denominator == 0 { 1 } else { denominator })
                }
                0xf006 => 0, 0xf007 => 0x800, 0xf008 => word(container.0, 0x30).wrapping_sub(0x800),
                0xf009 => xm1_buffers_configuration_byte(ConfigurationKey(0x4d9d)),
                _ => { let offset = cursor; cursor = cursor.wrapping_add(word(rule, 12)); offset }
            };
            store(destination, 12, value);
        }
        store(extension.0, 0x124, 0);
        if extension_mirror.0 != 0 { copy(extension_mirror.0.wrapping_add(32), extension.0.wrapping_add(32), 0x108); }
        let first = word(job.0, 0x1610);
        let average = (word(job.0, 0x160c) >> 2).wrapping_add(first >> 2);
        let average = average.wrapping_add(word(job.0, 0x1614) >> 2).wrapping_add(word(job.0, 0x1618) >> 2).wrapping_add(word(job.0, 0x161c) >> 2).wrapping_add(word(job.0, 0x1620) >> 2).wrapping_add(word(job.0, 0x1624) >> 2).wrapping_add(word(job.0, 0x1628) >> 2) >> 3;
        for index in 0..36 { store(extension.0, 0x128 + index * 4, average); }
        populate_raf_curves(job, MetadataBlock(extension.0), MetadataBlock(extension_mirror.0));
    }
}
