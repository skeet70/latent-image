use super::font_fdl::{BackendMetric, FontEngine, GlyphCode};
use super::shared::{Address32, ByteOffset};
use crate::develop::font_core::shaping::{
    CharacterCode, CharacterIndex, CharacterMapping, CharacterMetric, CharacterRange, GlyphIndex, GlyphPosition,
    LanguageId, LayoutOffset, RunIndex, ScriptId, ScriptIdentity, ScriptRun, ShapingMetrics, ShapingStore,
    SourceIndex, TextDirection, ThaiMetrics,
};
use crate::develop::font_core::FontError;

const RECORD_CAPACITY: u16 = 50;

fn record_index(index: u16) -> Result<u32, FontError> {
    if index < RECORD_CAPACITY { Ok(u32::from(index)) } else { Err(FontError::Truncated) }
}

/// Guest entries discard the store's `Err`: an index at or beyond the 50-record
/// arrays stops the routine, where the original code wrote past them.
pub(super) struct GuestStore {
    engine: Address32,
}

impl GuestStore {
    /// # Safety
    /// The engine context and the positioned output record it points at must be
    /// live, aligned guest records for the store's lifetime.
    pub(super) unsafe fn new(engine: Address32) -> Self { Self { engine } }

    /// # Safety
    /// `offset` must address four readable bytes of the engine context.
    unsafe fn word(&self, offset: u32) -> u32 { unsafe { self.engine.offset(ByteOffset(offset)).word() } }
    /// # Safety
    /// `offset` must address two readable bytes of the engine context.
    unsafe fn half(&self, offset: u32) -> u16 { unsafe { self.engine.offset(ByteOffset(offset)).half() } }
    /// # Safety
    /// `offset` must address two writable bytes of the engine context.
    unsafe fn put_half(&self, offset: u32, value: u16) { unsafe { self.engine.offset(ByteOffset(offset)).write_half(value) } }
    /// # Safety
    /// `offset` must address four writable bytes of the engine context.
    unsafe fn put_word(&self, offset: u32, value: u32) { unsafe { self.engine.offset(ByteOffset(offset)).write_word(value) } }
    /// # Safety
    /// The engine's output word at 0x53c must hold a live positioned record.
    unsafe fn output(&self) -> Address32 { Address32(unsafe { self.word(0x53c) }) }
    /// # Safety
    /// `address` must point to a live, aligned `T` that outlives the borrow.
    unsafe fn record<T>(&mut self, address: Address32) -> &mut T { unsafe { &mut *(address.0 as *mut T) } }
}

impl ShapingStore for GuestStore {
    fn text_code(&self, character: CharacterIndex) -> Result<CharacterCode, FontError> {
        let request = Address32(unsafe { self.word(0x10) });
        let text = Address32(unsafe { request.offset(ByteOffset(16)).word() });
        Ok(CharacterCode(unsafe { text.offset(ByteOffset(u32::from(character.0) * 2)).half() }))
    }

    fn set_source_code(&mut self, index: SourceIndex, code: CharacterCode) -> Result<(), FontError> {
        let index = record_index(index.0)?;
        unsafe { self.put_half(0x540 + index * 2, code.0) };
        Ok(())
    }

    fn glyph_code(&self, index: GlyphIndex) -> Result<CharacterCode, FontError> {
        let index = record_index(index.0)?;
        Ok(CharacterCode(unsafe { self.half(0x5a6 + index * 2) }))
    }

    fn set_glyph_code(&mut self, index: GlyphIndex, code: CharacterCode) -> Result<(), FontError> {
        let index = record_index(index.0)?;
        unsafe { self.put_half(0x5a6 + index * 2, code.0) };
        Ok(())
    }

    fn mapping(&mut self, character: CharacterIndex) -> Result<&mut CharacterMapping, FontError> {
        let character = record_index(character.0)?;
        Ok(unsafe { self.record(self.engine.offset(ByteOffset(0x60c + character * 16))) })
    }

    fn metric(&mut self, character: CharacterIndex) -> Result<&mut CharacterMetric, FontError> {
        let character = record_index(character.0)?;
        Ok(unsafe { self.record(self.engine.offset(ByteOffset(0x92c + character * 28))) })
    }

    fn main_run(&mut self, index: RunIndex) -> Result<&mut ScriptRun, FontError> {
        let index = record_index(index.0)?;
        Ok(unsafe { self.record(self.engine.offset(ByteOffset(0xea8 + index * 36))) })
    }

