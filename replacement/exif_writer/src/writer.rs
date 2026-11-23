//! The overlay-46 Exif writer of X-M1 1.50, read from the A32 bytes at `0x011c14f0..0x011db1ac`.
//!
//! Every address is a guest address. Stack temporaries whose addresses reach a service, a
//! callback or a retained table are taken from [`Guest::frame`]; the rest are Rust locals.

/// Writer state at `0x014e9d50`: TIFF base, then the slots later passes patch.
pub const WRITER: u32 = 0x014e_9d50;
/// Settings at `0x014e9e3c`: mode, byte order, directory format, buffer, option.
pub const SETTINGS: u32 = 0x014e_9e3c;
/// Camera identity at `0x014e9d98`: make (16), model (32), software (64), then text slots.
pub const IDENTITY: u32 = 0x014e_9d98;
/// Last-directory trace the stock writer keeps at `0x014e9ea0`.
pub const DEBUG: u32 = 0x014e_9ea0;

const MODEL_TEXT: u32 = IDENTITY + 0x10;
const SOFTWARE_TEXT: u32 = IDENTITY + 0x30;
#[cfg_attr(exif_control_interop_index_fixed, allow(dead_code))]
const INTEROP_INDEX_TEXT: u32 = IDENTITY + 0x70;
const DEFAULT_TEXT: u32 = IDENTITY + 0x74;

const SAVED_STATE: [u32; 2] = [0x014e_9e50, 0x014e_9e54];
const SAVED_FILE: [u32; 2] = [0x014e_9e58, 0x014e_9e4c];

const SOI: u32 = 0x011c_2ae8;
const INTEL_ORDER: u32 = 0x011c_2aec;
const MOTOROLA_ORDER: u32 = 0x011c_2af0;
const APP1_MARKER: u32 = 0x011c_2af4;
const EXIF_IDENTIFIER: u32 = 0x011c_2af8;
const INTEROP_VERSION: u32 = 0x011c_2b00;
const INTEROP_R03: u32 = 0x011c_2b18;
const TYPE_SIZES: u32 = 0x011c_2cb8;
const MAKER_SIGNATURE: u32 = 0x011d_0404;
const SAVED_MASK_B: u32 = 0x011d_040c;
const PRINTIM: u32 = 0x011d_22fc;
const PRINTIM_VERSION: u32 = 0x011d_2304;
const MODE_MASKS: u32 = 0x011d_230c;
const SAVED_MASK_ALWAYS: u32 = 0x011d_2314;
const SAVED_MASK_A: u32 = 0x011d_24e4;

/// Bytes of a retained tag-table rule: tag, flags, type and maximum count halfwords, callback,
/// two words, the saved-replay masks of passes 0 and 1, and the value offset in the source block.
const RULE_BYTES: u32 = 32;
const ENTRY_BYTES: u32 = 12;
const SAVED_SLOT_BYTES: u32 = 12;

/// Directory selectors the tag tables, the filters and the saved parser share.
pub mod kind {
    pub const PRIMARY: u32 = 0;
    pub const EXIF: u32 = 1;
    pub const MAKER: u32 = 2;
    pub const IMAGE_RECORD: u32 = 3;
    pub const GPS: u32 = 5;
    pub const THUMBNAIL: u32 = 6;
    pub const MPF_INDEX: u32 = 7;
}

/// Directory formats in `SETTINGS+4`: build from the source block, or replay a saved parse.
mod format {
    pub const FILTERED: u16 = 0;
    pub const SAVED_FIRST: u16 = 1;
    pub const SAVED_SECOND: u16 = 2;
}

/// Memory and the retained services the writer calls.
pub trait Guest {
    fn r8(&self, address: u32) -> u8;
    fn r16(&self, address: u32) -> u16;
    fn r32(&self, address: u32) -> u32;
    fn w8(&mut self, address: u32, value: u8);
    fn w16(&mut self, address: u32, value: u16);
    fn w32(&mut self, address: u32, value: u32);
    /// Byte copy `0x0004357c`.
    fn copy_bytes(&mut self, destination: u32, source: u32, length: u32);
    /// `memcpy` `0x00722054`.
    fn copy_memory(&mut self, destination: u32, source: u32, length: u32);
    /// `strcpy` `0x00721b04`.
    fn copy_string(&mut self, destination: u32, source: u32);
    /// `strncpy` `0x00721b40`.
    fn copy_string_bounded(&mut self, destination: u32, source: u32, length: u32);
    /// `strlen` `0x00721f30`.
    fn string_length(&mut self, source: u32) -> u32;
    /// `memset` `0x007221c8`.
    fn fill(&mut self, destination: u32, value: u32, length: u32);
    /// Calls `callback(source, slot, cursor, layout)` with `entry` as the stack argument.
    fn rule_callback(&mut self, callback: u32, source: u32, slot: u32, cursor: u32, layout: u32, entry: u32);
    /// Runs `body` with the guest address of 64 word-aligned bytes that live for the call.
    fn frame<R>(&mut self, body: impl FnOnce(&mut Self, u32) -> R) -> R;
}

fn mode<G: Guest>(g: &G) -> u16 {
    g.r16(SETTINGS)
}

fn directory_format<G: Guest>(g: &G) -> u16 {
    g.r16(SETTINGS + 4)
}

/// `0x011c14f0`.
pub fn initialize<G: Guest>(g: &mut G, writer_mode: u16, order: u16, directory: u16, buffer: u32, option: u16) {
    g.w16(SETTINGS, writer_mode);
    g.w16(SETTINGS + 2, order);
    g.w16(SETTINGS + 4, directory);
    g.w32(SETTINGS + 8, buffer);
    g.w16(SETTINGS + 12, option);
    for offset in [0x0, 0x4, 0x8, 0xc, 0x10, 0x18, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c, 0x40] {
        g.w32(WRITER + offset, 0);
    }
    g.w16(WRITER + 68, 0);
    g.w16(WRITER + 70, 0);
}

/// `0x011c2c40`.
pub fn copy_value<G: Guest>(g: &mut G, destination: u32, source: u32, length: u32) -> u32 {
    g.copy_memory(destination, source, length);
    length
}

/// `0x011c2c58`: claims `size` bytes at the cursor, padded to an even length with a zero.
fn allocate<G: Guest>(g: &mut G, size: u32, cursor: &mut u32) -> u32 {
    let start = *cursor;
    *cursor = start.wrapping_add(size);
    if size & 1 != 0 {
        g.w8(start.wrapping_add(size), 0);
        *cursor = cursor.wrapping_add(1);
        return size + 1;
    }
    size
}

