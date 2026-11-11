extern crate std;
use super::*;
use super::super::commands::{ShapePalette, ShapeSecondarySource, StrokeWidth, TextAdvance, TextCoordinate, TextX, TextY};
use std::vec;
use std::vec::Vec;

fn record(bytes: &mut Vec<u8>, kind: u8, id: u32, key: u32, size: [u16; 2], stride: u16, advance: i16, data: &[u8]) {
    bytes.extend_from_slice(&[kind, 0, 0, 0]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(&key.to_le_bytes());
    bytes.extend_from_slice(&size[0].to_le_bytes());
    bytes.extend_from_slice(&size[1].to_le_bytes());
    bytes.extend_from_slice(&stride.to_le_bytes());
    bytes.extend_from_slice(&advance.to_le_bytes());
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(data);
}
fn header(count: u32) -> Vec<u8> {
    let mut bytes = b"RCP1".to_vec();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes
}
fn pack(bytes: &[u8]) -> AssetPack<'_> {
    AssetPack::parse(bytes, AssetProvenance::Original { author: "test vectors" }).unwrap()
}
fn point(x: u16, y: u16) -> PanelPoint { PanelPoint::new(x, y).unwrap() }
fn rect(x: u16, y: u16, w: u16, h: u16) -> PanelRect { PanelRect::new(point(x,y),w,h).unwrap() }
fn colour(alpha: u8) -> Colour { Colour { palette: PaletteIndex(7), alpha: Alpha(alpha) } }
fn pixel(bytes: &[u8], x: usize, y: usize) -> &[u8] { &bytes[(y*720+x)*4..(y*720+x)*4+4] }
fn render(bytes: &mut [u8], commands: &[DrawCommand<'_>], assets: &AssetPack<'_>) -> Result<(), RasterError> {
    Surface::new(SurfaceId::Osd, bytes)?.render(DrawList::new(SurfaceId::Osd, commands), &mut RasterResources::coverage8(assets))
}

#[test]
fn straight_alpha_preserves_colour_over_transparent_and_partial_destinations() {
    let mut assets = header(2);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[255,0,0,255]);
    record(&mut assets, 1, 8, 0, [0,0], 0, 0, &[0,0,255,128]);
    let mut bytes = vec![0; BYTES];
    render(&mut bytes, &[
        DrawCommand::Fill { rect: rect(1,0,1,1), colour: Colour { palette: PaletteIndex(8), alpha: Alpha::OPAQUE } },
        DrawCommand::Fill { rect: rect(0,0,2,1), colour: colour(128) },
    ], &pack(&assets)).unwrap();
    assert_eq!(pixel(&bytes,0,0), &[255,0,0,128]);
    assert_eq!(pixel(&bytes,1,0), &[170,0,85,192]);
    assert_eq!(pixel(&bytes,2,0), &[0,0,0,0]);
}

#[test]
fn clip_replaces_prior_clip_and_reset_restores_panel() {
    let mut assets = header(1);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[9,8,7,255]);
    let mut bytes = vec![0; BYTES];
    let commands = [
        DrawCommand::Clip(rect(2,2,1,1)),
        DrawCommand::Fill { rect: rect(1,1,3,3), colour: colour(255) },
        DrawCommand::Clip(rect(4,2,1,1)),
        DrawCommand::Fill { rect: rect(4,2,1,1), colour: colour(255) },
        DrawCommand::ResetClip,
        DrawCommand::Fill { rect: rect(719,479,1,1), colour: colour(255) },
    ];
    render(&mut bytes, &commands, &pack(&assets)).unwrap();
    assert_eq!(pixel(&bytes,2,2), &[9,8,7,255]);
    assert_eq!(pixel(&bytes,4,2), &[9,8,7,255]);
    assert_eq!(pixel(&bytes,719,479), &[9,8,7,255]);
    assert_eq!(pixel(&bytes,1,2), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,3,2), &[0,0,0,0]);
}

#[test]
fn translucent_border_draws_corners_once_and_keeps_interior() {
    let mut assets = header(1);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[20,40,60,255]);
    let mut bytes = vec![0; BYTES];
    render(&mut bytes, &[DrawCommand::Border { rect: rect(0,0,3,3), stroke: StrokeWidth::new(1).unwrap(), colour: colour(128) }], &pack(&assets)).unwrap();
    for (x,y) in [(0,0),(1,0),(2,0),(0,1),(2,1),(0,2),(1,2),(2,2)] {
        assert_eq!(pixel(&bytes,x,y), &[20,40,60,128]);
    }
    assert_eq!(pixel(&bytes,1,1), &[0,0,0,0]);
}

#[test]
fn clipped_image_keeps_source_offset_and_ignores_stride_padding() {
    let mut assets = header(1);
    record(&mut assets, 2, 19, 0, [2,2], 12, 0, &[255,0,0,255, 0,255,0,255, 99,99,99,99, 0,0,255,255, 8,16,32,128, 99,99,99,99]);
    let mut bytes = vec![0; BYTES];
    render(&mut bytes, &[
        DrawCommand::Clip(rect(6,6,1,1)),
        DrawCommand::Image { origin: point(5,5), image: ImageId::new(19).unwrap(), alpha: Alpha(128) },
    ], &pack(&assets)).unwrap();
    assert_eq!(pixel(&bytes,6,6), &[8,16,32,64]);
    assert_eq!(pixel(&bytes,5,5), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,7,6), &[0,0,0,0]);
}

#[test]
fn unicode_text_uses_supplied_coverage_and_advance() {
    let mut assets = header(3);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[100,50,25,255]);
    record(&mut assets, 3, 11, 'é' as u32, [2,1], 2, 3, &[255,128]);
    record(&mut assets, 4, 9, 0, [0,0], 0, 0, "éé".as_bytes());
    let mut bytes = vec![0; BYTES];
    render(&mut bytes, &[DrawCommand::Text { origin: point(1,1), text: TextRef::Catalog(CatalogTextId::new(9).unwrap()), font: FontId(11), colour: colour(255) }], &pack(&assets)).unwrap();
    assert_eq!(pixel(&bytes,1,1), &[100,50,25,255]);
    assert_eq!(pixel(&bytes,2,1), &[100,50,25,128]);
    assert_eq!(pixel(&bytes,3,1), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,4,1), &[100,50,25,255]);
    assert_eq!(pixel(&bytes,5,1), &[100,50,25,128]);
}

