//! Expected codes come from the owned X-M1 1.50 tables transcribed in
//! `develop/src/font_core/shaping/tables.rs`.
extern crate std;
use std::vec;
use std::vec::Vec;

use super::font_core::shaping::{
    shape_text, CharacterCode, DirectionHint, GlyphOffset, JoiningCategory, JoiningClass,
    JoiningState, LayoutExtent16, LayoutOffset, ScriptTables, ShapingMetrics, ThaiMetrics,
};

struct Metrics {
    thai: ThaiMetrics,
}
impl ShapingMetrics for Metrics {
    fn advance(&mut self, _code: CharacterCode) -> Result<LayoutOffset, super::font_core::FontError> {
        Ok(LayoutOffset(4))
    }
    fn height(&mut self) -> Result<LayoutOffset, super::font_core::FontError> { Ok(LayoutOffset(10)) }
    fn right_overhang(&mut self, _code: CharacterCode) -> Result<LayoutOffset, super::font_core::FontError> { Ok(LayoutOffset(0)) }
    fn stroke_padding_top(&mut self, _padding: &mut LayoutOffset) -> Result<(), super::font_core::FontError> { Ok(()) }
    fn thai_metrics(&mut self, output: &mut ThaiMetrics) -> Result<(), super::font_core::FontError> {
        *output = self.thai;
        Ok(())
    }
}

fn codes(text: &[u16]) -> Vec<u16> {
    let mut metrics = Metrics { thai: ThaiMetrics::default() };
    shape_text(text, DirectionHint(0x200e), &mut metrics, &ScriptTables::firmware())
        .unwrap()
        .glyphs()
        .iter()
        .map(|glyph| glyph.code.0)
        .collect()
}

fn forms(text: &[u16]) -> Vec<(u16, u32)> {
    let mut metrics = Metrics { thai: ThaiMetrics::default() };
    shape_text(text, DirectionHint(0x200e), &mut metrics, &ScriptTables::firmware())
        .unwrap()
        .glyphs()
        .iter()
        .map(|glyph| (glyph.code.0, glyph.joining.0))
        .collect()
}

#[test]
fn arabic_joining_run_follows_the_owned_class_category_and_transition_rows() {
    // ARABIC_CLASSES at 0x011887d8, ARABIC_CATEGORIES at 0x011888d8 and
    // ARABIC_TRANSITIONS at 0x01188954.
    let tables = ScriptTables::firmware();
    let beh = tables.arabic_class(CharacterCode(0x628));
    let alef = tables.arabic_class(CharacterCode(0x627));
    assert_eq!(beh, JoiningClass(5));
    assert_eq!(alef, JoiningClass(0x17));
    assert_eq!(tables.arabic_category(beh), JoiningCategory(2));
    assert_eq!(tables.arabic_category(alef), JoiningCategory(3));
    let dual_after_dual = tables.arabic_transition(JoiningState(2), JoiningCategory(2));
    let right_after_dual = tables.arabic_transition(JoiningState(2), JoiningCategory(3));
    assert_eq!((dual_after_dual.update, dual_after_dual.state), (JoiningState(2), JoiningState(3)));
    assert_eq!((right_after_dual.update, right_after_dual.state), (JoiningState(2), JoiningState(1)));
    assert_eq!(codes(&[0x628, 0x627]), vec![0x627, 0x628]);
    assert_eq!(forms(&[0x628]), vec![(0x628, 0)]);
    assert_eq!(forms(&[0x628, 0x627]), vec![(0x627, 1), (0x628, 2)]);
    assert_eq!(forms(&[0x628, 0x627, 0x628]), vec![(0x628, 0), (0x627, 1), (0x628, 2)]);
    assert_eq!(forms(&[0x644, 0x627]), vec![(0x627, 1), (0x644, 2)]);
    assert_eq!(forms(&[0x628, 0x627, 0x41]), vec![(0x627, 1), (0x628, 2)]);
    assert_eq!(forms(&[0x628, 0x628, 0x628]), vec![(0x628, 1), (0x628, 3), (0x628, 2)]);
    assert_eq!(forms(&[0x627, 0x653]), vec![(0x622, 0)]);
}

#[test]
fn thai_mark_placement_follows_the_owned_class_and_transition_rows() {
    // THAI_CLASSES at 0x0118a98c gives U+0E01 class 0x0002 and U+0E48 class
    // 0x8000; THAI_TRANSITIONS at 0x0118aa8c has 'C' at previous class index 2,
    // current class index 16.
    let mut metrics = Metrics { thai: ThaiMetrics { rise: LayoutExtent16(5), ..ThaiMetrics::default() } };
    let run = shape_text(&[0x0e01, 0x0e48], DirectionHint(0x200e), &mut metrics, &ScriptTables::firmware()).unwrap();
    let glyphs = run.glyphs();
    assert_eq!(glyphs.iter().map(|glyph| glyph.code.0).collect::<Vec<_>>(), vec![0x0e01, 0x0e48]);
    assert_eq!(glyphs[1].pen, LayoutOffset(0));
    assert_eq!(glyphs[1].mark, GlyphOffset { x: LayoutOffset(0), y: LayoutOffset(5) });
    assert_eq!(run.advance(), LayoutOffset(4));
}