/// `0x011c2c98`.
pub fn type_size<G: Guest>(g: &G, value_type: u32) -> u32 {
    g.r8(TYPE_SIZES.wrapping_add(value_type)) as u32
}

/// `0x011c2b50`.
pub fn swap_halfword<G: Guest>(g: &mut G, address: u32) {
    let high = g.r8(address + 1) as u16;
    let low = g.r8(address) as u16;
    g.w16(address, high | low << 8);
}

/// `0x011c1574`.
fn write_soi<G: Guest>(g: &mut G, destination: u32) -> u32 {
    g.frame(|g, local| {
        g.copy_bytes(local, SOI, 2);
        g.copy_memory(destination, local, 2);
        2
    })
}

/// `0x011c1670`.
fn write_app1<G: Guest>(g: &mut G, destination: u32) -> u32 {
    g.frame(|g, local| {
        let identifier = local;
        let length = local + 6;
        let marker = local + 8;
        g.copy_bytes(marker, APP1_MARKER, 2);
        g.w16(length, 0);
        g.copy_bytes(identifier, EXIF_IDENTIFIER, 6);
        let mut at = destination + copy_value(g, destination, marker, 2);
        g.w32(WRITER + 0x2c, at);
        at += copy_value(g, at, length, 2);
        at += copy_value(g, at, identifier, 6);
        at - destination
    })
}

/// `0x011c15b8`.
fn write_tiff_header<G: Guest>(g: &mut G, destination: u32) -> u32 {
    g.frame(|g, local| {
        let first = local;
        let magic = local + 6;
        let motorola = local + 8;
        let intel = local + 12;
        g.copy_bytes(intel, INTEL_ORDER, 2);
        g.copy_bytes(motorola, MOTOROLA_ORDER, 2);
        g.w16(magic, 42);
        g.w32(first, 8);
        g.w32(WRITER, destination);
        #[cfg(not(exif_control_motorola_header))]
        let order = if g.r16(SETTINGS + 2) == 0 { intel } else { motorola };
        #[cfg(exif_control_motorola_header)]
        let order = motorola;
        let mut at = destination + copy_value(g, destination, order, 2);
        at += copy_value(g, at, magic, 2);
        at += copy_value(g, at, first, 4);
        at - destination
    })
}

/// Copies `value` through the slot pointer held at `WRITER + slot_field`.
fn patch_offset<G: Guest>(g: &mut G, slot_field: u32, value: u32) {
    g.frame(|g, local| {
        g.w32(local, value);
        let slot = g.r32(WRITER + slot_field);
        g.copy_memory(slot, local, 4);
    })
}

/// `saved_pass` is the replay pass format 1 uses; format 2 always replays pass 1.
fn write_directory<G: Guest>(g: &mut G, kind: u32, source: u32, destination: u32, base: u32, saved_pass: u32) -> Option<u32> {
    match directory_format(g) {
        format::FILTERED => Some(write_filtered_directory(g, kind, source, destination, base)),
        format::SAVED_FIRST => Some(write_saved_directory(g, source, destination, kind, saved_pass)),
        format::SAVED_SECOND => Some(write_saved_directory(g, source, destination, kind, 1)),
        _ => None,
    }
}

/// `0x011c170c`.
pub fn write_primary<G: Guest>(g: &mut G, source: u32, destination: u32) -> u32 {
    let writer_mode = mode(g);
    if writer_mode > 3 {
        return 0;
    }
    #[cfg(not(exif_control_raw_base_unset))]
    if directory_format(g) == format::FILTERED && writer_mode == 3 {
        g.w32(SETTINGS + 8, destination);
        g.w32(WRITER, destination);
    }
    let base = g.r32(WRITER);
    write_directory(g, kind::PRIMARY, source, destination, base, 0).unwrap_or(0)
}

/// `0x011c17cc`.
pub fn write_exif<G: Guest>(g: &mut G, source: u32, destination: u32) -> u32 {
    let writer_mode = mode(g);
    if writer_mode > 3 {
        return 0;
    }
    patch_offset(g, 0x4, destination.wrapping_sub(g.r32(WRITER)));
    let base = g.r32(WRITER);
    write_directory(g, kind::EXIF, source, destination, base, (writer_mode == 3) as u32).unwrap_or(0)
}

/// `0x011c18ac`.
fn write_interoperability<G: Guest>(g: &mut G, source: u32, destination: u32) -> u32 {
    g.frame(|g, local| {
        let next = local;
        let version = local + 4;
        let count = local + 10;
        let offset = local + 12;
        let index = local + 16;
        let revision = local + 28;
        g.w16(count, 2);
        g.copy_bytes(version, INTEROP_VERSION, 4);
        g.w32(next, 0);
        g.w32(offset, destination.wrapping_sub(g.r32(WRITER)));
        let slot = g.r32(WRITER + 0x8);
        g.copy_memory(slot, offset, 4);
        g.w16(index, 1);
        g.w16(index + 2, 2);
        g.w32(index + 4, 4);
        #[cfg(not(exif_control_interop_index_fixed))]
        let text = if g.r16(source + 0x28a) == 0xffff { INTEROP_R03 } else { INTEROP_INDEX_TEXT };
        #[cfg(exif_control_interop_index_fixed)]
        let text = {
            let _ = source;
            INTEROP_R03
        };
        g.copy_string(index + 8, text);
        g.w16(revision, 2);
        g.w16(revision + 2, 7);
        g.w32(revision + 4, 4);
        g.copy_memory(revision + 8, version, 4);
        let mut at = destination + copy_value(g, destination, count, 2);
        at += copy_value(g, at, index, ENTRY_BYTES);
        at += copy_value(g, at, revision, ENTRY_BYTES);
        at += copy_value(g, at, next, 4);
        at - destination
    })
}

/// `0x011cf6e0`.
fn write_maker_signature<G: Guest>(g: &mut G, destination: u32) -> u32 {
    g.frame(|g, local| {
        let signature = local;
        let directory = local + 8;
        g.w32(directory, 12);
        g.copy_bytes(signature, MAKER_SIGNATURE, 8);
        g.w32(WRITER + 0x1c, destination);
        g.copy_memory(destination, signature, 8);
        g.copy_memory(destination + 8, directory, 4);
        12
    })
}