#[test]
fn arabic_run_mirrors_the_private_open_bracket_through_the_firmware_pair() {
    // MIRROR_PAIRS at 0x011805e0 pairs U+E128 with U+E129.
    let mut assets = header(4);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[100,50,25,255]);
    record(&mut assets, 3, 11, 0x627, [1,1], 1, 4, &[64]);
    record(&mut assets, 3, 11, 0xe129, [1,1], 1, 4, &[128]);
    record(&mut assets, 4, 9, 0, [0,0], 0, 0, "\u{627}\u{e128}\u{627}".as_bytes());
    let mut bytes = vec![0; BYTES];
    render(&mut bytes, &[DrawCommand::Text { origin: point(0,0), text: TextRef::Catalog(CatalogTextId::new(9).unwrap()), font: FontId(11), colour: colour(255) }], &pack(&assets)).unwrap();
    assert_eq!(pixel(&bytes,0,0), &[100,50,25,64]);
    assert_eq!(pixel(&bytes,4,0), &[100,50,25,128]);
    assert_eq!(pixel(&bytes,8,0), &[100,50,25,64]);
}

#[test]
fn missing_assets_are_typed_errors_without_substituted_pixels() {
    let empty = header(0);
    let mut bytes = vec![0; BYTES];
    assert_eq!(render(&mut bytes, &[DrawCommand::Image { origin: point(0,0), image: ImageId::new(42).unwrap(), alpha: Alpha::OPAQUE }], &pack(&empty)), Err(RasterError::MissingImage(ImageId::new(42).unwrap())));
    assert_eq!(render(&mut bytes, &[DrawCommand::Fill { rect: rect(0,0,1,1), colour: colour(255) }], &pack(&empty)), Err(RasterError::MissingPalette(PaletteIndex(7))));
    let mut assets = header(1);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[1,2,3,255]);
    assert_eq!(render(&mut bytes, &[DrawCommand::Glyph { origin: point(0,0), font: FontId(13), glyph: 'A', colour: colour(255) }], &pack(&assets)), Err(RasterError::MissingGlyph { font: FontId(13), glyph: super::super::font_core::shaping::CharacterCode(0x41) }));
    assert!(bytes.iter().all(|byte| *byte == 0));
}

#[test]
fn malformed_pack_dimensions_utf8_duplicates_and_truncation_are_rejected() {
    let provenance = AssetProvenance::Original { author: "test vectors" };
    let mut valid = header(1);
    record(&mut valid, 2, 1, 0, [1,1], 4, 0, &[1,2,3,4]);
    for length in 0..valid.len() { assert!(AssetPack::parse(&valid[..length], provenance).is_err()); }
    let mut invalid = header(1);
    record(&mut invalid, 2, 1, 0, [2,1], 4, 0, &[1,2,3,4]);
    assert!(matches!(AssetPack::parse(&invalid, provenance), Err(RasterError::InvalidAssetPack)));
    let mut invalid = header(1);
    record(&mut invalid, 4, 1, 0, [0,0], 0, 0, &[255]);
    assert!(matches!(AssetPack::parse(&invalid, provenance), Err(RasterError::InvalidAssetPack)));
    let mut duplicate = header(2);
    record(&mut duplicate, 1, 7, 0, [0,0], 0, 0, &[1,2,3,4]);
    record(&mut duplicate, 1, 7, 0, [0,0], 0, 0, &[5,6,7,8]);
    assert!(matches!(AssetPack::parse(&duplicate, provenance), Err(RasterError::DuplicateAsset)));
    assert!(matches!(AssetPack::parse(&valid, AssetProvenance::Original { author: "" }), Err(RasterError::InvalidProvenance)));
}

#[test]
fn surface_requires_exact_rgba_panel_extent() {
    assert!(matches!(Surface::new(SurfaceId::Osd, &mut [0;4]), Err(RasterError::InvalidSurfaceBuffer)));
    let mut oversized = vec![0; BYTES+1];
    assert!(matches!(Surface::new(SurfaceId::Osd, &mut oversized), Err(RasterError::InvalidSurfaceBuffer)));
}

#[test]
fn repeated_full_frames_ignore_prior_buffer_contents() {
    let mut assets = header(1);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[60,120,240,255]);
    let commands = [DrawCommand::Fill { rect: rect(1,1,1,1), colour: colour(128) }];
    let mut clean = vec![0; BYTES];
    let mut reused = vec![173; BYTES];
    render(&mut clean, &commands, &pack(&assets)).unwrap();
    render(&mut reused, &commands, &pack(&assets)).unwrap();
    assert_eq!(reused, clean);
    assert_eq!(pixel(&reused,1,1), &[60,120,240,128]);
    assert_eq!(pixel(&reused,0,0), &[0,0,0,0]);
    render(&mut reused, &commands, &pack(&assets)).unwrap();
    assert_eq!(reused, clean);
}

#[test]
fn typed_asset_domains_are_enforced_and_custom_text_resolves_separately() {
    assert!(CatalogTextId::new(0x83e).is_none());
    assert!(CustomTextId::new(0x0100_0000).is_none());
    assert!(ImageId::new(0xff00_0001).is_none());
    let mut assets = header(1);
    record(&mut assets, 4, 0x83e, 0, [0,0], 0, 0, b"invalid catalog id");
    assert!(matches!(AssetPack::parse(&assets, AssetProvenance::Original { author: "test vectors" }), Err(RasterError::InvalidAssetPack)));
    let mut assets = header(3);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[1,2,3,255]);
    record(&mut assets, 3, 11, 'x' as u32, [1,1], 1, 1, &[255]);
    record(&mut assets, 5, 9, 0, [0,0], 0, 0, b"x");
    let mut bytes = vec![0; BYTES];
    render(&mut bytes, &[DrawCommand::Text { origin: point(0,0), text: TextRef::Custom(CustomTextId::new(9).unwrap()), font: FontId(11), colour: colour(255) }], &pack(&assets)).unwrap();
    assert_eq!(pixel(&bytes,0,0), &[1,2,3,255]);
    assert_eq!(render(&mut bytes, &[DrawCommand::Text { origin: point(0,0), text: TextRef::Catalog(CatalogTextId::new(9).unwrap()), font: FontId(11), colour: colour(255) }], &pack(&assets)), Err(RasterError::MissingCatalogText(CatalogTextId::new(9).unwrap())));
}

#[test]
fn unsorted_asset_keys_are_rejected() {
    let mut assets = header(2);
    record(&mut assets, 1, 8, 0, [0,0], 0, 0, &[1,2,3,4]);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[5,6,7,8]);
    assert!(matches!(AssetPack::parse(&assets, AssetProvenance::Original { author: "test vectors" }), Err(RasterError::UnsortedAssets)));
}

#[test]
fn disjoint_clip_leaves_frame_transparent() {
    let mut assets = header(1);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[11,22,33,255]);
    let mut bytes = vec![99; BYTES];
    render(&mut bytes, &[
        DrawCommand::Clip(rect(20,20,2,2)),
        DrawCommand::Fill { rect: rect(0,0,2,2), colour: colour(255) },
        DrawCommand::Border { rect: rect(30,30,2,2), stroke: StrokeWidth::new(1).unwrap(), colour: colour(255) },
    ], &pack(&assets)).unwrap();
    assert!(bytes.iter().all(|byte| *byte == 0));
}

