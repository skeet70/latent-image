use super::shared::{Address32, ByteOffset};
use super::font_fdl::{BackendMetric, FontBitmapFormat, FontEngine, GlyphCode, GlyphDraw};
use super::font_script_store::{GuestMetrics, GuestStore};
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct ScriptContext(Address32);

impl ScriptContext {
    fn field(self, offset: u32) -> Address32 { self.0.offset(ByteOffset(offset)) }
    unsafe fn word(self, offset: u32) -> u32 { unsafe { self.field(offset).word() } }
    unsafe fn half(self, offset: u32) -> u16 { unsafe { self.field(offset).half() } }
    unsafe fn put_word(self, offset: u32, value: u32) { unsafe { self.field(offset).write_word(value) }; }
    unsafe fn put_half(self, offset: u32, value: u16) { unsafe { self.field(offset).write_half(value) }; }
    unsafe fn next(self) -> u16 {
        let cursor = unsafe { self.word(0x74) };
        let stream = Address32(unsafe { self.word(0x70) });
        let value = unsafe { little_half(stream.offset(ByteOffset(cursor))) };
        unsafe { self.put_word(0x74, cursor.wrapping_add(2)) };
        value
    }
}

unsafe fn little_half(address: Address32) -> u16 {
    unsafe { address.byte() as u16 | (address.offset(ByteOffset(1)).byte() as u16) << 8 }
}
unsafe fn little_word(address: Address32) -> u32 {
    let mut value = 0;
    for byte in 0..4 { value |= (unsafe { address.offset(ByteOffset(byte)).byte() } as u32) << (byte * 8); }
    value
}

pub use crate::develop::font_core::shaping::{ScriptMode, UnicodeCategory, UnicodeClass};

use crate::develop::font_core::shaping::{self, CharacterCode, CharacterIndex, CharacterRange, DirectionHint, JoiningCategory, JoiningClass, JoiningState, PackedCount, ScriptRun, ScriptTables};

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_glyph_cell_width(code: u16) -> i32 {
    shaping::glyph_cell_width(CharacterCode(code))
}