/// `0x011c19c8`.
pub fn write_makernote<G: Guest>(g: &mut G, source: u32, destination: u32) -> u32 {
    if g.r16(WRITER + 68) == 0 {
        return 0;
    }
    let writer_mode = mode(g);
    if writer_mode > 3 {
        return 0;
    }
    patch_offset(g, 0x18, destination.wrapping_sub(g.r32(WRITER)));
    let flipped = g.r16(SETTINGS + 2) == 1;
    if flipped {
        g.w16(SETTINGS + 2, 0);
    }
    let mut size = write_maker_signature(g, destination);
    let at = destination.wrapping_add(size);
    let base = g.r32(WRITER + 0x1c);
    if let Some(body) = write_directory(g, kind::MAKER, source, at, base, (writer_mode == 3) as u32) {
        size = size.wrapping_add(body);
    }
    if flipped {
        g.w16(SETTINGS + 2, 1);
    }
    patch_offset(g, 0x14, size);
    size
}

/// `0x011c1b30`.
pub fn write_gps<G: Guest>(g: &mut G, source: u32, destination: u32) -> u32 {
    if g.r16(WRITER + 70) == 0 {
        return 0;
    }
    if mode(g) > 3 {
        return 0;
    }
    patch_offset(g, 0xc, destination.wrapping_sub(g.r32(WRITER)));
    let base = g.r32(WRITER);
    write_directory(g, kind::GPS, source, destination, base, 0).unwrap_or(0)
}

/// `0x011c1c14`.
fn write_thumbnail<G: Guest>(g: &mut G, source: u32, destination: u32) -> u32 {
    if mode(g) > 2 {
        return 0;
    }
    patch_offset(g, 0x28, destination.wrapping_sub(g.r32(WRITER)));
    let base = g.r32(WRITER);
    let size = write_directory(g, kind::THUMBNAIL, source, destination, base, 0).unwrap_or(0);
    patch_offset(g, 0x24, destination.wrapping_add(size).wrapping_sub(g.r32(WRITER)));
    size
}

#[derive(Clone, Copy)]
pub struct Sources {
    pub primary: u32,
    pub exif: u32,
    pub maker: u32,
    pub gps: u32,
    pub thumbnail: u32,
}

/// `0x011db098`: SOI, APP1, TIFF header, IFD0, Exif, maker note, interoperability, GPS when
/// `option` is 1, then IFD1; returns the bytes written.
pub fn write_exif_header<G: Guest>(g: &mut G, output: u32, sources: Sources, directory: u16, option: u32) -> u32 {
    initialize(g, 0, 0, directory, 0, option as u16);
    let mut at = output.wrapping_add(write_soi(g, output));
    at = at.wrapping_add(write_app1(g, at));
    at = at.wrapping_add(write_tiff_header(g, at));
    at = at.wrapping_add(write_primary(g, sources.primary, at));
    at = at.wrapping_add(write_exif(g, sources.exif, at));
    at = at.wrapping_add(write_makernote(g, sources.maker, at));
    at = at.wrapping_add(write_interoperability(g, sources.exif, at));
    if option == 1 {
        at = at.wrapping_add(write_gps(g, sources.gps, at));
    }
    write_thumbnail(g, sources.thumbnail, at).wrapping_add(at.wrapping_sub(output))
}

/// `0x011c1e00`.
fn patch_app1_length<G: Guest>(g: &mut G, length: u16) {
    g.frame(|g, local| {
        g.w16(local, length);
        if g.r16(SETTINGS + 2) == 0 {
            swap_halfword(g, local);
        }
        let slot = g.r32(WRITER + 0x2c);
        g.copy_memory(slot, local, 2);
    })
}

/// `0x011c1e4c`.
pub fn patch_thumbnail_length<G: Guest>(g: &mut G, length: u32) {
    patch_offset(g, 0x20, length)
}

/// `0x011db188`.
pub fn finish_exif<G: Guest>(g: &mut G, total: u32, thumbnail_length: u32) {
    patch_app1_length(g, total.wrapping_sub(4) as u16);
    patch_thumbnail_length(g, thumbnail_length);
}

/// `0x011cf854`.
pub fn select_table(kind: u32) -> u32 {
    match kind {
        0 => 0x011d_09dc,
        1 => 0x011d_041c,
        2 => 0x011d_0fbc,
        3 => 0x011d_23c4,
        4 => 0x011d_099c,
        5 => 0x011d_0c1c,
        6 => 0x011d_0ebc,
        7 => 0x011d_0e5c,
        8 => 0x011d_2324,
        _ => 0,
    }
}

/// `0x011cf908`.
pub fn table_count(kind: u32) -> u32 {
    match kind {
        0 | 5 => 18,
        1 => 44,
        2 => 154,
        3 => 9,
        4 => 2,
        6 => 8,
        7 => 3,
        8 => 5,
        _ => 0,
    }
}

/// Whether the rule's flags admit it in the current mode, or its option flag is set.
fn rule_selected<G: Guest>(g: &G, rule: u32) -> bool {
    let flags = g.r16(rule + 2);
    if flags & 0x10 != 0 && g.r16(SETTINGS + 12) == 1 {
        return true;
    }
    let mask = g.r16(MODE_MASKS + 2 * mode(g) as u32);
    flags & mask == mask
}

/// `0x011ceea0`: element width and whether the value lives outside the entry.
fn value_layout<G: Guest>(g: &mut G, entry: u32, layout: u32) {
    g.w16(layout, 0);
    g.w16(layout + 2, 0);
    let (width, inline_limit) = match (g.r16(entry + 2) as u32).wrapping_sub(1) {
        0 | 1 | 6 => (1, Some(4)),
        2 => (2, Some(2)),
        3 | 8 => (4, Some(1)),
        4 | 9 => (8, None),
        _ => return,
    };
    g.w16(layout, width);
    match inline_limit {
        Some(limit) => {
            if g.r32(entry + 4) > limit {
                g.w16(layout + 2, 1);
            }
        }
        None => g.w16(layout + 2, 1),
    }
}

/// `0x011cf490`: copies `size` value bytes element by element and pads the result.
fn copy_typed_value<G: Guest>(g: &mut G, value_type: u16, size: u16, source: u32, destination: u32) -> u32 {
    g.w16(DEBUG + 80, value_type);
    g.w16(DEBUG + 82, size);
    g.w32(DEBUG + 0x54, source);
    g.w32(DEBUG + 0x58, destination);
    let size = size as u32;
    let (unit, units) = match (value_type as u32).wrapping_sub(1) {
        0 | 1 | 6 => (size, 1),
        2 => (2, size >> 1),
        3 | 4 | 8 | 9 => (4, size >> 2),
        _ => return size,
    };
    for index in 0..units {
        g.copy_memory(destination + index * unit, source + index * unit, unit);
    }
    let end = destination.wrapping_add(units.wrapping_mul(unit));
    if size <= 3 {
        g.fill(end, 0, 4 - size);
        return 4;
    }
    #[cfg(not(exif_control_odd_values_unpadded))]
    if size & 1 != 0 {
        g.w8(end, 0);
        return (size + 1) as u16 as u32;
    }
    size
}