#[test]
fn image_and_glyph_crossing_panel_edges_only_write_visible_source_pixels() {
    let mut assets = header(3);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[12,24,48,255]);
    record(&mut assets, 2, 19, 0, [2,2], 12, 0, &[1,2,3,255, 4,5,6,255, 99,99,99,99, 7,8,9,255, 10,11,12,255, 99,99,99,99]);
    record(&mut assets, 3, 11, 'x' as u32, [2,2], 3, 2, &[128,255,99, 64,0,99]);
    let mut bytes = vec![0; BYTES];
    render(&mut bytes, &[DrawCommand::Image { origin: point(719,479), image: ImageId::new(19).unwrap(), alpha: Alpha::OPAQUE }], &pack(&assets)).unwrap();
    assert_eq!(pixel(&bytes,719,479), &[1,2,3,255]);
    assert_eq!(pixel(&bytes,718,479), &[0,0,0,0]);
    render(&mut bytes, &[DrawCommand::Glyph { origin: point(719,479), font: FontId(11), glyph: 'x', colour: colour(255) }], &pack(&assets)).unwrap();
    assert_eq!(pixel(&bytes,719,479), &[12,24,48,128]);
    assert_eq!(pixel(&bytes,0,479), &[0,0,0,0]);
    render(&mut bytes, &[DrawCommand::Glyph { origin: point(5,5), font: FontId(11), glyph: 'x', colour: colour(255) }], &pack(&assets)).unwrap();
    assert_eq!(pixel(&bytes,5,5), &[12,24,48,128]);
    assert_eq!(pixel(&bytes,6,5), &[12,24,48,255]);
    assert_eq!(pixel(&bytes,5,6), &[12,24,48,64]);
    assert_eq!(pixel(&bytes,6,6), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,7,5), &[0,0,0,0]);
}

#[test]
fn zero_alpha_and_zero_coverage_preserve_prior_drawn_pixels() {
    let mut assets = header(4);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[20,40,60,255]);
    record(&mut assets, 1, 8, 0, [0,0], 0, 0, &[255,0,0,0]);
    record(&mut assets, 2, 19, 0, [1,1], 4, 0, &[255,255,255,0]);
    record(&mut assets, 3, 11, 'x' as u32, [1,1], 1, 1, &[0]);
    let mut bytes = vec![0; BYTES];
    render(&mut bytes, &[
        DrawCommand::Fill { rect: rect(0,0,3,1), colour: colour(128) },
        DrawCommand::Fill { rect: rect(0,0,1,1), colour: Colour { palette: PaletteIndex(8), alpha: Alpha::OPAQUE } },
        DrawCommand::Fill { rect: rect(0,0,1,1), colour: colour(0) },
        DrawCommand::Image { origin: point(1,0), image: ImageId::new(19).unwrap(), alpha: Alpha::OPAQUE },
        DrawCommand::Glyph { origin: point(2,0), font: FontId(11), glyph: 'x', colour: colour(255) },
    ], &pack(&assets)).unwrap();
    for x in 0..3 { assert_eq!(pixel(&bytes,x,0), &[20,40,60,128]); }
}

#[test]
fn oversize_border_covers_each_pixel_once() {
    let mut assets = header(1);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[10,20,30,255]);
    let mut bytes = vec![0; BYTES];
    render(&mut bytes, &[DrawCommand::Border { rect: rect(1,1,2,3), stroke: StrokeWidth::new(u16::MAX).unwrap(), colour: colour(128) }], &pack(&assets)).unwrap();
    for y in 1..4 { for x in 1..3 { assert_eq!(pixel(&bytes,x,y), &[10,20,30,128]); } }
    assert_eq!(pixel(&bytes,0,1), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,3,3), &[0,0,0,0]);
}

#[test]
fn malformed_record_tags_counts_reserved_fields_and_scalar_fields_are_rejected() {
    let provenance = AssetProvenance::Original { author: "test vectors" };
    let mut valid = header(1);
    record(&mut valid, 1, 7, 0, [0,0], 0, 0, &[1,2,3,4]);
    for (offset, value) in [(8,99), (9,1), (10,1), (11,1), (16,1), (20,1), (22,1), (24,1), (26,1)] {
        let mut malformed = valid.clone();
        malformed[offset] = value;
        assert!(matches!(AssetPack::parse(&malformed, provenance), Err(RasterError::InvalidAssetPack)));
    }
    for count in [0u32,2,u32::MAX] {
        let mut malformed = valid.clone();
        malformed[4..8].copy_from_slice(&count.to_le_bytes());
        assert!(matches!(AssetPack::parse(&malformed, provenance), Err(RasterError::InvalidAssetPack)));
    }
    valid.push(0);
    assert!(matches!(AssetPack::parse(&valid, provenance), Err(RasterError::InvalidAssetPack)));
    let mut negative_advance = header(1);
    record(&mut negative_advance, 3, 11, 'x' as u32, [1,1], 1, -1, &[255]);
    assert!(matches!(AssetPack::parse(&negative_advance, provenance), Err(RasterError::InvalidAssetPack)));
}

#[test]
fn nonpalette_duplicate_keys_are_rejected_but_distinct_glyph_keys_are_valid() {
    let provenance = AssetProvenance::Original { author: "test vectors" };
    for kind in [2,3,4,5] {
        let mut bytes = header(2);
        for _ in 0..2 {
            if kind == 2 { record(&mut bytes, kind, 1, 0, [1,1], 4, 0, &[1,2,3,255]); }
            else if kind == 3 { record(&mut bytes, kind, 11, 'x' as u32, [1,1], 1, 1, &[255]); }
            else { record(&mut bytes, kind, 1, 0, [0,0], 0, 0, b"x"); }
        }
        assert!(matches!(AssetPack::parse(&bytes, provenance), Err(RasterError::DuplicateAsset)));
    }
    let mut bytes = header(2);
    record(&mut bytes, 3, 11, 'x' as u32, [1,1], 1, 1, &[255]);
    record(&mut bytes, 3, 11, 'y' as u32, [1,1], 1, 1, &[128]);
    assert!(AssetPack::parse(&bytes, provenance).is_ok());
}