    fn position(&mut self, index: GlyphIndex) -> Result<&mut GlyphPosition, FontError> {
        let index = record_index(index.0)?;
        let output = unsafe { self.output() };
        Ok(unsafe { self.record(output.offset(ByteOffset(0x68 + index * 60))) })
    }

    fn output_code(&self, index: GlyphIndex) -> Result<CharacterCode, FontError> {
        let index = record_index(index.0)?;
        let output = unsafe { self.output() };
        Ok(CharacterCode(unsafe { output.offset(ByteOffset(4 + index * 2)).half() }))
    }

    fn set_output_code(&mut self, index: GlyphIndex, code: CharacterCode) -> Result<(), FontError> {
        let index = record_index(index.0)?;
        let output = unsafe { self.output() };
        unsafe { output.offset(ByteOffset(4 + index * 2)).write_half(code.0) };
        Ok(())
    }

    /// For `end` 0 this is glyph code 48, the halfword before the first mapping.
    fn glyph_before(&mut self, end: CharacterIndex) -> Result<GlyphIndex, FontError> {
        if end.0 > RECORD_CAPACITY { return Err(FontError::Truncated); }
        Ok(GlyphIndex(unsafe { self.half(0x606 + u32::from(end.0) * 16) }))
    }

    fn range(&self) -> CharacterRange {
        CharacterRange {
            start: CharacterIndex(unsafe { self.half(0x524) }),
            end: CharacterIndex(unsafe { self.half(0x526) }),
        }
    }

    fn identity(&self) -> ScriptIdentity {
        ScriptIdentity {
            script: ScriptId(unsafe { self.word(0x52c) }),
            language: LanguageId(unsafe { self.word(0x530) }),
        }
    }

    fn direction(&self) -> TextDirection {
        TextDirection(unsafe { self.output().offset(ByteOffset(0xc20)).half() })
    }

    fn main_run_index(&self) -> RunIndex { RunIndex(unsafe { self.half(0xea4) }) }
    fn source_count(&self) -> SourceIndex { SourceIndex(unsafe { self.half(0x5a4) }) }
    fn set_source_count(&mut self, count: SourceIndex) { unsafe { self.put_half(0x5a4, count.0) } }
    fn glyph_count(&self) -> GlyphIndex { GlyphIndex(unsafe { self.half(0x60a) }) }
    fn set_glyph_count(&mut self, count: GlyphIndex) { unsafe { self.put_half(0x60a, count.0) } }
    fn set_advance(&mut self, advance: LayoutOffset) { unsafe { self.put_word(0x534, advance.bits()) } }
    fn advance(&self) -> LayoutOffset { LayoutOffset(unsafe { self.word(0x534) } as i32) }
}

pub(super) struct GuestMetrics {
    engine: Address32,
}

impl GuestMetrics {
    /// # Safety
    /// `engine` must be a live font engine context for the metrics' lifetime.
    pub(super) unsafe fn new(engine: Address32) -> Self { Self { engine } }
    fn engine(&self) -> FontEngine { FontEngine(self.engine.0) }
}

impl ShapingMetrics for GuestMetrics {
    fn advance(&mut self, code: CharacterCode) -> Result<LayoutOffset, FontError> {
        Ok(LayoutOffset(unsafe { self.engine().glyph_advance(GlyphCode(u32::from(code.0))) }))
    }

    fn height(&mut self) -> Result<LayoutOffset, FontError> {
        Ok(LayoutOffset(unsafe { self.engine().metric(BackendMetric::Height, GlyphCode(0)) }))
    }

    fn right_overhang(&mut self, code: CharacterCode) -> Result<LayoutOffset, FontError> {
        Ok(LayoutOffset(unsafe { self.engine().metric(BackendMetric::RightOverhang, GlyphCode(u32::from(code.0))) }))
    }

    fn stroke_padding_top(&mut self, padding: &mut LayoutOffset) -> Result<(), FontError> {
        unsafe { self.engine().stroke_padding(Address32(0), Address32((padding as *mut LayoutOffset) as u32), Address32(0), Address32(0)) };
        Ok(())
    }

    fn thai_metrics(&mut self, output: &mut ThaiMetrics) -> Result<(), FontError> {
        unsafe { self.engine().thai_metrics(Address32((output as *mut ThaiMetrics) as u32)) };
        Ok(())
    }
}