/// `0x011cf5a8`: one entry built from the rule and the source bytes; returns the entry and
/// value-area bytes written.
fn write_value_entry<G: Guest>(g: &mut G, rule: u32, source: u32, destination: u32, capacity: u32, base: u32, cursor: &mut u32) -> u32 {
    g.frame(|g, local| {
        let count = local;
        let offset = local + 4;
        g.w32(count, capacity);
        let start = *cursor;
        for word in 0..8 {
            let value = g.r32(rule + 4 * word);
            g.w32(DEBUG + 36 + 4 * word, value);
        }
        g.w32(DEBUG + 0x44, capacity);
        g.w32(DEBUG + 0x48, base);
        g.w32(DEBUG + 0x4c, *cursor);
        g.w32(DEBUG + 0x54, source);
        g.w32(DEBUG + 0x58, destination);
        g.copy_memory(destination, rule, 2);
        g.copy_memory(destination + 2, rule + 4, 2);
        g.copy_memory(destination + 4, count, 4);
        let value_type = g.r16(rule + 4);
        let size = g.r32(count).wrapping_mul(type_size(g, value_type as u32)) as u16;
        let end = if size > 4 {
            g.w32(offset, cursor.wrapping_sub(base));
            g.copy_memory(destination + 8, offset, 4);
            let at = *cursor;
            let written = copy_typed_value(g, value_type, size, source, at);
            *cursor = at.wrapping_add(written);
            destination + 12
        } else {
            destination + 8 + copy_typed_value(g, value_type, size, source, destination + 8)
        };
        (end - destination).wrapping_add(cursor.wrapping_sub(start))
    })
}

/// `0x011cf0dc`: the element count a rule writes for this source.
fn capacity<G: Guest>(g: &mut G, kind: u32, rule: u32, source: u32) -> u32 {
    let tag = g.r16(rule) as u32;
    let limit = g.r16(rule + 6) as u32;
    match kind {
        kind::PRIMARY => match tag {
            0x110 => g.string_length(MODEL_TEXT) + 1,
            0x131 => g.string_length(SOFTWARE_TEXT) + 1,
            0x10f => g.string_length(IDENTITY) + 1,
            _ => limit,
        },
        kind::MAKER => {
            let field = |offset: u32| g.r16(source + offset) as u32;
            let wanted = match tag {
                0x1027 => field(202) * 3,
                0x1428 => field(0x112) << 1,
                0x4009 => field(0x20a) << 2,
                0x400a | 0x400b => field(0x20a),
                0x4101 | 0x4102 | 0x4105 | 0x4107 | 0x4180 | 0x4181 => field(0x222),
                0x4103 => field(0xe03c),
                0x4104 => field(0x222) << 2,
                0x4106 => field(0xe03e),
                0x4201 | 0x4202 | 0x4205 | 0x4207 | 0x4208 | 0x4280 | 0x4281 | 0x4283 | 0x4284 => field(0x58a),
                0x4203 => field(0xe040),
                0x4204 => field(0x58a) << 2,
                0x4206 => field(0xe042),
                0x4282 => 8,
                _ => limit,
            };
            #[cfg(not(exif_control_capacity_unclamped))]
            let wanted = if wanted < limit { wanted } else { limit };
            wanted
        }
        kind::GPS => match tag {
            8 => g.string_length(source + 96) + 1,
            18 => g.string_length(source + 136) + 1,
            _ => limit,
        },
        kind::IMAGE_RECORD => {
            if tag == 0x1001 {
                g.r16(source + 8) as u32
            } else if tag > 0x1000 && tag.wrapping_sub(0x1110) <= 2 {
                g.r16(source + 60) as u32
            } else {
                limit
            }
        }
        _ => limit,
    }
}

/// Replays the directory entry of a callback rule: layout, callback, out-of-line value area.
fn write_callback_entry<G: Guest>(g: &mut G, callback: u32, source: u32, slot: u32, entry: u32, layout: u32, base: u32, cursor: &mut u32) {
    value_layout(g, entry, layout);
    g.rule_callback(callback, source, slot, *cursor, layout, entry);
    if g.r16(layout + 2) == 1 {
        g.w32(entry + 8, cursor.wrapping_sub(base));
        let size = g.r32(entry + 4).wrapping_mul(g.r16(layout) as u32);
        allocate(g, size, cursor);
    }
    g.copy_memory(slot, entry, ENTRY_BYTES);
}

fn trace_directory<G: Guest>(g: &mut G, kind: u32, source: u32, destination: u32, base: u32) {
    g.w32(DEBUG, kind);
    g.w32(DEBUG + 4, source);
    g.w32(DEBUG + 8, destination);
    g.w32(DEBUG + 0x48, base);
    for offset in [0xc, 0x10, 0x14, 0x18, 0x1c, 0x20] {
        g.w32(DEBUG + offset, source);
    }
}

fn copy_identity<G: Guest>(g: &mut G, source: u32) {
    #[cfg(not(exif_control_identity_not_copied))]
    {
        g.copy_memory(source, IDENTITY, 16);
        g.copy_memory(source + 16, IDENTITY + 16, 32);
        g.copy_memory(source + 48, IDENTITY + 48, 64);
    }
    #[cfg(exif_control_identity_not_copied)]
    let _ = (g, source);
}