#[test]
fn stage2_image_clips_at_360_by_240_extent() {
    let id = SurfaceId::Stage2;
    let mut assets = header(1);
    record(&mut assets, 2, 19, 0, [2,2], 8, 0, &[11,22,33,255, 44,55,66,255, 77,88,99,255, 111,122,133,255]);
    let mut bytes = vec![0; 360*240*4];
    let commands = [DrawCommand::Image { origin: PanelPoint::new_for(id,359,239).unwrap(), image: ImageId::new(19).unwrap(), alpha: Alpha::OPAQUE }];
    Surface::new(id, &mut bytes).unwrap().render(DrawList::new(id, &commands), &mut RasterResources::coverage8(&pack(&assets))).unwrap();
    assert_eq!(&bytes[bytes.len()-4..], &[11,22,33,255]);
    assert!(bytes[..bytes.len()-4].iter().all(|byte| *byte == 0));
    let mut wrong_extent = vec![0; BYTES];
    assert!(matches!(Surface::new(id, &mut wrong_extent), Err(RasterError::InvalidSurfaceBuffer)));
}

#[test]
fn stage4_fill_reaches_1280_by_720_last_pixel() {
    let id = SurfaceId::Stage4;
    let mut assets = header(1);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[21,42,84,255]);
    let mut bytes = vec![0; 1280*720*4];
    let origin = PanelPoint::new_for(id,1279,719).unwrap();
    let commands = [DrawCommand::Fill { rect: PanelRect::new_for(id,origin,1,1).unwrap(), colour: colour(255) }];
    Surface::new(id, &mut bytes).unwrap().render(DrawList::new(id, &commands), &mut RasterResources::coverage8(&pack(&assets))).unwrap();
    assert_eq!(&bytes[bytes.len()-4..], &[21,42,84,255]);
    assert!(bytes[..bytes.len()-4].iter().all(|byte| *byte == 0));
}

#[test]
fn surface_mismatch_rejects_before_modifying_pixels() {
    let mut bytes = vec![71; 360*240*4];
    let assets = header(0);
    let result = Surface::new(SurfaceId::Stage2, &mut bytes).unwrap().render(DrawList::new(SurfaceId::Osd, &[]), &mut RasterResources::coverage8(&pack(&assets)));
    assert_eq!(result, Err(RasterError::SurfaceMismatch));
    assert!(bytes.iter().all(|byte| *byte == 71));
}

#[test]
fn continuing_text_requires_a_pen_initialized_in_this_frame() {
    let mut assets = header(1);
    record(&mut assets, 1, 7, 0, [0,0], 0, 0, &[1,2,3,255]);
    let mut bytes = vec![0; BYTES];
    let continuation = DrawCommand::ContinueText { text: TextRef::Inline(""), font: FontId(11), colour: colour(255) };
    assert_eq!(render(&mut bytes, &[continuation], &pack(&assets)), Err(RasterError::TextPenUnset));
    render(&mut bytes, &[DrawCommand::TextPen(TextPenOp::Set(point(2,3))), continuation], &pack(&assets)).unwrap();
    assert_eq!(render(&mut bytes, &[continuation], &pack(&assets)), Err(RasterError::TextPenUnset));
    render(&mut bytes, &[DrawCommand::Text { origin: point(2,3), text: TextRef::Inline(""), font: FontId(11), colour: colour(255) }, continuation], &pack(&assets)).unwrap();
}

#[test]
fn axis_pen_operations_preserve_live_x_and_bounded_relative_advance() {
    let assets = header(0);
    let mut bytes = vec![0; BYTES];
    let advance = DrawCommand::TextPen(TextPenOp::AdvanceX { pixels: TextAdvance::new(640), max_before: TextCoordinate::new(19) });
    render(&mut bytes, &[
        DrawCommand::TextPen(TextPenOp::Set(point(19,0))),
        advance,
        DrawCommand::TextPen(TextPenOp::SetY(TextY::new_for(SurfaceId::Osd,10).unwrap())),
        DrawCommand::TextPen(TextPenOp::AdvanceX { pixels: TextAdvance::new(0), max_before: TextCoordinate::new(659) }),
    ], &pack(&assets)).unwrap();
    assert_eq!(render(&mut bytes, &[
        DrawCommand::TextPen(TextPenOp::Set(point(19,0))),
        advance,
        DrawCommand::TextPen(TextPenOp::SetY(TextY::new_for(SurfaceId::Osd,10).unwrap())),
        DrawCommand::TextPen(TextPenOp::AdvanceX { pixels: TextAdvance::new(0), max_before: TextCoordinate::new(658) }),
    ], &pack(&assets)), Err(RasterError::TextPenOutOfBounds));
    assert_eq!(render(&mut bytes, &[
        DrawCommand::TextPen(TextPenOp::Set(point(19,0))),
        DrawCommand::TextPen(TextPenOp::SetX(TextX::new_for(SurfaceId::Osd,20).unwrap())),
        advance,
    ], &pack(&assets)), Err(RasterError::TextPenOutOfBounds));
}

#[test]
fn axis_pen_operations_require_initialization_and_reject_cross_surface_coordinates() {
    let assets = header(0);
    let mut bytes = vec![0; BYTES];
    for operation in [
        TextPenOp::SetX(TextX::new_for(SurfaceId::Osd,1).unwrap()),
        TextPenOp::SetY(TextY::new_for(SurfaceId::Osd,1).unwrap()),
        TextPenOp::AdvanceX { pixels: TextAdvance::new(1), max_before: TextCoordinate::new(100) },
    ] {
        assert_eq!(render(&mut bytes, &[DrawCommand::TextPen(operation)], &pack(&assets)), Err(RasterError::TextPenUnset));
    }
    let mut stage2 = vec![0; 360*240*4];
    let commands = [
        DrawCommand::TextPen(TextPenOp::Set(PanelPoint::new_for(SurfaceId::Stage2,0,0).unwrap())),
        DrawCommand::TextPen(TextPenOp::SetX(TextX::new_for(SurfaceId::Osd,400).unwrap())),
    ];
    assert_eq!(Surface::new(SurfaceId::Stage2, &mut stage2).unwrap().render(DrawList::new(SurfaceId::Stage2,&commands), &mut RasterResources::coverage8(&pack(&assets))), Err(RasterError::TextPositionOverflow));
}

fn bitmap_font_config(coverage: CoverageEncoding) -> super::super::font_core::FontConfig {
    use super::super::font_core::{FontConfig, FontSize, PixelExtent, WritingMode};
    FontConfig { size: FontSize { width: PixelExtent::new(5), height: PixelExtent::new(3) }, coverage, writing: WritingMode::Horizontal }
}