fn classify(code: u16) -> UnicodeClass {
    shaping::classify(CharacterCode(code))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_classify_unicode(output: *mut UnicodeClass, code: u32) -> *mut UnicodeClass {
    unsafe { write_volatile(output, classify(code as u16)) };
    output
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_private_character_class(output: *mut UnicodeClass, code: u16) -> *mut UnicodeClass {
    unsafe { write_volatile(output, shaping::private_character_class(CharacterCode(code))) };
    output
}

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_pack_count(count: u16, groups: u16) -> u16 {
    shaping::pack_count(count, groups).0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_unpack_count(packed: u16, count: *mut u16, groups: *mut u16) {
    let (characters, glyphs) = shaping::unpack_count(PackedCount(packed));
    unsafe { write_volatile(count, characters); write_volatile(groups, glyphs); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_next_u16(engine: u32) -> u16 { unsafe { ScriptContext(Address32(engine)).next() } }

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_flags_for_mode(mode: i32) -> u16 {
    match mode { 2 | 6 => 0x200f, _ => 0x200e }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_detect_mode(text: *const u16, length: i32, mode_out: *mut i32, language_out: *mut i32) -> u32 {
    let length = length.max(0) as usize;
    let text = unsafe { core::slice::from_raw_parts(text, length) };
    let (mode, language, direction) = shaping::detect_mode(text)
        .map_or((0, 0, 0), |mode| (mode as i32, mode.language().0 as i32, mode.direction().0 as u32));
    if !mode_out.is_null() { unsafe { write_volatile(mode_out, mode) }; }
    if !language_out.is_null() { unsafe { write_volatile(language_out, language) }; }
    direction
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_detect_script_run(text: *const u16, length: u16, start: u16, end_out: *mut u16, base_direction: u32, mode_out: *mut i32, language_out: *mut i32, direction_out: *mut u16) -> i32 {
    let text = unsafe { core::slice::from_raw_parts(text, length as usize) };
    let detected = shaping::detect_script_run(text, shaping::CharacterIndex(start), DirectionHint(base_direction));
    unsafe {
        write_volatile(end_out, detected.range.end.0);
        write_volatile(mode_out, detected.identity.script.0 as i32);
        write_volatile(language_out, detected.identity.language.0 as i32);
        write_volatile(direction_out, detected.direction.0);
    }
    (detected.range.end.0 as i32).wrapping_sub(i32::from(start))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_select_row(engine: u32, script: u32, language: u32, index_out: *mut u16) -> i32 {
    let context = ScriptContext(Address32(engine));
    let stream = Address32(unsafe { context.word(0x70) });
    let count = unsafe { context.half(0x502) };
    if stream.0 == 0 || count == 0 { return -1; }
    let directory = Address32(unsafe { context.word(0x504) });
    for index in 0..count {
        let offset = unsafe { directory.offset(ByteOffset((index as u32).wrapping_mul(4))).word() };
        unsafe { context.put_word(0x74, offset) };
        let row = stream.offset(ByteOffset(offset));
        let row_script = unsafe { little_half(row) };
        unsafe { context.put_word(0x74, offset.wrapping_add(2)) };
        if row_script as u32 != script { continue; }
        let row_language = unsafe { little_half(row.offset(ByteOffset(2))) };
        unsafe { context.put_word(0x74, offset.wrapping_add(4)) };
        if row_language as u32 != language { continue; }
        unsafe {
            write_volatile(index_out, index);
            context.put_word(0x74, offset);
            let selected_script = little_half(row);
            context.put_word(0x74, offset.wrapping_add(2));
            context.put_half(0x508, selected_script);
            let selected_language = little_half(row.offset(ByteOffset(2)));
            context.put_word(0x74, offset.wrapping_add(4));
            context.put_half(0x50a, selected_language);
        }
        let entries = unsafe { little_half(row.offset(ByteOffset(4))) };
        unsafe { context.put_word(0x74, offset.wrapping_add(6)); context.put_half(0x50c, entries); }
        for entry in 0..entries {
            let value = unsafe { little_word(row.offset(ByteOffset(6u32.wrapping_add((entry as u32).wrapping_mul(4))))) };
            unsafe { context.put_word(0x74, offset.wrapping_add(10).wrapping_add((entry as u32).wrapping_mul(4))); context.put_word(0x478 + entry as u32 * 4, value); }
        }
        unsafe { context.put_word(0x510, engine.wrapping_add(0x478)) };
        return 0;
    }
    -1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_dispatch_script(engine: u32, script: u32, language: u32, phase: u32, run: u32) -> i32 {
    match (script, language, phase) {
        (_, _, 2..=u32::MAX) => 0,
        (2, 5, 0) => unsafe { xm1_pipeline_image_font_shape_arabic(engine, run) },
        (2, 5, 1) => unsafe { xm1_pipeline_image_font_position_arabic(engine, run) },
        (3, 6, 0) => unsafe { xm1_pipeline_image_font_shape_thai(engine, run) },
        (3, 6, 1) => unsafe { xm1_pipeline_image_font_position_thai(engine, run) },
        (6, 9, 0) => unsafe { xm1_pipeline_image_font_shape_hebrew(engine, run) },
        (6, 9, 1) => unsafe { xm1_pipeline_image_font_position_hebrew(engine, run) },
        (_, _, 0) => unsafe { xm1_pipeline_image_font_shape_basic(engine, run) },
        _ => unsafe { xm1_pipeline_image_font_position_forward(engine, run) },
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct RunAddress(Address32);

impl RunAddress {
    unsafe fn start(self) -> u16 { unsafe { self.0.half() } }
    unsafe fn end(self) -> u16 { unsafe { self.0.offset(ByteOffset(2)).half() } }
    unsafe fn word(self, offset: u32) -> u32 { unsafe { self.0.offset(ByteOffset(offset)).word() } }
    unsafe fn finish(self, x: u32) {
        unsafe { self.0.offset(ByteOffset(28)).write_word(x); self.0.offset(ByteOffset(32)).write_word(0); }
    }
}

unsafe fn positioned_context(context: ScriptContext) -> ScriptContext {
    ScriptContext(Address32(unsafe { context.word(0x53c) }))
}

unsafe fn initial_pen(context: ScriptContext, run: RunAddress) -> u32 {
    let output = unsafe { positioned_context(context) };
    let x = unsafe { run.word(20) };
    if unsafe { output.half(0xc20) } == 0x200f { x.wrapping_sub(unsafe { run.word(12) }) } else { x }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_compose_private_run(engine: u32) {
    let mut store = unsafe { GuestStore::new(Address32(engine)) };
    let mut metrics = unsafe { GuestMetrics::new(Address32(engine)) };
    let _ = shaping::measure_characters(&mut store, &mut metrics, ScriptMode::Arabic);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_store_main_run(engine: u32) {
    let mut store = unsafe { GuestStore::new(Address32(engine)) };
    let mut metrics = unsafe { GuestMetrics::new(Address32(engine)) };
    let _ = shaping::store_main_run(&mut store, &mut metrics);
}

unsafe fn render_glyph_span(context: ScriptContext, run: RunAddress) -> i32 {
    let first = unsafe { context.half(0x616 + (run.start() as u32) * 16) } as u32;
    let last = unsafe { context.half(0x606 + (run.end() as u32) * 16) } as u32;
    if first > last { return 0; }
    if unsafe { context.word(0xff4) } == 1 {
        for glyph in first..=last {
            let output = unsafe { positioned_context(context) };
            unsafe { context.put_word(0xfe8, output.word(0xc30)) };
            let code = unsafe { output.half(4 + glyph * 2) };
            let mut handle = 0u32;
            let result = unsafe { FontEngine(context.0.0).append_outline(GlyphCode(code as u32), output.field(0xc2c).0 as *mut u32, context.field(0xfe8).0 as *mut u32, &mut handle) };
            let output = unsafe { positioned_context(context) };
            unsafe { output.put_word(0xc30, context.word(0xfe8)) };
            if result != 0 { return result; }
            unsafe { output.put_word(0xc34 + glyph * 4, handle) };
        }
    }
    let mode = unsafe { context.word(0xff4) };
    if mode.wrapping_sub(3) < 3 || mode == 2 || mode.wrapping_sub(6) < 2 {
        for glyph in first..=last {
            let output = unsafe { positioned_context(context) };
            let code = unsafe { output.half(4 + glyph * 2) };
            let record = 0x68 + glyph * 60;
            unsafe { FontEngine(context.0.0).draw(GlyphDraw {
                glyph: GlyphCode(code as u32), width: output.word(0xd00) as i32, height: output.word(0xd04) as i32,
                x: output.word(record + 16) as i32, y: output.word(record + 20) as i32,
                stride: output.word(0xd08) as i32, pixels: Address32(output.word(0xd10)), format: FontBitmapFormat(context.word(0x106c)),
            }) };
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_shape_basic(engine: u32, run: u32) -> i32 {
    let context = ScriptContext(Address32(engine));
    let phase = unsafe { context.word(0x538) };
    if phase & 3 != 2 {
        let mut store = unsafe { GuestStore::new(Address32(engine)) };
        let mut metrics = unsafe { GuestMetrics::new(Address32(engine)) };
        if shaping::shape_basic(&mut store, &mut metrics).is_err() { return 0; }
        if phase & 2 == 0 { return 0; }
    }
    unsafe { render_glyph_span(context, RunAddress(Address32(run))) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_position_forward(engine: u32, run: u32) -> i32 {
    let mut store = unsafe { GuestStore::new(Address32(engine)) };
    let mut metrics = unsafe { GuestMetrics::new(Address32(engine)) };
    let run = unsafe { &mut *(run as *mut ScriptRun) };
    let _ = shaping::position_forward(&mut store, &mut metrics, run);
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_position_arabic(engine: u32, run: u32) -> i32 {
    unsafe { xm1_pipeline_image_font_position_metric_span(engine, run) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_position_metric_span(engine: u32, run: u32) -> i32 {
    let mut store = unsafe { GuestStore::new(Address32(engine)) };
    let mut metrics = unsafe { GuestMetrics::new(Address32(engine)) };
    let run = unsafe { &mut *(run as *mut ScriptRun) };
    let _ = shaping::position_reversed(&mut store, &mut metrics, run, ScriptMode::Arabic);
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_position_hebrew(engine: u32, run: u32) -> i32 {
    let mut store = unsafe { GuestStore::new(Address32(engine)) };
    let mut metrics = unsafe { GuestMetrics::new(Address32(engine)) };
    let run = unsafe { &mut *(run as *mut ScriptRun) };
    let _ = shaping::position_reversed(&mut store, &mut metrics, run, ScriptMode::Hebrew);
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_find_glyph(count: i32, table: u32, code: u16) -> i32 {
    let mut last = count;
    let mut first = 1i32;
    if last <= 0 { return -1; }
    let mut middle = last.wrapping_add(1) >> 1;
    loop {
        let candidate = unsafe { little_half(Address32(table.wrapping_add((middle as u32).wrapping_mul(2)))) };
        if code < candidate { last = middle.wrapping_sub(1); }
        else {
            first = middle.wrapping_add(1);
            if code == candidate { return middle; }
        }
        if last < first { return -1; }
        middle = last.wrapping_add(first) / 2;
    }
}

#[repr(C)]
pub struct FontScriptRun {
    source: u16,
    glyph: u16,
    category: u32,
    length: u16,
    padding: u16,
    characters: Address32,
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_read_run(engine: u32, base: u32, index: u16, run: *mut FontScriptRun) -> i32 {
    let context = ScriptContext(Address32(engine));
    let stream = Address32(unsafe { context.word(0x70) });
    let offset = unsafe { little_half(stream.offset(ByteOffset(base.wrapping_add(2).wrapping_add(index as u32 * 2)))) } as u32;
    let position = stream.offset(ByteOffset(base.wrapping_add(offset)));
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*run).glyph), little_half(position));
        write_volatile(core::ptr::addr_of_mut!((*run).category), little_half(position.offset(ByteOffset(2))) as u32);
        let count = little_half(position.offset(ByteOffset(4)));
        write_volatile(core::ptr::addr_of_mut!((*run).length), count);
        for character in 0..count.saturating_sub(1) as u32 {
            let destination = read_volatile(core::ptr::addr_of!((*run).characters));
            destination.offset(ByteOffset(character * 2)).write_half(little_half(position.offset(ByteOffset(6 + character * 2))));
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_mirror_character(code: u16) -> u32 {
    shaping::mirror_character(CharacterCode(code)).0 as u32
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_mirror_run(engine: u32, start: u16, end: u16) {
    let mut store = unsafe { GuestStore::new(Address32(engine)) };
    let _ = shaping::mirror_run(&mut store, CharacterRange { start: CharacterIndex(start), end: CharacterIndex(end) });
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct ArabicWorkspace(Address32);

impl ArabicWorkspace {
    unsafe fn state(self, index: u16) -> u32 { unsafe { self.0.offset(ByteOffset(index as u32 * 16 + 12)).word() } }
}

fn compose_arabic_pair(first: u16, second: u16) -> (u16, u16) {
    let combined = match (first, second) {
        (0x627, 0x653) => 0x622, (0x627, 0x654) => 0x623,
        (0x627, 0x655) => 0x625, (0x627, 0x674) => 0x675,
        (0x648, 0x654) => 0x624, (0x648, 0x674) => 0x676,
        (0x64a, 0x654) => 0x626, (0x64a, 0x674) => 0x678,
        (0x6c1, 0x654) => 0x6c2, (0x6d2, 0x654) => 0x6d3,
        (0x6d5, 0x654) => 0x6c0, (0x6c7, 0x674) => 0x677,
        _ => return (first, 1),
    };
    (combined, 2)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_compose_arabic_character(engine: u32, character: u32) -> i32 {
    let context = ScriptContext(Address32(engine));
    let mut character = character as u16;
    let request = Address32(unsafe { context.word(0x10) });
    let text = Address32(unsafe { request.offset(ByteOffset(16)).word() });
    let (code, count) = unsafe { compose_arabic_pair(text.offset(ByteOffset(character as u32 * 2)).half(), text.offset(ByteOffset(character as u32 * 2 + 2)).half()) };
    let composed = unsafe { context.half(0x5a4) };
    unsafe { context.put_half(0x540 + composed as u32 * 2, code) };
    for remaining in (1..=count).rev() {
        let record = 0x60c + character as u32 * 16;
        unsafe {
            context.put_half(record + 6, code);
            context.put_half(record + 4, context.half(0x5a4));
            context.put_half(record + 8, xm1_pipeline_image_font_pack_count(remaining, 1));
        }
        character = character.wrapping_add(1);
    }
    unsafe { context.put_half(0x5a4, context.half(0x5a4).wrapping_add(1)) };
    count as i32
}

fn arabic_class(code: u16) -> u32 { ScriptTables::firmware().arabic_class(CharacterCode(code)).0 }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_find_ligature(engine: u32, character: u32, dictionary_index: u32, workspace: u32, header: u32, glyph_out: *mut u16) -> u32 {
    let context = ScriptContext(Address32(engine));
    let character = character as u16;
    let mapping = 0x610 + character as u32 * 16;
    let source = unsafe { context.half(mapping + 2) };
    let header = Address32(header);
    let directory = Address32(unsafe { header.offset(ByteOffset(8)).word() });
    let offset = unsafe { little_half(directory.offset(ByteOffset((dictionary_index as u16) as u32 * 2))) } as u32;
    let base = unsafe { context.word(0x51c) }.wrapping_add(offset);
    unsafe { context.put_word(0x74, base) };
    let candidates = unsafe { context.next() } as u32;
    let workspace = ArabicWorkspace(Address32(workspace));
    let mut characters = [0u16; 20];
    let mut candidate = FontScriptRun { source, glyph: 0, category: 0, length: 0, padding: 0, characters: Address32(characters.as_mut_ptr() as u32) };
    let mut index = 0u16;
    let mut restarted = false;
    let mut original_state = 0u32;
    let mut wanted_state = 0u32;
    let mut required_length = 0u16;
    while (index as u32) < candidates {
        unsafe { xm1_pipeline_image_font_read_run(engine, base, index, &mut candidate) };
        if !restarted {
            original_state = unsafe { workspace.state(context.half(mapping)) };
            wanted_state = original_state;
        }
        let next_code = unsafe { context.half(mapping + 18) };
        if source.wrapping_add(0xfa00) < 256 && arabic_class(source) == 0x13 && next_code.wrapping_add(0xfa00) < 256 && arabic_class(next_code) == 0x17 {
            if original_state == 2 { wanted_state = 0; original_state = 0; }
            else if original_state == 3 { wanted_state = 1; original_state = 1; }
        }
        let mut matches = wanted_state == candidate.category && (required_length == 0 || required_length == candidate.length);
        if matches {
            let source_index = unsafe { context.half(mapping) };
            for additional in 0..candidate.length.saturating_sub(1) {
                let code = unsafe { context.half(0x540 + source_index.wrapping_add(additional).wrapping_add(1) as u32 * 2) };
                if code != unsafe { read_volatile(characters.as_ptr().add(additional as usize)) } { matches = false; break; }
            }
        }
        if matches && unsafe { FontEngine(engine).glyph_advance(GlyphCode(candidate.glyph as u32)) } != 0 && candidate.length != 0 {
            let last_state = unsafe { workspace.0.offset(ByteOffset((candidate.length as u32 + context.half(mapping) as u32) * 16 - 4)).word() };
            if restarted || original_state.wrapping_sub(2) >= 3 || last_state != 1 {
                unsafe { write_volatile(glyph_out, candidate.glyph) };
                return candidate.length as u32;
            }
            required_length = candidate.length;
            if original_state == 2 { wanted_state = 0; }
            else if original_state == 3 { wanted_state = 1; }
            index = 1;
            restarted = true;
        } else { index = index.wrapping_add(1); }
    }
    unsafe { write_volatile(glyph_out, candidate.source) };
    1
}

#[repr(C)]
struct ScriptDictionary {
    version: u16,
    glyph_directory: u16,
    table_count: u16,
    padding: u16,
    offsets: Address32,
}

unsafe fn arabic_dictionary(context: ScriptContext, dictionary: &mut ScriptDictionary) -> i32 {
    if unsafe { context.word(0x518) } == 0 { return 0; }
    let tables = unsafe { context.half(0x50c) } as u32;
    let mut offset = unsafe { context.word(0x51c) };
    for index in 0..tables {
        offset = unsafe { context.word(0x78 + context.half(0x514) as u32 * 4) }.wrapping_add(unsafe { context.word(0x478 + index * 4) });
    }
    if tables != 0 { unsafe { context.put_word(0x51c, offset) }; }
    unsafe { context.put_word(0x74, offset) };
    dictionary.version = unsafe { context.next() };
    if dictionary.version != 1 { unsafe { context.put_word(0x520, 0) }; return 4; }
    dictionary.glyph_directory = unsafe { context.next() };
    dictionary.table_count = unsafe { context.next() };
    unsafe { context.put_word(0x520, 1) };
    dictionary.offsets = Address32(unsafe { context.word(4) }.wrapping_add(unsafe { context.word(0x74) }));
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_shape_arabic(engine: u32, run: u32) -> i32 {
    let context = ScriptContext(Address32(engine));
    let first = unsafe { context.half(0x524) } as u32;
    let end = unsafe { context.half(0x526) } as u32;
    let auxiliary = Address32(unsafe { context.word(0x18) });
    let workspace_header = Address32(unsafe { auxiliary.offset(ByteOffset(0x70)).word() });
    let capacity = unsafe { workspace_header.offset(ByteOffset(12)).word() } as i32;
    if capacity < end.wrapping_sub(first).wrapping_mul(16) as i32 { return 0; }
    let workspace = ArabicWorkspace(Address32(unsafe { workspace_header.offset(ByteOffset(16)).word() }));
    let phase = unsafe { context.word(0x538) };
    if phase & 3 != 2 {
        let mut dictionary = ScriptDictionary { version: 0, glyph_directory: 0, table_count: 0, padding: 0, offsets: Address32(0) };
        let warning = unsafe { arabic_dictionary(context, &mut dictionary) };
        for character in first..end {
            let request = Address32(unsafe { context.word(0x10) });
            let text = Address32(unsafe { request.offset(ByteOffset(16)).word() });
            let code = unsafe { text.offset(ByteOffset(character * 2)).half() };
            unsafe { context.put_half(0x60c + character * 16, character as u16); context.put_half(0x60e + character * 16, code); }
        }
        let mut character = first;
        while character < unsafe { context.half(0x526) } as u32 {
            let count = unsafe { xm1_pipeline_image_font_compose_arabic_character(engine, character) } as u32;
            character = character.wrapping_add(count) & 0xffff;
        }
        character = first;
        while character < unsafe { context.half(0x526) } as u32 {
            let mapping = 0x610 + character * 16;
            let composed = unsafe { context.half(mapping) };
            let code = unsafe { context.half(mapping + 2) };
            let previous = if unsafe { context.half(0x610 + first * 16) } < composed { unsafe { workspace.0.offset(ByteOffset(composed as u32 * 16 - 4)).word() } } else { 0 };
            let class = arabic_class(code);
            let category = ScriptTables::firmware().arabic_category(JoiningClass(class)).0;
            let transition = ScriptTables::firmware().arabic_transition(JoiningState(previous), JoiningCategory(category));
            let mut state = if category == 4 { 0 } else { transition.state.0 };
            if character == first {
                if character == unsafe { context.half(0x526) } as u32 - 1 { state = 0; }
            } else {
                let updated = transition.update.0;
                if previous != updated { unsafe { workspace.0.offset(ByteOffset(composed as u32 * 16 - 4)).write_word(updated) }; }
                if category == 2 && character == unsafe { context.half(0x526) } as u32 - 1 {
                    if state == 2 { state = 0; } else if state == 3 { state = 1; }
                }
            }
            let row = workspace.0.offset(ByteOffset(composed as u32 * 16));
            unsafe {
                row.offset(ByteOffset(4)).write_word(class);
                row.offset(ByteOffset(8)).write_word(category);
                row.write_half(code);
                row.offset(ByteOffset(12)).write_word(state);
            }
            let packed = unsafe { context.half(0x614 + character * 16) };
            let count = if packed > 256 { packed & 255 } else { packed };
            character = character.wrapping_add(count as u32) & 0xffff;
        }
        character = first;
        while character < unsafe { context.half(0x526) } as u32 {
            let code = unsafe { context.half(0x612 + character * 16) };
            let glyph_index = unsafe { context.half(0x60a) };
            let mut output_code = code;
            let mut source_groups = 1u32;
            if unsafe { context.word(0x520) } != 0 && unsafe { context.word(0x518) } != 0 {
                unsafe { context.put_word(0x74, context.word(0x51c).wrapping_add(dictionary.glyph_directory as u32)) };
                if unsafe { context.next() } == 1 {
                    let count = unsafe { context.next() } as i32;
                    let table = unsafe { context.word(4) }.wrapping_add(unsafe { context.word(0x74) });
                    let index = unsafe { xm1_pipeline_image_font_find_glyph(count, table, code) };
                    if index >= 0 {
                        source_groups = unsafe { xm1_pipeline_image_font_find_ligature(engine, character, index as u32, workspace.0.0, (&dictionary as *const ScriptDictionary) as u32, &mut output_code) };
                    }
                }
            }
            unsafe { context.put_half(0x5a6 + glyph_index as u32 * 2, output_code) };
            let mut count = 0u16;
            let mut next = character;
            for _ in 0..source_groups {
                let packed = unsafe { context.half(0x614 + next * 16) };
                let cluster = if packed > 256 { packed & 255 } else { packed };
                count = count.wrapping_add(cluster);
                next = next.wrapping_add(cluster as u32) & 0xffff;
            }
            let mut remaining = count;
            let mut next = character;
            for _ in 0..source_groups {
                let packed = unsafe { context.half(0x614 + next * 16) };
                let cluster = if packed > 256 { packed & 255 } else { packed };
                for offset in 0..cluster as u32 {
                    let mapping = 0x60c + next.wrapping_add(offset) * 16;
                    unsafe {
                        context.put_half(mapping + 12, output_code);
                        context.put_half(mapping + 10, context.half(0x60a));
                        context.put_half(mapping + 14, xm1_pipeline_image_font_pack_count(remaining, 1));
                    }
                    remaining = remaining.wrapping_sub(1);
                }
                next = next.wrapping_add(cluster as u32) & 0xffff;
            }
            unsafe { context.put_half(0x60a, context.half(0x60a).wrapping_add(1)) };
            character = character.wrapping_add(count as u32) & 0xffff;
        }
        unsafe {
            xm1_pipeline_image_font_mirror_run(engine, context.half(0x524), context.half(0x526));
            xm1_pipeline_image_font_compose_private_run(engine);
            xm1_pipeline_image_font_store_main_run(engine);
        }
        if phase & 2 == 0 { return warning; }
    }
    unsafe { render_glyph_span(context, RunAddress(Address32(run))) }
}

fn thai_class(code: u16) -> u16 { ScriptTables::firmware().thai_class(CharacterCode(code)) }

#[repr(C)]
pub struct GlyphOffset { x: i32, y: i32 }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_thai_mark_offsets(output: *mut GlyphOffset, metrics: u32, text: u32, start: i32, last: i32, index: i32, decomposed: i32, followed: i32) {
    let metrics = Address32(metrics);
    let text = Address32(text);
    let code = unsafe { text.offset(ByteOffset((index as u32).wrapping_mul(2))).half() };
    let category = thai_class(code);
    let mut x = 0i32;
    let mut y = 0i32;
    let mut has_base = false;
    if start < index {
        let previous = index.wrapping_sub(1);
        let previous_code = unsafe { text.offset(ByteOffset((previous as u32).wrapping_mul(2))).half() };
        let previous_category = thai_class(previous_code);
        let base = if previous_category == 2 { Some(previous) } else {
            let earlier = index.wrapping_sub(2);
            if earlier >= start && unsafe { thai_class(text.offset(ByteOffset((earlier as u32).wrapping_mul(2))).half()) } == 2 { Some(earlier) } else { None }
        };
        if let Some(base) = base {
            has_base = true;
            if base >= 0 {
                let base_code = unsafe { text.offset(ByteOffset(base as u32 * 2)).half() };
                if matches!(base_code, 0xe1b | 0xe1d | 0xe1f) && category & 0xbf00 != 0 { x = -(unsafe { metrics.offset(ByteOffset(8)).half() } as i32); }
                if base == previous {
                    if category & 0x8000 != 0 {
                        y = unsafe { metrics.half() } as i32;
                        if index < last && followed != 0 { y = 0; }
                    } else if category & 0x1800 != 0 { y = unsafe { metrics.half() } as i32; }
                } else if base == index.wrapping_sub(2) {
                    if previous_category & 0x40c0 != 0 { y = unsafe { metrics.half() } as i32; }
                    if decomposed != 0 && code == 0xe4d && previous_category & 0x9800 != 0 { y = unsafe { metrics.half() } as i32; }
                }
            }
        }
    }
    unsafe {
        if has_base {
            write_volatile(core::ptr::addr_of_mut!((*output).y), y);
            write_volatile(core::ptr::addr_of_mut!((*output).x), x);
        } else {
            write_volatile(core::ptr::addr_of_mut!((*output).x), x);
            write_volatile(core::ptr::addr_of_mut!((*output).y), y);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_thai_split_character(engine: u32, character: i32, mapping: *mut u16) {
    let context = ScriptContext(Address32(engine));
    let request = Address32(unsafe { context.word(0x10) });
    let text = Address32(unsafe { request.offset(ByteOffset(16)).word() });
    let code = unsafe { text.offset(ByteOffset((character as u32).wrapping_mul(2))).half() };
    unsafe { write_volatile(mapping, character as u16); write_volatile(mapping.add(1), code); }
    let first = unsafe { context.half(0x524) } as i32;
    let mut split = false;
    if code == 0xe33 && first < character {
        let previous = unsafe { text.offset(ByteOffset((character as u32).wrapping_mul(2).wrapping_sub(2))).half() };
        let category = thai_class(previous);
        split = category == 2;
        if category == 0x8000 && first <= character.wrapping_sub(2) {
            let earlier = unsafe { text.offset(ByteOffset((character as u32).wrapping_mul(2).wrapping_sub(4))).half() };
            split = thai_class(earlier) == 2;
        }
    }
    let count = if split { 2 } else { 1 };
    let output_code = if split {
        let glyph = unsafe { context.half(0x60a) };
        let composed = unsafe { context.half(0x5a4) };
        unsafe {
            context.put_half(0x5a6 + glyph as u32 * 2, 0xe4d);
            context.put_half(0x5a4, composed.wrapping_add(1));
            context.put_half(0x540 + composed as u32 * 2, 0xe4d);
            context.put_half(0x60a, glyph.wrapping_add(1));
            context.put_half(0x5a6 + glyph.wrapping_add(1) as u32 * 2, 0xe32);
            context.put_half(0x540 + composed.wrapping_add(1) as u32 * 2, 0xe32);
        }
        0xe32
    } else {
        unsafe { context.put_half(0x540 + context.half(0x5a4) as u32 * 2, code); context.put_half(0x5a6 + context.half(0x60a) as u32 * 2, code); }
        code
    };
    unsafe {
        write_volatile(mapping.add(3), output_code);
        write_volatile(mapping.add(4), xm1_pipeline_image_font_pack_count(1, count));
        write_volatile(mapping.add(2), context.half(0x5a4));
        write_volatile(mapping.add(6), output_code);
        write_volatile(mapping.add(7), xm1_pipeline_image_font_pack_count(1, count));
        write_volatile(mapping.add(5), context.half(0x60a));
    }
}

fn thai_class_index(code: u16) -> shaping::ThaiClassIndex { shaping::thai_class_index(thai_class(code)) }

unsafe fn thai_combines(context: ScriptContext, first: u32, glyph: u16) -> bool {
    let glyph = glyph as u32;
    if glyph <= first { return false; }
    let previous = unsafe { context.half(0x5a6 + (glyph - 1) * 2) };
    let current = unsafe { context.half(0x5a6 + glyph * 2) };
    let previous_class = thai_class_index(previous);
    let current_class = thai_class_index(current);
    let transition = ScriptTables::firmware().thai_transition(previous_class, current_class);
    if transition != b'C' { return false; }
    let previous_flags = thai_class(previous);
    if previous_flags == 2 { return true; }
    if (glyph.wrapping_sub(2) as i32) < first as i32 || previous == 0 { return false; }
    if unsafe { thai_class(context.half(0x5a6 + (glyph - 2) * 2)) } != 2 { return false; }
    if current == 0 { return previous_flags & 0xffc0 != 0; }
    let current_flags = thai_class(current);
    let top = if previous_flags & 0x140 != 0 {
        if current_flags & 0x8800 != 0 { return true; }
        0
    } else {
        let top = current_flags >> 15;
        if previous_flags & 0x280 != 0 && top != 0 { return true; }
        top
    };
    previous_flags & 0x400 != 0 && (top != 0 || current_flags & 0x1000 != 0)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_position_thai(engine: u32, run: u32) -> i32 {
    let context = ScriptContext(Address32(engine));
    let run = RunAddress(Address32(run));
    let mut character = unsafe { run.start() } as u32;
    let end = unsafe { run.end() } as u32;
    let first = unsafe { context.half(0x616 + character * 16) } as u32;
    let last = unsafe { context.half(0x606 + end * 16) } as u32;
    let mut glyph = first as u16;
    let mut pen = unsafe { initial_pen(context, run) };
    let mut followed = false;
    let mut padding = 0i32;
    while character < end {
        let mapping = 0x60c + character * 16;
        let code = unsafe { context.half(mapping + 12) };
        let advance = unsafe { FontEngine(engine).glyph_advance(GlyphCode(code as u32)) } as u32;
        if character.wrapping_add(1) < end {
            let next = unsafe { context.half(mapping + 30) };
            followed = next > 256 && next >> 8 == 2;
        }
        let packed = unsafe { context.half(mapping + 14) };
        let (count, groups) = if packed > 256 { (packed & 255, packed >> 8) } else { (packed, 1) };
        for group in 0..groups {
            let output = unsafe { positioned_context(context) };
            let code = unsafe { context.half(0x5a6 + glyph as u32 * 2) };
            unsafe { output.put_half(4 + glyph as u32 * 2, code) };
            let combined = unsafe { thai_combines(context, first, glyph) };
            let decomposed = groups == 2 && group == 0;
            let packed = if group == 0 { unsafe { context.half(mapping + 14) } } else { 0 };
            let mut offset = GlyphOffset { x: 0, y: 0 };
            let x = if combined || decomposed {
                let code = unsafe { context.half(0x5a6 + glyph as u32 * 2) };
                let mark_advance = unsafe { FontEngine(engine).glyph_advance(GlyphCode(code as u32)) } as u32;
                unsafe { xm1_pipeline_image_font_thai_mark_offsets(&mut offset, engine.wrapping_add(0xfc8), engine.wrapping_add(0x5a6), first as i32, last as i32, glyph as i32, decomposed as i32, followed as i32) };
                pen.wrapping_add(offset.x as u32).wrapping_sub(mark_advance)
            } else {
                pen = pen.wrapping_add(advance);
                pen.wrapping_sub(advance)
            };
            let mut clip_top = 0u32;
            let mut clip_width = 0u32;
            let mut clip_height = 0u32;
            let code = unsafe { context.half(0x5a6 + glyph as u32 * 2) };
            let clipped = if (glyph as u32) < last && matches!(code, 0xe0d | 0xe10) {
                unsafe { context.half(0x5a6 + (glyph as u32 + 1) * 2) }.wrapping_sub(0xe38) < 3
            } else { false };
            if clipped {
                unsafe { FontEngine(engine).stroke_padding(Address32(0), Address32((&mut padding as *mut i32) as u32), Address32(0), Address32(0)) };
                let height = unsafe { FontEngine(engine).metric(BackendMetric::Height, GlyphCode(0)) } as u32;
                clip_top = height.wrapping_sub(padding as u32).wrapping_sub(unsafe { context.half(0xfce) } as u32);
                let code = unsafe { context.half(0x5a6 + glyph as u32 * 2) };
                let advance = unsafe { FontEngine(engine).glyph_advance(GlyphCode(code as u32)) } as u32;
                let code = unsafe { context.half(0x5a6 + glyph as u32 * 2) };
                let overhang = unsafe { FontEngine(engine).metric(BackendMetric::RightOverhang, GlyphCode(code as u32)) } as u32;
                clip_height = (unsafe { context.half(0xfce) } as u32).wrapping_add(padding as u32);
                clip_width = overhang.wrapping_add(advance);
            }
            let output = unsafe { positioned_context(context) };
            let record = 0x68 + glyph as u32 * 60;
            unsafe {
                output.put_word(record + 52, clip_width);
                output.put_word(record + 48, clip_top);
                output.put_word(record + 40, clipped as u32);
                output.put_word(record + 36, offset.y as u32);
                output.put_word(record + 32, offset.x as u32);
                output.put_word(record + 16, x);
                output.put_half(record + 6, packed);
                output.put_word(record, character);
                output.put_half(record + 4, glyph);
                output.put_word(record + 56, clip_height);
                output.put_word(record + 20, offset.y as u32);
                output.put_word(record + 44, 0);
                output.put_word(record + 28, 0);
                output.put_word(record + 24, advance);
                output.put_word(record + 12, 0);
                output.put_word(record + 8, pen);
            }
            glyph = glyph.wrapping_add(1);
        }
        character = character.wrapping_add(count as u32);
    }
    unsafe { run.finish(pen) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_measure_thai_lines(engine: u32) {
    let context = ScriptContext(Address32(engine));
    let mut character = unsafe { context.half(0x524) } as u32;
    let first = unsafe { context.half(0x616 + character * 16) } as u32;
    let mut glyph = first as u16;
    let mut pen = 0u32;
    while character < unsafe { context.half(0x526) } as u32 {
        let mapping = 0x60c + character * 16;
        let code = unsafe { context.half(mapping + 12) };
        let advance = unsafe { FontEngine(engine).glyph_advance(GlyphCode(code as u32)) } as u32;
        let metrics = 0x92c + character * 28;
        unsafe {
            context.put_half(metrics + 4, context.half(mapping + 10));
            context.put_half(metrics + 16, 0x200e);
            context.put_word(metrics + 12, 0);
            context.put_word(metrics + 24, 0);
            context.put_word(metrics, character);
            context.put_word(metrics + 20, pen);
            context.put_word(metrics + 8, advance);
        }
        let packed = unsafe { context.half(mapping + 14) };
        if packed > 256 && packed >> 8 == 2 && unsafe { context.half(0x5a6 + glyph as u32 * 2) } == 0xe4d && unsafe { context.half(0x5a6 + glyph as u32 * 2 + 2) } == 0xe32 { glyph = glyph.wrapping_add(1); }
        if !unsafe { thai_combines(context, first, glyph) } { pen = pen.wrapping_add(advance); }
        character = character.wrapping_add(1);
        if character < unsafe { context.half(0x526) } as u32 { glyph = glyph.wrapping_add(1); }
    }
    unsafe { context.put_word(0x534, pen) };
}

#[derive(Clone, Copy)]
#[repr(C)]
struct OutlineClip { kind: u32, left: i32, top: i32, width: i32, height: i32 }

impl OutlineClip {
    unsafe fn from_record(output: ScriptContext, record: u32, kind: u32) -> Self {
        unsafe { Self { kind, left: output.word(record + 44) as i32, top: output.word(record + 48) as i32, width: output.word(record + 52) as i32, height: output.word(record + 56) as i32 } }
    }
    fn address(&self) -> Address32 { Address32((self as *const Self) as u32) }
}

#[repr(C)]
struct RasterClip {
    x: i32, top: i32, source_x: i32, source_top: i32, advance: i32, height: i32, enabled: u32,
    first: OutlineClip, second: OutlineClip,
}

unsafe fn render_thai(context: ScriptContext, run: RunAddress) -> i32 {
    let first = unsafe { context.half(0x616 + run.start() as u32 * 16) } as u32;
    let last = unsafe { context.half(0x606 + run.end() as u32 * 16) } as u32;
    if first > last { return 0; }
    if unsafe { context.word(0xff4) } == 1 {
        for glyph in first..=last {
            let output = unsafe { positioned_context(context) };
            let record = 0x68 + glyph * 60;
            let kind = unsafe { output.word(record + 40) };
            unsafe { context.put_word(0xfe8, output.word(0xc30)) };
            let mut handle = 0u32;
            let clip = if kind == 0 { None } else { Some(unsafe { OutlineClip::from_record(output, record, kind) }) };
            let code = unsafe { output.half(4 + glyph * 2) };
            let result = unsafe { FontEngine(context.0.0).append_outline_clipped(GlyphCode(code as u32), clip.as_ref().map_or(Address32(0), OutlineClip::address), output.field(0xc2c).0 as *mut u32, context.field(0xfe8).0 as *mut u32, &mut handle) };
            let output = unsafe { positioned_context(context) };
            unsafe { output.put_word(0xc30, context.word(0xfe8)) };
            if result != 0 { return result; }
            unsafe { output.put_word(0xc34 + glyph * 4, handle) };
        }
    }
    let mode = unsafe { context.word(0xff4) };
    if mode.wrapping_sub(3) < 3 || mode == 2 || mode.wrapping_sub(6) < 2 {
        for glyph in first..=last {
            let output = unsafe { positioned_context(context) };
            let record = 0x68 + glyph * 60;
            let kind = unsafe { output.word(record + 40) };
            let clip = if kind == 0 { None } else {
                let outline = unsafe { OutlineClip::from_record(output, record, kind) };
                Some(RasterClip {
                    x: unsafe { output.word(record + 16) } as i32, top: outline.top, source_x: 0, source_top: outline.top,
                    advance: unsafe { output.word(record + 24) } as i32, height: outline.height, enabled: 1, first: outline, second: outline,
                })
            };
            let clip_address = clip.as_ref().map_or(Address32(0), |clip| Address32((clip as *const RasterClip) as u32));
            let code = unsafe { output.half(4 + glyph * 2) };
            unsafe { FontEngine(context.0.0).draw_clipped(GlyphDraw {
                glyph: GlyphCode(code as u32), width: output.word(0xd00) as i32, height: output.word(0xd04) as i32,
                x: output.word(record + 16) as i32, y: output.word(record + 20) as i32,
                stride: output.word(0xd08) as i32, pixels: Address32(output.word(0xd10)), format: FontBitmapFormat(context.word(0x106c)),
            }, clip_address) };
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_shape_thai(engine: u32, run: u32) -> i32 {
    let context = ScriptContext(Address32(engine));
    let phase = unsafe { context.word(0x538) };
    if phase & 3 != 2 {
        unsafe { FontEngine(engine).thai_metrics(context.field(0xfc8)) };
        let mut character = unsafe { context.half(0x524) } as u32;
        while character < unsafe { context.half(0x526) } as u32 {
            let mut mapping = [0u16; 8];
            unsafe { xm1_pipeline_image_font_thai_split_character(engine, character as i32, mapping.as_mut_ptr()) };
            let glyph = unsafe { context.half(0x60a) };
            let end = unsafe { context.half(0x526) } as u32;
            unsafe { context.put_half(0x5a4, context.half(0x5a4).wrapping_add(1)) };
            for (field, value) in mapping.into_iter().enumerate() { unsafe { context.put_half(0x60c + character * 16 + field as u32 * 2, value) }; }
            unsafe { context.put_half(0x60a, glyph.wrapping_add(1)) };
            character = character.wrapping_add(1);
            if character >= end { break; }
        }
        unsafe { xm1_pipeline_image_font_measure_thai_lines(engine); xm1_pipeline_image_font_store_main_run(engine); }
        if phase & 2 == 0 { return 0; }
    }
    unsafe { render_thai(context, RunAddress(Address32(run))) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_prepare_hebrew_characters(engine: u32) {
    let mut store = unsafe { GuestStore::new(Address32(engine)) };
    let _ = shaping::prepare_hebrew_characters(&mut store);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_shape_hebrew(engine: u32, run: u32) -> i32 {
    let context = ScriptContext(Address32(engine));
    let phase = unsafe { context.word(0x538) };
    if phase & 3 != 2 {
        unsafe {
            xm1_pipeline_image_font_prepare_hebrew_characters(engine);
            xm1_pipeline_image_font_mirror_run(engine, context.half(0x524), context.half(0x526));
        }
        let mut store = unsafe { GuestStore::new(Address32(engine)) };
        let mut metrics = unsafe { GuestMetrics::new(Address32(engine)) };
        let _ = shaping::measure_characters(&mut store, &mut metrics, ScriptMode::Hebrew);
        unsafe { xm1_pipeline_image_font_store_main_run(engine) };
        if phase & 2 == 0 { return 0; }
    }
    unsafe { render_glyph_span(context, RunAddress(Address32(run))) }
}