/// `0x011cf990`: a directory built from the source block through the tag table and filter.
pub fn write_filtered_directory<G: Guest>(g: &mut G, kind: u32, source: u32, destination: u32, base: u32) -> u32 {
    g.frame(|g, local| {
        let entry = local;
        let layout = local + 12;
        let count = local + 16;
        let zero = local + 20;
        g.w16(count, 0);
        trace_directory(g, kind, source, destination, base);
        let table = select_table(kind & 0xff);
        let rules = table_count(kind & 0xff);
        if table == 0 {
            return 0;
        }
        if kind == kind::PRIMARY {
            copy_identity(g, source);
        }
        let written = |g: &mut G, rule: u32| rule_selected(g, rule) && should_write_tag(g, kind, g.r16(rule) as u32, source) != 0;
        for index in 0..rules {
            if written(g, table + index * RULE_BYTES) {
                let counted = g.r16(count).wrapping_add(1);
                g.w16(count, counted);
            }
        }
        let mut cursor = destination.wrapping_add(g.r16(count) as u32 * ENTRY_BYTES + 6);
        g.copy_memory(destination, count, 2);
        let mut slot = destination.wrapping_add(2);
        let mut size: u32 = 2;
        for index in 0..rules {
            let rule = table + index * RULE_BYTES;
            if !written(g, rule) {
                continue;
            }
            let tag = g.r16(rule);
            g.w16(entry, tag);
            let value_type = g.r16(rule + 4);
            g.w16(entry + 2, value_type);
            let elements = capacity(g, kind, rule, source);
            g.w32(entry + 4, elements);
            let callback = g.r32(rule + 8);
            if callback == 0 {
                let offset = g.r32(rule + 0x1c);
                size = size.wrapping_add(write_value_entry(g, rule, source.wrapping_add(offset), slot, elements, base, &mut cursor));
            } else {
                let before = cursor;
                write_callback_entry(g, callback, source, slot, entry, layout, base, &mut cursor);
                size = size.wrapping_add(cursor.wrapping_sub(before)).wrapping_add(ENTRY_BYTES);
            }
            slot = slot.wrapping_add(ENTRY_BYTES);
        }
        match kind {
            kind::PRIMARY => {
                g.w32(WRITER + 0x28, slot);
                g.w32(source + 0x154, slot);
                g.w32(source + 0x158, 0);
                g.copy_memory(slot, source + 0x158, 4);
            }
            kind::MPF_INDEX => g.w32(WRITER + 0x40, slot),
            _ => {
                g.w32(zero, 0);
                g.copy_memory(slot, zero, 4);
            }
        }
        size.wrapping_add(4)
    })
}

/// `0x011c2154`: saves (`direction` 1) or loads (`direction` 0) a bank of the saved parser.
pub fn transfer_saved_parser<G: Guest>(g: &mut G, parser: u32, bank: u32, direction: u32) -> i32 {
    #[cfg(exif_control_saved_parser_bank_fixed)]
    let bank = bank & 0;
    let (state, file) = match bank {
        0 => (SAVED_STATE[0], SAVED_FILE[0]),
        1 => (SAVED_STATE[1], SAVED_FILE[1]),
        _ => return -1,
    };
    if parser == 0 {
        return -1;
    }
    match direction {
        0 => {
            g.fill(parser, 0, 28);
            let value = g.r32(file);
            g.w32(parser + 4, value);
            let value = g.r32(state);
            g.w32(parser + 0x14, value);
        }
        1 => {
            let value = g.r32(parser + 4);
            g.w32(file, value);
            let value = g.r32(parser + 0x14);
            g.w32(state, value);
        }
        _ => {}
    }
    0
}

/// `0x011c35f8`.
pub fn saved_directory_slots<G: Guest>(g: &G, kind: u32, parser: u32) -> u32 {
    let state = g.r32(parser + 0x14);
    match kind {
        0 => state + 0xffc,
        1 => state + 0x1c4c,
        2 => state + 0x250c,
        5 => state + 0xa444,
        6 => state + 0xa6f8,
        _ => 0,
    }
}

/// `0x011c3678`.
pub fn saved_directory_values<G: Guest>(g: &G, kind: u32, parser: u32) -> u32 {
    let state = g.r32(parser + 0x14);
    match kind {
        0 => state + 0x10d4,
        1 => state + 0x1e5c,
        2 => state + 0x2c44,
        5 => state + 0xa51c,
        6 => state + 0xa758,
        _ => 0,
    }
}

fn mask_at<G: Guest>(g: &G, table: u32) -> u32 {
    g.r32(table + 4 * mode(g) as u32)
}

fn covers(mask: u32, required: u32) -> bool {
    mask & required == required
}

/// `0x011cfe54`: a directory replayed from the saved parse, with generated entries where the
/// rule masks ask for them.
pub fn write_saved_directory<G: Guest>(g: &mut G, source: u32, destination: u32, kind: u32, pass: u32) -> u32 {
    g.frame(|g, local| {
        let parser = local;
        let count = local + 28;
        let entry = local + 32;
        let layout = local + 44;
        let zero = local + 48;
        transfer_saved_parser(g, parser, 0, 0);
        let first_pass = pass == 0;
        let maker_first = kind == kind::MAKER && pass == 0;
        let mask_of = |g: &G, rule: u32| g.r32(rule + if first_pass { 0x14 } else { 0x18 });
        let (table, rules, slots) = if kind <= 6 && (1u32 << kind) & 0x67 != 0 {
            (select_table(kind), table_count(kind), saved_directory_slots(g, kind, parser))
        } else {
            (0, 0, 0)
        };
        let mut entries: u32 = 0;
        if slots != 0 && table != 0 && rules != 0 {
            for index in 0..rules {
                let rule = table + index * RULE_BYTES;
                if maker_first && saved_tag_available(g, g.r16(rule) as u32, source) == 0 {
                    continue;
                }
                let mask = mask_of(g, rule);
                let saved = g.r32(slots + index * SAVED_SLOT_BYTES) != 0;
                if (saved && (covers(mask, mask_at(g, SAVED_MASK_A)) || covers(mask, mask_at(g, SAVED_MASK_B)))) || covers(mask, mask_at(g, SAVED_MASK_ALWAYS)) {
                    entries = (entries + 1) & 0xffff;
                }
            }
        }
        g.w16(count, entries as u16);
        g.copy_memory(destination, count, 2);
        let first_slot = destination.wrapping_add(2);
        let mut cursor = destination.wrapping_add(6).wrapping_add(entries * ENTRY_BYTES);
        let table = select_table(kind);
        let rules = table_count(kind);
        let slots = saved_directory_slots(g, kind, parser);
        let values = saved_directory_values(g, kind, parser);
        let base = if kind == kind::PRIMARY {
            copy_identity(g, source);
            g.r32(WRITER)
        } else if kind == kind::MAKER {
            g.r32(WRITER + 0x1c)
        } else {
            g.r32(WRITER)
        };
        trace_directory(g, kind, source, first_slot, base);
        let mut slot = first_slot;
        for index in 0..rules {
            let rule = table + index * RULE_BYTES;
            let saved = slots.wrapping_add(index * SAVED_SLOT_BYTES);
            if maker_first && saved_tag_available(g, g.r16(rule) as u32, source) == 0 {
                continue;
            }
            let mask = mask_of(g, rule);
            let from_saved = covers(mask, mask_at(g, SAVED_MASK_A));
            if !from_saved && !covers(mask, mask_at(g, SAVED_MASK_B)) {
                continue;
            }
            let generate = if g.r32(saved) == 0 {
                if !covers(mask, mask_at(g, SAVED_MASK_ALWAYS)) {
                    continue;
                }
                true
            } else {
                !from_saved
            };
            if generate {
                let tag = g.r16(rule);
                g.w16(entry, tag);
                let value_type = g.r16(rule + 4);
                g.w16(entry + 2, value_type);
                let elements = capacity(g, kind, rule, source);
                g.w32(entry + 4, elements);
                let callback = g.r32(rule + 8);
                if callback == 0 {
                    let offset = g.r32(rule + 0x1c);
                    write_value_entry(g, rule, source.wrapping_add(offset), slot, elements, base, &mut cursor);
                } else {
                    write_callback_entry(g, callback, source, slot, entry, layout, base, &mut cursor);
                }
            } else {
                let tag = g.r16(rule);
                g.w16(entry, tag);
                let saved_type = g.r16(saved + 8);
                g.w16(entry + 2, saved_type);
                let elements = g.r32(saved);
                g.w32(entry + 4, elements);
                let value = values.wrapping_add(g.r32(saved + 4));
                if g.r32(entry + 4).wrapping_mul(type_size(g, saved_type as u32)) > 4 {
                    g.w32(entry + 8, cursor.wrapping_sub(base));
                    let length = g.r32(entry + 4).wrapping_mul(type_size(g, g.r16(entry + 2) as u32));
                    g.copy_memory(cursor, value, length);
                    #[cfg(not(exif_control_saved_values_unpadded))]
                    let length = if length & 1 != 0 {
                        g.w8(cursor.wrapping_add(length), 0);
                        length + 1
                    } else {
                        length
                    };
                    cursor = cursor.wrapping_add(length);
                } else {
                    g.copy_memory(entry + 8, value, 4);
                }
                g.copy_memory(slot, entry, ENTRY_BYTES);
            }
            slot = slot.wrapping_add(ENTRY_BYTES);
        }
        if kind == kind::PRIMARY {
            g.w32(WRITER + 0x28, slot);
        }
        g.w32(zero, 0);
        g.copy_memory(slot, zero, 4);
        cursor.wrapping_add(2).wrapping_sub(first_slot)
    })
}