#[test]
fn actual_bdf_constructor_mask_and_raster_preserve_negative_bearings_at_edges() {
    use super::super::font_core::{FontAsset, FontEngine, FontWorkspace, GlyphId};
    let authored_bdf = b"STARTFONT 2.1\r\nFONT authored-raster-test\r\nSIZE 8 75 75\r\nFONTBOUNDINGBOX 8 7 0 -2\r\nCHARS 1\r\nSTARTCHAR authored-pattern\r\nENCODING 65\r\nSWIDTH 500 0\r\nDWIDTH 4 0\r\nBBX 5 3 -2 -1\r\nBITMAP\r\nA8\r\n50\r\nF8\r\nENDCHAR\r\nENDFONT\r\n";
    let asset = FontAsset::parse(authored_bdf).unwrap();
    let mut decoded = [0; 3];
    let mut engine = FontEngine::new(asset, bitmap_font_config(CoverageEncoding::PackedMsb1), FontWorkspace::bitmap(&mut decoded)).unwrap();
    let mask = engine.glyph_mask(GlyphId::new(65)).unwrap();
    assert_eq!(mask.metrics().bearing_x.get(), -2);
    assert_eq!(mask.metrics().bearing_y.get(), -2);
    assert_eq!(mask.metrics().advance_x.get(), 4);
    let mut bytes = vec![0; BYTES];
    Surface::new(SurfaceId::Osd, &mut bytes).unwrap().draw_glyph_mask(point(0,1), &mask, Rgba { red: 20, green: 40, blue: 80, alpha: 128 }).unwrap();
    assert_eq!(pixel(&bytes,0,0), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,1,0), &[20,40,80,128]);
    assert_eq!(pixel(&bytes,2,0), &[0,0,0,0]);
    for x in 0..3 { assert_eq!(pixel(&bytes,x,1), &[20,40,80,128]); }
    assert_eq!(pixel(&bytes,3,1), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,0,2), &[0,0,0,0]);
}

fn assert_actual_dcw_mask_at_panel_edge(encoding: CoverageEncoding) {
    use super::super::font_core::{FontAsset, FontEngine, FontWorkspace, GlyphId};
    let mut authored_icon = [0u8; 1065];
    authored_icon[..8].copy_from_slice(b"DCW ICON");
    authored_icon[8..10].copy_from_slice(&[5,3]);
    authored_icon[32..34].copy_from_slice(&1u16.to_le_bytes());
    authored_icon[296..300].copy_from_slice(&1028u32.to_le_bytes());
    authored_icon[1060..].copy_from_slice(&[5,3,0xa8,0x50,0xf8]);
    let asset = FontAsset::parse(&authored_icon).unwrap();
    let mut decoded = [0; 15];
    let mut engine = FontEngine::new(asset, bitmap_font_config(encoding), FontWorkspace::bitmap(&mut decoded)).unwrap();
    let mask = engine.glyph_mask(GlyphId::new(65)).unwrap();
    assert_eq!(mask.metrics().advance_x.get(), 5);
    assert_eq!(mask.metrics().bearing_x.get(), 0);
    assert_eq!(mask.metrics().bearing_y.get(), 0);
    let mut bytes = vec![0; BYTES];
    Surface::new(SurfaceId::Osd, &mut bytes).unwrap().draw_glyph_mask(point(718,478), &mask, Rgba { red: 9, green: 18, blue: 36, alpha: 128 }).unwrap();
    assert_eq!(pixel(&bytes,718,478), &[9,18,36,128]);
    assert_eq!(pixel(&bytes,719,478), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,718,479), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,719,479), &[9,18,36,128]);
    assert_eq!(pixel(&bytes,717,479), &[0,0,0,0]);
}

#[test]
fn actual_dcw_constructor_encodings_render_ink_at_panel_edge() {
    for encoding in [CoverageEncoding::PackedMsb1, CoverageEncoding::Gray4, CoverageEncoding::Gray8] {
        assert_actual_dcw_mask_at_panel_edge(encoding);
    }
}

fn gvff_paint_fixture(kind: u32, values: &[f32], stops: &[[f32;5]]) -> Vec<u8> {
    let mut bytes = vec![0;56];
    bytes[..4].copy_from_slice(b"GVFF");
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&1u32.to_le_bytes());
    if !stops.is_empty() {
        bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(stops.len() as u32).to_le_bytes());
        for stop in stops { for value in stop { bytes.extend_from_slice(&value.to_le_bytes()); } }
    }
    let mut paint = [0;36];
    paint[4..8].copy_from_slice(&kind.to_le_bytes());
    for (index,value) in values.iter().enumerate() { paint[8+index*4..12+index*4].copy_from_slice(&value.to_le_bytes()); }
    bytes.extend_from_slice(&paint);
    let mut item = [0;52];
    item[..4].copy_from_slice(&2u32.to_le_bytes());
    item[20..24].copy_from_slice(&0x29au32.to_le_bytes());
    item[24..28].copy_from_slice(&0x1900u32.to_le_bytes());
    item[28..32].copy_from_slice(&0x1800u32.to_le_bytes());
    item[32..36].copy_from_slice(&0x1700u32.to_le_bytes());
    bytes.extend_from_slice(&item);
    bytes.extend_from_slice(&[0;4]);
    bytes
}

#[test]
fn linear_ramp_interpolates_premultiplied_colours_and_pads() {
    use super::super::vector_raster::CoverageSink;
    let asset = gvff_paint_fixture(0x1b01,&[0.0,0.0,2.0,0.0],&[
        [0.0,1.0,0.0,0.0,0.0],
        [1.0,0.0,0.0,1.0,1.0],
    ]);
    let file = Gvff::parse(&asset).unwrap();
    let paint = file.items().next().unwrap().unwrap().fill.unwrap();
    let placement = NaturalPlacement::natural(point(0,0),file.bounds());
    let mut bytes = vec![0;BYTES];
    {
        let mut surface = Surface::new(SurfaceId::Osd,&mut bytes).unwrap();
        let mut sink = PaintCoverage::new(&mut surface,file,paint,None,placement,Alpha::OPAQUE,PaintBlend::SourceOver).unwrap();
        sink.cover(0,0,255);
        sink.cover(1,0,255);
        sink.cover(3,0,255);
        sink.cover(-1,0,255);
    }
    assert_eq!(pixel(&bytes,0,0), &[0,0,255,64]);
    assert_eq!(pixel(&bytes,1,0), &[0,0,255,191]);
    assert_eq!(pixel(&bytes,3,0), &[0,0,255,255]);
    assert_eq!(pixel(&bytes,2,0), &[0,0,0,0]);
}

#[test]
fn radial_ramp_samples_local_coordinates_after_natural_placement() {
    use super::super::vector_raster::CoverageSink;
    let asset = gvff_paint_fixture(0x1b02,&[0.5,-0.5,0.5,-0.5,2.0],&[
        [0.0,1.0,0.0,0.0,1.0],
        [1.0,0.0,0.0,1.0,1.0],
    ]);
    let file = Gvff::parse(&asset).unwrap();
    let paint = file.items().next().unwrap().unwrap().fill.unwrap();
    let placement = NaturalPlacement::natural(point(2,3),file.bounds());
    let mut bytes = vec![0;BYTES];
    {
        let mut surface = Surface::new(SurfaceId::Osd,&mut bytes).unwrap();
        let mut sink = PaintCoverage::new(&mut surface,file,paint,None,placement,Alpha::OPAQUE,PaintBlend::SourceOver).unwrap();
        sink.cover(2,3,255);
        sink.cover(3,3,255);
        sink.cover(4,3,255);
    }
    assert_eq!(pixel(&bytes,2,3), &[255,0,0,255]);
    assert_eq!(pixel(&bytes,3,3), &[128,0,128,255]);
    assert_eq!(pixel(&bytes,4,3), &[0,0,255,255]);
    assert_eq!(pixel(&bytes,0,0), &[0,0,0,0]);
}

