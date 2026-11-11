use super::{AssetOffset, FontError};

macro_rules! index_type {
    ($name:ident, $word:ty) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        #[repr(transparent)]
        pub struct $name(pub $word);
    };
}
index_type!(CharacterCode, u16);
index_type!(CharacterIndex, u16);
index_type!(SourceIndex, u16);
index_type!(GlyphIndex, u16);
index_type!(RunIndex, u16);
index_type!(DictionaryIndex, u16);
index_type!(ScriptId, u32);
index_type!(LanguageId, u32);
index_type!(PackedCount, u16);
index_type!(JoiningClass, u32);
index_type!(JoiningCategory, u32);
index_type!(JoiningState, u32);
index_type!(LayoutExtent16, u16);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct LayoutOffset(pub i32);
impl LayoutOffset {
    pub const fn bits(self) -> u32 { self.0 as u32 }
    pub const fn wrapping_add(self, other: Self) -> Self { Self(self.0.wrapping_add(other.0)) }
    pub const fn wrapping_sub(self, other: Self) -> Self { Self(self.0.wrapping_sub(other.0)) }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct TextDirection(pub u16);
impl TextDirection {
    pub const LEFT_TO_RIGHT: Self = Self(0x200e);
    pub const RIGHT_TO_LEFT: Self = Self(0x200f);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum UnicodeCategory { Unknown = 0, Space = 1, Control = 2, Basic = 3, Thai = 4, Arabic = 8, Hebrew = 9, Devanagari = 14, Tamil = 15 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CharacterKind { None = 0, ArabicMark = 1, ArabicEnd = 2, Combining = 3, Space = 4, Control = 5, GeneralControl = 6, Digit = 7, Uppercase = 8, Lowercase = 9, Joiner = 10, Letter = 11, Open = 12, Close = 13, Symbol = 14, Currency = 16 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct UnicodeClass { pub category: UnicodeCategory, pub kind: CharacterKind }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ScriptMode { Basic = 1, Arabic = 2, Thai = 3, Devanagari = 4, Tamil = 5, Hebrew = 6 }
impl ScriptMode {
    pub const fn identity(self) -> ScriptIdentity { ScriptIdentity { script: ScriptId(self as u32), language: LanguageId(match self { Self::Basic => 1, Self::Arabic => 5, Self::Thai => 6, Self::Devanagari => 7, Self::Tamil => 8, Self::Hebrew => 9 }) } }
    pub(crate) const fn language(self) -> LanguageId { LanguageId(match self { Self::Basic => 1, Self::Arabic => 5, Self::Thai => 6, Self::Devanagari => 7, Self::Tamil => 8, Self::Hebrew => 9 }) }
    pub const fn direction(self) -> TextDirection { match self { Self::Arabic | Self::Hebrew => TextDirection::RIGHT_TO_LEFT, _ => TextDirection::LEFT_TO_RIGHT } }
    pub(crate) fn from_category(category: UnicodeCategory) -> Self { match category { UnicodeCategory::Arabic => Self::Arabic, UnicodeCategory::Hebrew => Self::Hebrew, UnicodeCategory::Thai => Self::Thai, UnicodeCategory::Devanagari => Self::Devanagari, UnicodeCategory::Tamil => Self::Tamil, _ => Self::Basic } }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScriptIdentity { pub script: ScriptId, pub language: LanguageId }
impl ScriptIdentity {
    pub const fn algorithm(self) -> ScriptMode { match (self.script.0, self.language.0) { (2,5) => ScriptMode::Arabic, (3,6) => ScriptMode::Thai, (6,9) => ScriptMode::Hebrew, _ => ScriptMode::Basic } }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CharacterRange { pub start: CharacterIndex, pub end: CharacterIndex }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphSpan { pub first: GlyphIndex, pub last: GlyphIndex }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetectedRun { pub range: CharacterRange, pub identity: ScriptIdentity, pub direction: TextDirection }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeWarning { None, InvalidDictionaryVersion }
impl ShapeWarning { pub const fn status(self) -> i32 { match self { Self::None => 0, Self::InvalidDictionaryVersion => 4 } } }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct CharacterMapping {
    pub character: CharacterIndex,
    pub original: CharacterCode,
    pub source: SourceIndex,
    pub source_code: CharacterCode,
    pub source_count: PackedCount,
    pub glyph: GlyphIndex,
    pub glyph_code: CharacterCode,
    pub glyph_count: PackedCount,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct CharacterMetric {
    pub character: u32,
    pub glyph: GlyphIndex,
    pub reserved: u16,
    pub advance: LayoutOffset,
    pub y: LayoutOffset,
    pub direction: TextDirection,
    pub reserved_direction: u16,
    pub pen_x: LayoutOffset,
    pub pen_y: LayoutOffset,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct ScriptRun {
    pub start: CharacterIndex,
    pub end: CharacterIndex,
    pub script: ScriptId,
    pub language: LanguageId,
    pub advance: LayoutOffset,
    pub height: LayoutOffset,
    pub x: LayoutOffset,
    pub y: LayoutOffset,
    pub finish_x: LayoutOffset,
    pub finish_y: LayoutOffset,
}
impl ScriptRun { pub const fn range(&self) -> CharacterRange { CharacterRange { start: self.start, end: self.end } } }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct GlyphPosition {
    pub character: u32,
    pub glyph: GlyphIndex,
    pub count: PackedCount,
    pub pen_x: LayoutOffset,
    pub pen_y: LayoutOffset,
    pub x: LayoutOffset,
    pub y: LayoutOffset,
    pub advance: LayoutOffset,
    pub reserved: u32,
    pub mark_x: LayoutOffset,
    pub mark_y: LayoutOffset,
    pub clipped: u32,
    pub clip_x: LayoutOffset,
    pub clip_y: LayoutOffset,
    pub clip_width: LayoutOffset,
    pub clip_height: LayoutOffset,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct ArabicState {
    pub code: CharacterCode,
    pub reserved: u16,
    pub class: JoiningClass,
    pub category: JoiningCategory,
    pub state: JoiningState,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct ThaiMetrics {
    pub rise: LayoutExtent16,
    pub reserved_first: LayoutExtent16,
    pub reserved_second: LayoutExtent16,
    pub clip_descent: LayoutExtent16,
    pub left_shift: LayoutExtent16,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphOffset { pub x: LayoutOffset, pub y: LayoutOffset }

pub struct ScriptResources<'asset> {
    pub font: &'asset [u8],
    pub stream: &'asset [u8],
    pub rows: &'asset [AssetOffset],
}
pub(crate) trait ScriptData {
    fn available(&self) -> bool;
    fn row_count(&self) -> u16;
    fn row_offset(&self, index: DictionaryIndex) -> Result<AssetOffset, FontError>;
    fn stream_half(&self, offset: AssetOffset) -> Result<u16, FontError>;
    fn stream_word(&self, offset: AssetOffset) -> Result<u32, FontError>;
    fn font_half(&self, offset: AssetOffset) -> Result<u16, FontError>;
}
impl ScriptData for ScriptResources<'_> {
    fn available(&self) -> bool { !self.stream.is_empty() }
    fn row_count(&self) -> u16 { self.rows.len().min(u16::MAX as usize) as u16 }
    fn row_offset(&self, index: DictionaryIndex) -> Result<AssetOffset, FontError> { self.rows.get(index.0 as usize).copied().ok_or(FontError::Truncated) }
    fn stream_half(&self, offset: AssetOffset) -> Result<u16, FontError> { half(self.stream, offset) }
    fn stream_word(&self, offset: AssetOffset) -> Result<u32, FontError> {
        let bytes = self.stream.get(offset.get() as usize..).and_then(|bytes| bytes.get(..4)).ok_or(FontError::Truncated)?;
        Ok(u32::from_le_bytes([bytes[0],bytes[1],bytes[2],bytes[3]]))
    }
    fn font_half(&self, offset: AssetOffset) -> Result<u16, FontError> { half(self.font, offset) }
}
fn half(bytes: &[u8], offset: AssetOffset) -> Result<u16, FontError> {
    let bytes = bytes.get(offset.get() as usize..).and_then(|bytes| bytes.get(..2)).ok_or(FontError::Truncated)?;
    Ok(u16::from_le_bytes([bytes[0],bytes[1]]))
}
fn offset(base: AssetOffset, amount: u32) -> AssetOffset { AssetOffset::new(base.get().wrapping_add(amount)) }

pub(crate) trait ShapingStore {
    fn text_code(&self, character: CharacterIndex) -> Result<CharacterCode, FontError>;
    fn source_code(&self, index: SourceIndex) -> Result<CharacterCode, FontError>;
    fn set_source_code(&mut self, index: SourceIndex, code: CharacterCode) -> Result<(), FontError>;
    fn glyph_code(&self, index: GlyphIndex) -> Result<CharacterCode, FontError>;
    fn set_glyph_code(&mut self, index: GlyphIndex, code: CharacterCode) -> Result<(), FontError>;
    fn mapping(&mut self, character: CharacterIndex) -> Result<&mut CharacterMapping, FontError>;
    fn metric(&mut self, character: CharacterIndex) -> Result<&mut CharacterMetric, FontError>;
    fn main_run(&mut self, index: RunIndex) -> Result<&mut ScriptRun, FontError>;
    fn position(&mut self, index: GlyphIndex) -> Result<&mut GlyphPosition, FontError>;
    fn output_code(&self, index: GlyphIndex) -> Result<CharacterCode, FontError>;
    fn set_output_code(&mut self, index: GlyphIndex, code: CharacterCode) -> Result<(), FontError>;
    fn glyph_span(&mut self, range: CharacterRange) -> Result<Option<GlyphSpan>, FontError>;
    fn glyph_before(&mut self, end: CharacterIndex) -> Result<GlyphIndex, FontError>;
    fn range(&self) -> CharacterRange;
    fn identity(&self) -> ScriptIdentity;
    fn direction(&self) -> TextDirection;
    fn main_run_index(&self) -> RunIndex;
    fn source_count(&self) -> SourceIndex;
    fn set_source_count(&mut self, count: SourceIndex);
    fn glyph_count(&self) -> GlyphIndex;
    fn set_glyph_count(&mut self, count: GlyphIndex);
    fn set_advance(&mut self, advance: LayoutOffset);
    fn advance(&self) -> LayoutOffset;
}

pub(crate) trait DictionaryStore: ShapingStore {
    type Data: ScriptData;
    fn data(&self) -> &Self::Data;
    fn arabic(&mut self, source: SourceIndex) -> Result<&mut ArabicState, FontError>;
    fn arabic_capacity(&self) -> usize;
    fn thai_metrics(&mut self) -> &mut ThaiMetrics;
    fn cursor(&self) -> AssetOffset;
    fn set_cursor(&mut self, cursor: AssetOffset);
    fn dictionary_base(&self) -> AssetOffset;
    fn set_dictionary_base(&mut self, base: AssetOffset);
    fn dictionary_enabled(&self) -> bool;
    fn dictionary_valid(&self) -> bool;
    fn set_dictionary_valid(&mut self, valid: bool);
    fn table_base(&self) -> AssetOffset;
    fn table_count(&self) -> u16;
    fn set_table_count(&mut self, count: u16);
    fn table_offset(&self, index: DictionaryIndex) -> Result<AssetOffset, FontError>;
    fn set_table_offset(&mut self, index: DictionaryIndex, value: AssetOffset) -> Result<(), FontError>;
    fn set_selected_script(&mut self, script: ScriptId);
    fn set_selected_language(&mut self, language: LanguageId);
}

pub trait ShapingMetrics {
    fn advance(&mut self, code: CharacterCode) -> Result<LayoutOffset, FontError>;
    fn height(&mut self) -> Result<LayoutOffset, FontError>;
    fn right_overhang(&mut self, code: CharacterCode) -> Result<LayoutOffset, FontError>;
    fn stroke_padding_top(&mut self, padding: &mut LayoutOffset) -> Result<(), FontError>;
    fn thai_metrics(&mut self, output: &mut ThaiMetrics) -> Result<(), FontError>;
}

index_type!(DirectionHint, u32);

pub fn glyph_cell_width(code: CharacterCode) -> i32 {
    let index = code.0.wrapping_add(0x1ee0);
    if index < 0xdf {
        match code.0.wrapping_add(0x1f00) { 0x28 | 0x5b | 0x7b => 2, 0x29 | 0x5d | 0x7d => 3, _ => 4 }
    } else if index < 0x1610 { 4 } else { 0 }
}

pub fn classify(code: CharacterCode) -> UnicodeClass {
    use CharacterKind as K;
    use UnicodeCategory as C;
    let code = code.0;
    let (category, kind) = if code <= 31 || code == 127 || code.wrapping_sub(128) < 32 { (C::Control, K::Control) }
    else if code == 0x20 || code == 0x3000 { (C::Space, K::Space) }
    else if code.wrapping_sub(0x21) < 0x5e {
        let kind = if code.wrapping_sub(0x41) < 26 { K::Uppercase }
        else if code.wrapping_sub(0x61) < 26 { K::Lowercase }
        else if code.wrapping_sub(0x30) < 10 { K::Digit }
        else if code == 0x40 || code.wrapping_sub(0x23) <= 3 { K::Currency }
        else { match code { 0x28 | 0x5b | 0x7b => K::Open, 0x29 | 0x5d | 0x7d => K::Close, _ => K::Symbol } };
        (C::Basic, kind)
    } else if code.wrapping_sub(0xa0) < 0x260 { (C::Basic, K::Letter) }
    else if code.wrapping_add(0xfd00) <= 0x6f { (C::Basic, K::Combining) }
    else if code.wrapping_sub(0x370) <= 0x8f || code.wrapping_add(0xfc00) < 0x130 || code.wrapping_sub(0x530) <= 0x5f { (C::Basic, K::Letter) }
    else if code.wrapping_sub(0x590) <= 0x6f || code.wrapping_add(0x4e3) <= 0x32 { (C::Hebrew, K::Letter) }
    else if code.wrapping_add(0x4b0) <= 0x2b0 || code.wrapping_add(0xfa00) <= 0xff || code.wrapping_add(0x190) <= 0x8e || code.wrapping_add(0x2000) <= 0x8e {
        let mark = code.wrapping_sub(0x610) <= 5 || code.wrapping_sub(0x64b) <= 0x13 || code.wrapping_sub(0x6d6) <= 6 || code.wrapping_sub(0x6df) <= 5 || code.wrapping_sub(0x6e7) <= 1 || code == 0x670 || code.wrapping_sub(0x6ea) <= 3;
        (C::Arabic, if mark { K::ArabicMark } else if code == 0x6de { K::ArabicEnd } else if code == 0x640 { K::Joiner } else { K::Letter })
    } else if code.wrapping_sub(0x900) <= 0x7f || code.wrapping_add(0x1700) <= 0xc1 { (C::Devanagari, K::Letter) }
    else if code.wrapping_sub(0xb80) <= 0x7f || code.wrapping_add(0x1200) <= 0x53 { (C::Tamil, K::Letter) }
    else if code.wrapping_sub(0xe00) <= 0x7f { (C::Thai, K::Letter) }
    else if code.wrapping_sub(0x2000) < 0x200 { (C::Control, K::GeneralControl) }
    else if code.wrapping_add(0x2000) < 0x1900 { return private_character_class(CharacterCode(code)); }
    else if code == 0xfeff { (C::Unknown, K::None) }
    else { (C::Basic, K::Letter) };
    UnicodeClass { category, kind }
}

pub fn private_character_class(code: CharacterCode) -> UnicodeClass {
    use CharacterKind as K;
    use UnicodeCategory as C;
    let (category, kind) = match glyph_cell_width(code) { 1 => (C::Basic, K::Symbol), 2 => (C::Basic, K::Open), 3 => (C::Basic, K::Close), 4 => (C::Basic, K::Letter), 5 => (C::Arabic, K::Letter), _ => (C::Unknown, K::None) };
    UnicodeClass { category, kind }
}

impl UnicodeClass {
    pub(crate) fn weak(self) -> bool {
        matches!(self.category, UnicodeCategory::Unknown | UnicodeCategory::Space | UnicodeCategory::Control) ||
            self.category == UnicodeCategory::Basic && matches!(self.kind, CharacterKind::Symbol | CharacterKind::Currency | CharacterKind::Open | CharacterKind::Close)
    }
}

pub fn pack_count(count: u16, groups: u16) -> PackedCount {
    PackedCount(if groups > 1 { count.wrapping_add(groups.wrapping_shl(8)) } else { count })
}
pub fn unpack_count(packed: PackedCount) -> (u16, u16) {
    if packed.0 > 256 { (packed.0 & 255, packed.0 >> 8) } else { (packed.0, 1) }
}
pub fn detect_mode(text: &[u16]) -> Option<ScriptMode> {
    text.iter().map(|code| classify(CharacterCode(*code))).find(|class| !class.weak()).map(|class| ScriptMode::from_category(class.category))
}
pub fn detect_script_run(text: &[u16], start: CharacterIndex, base: DirectionHint) -> DetectedRun {
    let mut position = start.0;
    let mut selected = None;
    let mut direction = TextDirection(0);
    let mut nesting = 0u8;
    while (position as usize) < text.len() && position <= 49 {
        let class = classify(CharacterCode(text[position as usize]));
        if !class.weak() {
            let mode = ScriptMode::from_category(class.category);
            if let Some(current) = selected {
                if current != mode { break; }
            } else {
                selected = Some(mode);
                direction = mode.direction();
            }
        } else {
            let mut boundary = false;
            if selected.is_none() {
                let mode = if base.0 == 0x200f { ScriptMode::Arabic } else { ScriptMode::Basic };
                selected = Some(mode);
                direction = mode.direction();
            } else {
                let next = detect_mode(&text[position as usize..]);
                if selected != next {
                    boundary = direction.0 as u32 != base.0;
                    if boundary && nesting != 0 {
                        if class.kind == CharacterKind::Close { nesting = nesting.wrapping_sub(1); position = position.wrapping_add(1); continue; }
                        boundary = false;
                    }
                }
            }
            if class.kind == CharacterKind::Open && direction.0 as u32 != base.0 { nesting = nesting.wrapping_add(1); }
            if boundary { break; }
        }
        position = position.wrapping_add(1);
    }
    let mode = selected.unwrap_or(if base.0 == 0x200f { ScriptMode::Arabic } else { ScriptMode::Basic });
    DetectedRun { range: CharacterRange { start, end: CharacterIndex(position) }, identity: mode.identity(), direction: if direction.0 == 0 { mode.direction() } else { direction } }
}

pub fn mirror_character(code: CharacterCode) -> CharacterCode { mirror_code(code, &ScriptTables::firmware()) }

pub(crate) fn mirror_run(store: &mut impl ShapingStore, range: CharacterRange) -> Result<(), FontError> {
    for character in range.start.0..range.end.0 {
        let index = CharacterIndex(character);
        let code = store.mapping(index)?.glyph_code;
        if matches!(classify(code).kind, CharacterKind::Open | CharacterKind::Close) {
            let mirrored = mirror_character(code);
            store.mapping(index)?.glyph_code = mirrored;
            let glyph = store.mapping(index)?.glyph;
            store.set_glyph_code(glyph, mirrored)?;
        }
    }
    Ok(())
}

pub(crate) fn is_hebrew_mark(code: CharacterCode) -> bool {
    code.0.wrapping_sub(0x591) < 0x37 && !matches!(code.0, 0x5be | 0x5c6 | 0x5c0)
}

pub(crate) fn store_main_run(store: &mut impl ShapingStore, metrics: &mut impl ShapingMetrics) -> Result<(), FontError> {
    let index = store.main_run_index();
    let range = store.range();
    store.main_run(index)?.start = range.start;
    store.main_run(index)?.end = range.end;
    let identity = store.identity();
    store.main_run(index)?.script = identity.script;
    store.main_run(index)?.language = identity.language;
    let advance = store.advance();
    store.main_run(index)?.advance = advance;
    let height = metrics.height()?;
    store.main_run(index)?.height = height;
    Ok(())
}

pub(crate) fn measure_characters(store: &mut impl ShapingStore, metrics: &mut impl ShapingMetrics, script: ScriptMode) -> Result<(), FontError> {
    let first = store.range().start;
    let mut character = first;
    let mut advance = LayoutOffset(0);
    let mut previous = GlyphIndex(0);
    while character.0 < store.range().end.0 {
        let code = store.mapping(character)?.glyph_code;
        let width = metrics.advance(code)?;
        let glyph = store.mapping(character)?.glyph;
        let output = store.metric(character)?;
        output.glyph = glyph;
        output.direction = script.direction();
        output.character = character.0 as u32;
        output.pen_x = advance;
        output.y = LayoutOffset(0);
        output.pen_y = LayoutOffset(0);
        output.advance = width;
        if (character == first || glyph != previous) && (script != ScriptMode::Hebrew || !is_hebrew_mark(code)) { advance = advance.wrapping_add(width); }
        previous = glyph;
        character.0 = character.0.wrapping_add(1);
    }
    store.set_advance(advance);
    Ok(())
}

pub(crate) fn shape_basic(store: &mut impl ShapingStore, metrics: &mut impl ShapingMetrics) -> Result<ShapeWarning, FontError> {
    let mut character = store.range().start;
    while character.0 < store.range().end.0 {
        let code = store.text_code(character)?;
        let source = store.source_count();
        let glyph = store.glyph_count();
        store.set_source_code(source, code)?;
        store.set_glyph_code(glyph, code)?;
        let mapping = store.mapping(character)?;
        mapping.source = source;
        mapping.character = character;
        mapping.glyph = glyph;
        mapping.source_count = pack_count(1,1);
        mapping.original = code;
        mapping.glyph_code = code;
        mapping.source_code = code;
        mapping.glyph_count = pack_count(1,1);
        store.set_glyph_count(GlyphIndex(glyph.0.wrapping_add(1)));
        store.set_source_count(SourceIndex(source.0.wrapping_add(1)));
        character.0 = character.0.wrapping_add(1);
    }
    measure_characters(store, metrics, ScriptMode::Basic)?;
    store_main_run(store, metrics)?;
    Ok(ShapeWarning::None)
}

pub(crate) fn prepare_hebrew_characters(store: &mut impl ShapingStore) -> Result<(), FontError> {
    let first = store.range().start;
    let count = store.range().end.0.wrapping_sub(first.0);
    for offset in 0..count {
        let character = CharacterIndex(first.0.wrapping_add(offset));
        let original = store.text_code(character)?;
        let code = if is_hebrew_mark(original) { CharacterCode(0x25cc) } else { original };
        let source = store.source_count();
        let glyph = store.glyph_count();
        store.set_source_code(source, code)?;
        store.set_glyph_code(glyph, code)?;
        store.mapping(character)?.character = character;
        let original = store.text_code(character)?;
        let mapping = store.mapping(character)?;
        mapping.original = original;
        mapping.source_code = code;
        mapping.source = source;
        mapping.glyph = glyph;
        mapping.glyph_code = code;
        mapping.source_count = pack_count(1,1);
        mapping.glyph_count = pack_count(1,1);
        store.set_source_count(SourceIndex(source.0.wrapping_add(1)));
        store.set_glyph_count(GlyphIndex(glyph.0.wrapping_add(1)));
    }
    Ok(())
}

fn initial_pen(store: &impl ShapingStore, run: &ScriptRun) -> LayoutOffset {
    if store.direction() == TextDirection::RIGHT_TO_LEFT { run.x.wrapping_sub(run.advance) } else { run.x }
}

pub(crate) fn position_forward(store: &mut impl ShapingStore, metrics: &mut impl ShapingMetrics, run: &mut ScriptRun) -> Result<(), FontError> {
    let mut character = u32::from(run.start.0);
    let end = u32::from(run.end.0);
    let mut pen = initial_pen(store, run);
    while character < end {
        let mapping = *store.mapping(CharacterIndex(character as u16))?;
        let (count, groups) = unpack_count(mapping.glyph_count);
        let mut glyph = mapping.glyph;
        let width = metrics.advance(store.glyph_code(glyph)?)?;
        pen = pen.wrapping_add(width);
        for group in 0..groups {
            let code = store.glyph_code(glyph)?;
            store.set_output_code(glyph, code)?;
            let position = store.position(glyph)?;
            *position = GlyphPosition {
                character,
                glyph,
                count: if group == 0 { pack_count(count, groups) } else { PackedCount(0) },
                pen_x: pen,
                x: pen.wrapping_sub(width),
                advance: width,
                ..GlyphPosition::default()
            };
            glyph = GlyphIndex(glyph.0.wrapping_add(1));
        }
        character = character.wrapping_add(u32::from(count));
    }
    run.finish_x = pen;
    run.finish_y = LayoutOffset(0);
    Ok(())
}

pub(crate) fn position_reversed(store: &mut impl ShapingStore, metrics: &mut impl ShapingMetrics, run: &mut ScriptRun, script: ScriptMode) -> Result<(), FontError> {
    let first = store.mapping(run.start)?.glyph;
    let last = store.glyph_before(run.end)?;
    let mut character = u32::from(run.start.0);
    let end = u32::from(run.end.0);
    let mut original = last;
    let mut glyph = first;
    let mut count = 0u16;
    while character < end {
        let code = store.glyph_code(original)?;
        store.set_output_code(GlyphIndex(first.0.wrapping_add(count)), code)?;
        let packed = store.mapping(CharacterIndex(character as u16))?.glyph_count;
        let (characters, groups) = unpack_count(packed);
        let packed = if packed.0 > 256 && groups == 1 { PackedCount(characters) } else { packed };
        let position = store.position(original)?;
        position.character = character;
        position.glyph = glyph;
        position.count = packed;
        count = count.wrapping_add(1);
        glyph = GlyphIndex(glyph.0.wrapping_add(1));
        original = GlyphIndex(original.0.wrapping_sub(1));
        character = character.wrapping_add(u32::from(characters));
    }
    let mut pen = initial_pen(store, run);
    if first.0 <= last.0 {
        let mut index = u32::from(first.0);
        while index <= u32::from(last.0) {
            let glyph = GlyphIndex(index as u16);
            let code = store.output_code(glyph)?;
            let width = metrics.advance(code)?;
            let position = store.position(glyph)?;
            position.pen_x = pen;
            position.pen_y = LayoutOffset(0);
            position.x = pen;
            position.y = LayoutOffset(0);
            position.advance = width;
            position.reserved = 0;
            position.mark_x = LayoutOffset(0);
            position.mark_y = LayoutOffset(0);
            position.clipped = 0;
            position.clip_x = LayoutOffset(0);
            position.clip_y = LayoutOffset(0);
            position.clip_width = LayoutOffset(0);
            position.clip_height = LayoutOffset(0);
            if script != ScriptMode::Hebrew || !is_hebrew_mark(code) { pen = pen.wrapping_add(width); }
            index = index.wrapping_add(1);
        }
    }
    run.finish_x = pen;
    run.finish_y = LayoutOffset(0);
    Ok(())
}

index_type!(ThaiClassIndex, u32);

mod tables;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MirrorPair { pub open: CharacterCode, pub close: CharacterCode }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JoiningTransition { pub update: JoiningState, pub state: JoiningState }

/// Script tables. A lookup outside a table yields the unsubstituted default.
#[derive(Clone, Copy)]
pub struct ScriptTables<'a> {
    mirror_pairs: &'a [MirrorPair],
    arabic_classes: &'a [u8],
    arabic_categories: &'a [u8],
    arabic_transitions: &'a [JoiningTransition],
    thai_classes: &'a [u16],
    thai_transitions: &'a [[u8; 17]],
}

impl ScriptTables<'static> {
    pub const fn firmware() -> Self {
        Self::new(&tables::MIRROR_PAIRS, &tables::ARABIC_CLASSES, &tables::ARABIC_CATEGORIES, &tables::ARABIC_TRANSITIONS, &tables::THAI_CLASSES, &tables::THAI_TRANSITIONS)
    }
}

impl<'a> ScriptTables<'a> {
    pub const fn new(
        mirror_pairs: &'a [MirrorPair], arabic_classes: &'a [u8], arabic_categories: &'a [u8],
        arabic_transitions: &'a [JoiningTransition], thai_classes: &'a [u16], thai_transitions: &'a [[u8; 17]],
    ) -> Self {
        Self { mirror_pairs, arabic_classes, arabic_categories, arabic_transitions, thai_classes, thai_transitions }
    }
    pub fn mirror(&self, kind: CharacterKind, code: CharacterCode) -> Option<CharacterCode> {
        self.mirror_pairs.iter().find_map(|pair| match kind {
            CharacterKind::Open if pair.open == code => Some(pair.close),
            CharacterKind::Close if pair.close == code => Some(pair.open),
            _ => None,
        })
    }
    pub fn arabic_class(&self, code: CharacterCode) -> JoiningClass {
        JoiningClass(if code.0.wrapping_add(0xfa00) < 256 {
            self.arabic_classes.get(code.0.wrapping_sub(0x600) as usize).copied().map_or(0, u32::from)
        } else { u32::from(code.0 == 0x20) })
    }
    pub fn arabic_category(&self, class: JoiningClass) -> JoiningCategory {
        JoiningCategory(self.arabic_categories.get(class.0 as usize).copied().map_or(0, u32::from))
    }
    pub fn arabic_transition(&self, previous: JoiningState, category: JoiningCategory) -> JoiningTransition {
        let index = (previous.0 as usize).checked_mul(4).and_then(|row| row.checked_add(category.0 as usize));
        index.and_then(|index| self.arabic_transitions.get(index)).copied().unwrap_or_default()
    }
    pub fn thai_class(&self, code: CharacterCode) -> u16 {
        self.thai_classes.get(code.0.wrapping_sub(0xe00) as usize).copied().unwrap_or(1)
    }
    pub fn thai_transition(&self, previous: ThaiClassIndex, current: ThaiClassIndex) -> u8 {
        self.thai_transitions.get(previous.0 as usize).and_then(|row| row.get(current.0 as usize)).copied().unwrap_or(0)
    }
}

pub fn thai_class_index(class: u16) -> ThaiClassIndex {
    ThaiClassIndex(if class == 0 { 0 } else if class.is_power_of_two() { u32::from(class.trailing_zeros()) + 1 } else { 1 })
}

pub fn find_glyph(count: u16, table: &[u16], code: CharacterCode) -> Option<GlyphIndex> {
    let mut last = i32::from(count);
    let mut first = 1i32;
    if last <= 0 { return None; }
    let mut middle = last.wrapping_add(1) >> 1;
    loop {
        let candidate = *table.get(middle as usize)?;
        if code.0 < candidate { last = middle.wrapping_sub(1); }
        else {
            first = middle.wrapping_add(1);
            if code.0 == candidate { return Some(GlyphIndex(middle as u16)); }
        }
        if last < first { return None; }
        middle = last.wrapping_add(first) / 2;
    }
}

pub fn compose_arabic_pair(first: CharacterCode, second: CharacterCode) -> (CharacterCode, u16) {
    let combined = match (first.0, second.0) {
        (0x627, 0x653) => 0x622, (0x627, 0x654) => 0x623,
        (0x627, 0x655) => 0x625, (0x627, 0x674) => 0x675,
        (0x648, 0x654) => 0x624, (0x648, 0x674) => 0x676,
        (0x64a, 0x654) => 0x626, (0x64a, 0x674) => 0x678,
        (0x6c1, 0x654) => 0x6c2, (0x6d2, 0x654) => 0x6d3,
        (0x6d5, 0x654) => 0x6c0, (0x6c7, 0x674) => 0x677,
        _ => return (first, 1),
    };
    (CharacterCode(combined), 2)
}

pub fn mirror_code(code: CharacterCode, tables: &ScriptTables<'_>) -> CharacterCode {
    let kind = classify(code).kind;
    let fixed = match (kind, code.0) {
        (CharacterKind::Open, 0x5b) => Some(0x5d), (CharacterKind::Open, 0x7b) => Some(0x7d), (CharacterKind::Open, 0x28) => Some(0x29),
        (CharacterKind::Close, 0x5d) => Some(0x5b), (CharacterKind::Close, 0x7d) => Some(0x7b), (CharacterKind::Close, 0x29) => Some(0x28),
        _ => None,
    };
    if let Some(fixed) = fixed { return CharacterCode(fixed); }
    tables.mirror(kind, code).unwrap_or(code)
}

pub fn thai_combines(codes: &[CharacterCode], first: GlyphIndex, glyph: GlyphIndex, tables: &ScriptTables<'_>) -> bool {
    let glyph = glyph.0 as usize;
    let first = first.0 as usize;
    if glyph <= first || glyph >= codes.len() { return false; }
    let previous = codes[glyph - 1];
    let current = codes[glyph];
    let previous_class = thai_class_index(tables.thai_class(previous));
    let current_class = thai_class_index(tables.thai_class(current));
    if tables.thai_transition(previous_class, current_class) != b'C' { return false; }
    let previous_flags = tables.thai_class(previous);
    if previous_flags == 2 { return true; }
    if glyph < first + 2 || previous.0 == 0 { return false; }
    if tables.thai_class(codes[glyph - 2]) != 2 { return false; }
    if current.0 == 0 { return previous_flags & 0xffc0 != 0; }
    let current_flags = tables.thai_class(current);
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

pub fn thai_mark_offset(
    codes: &[CharacterCode], metrics: &ThaiMetrics, start: GlyphIndex, last: GlyphIndex, index: GlyphIndex,
    decomposed: bool, followed: bool, tables: &ScriptTables<'_>,
) -> GlyphOffset {
    let mut offset = GlyphOffset::default();
    let index = index.0 as usize;
    let start = start.0 as usize;
    if index >= codes.len() || index <= start { return offset; }
    let category = tables.thai_class(codes[index]);
    let previous = index - 1;
    let previous_category = tables.thai_class(codes[previous]);
    let base = if previous_category == 2 { Some(previous) }
        else if previous >= start + 1 && tables.thai_class(codes[index - 2]) == 2 { Some(index - 2) }
        else { None };
    let Some(base) = base else { return offset };
    let base_code = codes[base].0;
    if matches!(base_code, 0xe1b | 0xe1d | 0xe1f) && category & 0xbf00 != 0 {
        offset.x = LayoutOffset(-i32::from(metrics.left_shift.0));
    }
    if base == previous {
        if category & 0x8000 != 0 {
            offset.y = LayoutOffset(i32::from(metrics.rise.0));
            if (index as u16) < last.0 && followed { offset.y = LayoutOffset(0); }
        } else if category & 0x1800 != 0 { offset.y = LayoutOffset(i32::from(metrics.rise.0)); }
    } else if base + 2 == index {
        if previous_category & 0x40c0 != 0 { offset.y = LayoutOffset(i32::from(metrics.rise.0)); }
        if decomposed && codes[index].0 == 0xe4d && previous_category & 0x9800 != 0 { offset.y = LayoutOffset(i32::from(metrics.rise.0)); }
    }
    offset
}

/// Thai 0x0e33 decomposes after a base into the mark 0x0e4d followed by 0x0e32.
pub fn thai_decomposes(codes: &[CharacterCode], index: usize, tables: &ScriptTables<'_>) -> bool {
    if codes.get(index).map(|code| code.0) != Some(0xe33) || index == 0 { return false; }
    let category = tables.thai_class(codes[index - 1]);
    if category == 2 { return true; }
    category == 0x8000 && index >= 2 && tables.thai_class(codes[index - 2]) == 2
}

pub const MAX_SHAPED_GLYPHS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapedGlyph {
    pub character: CharacterIndex,
    pub code: CharacterCode,
    pub glyph: GlyphIndex,
    pub group: PackedCount,
    pub pen: LayoutOffset,
    pub advance: LayoutOffset,
    pub mark: GlyphOffset,
    pub direction: TextDirection,
}
impl ShapedGlyph {
    pub const EMPTY: Self = Self {
        character: CharacterIndex(0), code: CharacterCode(0), glyph: GlyphIndex(0), group: PackedCount(0),
        pen: LayoutOffset(0), advance: LayoutOffset(0),
        mark: GlyphOffset { x: LayoutOffset(0), y: LayoutOffset(0) }, direction: TextDirection(0),
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapedRun {
    glyphs: [ShapedGlyph; MAX_SHAPED_GLYPHS],
    count: u16,
    identity: ScriptIdentity,
    direction: TextDirection,
    advance: LayoutOffset,
    height: LayoutOffset,
    warning: ShapeWarning,
}
impl ShapedRun {
    pub fn empty(identity: ScriptIdentity, direction: TextDirection) -> Self {
        Self { glyphs: [ShapedGlyph::EMPTY; MAX_SHAPED_GLYPHS], count: 0, identity, direction, advance: LayoutOffset(0), height: LayoutOffset(0), warning: ShapeWarning::None }
    }
    pub fn glyphs(&self) -> &[ShapedGlyph] { &self.glyphs[..self.count as usize] }
    pub const fn count(&self) -> u16 { self.count }
    pub const fn identity(&self) -> ScriptIdentity { self.identity }
    pub const fn direction(&self) -> TextDirection { self.direction }
    pub const fn advance(&self) -> LayoutOffset { self.advance }
    pub const fn height(&self) -> LayoutOffset { self.height }
    pub const fn warning(&self) -> ShapeWarning { self.warning }
    fn push(&mut self, glyph: ShapedGlyph) -> Result<(), FontError> {
        let slot = self.glyphs.get_mut(self.count as usize).ok_or(FontError::InvalidSize)?;
        *slot = glyph;
        self.count += 1;
        Ok(())
    }
}

/// Shapes the leading detected run of `text` into visual-order glyphs with pen
/// origins relative to the run origin. Bounded by [`MAX_SHAPED_GLYPHS`]; iterate
/// with [`detect_script_run`] for mixed text.
pub fn shape_text(
    text: &[u16], base: DirectionHint, metrics: &mut impl ShapingMetrics, tables: &ScriptTables<'_>,
) -> Result<ShapedRun, FontError> {
    if text.len() > MAX_SHAPED_GLYPHS { return Err(FontError::InvalidSize); }
    let detected = detect_script_run(text, CharacterIndex(0), base);
    let mode = detected.identity.algorithm();
    let mut run = ShapedRun::empty(detected.identity, mode.direction());
    match mode {
        ScriptMode::Arabic => shape_arabic_text(text, metrics, tables, &mut run)?,
        ScriptMode::Hebrew => shape_hebrew_text(text, metrics, tables, &mut run)?,
        ScriptMode::Thai => shape_thai_text(text, metrics, tables, &mut run)?,
        _ => shape_basic_text(text, metrics, &mut run)?,
    }
    finalize_text(&mut run, metrics)?;
    Ok(run)
}

fn shape_basic_text(text: &[u16], metrics: &mut impl ShapingMetrics, run: &mut ShapedRun) -> Result<(), FontError> {
    let mut pen = LayoutOffset(0);
    for (index, code) in text.iter().enumerate() {
        let code = CharacterCode(*code);
        let advance = metrics.advance(code)?;
        run.push(ShapedGlyph {
            character: CharacterIndex(index as u16), code, glyph: GlyphIndex(code.0), group: pack_count(1, 1),
            pen, advance, mark: GlyphOffset::default(), direction: run.direction,
        })?;
        pen = pen.wrapping_add(advance);
    }
    run.advance = pen;
    Ok(())
}

fn shape_hebrew_text(text: &[u16], metrics: &mut impl ShapingMetrics, tables: &ScriptTables<'_>, run: &mut ShapedRun) -> Result<(), FontError> {
    let mut codes = [CharacterCode(0); MAX_SHAPED_GLYPHS];
    let mut characters = [CharacterIndex(0); MAX_SHAPED_GLYPHS];
    for (index, code) in text.iter().enumerate() {
        let original = CharacterCode(*code);
        let prepared = if is_hebrew_mark(original) { CharacterCode(0x25cc) } else { original };
        codes[index] = mirror_code(prepared, tables);
        characters[index] = CharacterIndex(index as u16);
    }
    place_reversed(&codes[..text.len()], &characters[..text.len()], metrics, run)
}

fn shape_arabic_text(text: &[u16], metrics: &mut impl ShapingMetrics, tables: &ScriptTables<'_>, run: &mut ShapedRun) -> Result<(), FontError> {
    let mut codes = [CharacterCode(0); MAX_SHAPED_GLYPHS];
    let mut characters = [CharacterIndex(0); MAX_SHAPED_GLYPHS];
    let mut count = 0usize;
    let mut character = 0usize;
    while character < text.len() {
        let second = text.get(character + 1).copied().unwrap_or(0);
        let (code, consumed) = compose_arabic_pair(CharacterCode(text[character]), CharacterCode(second));
        codes[count] = mirror_code(code, tables);
        characters[count] = CharacterIndex(character as u16);
        count += 1;
        character += consumed as usize;
    }
    place_reversed(&codes[..count], &characters[..count], metrics, run)
}

fn place_reversed(codes: &[CharacterCode], characters: &[CharacterIndex], metrics: &mut impl ShapingMetrics, run: &mut ShapedRun) -> Result<(), FontError> {
    let mut pen = LayoutOffset(0);
    for visual in 0..codes.len() {
        let logical = codes.len() - 1 - visual;
        let code = codes[logical];
        let advance = metrics.advance(code)?;
        run.push(ShapedGlyph {
            character: characters[logical], code, glyph: GlyphIndex(code.0), group: pack_count(1, 1),
            pen, advance, mark: GlyphOffset::default(), direction: run.direction,
        })?;
        pen = pen.wrapping_add(advance);
    }
    run.advance = pen;
    Ok(())
}

fn shape_thai_text(text: &[u16], metrics: &mut impl ShapingMetrics, tables: &ScriptTables<'_>, run: &mut ShapedRun) -> Result<(), FontError> {
    let mut source = [CharacterCode(0); MAX_SHAPED_GLYPHS];
    let mut codes = [CharacterCode(0); MAX_SHAPED_GLYPHS];
    let mut clusters = [CharacterCode(0); MAX_SHAPED_GLYPHS];
    let mut characters = [CharacterIndex(0); MAX_SHAPED_GLYPHS];
    let mut groups = [1u16; MAX_SHAPED_GLYPHS];
    let mut decomposed = [false; MAX_SHAPED_GLYPHS];
    let mut count = 0usize;
    for (index, code) in text.iter().enumerate() {
        source[index] = CharacterCode(*code);
        if thai_decomposes(&source[..=index], index, tables) {
            codes[count] = CharacterCode(0xe4d); clusters[count] = CharacterCode(0xe32);
            characters[count] = CharacterIndex(index as u16); groups[count] = 2; decomposed[count] = true; count += 1;
            codes[count] = CharacterCode(0xe32); clusters[count] = CharacterCode(0xe32);
            characters[count] = CharacterIndex(index as u16); groups[count] = 2; count += 1;
        } else {
            codes[count] = CharacterCode(*code); clusters[count] = CharacterCode(*code);
            characters[count] = CharacterIndex(index as u16); count += 1;
        }
    }
    let mut thai = ThaiMetrics::default();
    metrics.thai_metrics(&mut thai)?;
    let last = GlyphIndex(count.saturating_sub(1) as u16);
    let mut pen = LayoutOffset(0);
    for glyph in 0..count {
        let advance = metrics.advance(clusters[glyph])?;
        let combined = thai_combines(&codes[..count], GlyphIndex(0), GlyphIndex(glyph as u16), tables);
        let (origin, mark) = if combined || decomposed[glyph] {
            let mark_advance = metrics.advance(codes[glyph])?;
            let followed = groups.get(glyph + 1).map(|group| *group == 2).unwrap_or(false);
            let offset = thai_mark_offset(&codes[..count], &thai, GlyphIndex(0), last, GlyphIndex(glyph as u16), decomposed[glyph], followed, tables);
            (LayoutOffset(pen.0.wrapping_add(offset.x.0).wrapping_sub(mark_advance.0)), offset)
        } else {
            let origin = pen;
            pen = pen.wrapping_add(advance);
            (origin, GlyphOffset::default())
        };
        run.push(ShapedGlyph {
            character: characters[glyph], code: codes[glyph], glyph: GlyphIndex(codes[glyph].0), group: pack_count(1, groups[glyph]),
            pen: origin, advance, mark, direction: run.direction,
        })?;
    }
    run.advance = pen;
    Ok(())
}

fn finalize_text(run: &mut ShapedRun, metrics: &mut impl ShapingMetrics) -> Result<(), FontError> {
    run.height = metrics.height()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec;
    use std::vec::Vec;
    use super::*;

    struct Metrics { width: i32 }
    impl ShapingMetrics for Metrics {
        fn advance(&mut self, _code: CharacterCode) -> Result<LayoutOffset, FontError> { Ok(LayoutOffset(self.width)) }
        fn height(&mut self) -> Result<LayoutOffset, FontError> { Ok(LayoutOffset(10)) }
        fn right_overhang(&mut self, _code: CharacterCode) -> Result<LayoutOffset, FontError> { Ok(LayoutOffset(0)) }
        fn stroke_padding_top(&mut self, _padding: &mut LayoutOffset) -> Result<(), FontError> { Ok(()) }
        fn thai_metrics(&mut self, _output: &mut ThaiMetrics) -> Result<(), FontError> { Ok(()) }
    }

    fn shape(text: &[u16]) -> ShapedRun {
        let mut metrics = Metrics { width: 4 };
        shape_text(text, DirectionHint(0x200e), &mut metrics, &ScriptTables::firmware()).unwrap()
    }

    struct MemoryStore {
        text: [u16; 50],
        sources: [u16; 50],
        glyphs: [u16; 50],
        mappings: [CharacterMapping; 50],
        metrics: [CharacterMetric; 50],
        runs: [ScriptRun; 50],
        positions: [GlyphPosition; 50],
        outputs: [u16; 50],
        range: CharacterRange,
        identity: ScriptIdentity,
        direction: TextDirection,
        run_index: RunIndex,
        source_count: SourceIndex,
        glyph_count: GlyphIndex,
        advance: LayoutOffset,
    }

    impl MemoryStore {
        fn new(text: &[u16]) -> Self {
            let mut store = Self {
                text: [0; 50],
                sources: [0; 50],
                glyphs: [0; 50],
                mappings: [CharacterMapping::default(); 50],
                metrics: [CharacterMetric::default(); 50],
                runs: [ScriptRun::default(); 50],
                positions: [GlyphPosition::default(); 50],
                outputs: [0; 50],
                range: CharacterRange { start: CharacterIndex(0), end: CharacterIndex(text.len() as u16) },
                identity: ScriptIdentity::default(),
                direction: TextDirection::LEFT_TO_RIGHT,
                run_index: RunIndex(0),
                source_count: SourceIndex(0),
                glyph_count: GlyphIndex(0),
                advance: LayoutOffset(0),
            };
            store.text[..text.len()].copy_from_slice(text);
            store
        }

        fn slot(index: u16) -> Result<usize, FontError> {
            if index < 50 { Ok(index as usize) } else { Err(FontError::Truncated) }
        }
    }

    impl ShapingStore for MemoryStore {
        fn text_code(&self, character: CharacterIndex) -> Result<CharacterCode, FontError> {
            Ok(CharacterCode(*self.text.get(character.0 as usize).ok_or(FontError::Truncated)?))
        }
        fn source_code(&self, index: SourceIndex) -> Result<CharacterCode, FontError> { Ok(CharacterCode(self.sources[Self::slot(index.0)?])) }
        fn set_source_code(&mut self, index: SourceIndex, code: CharacterCode) -> Result<(), FontError> { self.sources[Self::slot(index.0)?] = code.0; Ok(()) }
        fn glyph_code(&self, index: GlyphIndex) -> Result<CharacterCode, FontError> { Ok(CharacterCode(self.glyphs[Self::slot(index.0)?])) }
        fn set_glyph_code(&mut self, index: GlyphIndex, code: CharacterCode) -> Result<(), FontError> { self.glyphs[Self::slot(index.0)?] = code.0; Ok(()) }
        fn mapping(&mut self, character: CharacterIndex) -> Result<&mut CharacterMapping, FontError> { let index = Self::slot(character.0)?; Ok(&mut self.mappings[index]) }
        fn metric(&mut self, character: CharacterIndex) -> Result<&mut CharacterMetric, FontError> { let index = Self::slot(character.0)?; Ok(&mut self.metrics[index]) }
        fn main_run(&mut self, index: RunIndex) -> Result<&mut ScriptRun, FontError> { Ok(&mut self.runs[index.0 as usize]) }
        fn position(&mut self, index: GlyphIndex) -> Result<&mut GlyphPosition, FontError> { let index = Self::slot(index.0)?; Ok(&mut self.positions[index]) }
        fn output_code(&self, index: GlyphIndex) -> Result<CharacterCode, FontError> { Ok(CharacterCode(self.outputs[Self::slot(index.0)?])) }
        fn set_output_code(&mut self, index: GlyphIndex, code: CharacterCode) -> Result<(), FontError> { self.outputs[Self::slot(index.0)?] = code.0; Ok(()) }
        fn glyph_span(&mut self, range: CharacterRange) -> Result<Option<GlyphSpan>, FontError> {
            let first = self.mapping(range.start)?.glyph;
            let last = self.glyph_before(range.end)?;
            Ok(if first.0 > last.0 { None } else { Some(GlyphSpan { first, last }) })
        }
        fn glyph_before(&mut self, end: CharacterIndex) -> Result<GlyphIndex, FontError> {
            Ok(self.mapping(CharacterIndex(end.0.checked_sub(1).ok_or(FontError::Truncated)?))?.glyph)
        }
        fn range(&self) -> CharacterRange { self.range }
        fn identity(&self) -> ScriptIdentity { self.identity }
        fn direction(&self) -> TextDirection { self.direction }
        fn main_run_index(&self) -> RunIndex { self.run_index }
        fn source_count(&self) -> SourceIndex { self.source_count }
        fn set_source_count(&mut self, count: SourceIndex) { self.source_count = count; }
        fn glyph_count(&self) -> GlyphIndex { self.glyph_count }
        fn set_glyph_count(&mut self, count: GlyphIndex) { self.glyph_count = count; }
        fn set_advance(&mut self, advance: LayoutOffset) { self.advance = advance; }
        fn advance(&self) -> LayoutOffset { self.advance }
    }

    #[test]
    fn basic_shaping_fills_mappings_metrics_and_the_main_run() {
        let mut store = MemoryStore::new(&[0x41, 0x42]);
        let mut metrics = Metrics { width: 4 };
        assert_eq!(shape_basic(&mut store, &mut metrics), Ok(ShapeWarning::None));
        assert_eq!(store.sources[0], 0x41);
        assert_eq!(store.mappings[0].glyph, GlyphIndex(0));
        assert_eq!(store.mappings[1].glyph, GlyphIndex(1));
        assert_eq!(store.metrics[0].pen_x, LayoutOffset(0));
        assert_eq!(store.metrics[1].pen_x, LayoutOffset(4));
        assert_eq!(store.runs[0].range(), CharacterRange { start: CharacterIndex(0), end: CharacterIndex(2) });
        assert_eq!(store.runs[0].advance, LayoutOffset(8));
        assert_eq!(store.runs[0].height, LayoutOffset(10));
    }

    #[test]
    fn forward_positions_track_the_pen_after_each_glyph() {
        let mut store = MemoryStore::new(&[0x41, 0x42]);
        let mut metrics = Metrics { width: 4 };
        shape_basic(&mut store, &mut metrics).unwrap();
        store.runs[0].x = LayoutOffset(100);
        let mut run = store.runs[0];
        position_forward(&mut store, &mut metrics, &mut run).unwrap();
        assert_eq!(store.outputs[0], 0x41);
        assert_eq!(store.outputs[1], 0x42);
        assert_eq!(store.positions[0].pen_x, LayoutOffset(104));
        assert_eq!(store.positions[0].x, LayoutOffset(100));
        assert_eq!(store.positions[1].pen_x, LayoutOffset(108));
        assert_eq!(store.positions[1].x, LayoutOffset(104));
        assert_eq!(run.finish_x, LayoutOffset(108));
        assert_eq!(run.finish_y, LayoutOffset(0));
    }

    #[test]
    fn hebrew_mark_does_not_advance_the_reversed_pen() {
        let mut store = MemoryStore::new(&[0x5d0, 0x5b0]);
        store.direction = TextDirection::RIGHT_TO_LEFT;
        store.glyphs[0] = 0x5d0;
        store.glyphs[1] = 0x5b0;
        for index in 0..2 {
            store.mappings[index].glyph = GlyphIndex(index as u16);
            store.mappings[index].glyph_count = pack_count(1, 1);
        }
        let mut metrics = Metrics { width: 4 };
        let mut run = ScriptRun { start: CharacterIndex(0), end: CharacterIndex(2), ..ScriptRun::default() };
        position_reversed(&mut store, &mut metrics, &mut run, ScriptMode::Hebrew).unwrap();
        assert_eq!(store.outputs[0], 0x5b0);
        assert_eq!(store.outputs[1], 0x5d0);
        assert_eq!(store.positions[0].pen_x, LayoutOffset(0));
        assert_eq!(store.positions[0].advance, LayoutOffset(4));
        assert_eq!(store.positions[1].pen_x, LayoutOffset(0));
        assert_eq!(run.finish_x, LayoutOffset(4));
    }

    #[test]
    fn mirroring_swaps_a_bracket_mapping_and_glyph_code() {
        let mut store = MemoryStore::new(&[0x28]);
        store.mappings[0].glyph = GlyphIndex(0);
        store.mappings[0].glyph_code = CharacterCode(0x28);
        store.glyphs[0] = 0x28;
        mirror_run(&mut store, CharacterRange { start: CharacterIndex(0), end: CharacterIndex(1) }).unwrap();
        assert_eq!(store.mappings[0].glyph_code, CharacterCode(0x29));
        assert_eq!(store.glyphs[0], 0x29);
    }

    #[test]
    fn latin_run_places_each_glyph_at_the_accumulated_pen() {
        let run = shape(&[0x41, 0x42]);
        assert_eq!(run.glyphs().iter().map(|glyph| glyph.pen.0).collect::<Vec<_>>(), vec![0, 4]);
        assert_eq!(run.glyphs().iter().map(|glyph| glyph.code.0).collect::<Vec<_>>(), vec![0x41, 0x42]);
        assert_eq!(run.advance().0, 8);
        assert_eq!(run.direction().0, 0x200e);
        assert_eq!(run.height().0, 10);
    }

    #[test]
    fn arabic_run_publishes_visual_order_with_composed_hamza() {
        let run = shape(&[0x627, 0x653]);
        assert_eq!(run.count(), 1);
        assert_eq!(run.glyphs()[0].code.0, 0x622);
        assert_eq!(run.direction().0, 0x200f);
        let reversed = shape(&[0x644, 0x627]);
        assert_eq!(reversed.glyphs().iter().map(|glyph| glyph.code.0).collect::<Vec<_>>(), vec![0x627, 0x644]);
        assert_eq!(reversed.glyphs().iter().map(|glyph| glyph.pen.0).collect::<Vec<_>>(), vec![0, 4]);
    }

    #[test]
    fn hebrew_mark_renders_as_dotted_circle_in_visual_order() {
        let run = shape(&[0x5d0, 0x5b0]);
        assert_eq!(run.glyphs().iter().map(|glyph| glyph.code.0).collect::<Vec<_>>(), vec![0x25cc, 0x5d0]);
    }

    #[test]
    fn bidi_detection_stops_the_run_at_a_script_boundary() {
        let detected = detect_script_run(&[0x41, 0x5d0], CharacterIndex(0), DirectionHint(0x200e));
        assert_eq!(detected.range.end.0, 1);
        assert_eq!(detected.identity.algorithm(), ScriptMode::Basic);
    }

    #[test]
    fn rtl_run_mirrors_brackets_in_both_directions() {
        let tables = ScriptTables::firmware();
        for (open, close) in [(0x28, 0x29), (0x5b, 0x5d), (0x7b, 0x7d), (0xe128, 0xe129), (0xe15b, 0xe15d), (0xe17b, 0xe17d)] {
            assert_eq!(mirror_code(CharacterCode(open), &tables).0, close);
            assert_eq!(mirror_code(CharacterCode(close), &tables).0, open);
        }
        assert_eq!(mirror_code(CharacterCode(0x41), &tables).0, 0x41);
    }

    #[test]
    fn arabic_joining_follows_the_class_category_and_state_rows() {
        let tables = ScriptTables::firmware();
        let beh = tables.arabic_class(CharacterCode(0x628));
        assert_eq!(tables.arabic_category(beh), JoiningCategory(2));
        assert_eq!(tables.arabic_category(tables.arabic_class(CharacterCode(0x627))), JoiningCategory(3));
        assert_eq!(tables.arabic_category(tables.arabic_class(CharacterCode(0x64b))), JoiningCategory(4));
        assert_eq!(tables.arabic_class(CharacterCode(0x20)), JoiningClass(1));
        assert_eq!(tables.arabic_class(CharacterCode(0x41)), JoiningClass(0));
        let after_dual = tables.arabic_transition(JoiningState(2), JoiningCategory(2));
        assert_eq!((after_dual.update, after_dual.state), (JoiningState(2), JoiningState(3)));
        let after_right = tables.arabic_transition(JoiningState(2), JoiningCategory(3));
        assert_eq!((after_right.update, after_right.state), (JoiningState(2), JoiningState(1)));
    }

    #[test]
    fn thai_tone_combines_only_after_an_upper_vowel() {
        let tables = ScriptTables::firmware();
        let codes = [CharacterCode(0xe01), CharacterCode(0xe34), CharacterCode(0xe48)];
        assert!(thai_combines(&codes, GlyphIndex(0), GlyphIndex(1), &tables));
        assert!(thai_combines(&codes, GlyphIndex(0), GlyphIndex(2), &tables));
        let latin = [CharacterCode(0x41), CharacterCode(0xe48)];
        assert!(!thai_combines(&latin, GlyphIndex(0), GlyphIndex(1), &tables));
    }

    #[test]
    fn thai_sara_am_decomposes_after_a_base_character() {
        let mut metrics = Metrics { width: 4 };
        let run = shape_text(&[0xe01, 0xe33], DirectionHint(0x200e), &mut metrics, &ScriptTables::firmware()).unwrap();
        assert_eq!(run.glyphs().iter().map(|glyph| glyph.code.0).collect::<Vec<_>>(), vec![0xe01, 0xe4d, 0xe32]);
    }

    #[test]
    fn oversized_text_is_refused_rather_than_truncated() {
        let mut metrics = Metrics { width: 4 };
        let text = [0x41u16; MAX_SHAPED_GLYPHS + 1];
        assert_eq!(shape_text(&text, DirectionHint(0x200e), &mut metrics, &ScriptTables::firmware()), Err(FontError::InvalidSize));
    }
}