fn in_range(tag: u32, low: u32, span: u32) -> bool {
    (tag.wrapping_sub(low) & 0xffff) <= span
}

/// The capture-block checks `0x011cda48` and the maker branch of `0x011cdea0` share, from the
/// `0x1e8` capture byte to the face fields; `None` lets later checks decide.
fn maker_capture_tags<G: Guest>(g: &G, tag: u32, s: u32, kind_filter: bool) -> Option<u32> {
    let half = |offset: u32| g.r16(s + offset) as u32;
    let captured = g.r8(s + 0x1e8) != 0;
    let mut skip_low = false;
    if !captured {
        if tag == 0x140c || in_range(tag, 0x1408, 2) {
            return Some(0);
        }
        if tag == 0x4200 || tag == 0x4100 {
            return Some(0);
        }
        if !kind_filter && (tag == 0x4002 || tag == 0x4003 || tag == 0x4004 || tag == 0x4005 || tag == 0x4006 || tag == 0x4007) {
            return Some(0);
        }
    } else if half(0x222) != 0 {
        skip_low = true;
    }
    if !skip_low {
        if in_range(tag, 0x4000, 1) {
            return Some(0);
        }
        if kind_filter && !captured && in_range(tag, 0x4002, 5) {
            return Some(0);
        }
    }
    let face_count = match tag {
        0x4002..=0x4007 => Some(0xe044 + 2 * (tag - 0x4002)),
        _ => None,
    };
    if face_count.is_some_and(|offset| half(offset) == 0) {
        return Some(0);
    }
    let records = half(0x20a);
    if captured && records != 0xffff {
        if records == 0 && in_range(tag, 0x4009, 2) {
            return Some(0);
        }
    } else {
        if tag == 0x4008 {
            return Some(0);
        }
        if in_range(tag, 0x4009, 2) {
            return Some(0);
        }
    }
    if !captured || half(0x222) == 0 {
        if tag == 0x4180 || in_range(tag, 0x4101, 6) || tag == 0x4181 {
            return Some(0);
        }
    }
    if tag == 0x4106 && half(0xe03e) == 0 {
        return Some(0);
    }
    if !captured || half(0x58a) == 0 {
        if tag == 0x4280 || in_range(tag, 0x4201, 7) || (0x4281..=0x4284).contains(&tag) {
            return Some(0);
        }
    }
    if tag == 0x4206 && half(0xe042) == 0 {
        return Some(0);
    }
    if g.r8(s + 0x1f2) == 0 && (tag == 0x4282 || tag == 0x140c) {
        return Some(0);
    }
    if tag == 0x4282 && g.r32(s + 0x1d38) != 1 {
        return Some(0);
    }
    if half(0x1f0) & 1 == 0 && (tag == 0x4282 || in_range(tag, 0x4000, 11) || tag == 0x4283 || tag == 0x4284) {
        return Some(0);
    }
    None
}

/// `0x011cda48`: whether a maker tag of a saved replay has its capture data.
pub fn saved_tag_available<G: Guest>(g: &G, tag: u32, s: u32) -> u32 {
    if let Some(result) = maker_capture_tags(g, tag, s, false) {
        return result;
    }
    if tag == 0x1420 && g.r16(s + 0xf8) != 1 {
        return 0;
    }
    if tag == 0x1423 && g.r16(s + 0x106) == 0xffff {
        return 0;
    }
    trailing_maker_tags(g, tag, s)
}

/// The tail `0x011cda48` and `0x011cdea0` share: the `0xb211..0xb214` flags and `0x1437`.
fn trailing_maker_tags<G: Guest>(g: &G, tag: u32, s: u32) -> u32 {
    let flag = |offset: u32| (g.r32(s + offset) == 1) as u32;
    match tag {
        0xb211 => flag(0xe870),
        0xb212 => flag(0xe874),
        0xb213 => flag(0xe878),
        0xb214 => flag(0xe87c),
        0x1437 => flag(0xe8e8),
        _ => 1,
    }
}