#[test]
fn source_and_source_over_apply_distinct_coverage_to_transparent_paint() {
    use super::super::vector_raster::CoverageSink;
    let asset = gvff_paint_fixture(0x1b00,&[1.0,0.0,0.0,0.0],&[]);
    let file = Gvff::parse(&asset).unwrap();
    let paint = file.items().next().unwrap().unwrap().fill.unwrap();
    let placement = NaturalPlacement::natural(point(0,0),file.bounds());
    let mut bytes = vec![0;BYTES];
    bytes[..8].copy_from_slice(&[0,0,255,255,0,0,255,255]);
    {
        let mut surface = Surface::new(SurfaceId::Osd,&mut bytes).unwrap();
        PaintCoverage::new(&mut surface,file,paint,None,placement,Alpha::OPAQUE,PaintBlend::SourceOver).unwrap().cover(0,0,128);
        PaintCoverage::new(&mut surface,file,paint,None,placement,Alpha::OPAQUE,PaintBlend::Source).unwrap().cover(1,0,128);
    }
    assert_eq!(pixel(&bytes,0,0), &[0,0,255,255]);
    assert_eq!(pixel(&bytes,1,0), &[0,0,255,127]);
}

#[test]
fn invalid_ramp_order_is_an_error_without_substituting_a_paint() {
    let asset = gvff_paint_fixture(0x1b01,&[0.0,0.0,2.0,0.0],&[
        [0.75,1.0,0.0,0.0,1.0],
        [0.25,0.0,0.0,1.0,1.0],
    ]);
    let file = Gvff::parse(&asset).unwrap();
    let paint = file.items().next().unwrap().unwrap().fill.unwrap();
    let placement = NaturalPlacement::natural(point(0,0),file.bounds());
    let mut bytes = vec![0;BYTES];
    let mut surface = Surface::new(SurfaceId::Osd,&mut bytes).unwrap();
    assert!(matches!(PaintCoverage::new(&mut surface,file,paint,None,placement,Alpha::OPAQUE,PaintBlend::SourceOver), Err(RasterError::InvalidPaint)));
}

#[test]
fn shared_gray4_mask_scales_byte_coverage_after_signed_left_clipping() {
    use super::super::font_core::{ByteStride, GlyphMetrics, GlyphMask, PixelExtent, PixelOffset};
    let metrics = GlyphMetrics {
        width: PixelExtent::new(3), height: PixelExtent::new(1),
        advance_x: PixelOffset::new(3), advance_y: PixelOffset::new(0),
        bearing_x: PixelOffset::new(-1), bearing_y: PixelOffset::new(0),
    };
    let mask = GlyphMask::new(&[0,8,15],metrics.width,metrics.height,ByteStride::new(3),CoverageEncoding::Gray4,metrics).unwrap();
    let mut bytes = vec![0;BYTES];
    Surface::new(SurfaceId::Osd,&mut bytes).unwrap().draw_glyph_mask(point(0,0),&mask,Rgba { red: 50, green: 100, blue: 200, alpha: 128 }).unwrap();
    assert_eq!(pixel(&bytes,0,0), &[50,100,200,68]);
    assert_eq!(pixel(&bytes,1,0), &[50,100,200,128]);
    assert_eq!(pixel(&bytes,2,0), &[0,0,0,0]);
}

#[test]
fn shared_mask_rejects_out_of_range_gray4_samples() {
    use super::super::font_core::{ByteStride, GlyphMetrics, GlyphMask, PixelExtent, PixelOffset};
    let metrics = GlyphMetrics {
        width: PixelExtent::new(1), height: PixelExtent::new(1),
        advance_x: PixelOffset::new(1), advance_y: PixelOffset::new(0),
        bearing_x: PixelOffset::new(0), bearing_y: PixelOffset::new(0),
    };
    assert!(matches!(GlyphMask::new(&[16],metrics.width,metrics.height,ByteStride::new(1),CoverageEncoding::Gray4,metrics),Err(FontError::InvalidBitmap)));
}

#[test]
fn actual_font_registry_drives_text_continuation_and_glyph_into_runtime_output() {
    use super::super::context::{GraphicsContext, OutputMode};
    use super::super::publisher::{ContextId, Publisher, SubmissionError};
    use super::super::font_core::FontWorkspace;
    use core::sync::atomic::{AtomicBool, Ordering};
    let authored_bdf = b"STARTFONT 2.1\r\nFONT authored-raster-test\r\nSIZE 8 75 75\r\nFONTBOUNDINGBOX 8 7 0 -2\r\nCHARS 1\r\nSTARTCHAR authored-pattern\r\nENCODING 65\r\nSWIDTH 500 0\r\nDWIDTH 4 0\r\nBBX 5 3 1 -1\r\nBITMAP\r\nA8\r\n50\r\nF8\r\nENDCHAR\r\nENDFONT\r\n";
    let mut pack_bytes = header(1);
    record(&mut pack_bytes,1,7,0,[0,0],0,0,&[25,50,100,255]);
    let pack = pack(&pack_bytes);
    let mut decoded = [0;3];
    let mut bindings = [FontBinding::new(FontId(11),authored_bdf,bitmap_font_config(CoverageEncoding::PackedMsb1),FontWorkspace::bitmap(&mut decoded)).unwrap()];
    let mut resources = RasterResources::new(&pack,FontRegistry::Engines(&mut bindings),&[],None).unwrap();
    let commands = [
        DrawCommand::Text { origin: point(1,3), text: TextRef::Inline("A"), font: FontId(11), colour: colour(255) },
        DrawCommand::ContinueText { text: TextRef::Inline("A"), font: FontId(11), colour: colour(255) },
        DrawCommand::Glyph { origin: point(20,3), glyph: 'A', font: FontId(11), colour: colour(255) },
    ];
    static CLAIMED: AtomicBool = AtomicBool::new(false);
    static mut FIRST: [u8;BYTES] = [0;BYTES];
    static mut SECOND: [u8;BYTES] = [0;BYTES];
    assert!(CLAIMED.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_ok());
    let (first,second) = unsafe { (&mut *core::ptr::addr_of_mut!(FIRST), &mut *core::ptr::addr_of_mut!(SECOND)) };
    let publisher = Publisher::new(ContextId::new(901).unwrap(),SurfaceId::Osd,first,second).unwrap();
    let mut context = GraphicsContext::new();
    context.install(publisher).ok().unwrap();
    let batch = super::super::runtime::render_output(&mut context,&mut resources,OutputMode::Panel,DrawList::new(SurfaceId::Osd,&commands),None).unwrap();
    let frame = batch.frames()[0].unwrap();
    context.surface(SurfaceId::Osd).unwrap().submit(frame.frame,|surface,submitted,pixels| {
        assert_eq!(surface,SurfaceId::Osd);
        assert_eq!(submitted,frame.frame);
        for x in [2,4,6,8,10,21,23,25] { assert_eq!(pixel(pixels,x,1), &[25,50,100,255]); }
        for x in [3,5,7,9,11,20,22,24,26] { assert_eq!(pixel(pixels,x,1), &[0,0,0,0]); }
        Ok::<(),SubmissionError<()>>(())
    }).unwrap().unwrap();
}