/// `0x011cdea0`: whether a rule's tag goes into the directory for this source block.
pub fn should_write_tag<G: Guest>(g: &G, kind: u32, tag: u32, s: u32) -> u32 {
    let half = |offset: u32| g.r16(s + offset) as u32;
    let word = |offset: u32| g.r32(s + offset);
    let byte = |offset: u32| g.r8(s + offset) as i8;
    match kind {
        kind::PRIMARY => match tag {
            0x13e => (word(0xf4) != 0) as u32,
            0x13f => (word(0x104) != 0) as u32,
            0x211 => (word(0x134) != 0) as u32,
            _ => 1,
        },
        kind::EXIF => match tag {
            0xa40a => (half(692) != 0xffff) as u32,
            0xa500 => (word(0x33c) != 0) as u32,
            _ => 1,
        },
        kind::MAKER => should_write_maker_tag(g, tag, s),
        kind::IMAGE_RECORD => {
            if word(0) != 1 && (tag == 0x1000 || tag == 0x100 || tag == 0x1020 || tag == 0x1001 || tag == 0x1021) {
                return 0;
            }
            if word(0x38) == 1 {
                let value = half(60);
                if value != 0 && value != 0xffff {
                    return 1;
                }
            }
            if tag == 0x1110 || tag == 0x1100 {
                return 0;
            }
            !(tag == 0x1112 || tag == 0x1111) as u32
        }
        kind::GPS => {
            let text = match tag {
                8 => Some(96),
                9 => Some(99),
                10 => Some(101),
                _ => None,
            };
            if text.is_some_and(|offset| byte(offset) == 0) {
                return 0;
            }
            if tag == 11 && word(0x68) == 0 {
                return (word(0x6c) != 0) as u32;
            }
            if tag == 16 {
                return (byte(124) != 0) as u32;
            }
            if tag != 17 || word(0x80) != 1 {
                return 1;
            }
            (word(0x84) != 0) as u32
        }
        8 => {
            if tag != 0xb000 {
                return 1;
            }
            #[cfg(exif_control_mpf_version_unfiltered)]
            return 1;
            #[cfg(not(exif_control_mpf_version_unfiltered))]
            return (word(0x1c) != 0) as u32;
        }
        _ => 1,
    }
}

fn should_write_maker_tag<G: Guest>(g: &G, tag: u32, s: u32) -> u32 {
    let half = |offset: u32| g.r16(s + offset) as u32;
    let word = |offset: u32| g.r32(s + offset);
    let missing = match tag {
        0x1210 => half(64) == 0xffff || word(0xd8) == 0,
        0x1004 => half(66) == 0xffff,
        0x1003 => half(64) == 0xffff,
        0x1001 => half(60) == 0xffff,
        0x1005 => half(68) == 0,
        0x100a => word(0x48) == 0,
        0x1401 => word(0xec) == 0,
        0x1402 => half(242) == 0xffff,
        0x1403 => half(244) == 0xffff,
        0x1431 => word(0x134) == 0xffff_ffff,
        _ => false,
    };
    if missing {
        return 0;
    }
    if let Some(result) = maker_capture_tags(g, tag, s, true) {
        return result;
    }
    let missing = match tag {
        0x1201 => word(0xd4) == 0xffff_ffff,
        0x1026 => half(120) == 0xffff,
        0x1033 => half(162) == 0xffff,
        0x1034 => half(164) == 0xffff,
        0x1035 => half(166) == 0xffff,
        0x1040 => word(0xa8) == 0xffff,
        0x1041 => word(0xac) == 0xffff,
        0x1042 => word(0xb0) == 0,
        0x1043 => word(0xb8) == 0,
        0x140b => half(246) == 0xffff,
        0x1422 => word(0xfc) == 0,
        0x1424 => half(264) == 0xffff,
        0x100b => half(84) == 0xffff,
        0x100e => word(0x5c) != 1,
        0x100d => g.r8(s + 0x58) == 0,
        0x1425 | 0x1427 | 0x1428 => word(0x10c) != 1,
        0x1150..=0x1152 => word(0xc4) == 0xffff_ffff,
        0x1027 => word(0xc4) != 2,
        0x1153 => half(204) == 0xffff,
        0x1154 => half(206) == 0xffff,
        0x1028 => half(146) == 0xffff,
        0x1029 => half(146).wrapping_sub(1) & 0xffff > 0xfffd,
        0x102a => half(146).wrapping_sub(1) & 0xffff > 0xfffd || word(0x94) == 0,
        0x1044 => word(0xe8a4) == 0xffff_ffff,
        0x1045 => word(0xe8f0) == 0xffff_ffff,
        0x1440 => word(0xe8f4) == 0xffff_ffff,
        0x1441 => word(0xe8f8) == 0x8000_0000,
        0x1436 => half(0xe8a8) == 0xffff,
        0x4500 | 0x4510 | 0x4520..=0x4537 => g.r8(s + 0xe050) == b'F',
        0x1432 => half(0xe894) == 0xffff,
        0x1433..=0x1435 => half(0xe894).wrapping_sub(1) & 0xffff > 0xfffd,
        0x1020 => half(110) == 0xffff,
        _ => false,
    };
    if missing {
        return 0;
    }
    trailing_maker_tags(g, tag, s)
}

/// `0x011cf448`: a six-byte PrintIM record, tag then value.
fn write_short_record<G: Guest>(g: &mut G, destination: u32, tag: u16, value: u32) -> u32 {
    g.frame(|g, local| {
        let word = local;
        let record = local + 4;
        g.w32(word, value);
        g.w16(record, tag);
        copy_value(g, record + 2, word, 4);
        destination + copy_value(g, destination, record, 6)
    })
}

/// `0x011cf748`: the PrintIM block from its source fields.
fn write_printim<G: Guest>(g: &mut G, source: u32, destination: u32) -> u32 {
    g.frame(|g, local| {
        let header = local;
        let offset = local + 16;
        g.copy_string(header, PRINTIM);
        g.copy_string(header + 8, PRINTIM_VERSION);
        g.w16(header + 12, 0);
        g.w16(header + 14, 3);
        let first = destination + copy_value(g, destination, header, 16);
        let record = write_short_record(g, first, 2, g.r8(source) as u32);
        let has_block = g.r32(source + 4) != 0;
        let (after, records) = if has_block { (write_short_record(g, record, 3, 0), 28) } else { (record, 22) };
        let end = write_short_record(g, after, 0x101, g.r8(source + 0x4c) as u32);
        if !has_block {
            return records + 6;
        }
        g.w32(offset, end.wrapping_sub(destination));
        copy_value(g, record + 2, offset, 4);
        copy_value(g, end, source + 4, 72);
        records + 78
    })
}

/// Rule callbacks the retained tag tables name, `(source, slot, cursor, layout, entry)`.
pub mod callback {
    use super::*;

    fn set_value<G: Guest>(g: &mut G, entry: u32, value: u32) {
        g.w32(entry + 8, value);
    }

    /// `0x011cecb8`: IFD1 JPEGInterchangeFormat.
    pub fn thumbnail_offset<G: Guest>(g: &mut G, slot: u32, entry: u32) {
        g.w32(WRITER + 0x24, slot + 8);
        set_value(g, entry, 0);
    }

    /// `0x011cece8`: IFD1 JPEGInterchangeFormatLength.
    pub fn thumbnail_length<G: Guest>(g: &mut G, slot: u32, entry: u32) {
        g.w32(WRITER + 0x20, slot + 8);
        set_value(g, entry, 0);
    }

    /// `0x011ced18`: ExifIFD pointer.
    pub fn exif_pointer<G: Guest>(g: &mut G, slot: u32, entry: u32) {
        g.w32(WRITER + 0x4, slot + 8);
        set_value(g, entry, 0);
    }

    /// `0x011ced48`: GPS IFD pointer; arms the GPS directory.
    pub fn gps_pointer<G: Guest>(g: &mut G, slot: u32, entry: u32) {
        g.w32(WRITER + 0xc, slot + 8);
        #[cfg(not(exif_control_gps_pointer_unarmed))]
        g.w16(WRITER + 70, 1);
        set_value(g, entry, 0);
    }

    /// `0x011ced80`.
    pub fn one<G: Guest>(g: &mut G, entry: u32) {
        set_value(g, entry, 1);
    }

    /// `0x011ceda0`: MakerNote; arms the maker directory.
    pub fn makernote<G: Guest>(g: &mut G, slot: u32, entry: u32) {
        g.w32(WRITER + 0x14, slot + 4);
        g.w32(WRITER + 0x18, slot + 8);
        g.w16(WRITER + 68, 1);
        set_value(g, entry, 0);
    }

    /// `0x011cede0`: interoperability IFD pointer.
    pub fn interoperability_pointer<G: Guest>(g: &mut G, slot: u32, entry: u32) {
        g.w32(WRITER + 0x8, slot + 8);
        set_value(g, entry, 0);
    }

    /// `0x011cee10`.
    pub fn packed_halves<G: Guest>(g: &mut G, source: u32, entry: u32) {
        let value = if directory_format(g) == 1 { 0 } else { ((g.r16(source + 118) as u32) << 16).wrapping_add(g.r16(source + 116) as u32) };
        set_value(g, entry, value);
    }

    /// `0x011cee4c`.
    pub fn source_word<G: Guest>(g: &mut G, source: u32, entry: u32) {
        let value = if directory_format(g) <= 1 { g.r32(source + 0x134) } else { 0 };
        set_value(g, entry, value);
    }

    fn element_bytes<G: Guest>(g: &G, layout: u32, entry: u32) -> u32 {
        g.r32(entry + 4).wrapping_mul(g.r16(layout) as u32)
    }

    /// `0x011cef60`.
    pub fn staged_block<G: Guest>(g: &mut G, source: u32, cursor: u32, layout: u32, entry: u32) {
        g.frame(|g, local| {
            g.copy_memory(local, source + 8, 32);
            let length = element_bytes(g, layout, entry);
            g.copy_memory(cursor, local, length);
        })
    }

    /// `0x011cefa8` (`offset` 256) and `0x011ceff0` (`offset` 122).
    pub fn source_bytes<G: Guest>(g: &mut G, source: u32, cursor: u32, layout: u32, entry: u32, offset: u32) {
        let length = element_bytes(g, layout, entry);
        if length <= 4 {
            set_value(g, entry, 0);
            let length = element_bytes(g, layout, entry);
            g.copy_memory(entry + 8, source + offset, length);
        } else {
            g.copy_memory(cursor, source + offset, length);
        }
    }

    /// `0x011cf038`.
    pub fn default_text<G: Guest>(g: &mut G, source: u32, cursor: u32, layout: u32, entry: u32) {
        if g.r8(source + 4) as i8 != 0 {
            g.copy_memory(cursor, source + 4, 48);
            return;
        }
        g.frame(|g, local| {
            g.copy_memory(local, DEFAULT_TEXT, 4);
            g.copy_memory(local + 4, DEFAULT_TEXT + 4, 12);
            g.w8(local + 16, b' ');
            g.copy_memory(local + 17, DEFAULT_TEXT + 16, 30);
            g.w8(local + 47, 0);
            let length = element_bytes(g, layout, entry);
            g.copy_memory(cursor, local, length);
        })
    }

    /// `0x011cf3cc`: Copyright from `source+178`.
    pub fn copyright<G: Guest>(g: &mut G, source: u32, cursor: u32, layout: u32, entry: u32) {
        let text = source + 178;
        let count = g.string_length(text) + 1;
        g.w32(entry + 4, count);
        if count <= 4 {
            set_value(g, entry, 0);
            g.copy_string_bounded(entry + 8, text, count);
            g.w16(layout + 2, 0);
            return;
        }
        let length = (g.r16(layout) as u32).wrapping_mul(count);
        g.copy_string_bounded(cursor, text, length);
        let count = g.r32(entry + 4);
        if count & 1 != 0 {
            g.w8(cursor + count, 0);
        }
        g.w16(layout + 2, 1);
    }

    /// `0x011cf82c`: PrintIM.
    pub fn printim<G: Guest>(g: &mut G, source: u32, slot: u32, cursor: u32) -> u32 {
        g.w32(WRITER + 0x10, slot + 8);
        write_printim(g, source + 348, cursor)
    }

    /// `0x011cfd58`: the image-record directories of the maker note's frame records.
    pub fn image_records<G: Guest>(g: &mut G, source: u32, cursor: u32, entry: u32) {
        g.frame(|g, local| {
            let offset = local;
            let size = local + 4;
            let mut total = (g.r16(source + 0x58a) as u32) << 3;
            if g.r16(source + 0x58a) != 0 {
                let mut at = cursor.wrapping_add(total);
                let mut index: u32 = 0;
                loop {
                    let record = source.wrapping_add(832 * index);
                    #[cfg(not(exif_control_image_records_unskipped))]
                    let present = g.r32(record + 0x1d3c) == 1 || g.r32(record + 0x1d74) == 1;
                    #[cfg(exif_control_image_records_unskipped)]
                    let present = true;
                    if present {
                        let written = write_filtered_directory(g, kind::IMAGE_RECORD, record + 0x1d3c, at, cursor);
                        g.w32(size, written);
                        g.w32(offset, at.wrapping_sub(cursor));
                        total = total.wrapping_add(written);
                        at = at.wrapping_add(written);
                    } else {
                        g.w32(offset, 0);
                        g.w32(size, 0);
                    }
                    let pair = cursor + index * 8;
                    g.copy_memory(pair, offset, 4);
                    g.copy_memory(pair + 4, size, 4);
                    index = (index + 1) & 0xffff;
                    if g.r16(source + 0x58a) as u32 <= index {
                        break;
                    }
                }
            }
            g.w32(entry + 4, total);
        })
    }
}