#[test]
fn actual_font_registry_never_falls_back_to_coverage8_for_missing_glyph() {
    use super::super::font_core::{FontWorkspace, GlyphId};
    let authored_bdf = b"STARTFONT 2.1\r\nFONT authored-raster-test\r\nSIZE 8 75 75\r\nFONTBOUNDINGBOX 8 7 0 -2\r\nCHARS 1\r\nSTARTCHAR authored-pattern\r\nENCODING 65\r\nDWIDTH 4 0\r\nBBX 5 3 1 -1\r\nBITMAP\r\nA8\r\n50\r\nF8\r\nENDCHAR\r\nENDFONT\r\n";
    let mut pack_bytes = header(2);
    record(&mut pack_bytes,1,7,0,[0,0],0,0,&[25,50,100,255]);
    record(&mut pack_bytes,3,11,'B' as u32,[1,1],1,1,&[255]);
    let pack = pack(&pack_bytes);
    let mut decoded = [0;3];
    let mut bindings = [FontBinding::new(FontId(11),authored_bdf,bitmap_font_config(CoverageEncoding::PackedMsb1),FontWorkspace::bitmap(&mut decoded)).unwrap()];
    let mut resources = RasterResources::new(&pack,FontRegistry::Engines(&mut bindings),&[],None).unwrap();
    let commands = [DrawCommand::Text { origin: point(1,3), text: TextRef::Inline("B"), font: FontId(11), colour: colour(255) }];
    let mut bytes = vec![0;BYTES];
    assert_eq!(Surface::new(SurfaceId::Osd,&mut bytes).unwrap().render(DrawList::new(SurfaceId::Osd,&commands),&mut resources),Err(RasterError::Font { font: FontId(11), error: FontError::MissingGlyph(GlyphId::new(66)) }));
    assert!(bytes.iter().all(|byte| *byte == 0));
}

fn gvff_rectangle_fixture() -> Vec<u8> {
    let mut bytes = gvff_paint_fixture(0x1b00,&[1.0,0.0,0.0,1.0],&[]);
    bytes[48..52].copy_from_slice(&2.0f32.to_le_bytes());
    bytes[52..56].copy_from_slice(&(-2.0f32).to_le_bytes());
    let item = bytes.len()-56;
    bytes[item+44..item+48].copy_from_slice(&5u32.to_le_bytes());
    bytes[item+48..item+52].copy_from_slice(&32u32.to_le_bytes());
    bytes.truncate(bytes.len()-4);
    bytes.extend_from_slice(&[2,4,4,4,0,0,0,0]);
    for value in [0.0f32,0.0,2.0,0.0,2.0,-2.0,0.0,-2.0] { bytes.extend_from_slice(&value.to_le_bytes()); }
    bytes
}

#[test]
fn decoded_gvff_shape_command_consumes_bounded_workspace_and_natural_placement() {
    use super::super::resources::{BitmapEntry, BitmapKind, DataAddress, DataRegion, ResourceMemory};
    let shape_bytes = gvff_rectangle_fixture();
    let regions = [DataRegion { base: DataAddress(0x1000), bytes: &shape_bytes }];
    let memory = ResourceMemory::new(&regions).unwrap();
    let entry = BitmapEntry { image: ImageId::new(7).unwrap(), width: 2, height: 2, kind: BitmapKind::ShapeKindOne, data: DataAddress(0x1000) };
    let bindings = [ShapeBinding::new(ShapeAsset::decode(entry,&memory).unwrap())];
    let pack_bytes = header(0);
    let pack = pack(&pack_bytes);
    let mut vertices = [Vertex::EMPTY;5];
    let mut strokes = [];
    let workspace = VectorWorkspace { vertices: &mut vertices, strokes: &mut strokes, dash_budget: DashBudget::new(64), accuracy: CurveAccuracy::new(0.25).unwrap(), samples: SampleGrid::One };
    let mut resources = RasterResources::new(&pack,FontRegistry::Coverage8,&bindings,Some(workspace)).unwrap();
    let commands = [
        DrawCommand::Clip(rect(4,4,1,2)),
        DrawCommand::Shape { origin: point(3,4), image: ImageId::new(7).unwrap(), alpha: Alpha(128), paint: ShapePaint::Original },
    ];
    let mut bytes = vec![0;BYTES];
    Surface::new(SurfaceId::Osd,&mut bytes).unwrap().render(DrawList::new(SurfaceId::Osd,&commands),&mut resources).unwrap();
    assert_eq!(pixel(&bytes,4,4), &[255,0,0,128]);
    assert_eq!(pixel(&bytes,4,5), &[255,0,0,128]);
    assert_eq!(pixel(&bytes,3,4), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,5,5), &[0,0,0,0]);
}

#[test]
fn resource_bindings_reject_duplicate_shapes_and_ambiguous_image_ownership() {
    use super::super::resources::{BitmapEntry, BitmapKind, DataAddress, DataRegion, ResourceMemory};
    let shape_bytes = gvff_rectangle_fixture();
    let regions = [DataRegion { base: DataAddress(0x1000), bytes: &shape_bytes }];
    let memory = ResourceMemory::new(&regions).unwrap();
    let entry = BitmapEntry { image: ImageId::new(7).unwrap(), width: 2, height: 2, kind: BitmapKind::ShapeKindTwo, data: DataAddress(0x1000) };
    let shape = ShapeBinding::new(ShapeAsset::decode(entry,&memory).unwrap());
    let empty_bytes = header(0);
    let empty = pack(&empty_bytes);
    let duplicates = [shape,shape];
    assert!(matches!(RasterResources::new(&empty,FontRegistry::Coverage8,&duplicates,None),Err(RasterError::DuplicateShape(id)) if id == ImageId::new(7).unwrap()));
    let mut image_bytes = header(1);
    record(&mut image_bytes,2,7,0,[1,1],4,0,&[0,0,0,0]);
    let images = pack(&image_bytes);
    let shapes = [shape];
    assert!(matches!(RasterResources::new(&images,FontRegistry::Coverage8,&shapes,None),Err(RasterError::AmbiguousImageOwnership(id)) if id == ImageId::new(7).unwrap()));
    let mut resources = RasterResources::new(&empty,FontRegistry::Coverage8,&shapes,None).unwrap();
    let commands = [DrawCommand::Shape { origin: point(0,0), image: ImageId::new(7).unwrap(), alpha: Alpha::OPAQUE, paint: ShapePaint::Original }];
    let mut bytes = vec![0;BYTES];
    assert_eq!(Surface::new(SurfaceId::Osd,&mut bytes).unwrap().render(DrawList::new(SurfaceId::Osd,&commands),&mut resources),Err(RasterError::MissingVectorWorkspace));
}

#[test]
fn actual_styled_request_command_uses_constructor_height_and_supplied_rgb() {
    use super::super::resources::{BitmapEntry, BitmapKind, DataAddress, DataRegion, ResourceMemory};
    let segments = *b"F BMMLLLE";
    let mut values = Vec::new();
    for value in [1.0f32,4.0,0.0,0.0,0.0,0.0,1.0,0.0,5.0,0.0,0.0,2.0,0.0,2.0,2.0,0.0,2.0] { values.extend_from_slice(&value.to_le_bytes()); }
    let regions = [
        DataRegion { base: DataAddress(0x1000), bytes: &segments },
        DataRegion { base: DataAddress(0x2000), bytes: &values },
    ];
    let memory = ResourceMemory::new(&regions).unwrap();
    let entry = BitmapEntry { image: ImageId::new(8).unwrap(), width: 2, height: 2, kind: BitmapKind::ShapeKindTwo, data: DataAddress(0) };
    let mut request = [0;20];
    request[..4].copy_from_slice(&9u32.to_le_bytes());
    request[8..12].copy_from_slice(&0x1000u32.to_le_bytes());
    request[12..16].copy_from_slice(&0x2000u32.to_le_bytes());
    let shapes = [ShapeBinding::new(ShapeAsset::from_request(entry,&request,&memory).unwrap())];
    let pack_bytes = header(0);
    let pack = pack(&pack_bytes);
    let mut vertices = [Vertex::EMPTY;5];
    let mut strokes = [];
    let workspace = VectorWorkspace { vertices: &mut vertices, strokes: &mut strokes, dash_budget: DashBudget::new(64), accuracy: CurveAccuracy::new(0.25).unwrap(), samples: SampleGrid::One };
    let mut resources = RasterResources::new(&pack,FontRegistry::Coverage8,&shapes,Some(workspace)).unwrap();
    let commands = [DrawCommand::Shape { origin: point(2,3), image: ImageId::new(8).unwrap(), alpha: Alpha(128), paint: ShapePaint::Original }];
    let mut bytes = vec![0;BYTES];
    Surface::new(SurfaceId::Osd,&mut bytes).unwrap().render(DrawList::new(SurfaceId::Osd,&commands),&mut resources).unwrap();
    for y in 3..5 { for x in 2..4 { assert_eq!(pixel(&bytes,x,y), &[0,255,0,128]); } }
    assert_eq!(pixel(&bytes,2,2), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,2,5), &[0,0,0,0]);
}

#[test]
fn per_draw_shape_palettes_preserve_primary_alpha_and_selected_secondary_source() {
    use super::super::resources::{BitmapEntry, BitmapKind, DataAddress, DataRegion, ResourceMemory};
    let mut black_shape = gvff_rectangle_fixture();
    black_shape[64..68].copy_from_slice(&0.0f32.to_le_bytes());
    let red_shape = gvff_rectangle_fixture();
    let regions = [
        DataRegion { base: DataAddress(0x1000), bytes: &black_shape },
        DataRegion { base: DataAddress(0x2000), bytes: &red_shape },
    ];
    let memory = ResourceMemory::new(&regions).unwrap();
    let black_entry = BitmapEntry { image: ImageId::new(7).unwrap(), width: 2, height: 2, kind: BitmapKind::ShapeKindOne, data: DataAddress(0x1000) };
    let red_entry = BitmapEntry { image: ImageId::new(8).unwrap(), data: DataAddress(0x2000), ..black_entry };
    let shapes = [
        ShapeBinding::new(ShapeAsset::decode(black_entry,&memory).unwrap()),
        ShapeBinding::new(ShapeAsset::decode(red_entry,&memory).unwrap()),
    ];
    let first = ShapePalette {
        primary: Rgba { red: 20, green: 40, blue: 80, alpha: 128 },
        secondary: Rgba { red: 30, green: 60, blue: 90, alpha: 200 },
        secondary_source: ShapeSecondarySource::White,
    };
    let second = ShapePalette { primary: Rgba { red: 100, green: 50, blue: 25, alpha: 255 }, secondary_source: ShapeSecondarySource::Red, ..first };
    let pack_bytes = header(0);
    let pack = pack(&pack_bytes);
    let mut vertices = [Vertex::EMPTY;5];
    let mut strokes = [];
    let workspace = VectorWorkspace { vertices: &mut vertices, strokes: &mut strokes, dash_budget: DashBudget::new(64), accuracy: CurveAccuracy::new(0.25).unwrap(), samples: SampleGrid::One };
    let mut resources = RasterResources::new(&pack,FontRegistry::Coverage8,&shapes,Some(workspace)).unwrap();
    let commands = [
        DrawCommand::Shape { origin: point(0,0), image: black_entry.image, alpha: Alpha::OPAQUE, paint: ShapePaint::Recolour(first) },
        DrawCommand::Shape { origin: point(3,0), image: black_entry.image, alpha: Alpha(128), paint: ShapePaint::Recolour(second) },
        DrawCommand::Shape { origin: point(6,0), image: red_entry.image, alpha: Alpha::OPAQUE, paint: ShapePaint::Recolour(first) },
        DrawCommand::Shape { origin: point(9,0), image: red_entry.image, alpha: Alpha::OPAQUE, paint: ShapePaint::Recolour(second) },
    ];
    let mut bytes = vec![0;BYTES];
    Surface::new(SurfaceId::Osd,&mut bytes).unwrap().render(DrawList::new(SurfaceId::Osd,&commands),&mut resources).unwrap();
    assert_eq!(pixel(&bytes,0,0), &[20,40,80,128]);
    assert_eq!(pixel(&bytes,3,0), &[100,50,25,128]);
    assert_eq!(pixel(&bytes,6,0), &[0,0,0,0]);
    assert_eq!(pixel(&bytes,9,0), &[30,60,90,200]);
}
