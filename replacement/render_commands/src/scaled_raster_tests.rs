extern crate std;
use super::*;
use super::super::commands::{PixelOrder, RecolourBlock};
use std::vec;
use std::vec::Vec;

const EMPTY_PACK: &[u8] = b"RCP1\0\0\0\0";
const MOVE: u8 = 2;
const LINE: u8 = 4;
const CLOSE: u8 = 0;
const EVEN_ODD: u32 = 0x1900;
const NON_ZERO: u32 = 0x1901;
const BUTT: u32 = 0x1700;
const SQUARE: u32 = 0x1702;
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

fn pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 4] { bytes[(y * 720 + x) * 4..(y * 720 + x) * 4 + 4].try_into().unwrap() }
fn scene(x: i32, y: i32, width: u16, height: u16) -> SceneRect { SceneRect::new(x as f32, y as f32, f32::from(width), f32::from(height)).unwrap() }
fn fractional(x: f32, y: f32, width: f32, height: f32) -> SceneRect { SceneRect::new(x, y, width, height).unwrap() }
fn panel_rect(x: u16, y: u16, width: u16, height: u16) -> PanelRect { PanelRect::new(PanelPoint::new(x, y).unwrap(), width, height).unwrap() }
fn rgba_source(width: u16, height: u16, stride: usize, bytes: &[u8]) -> PixelSource<'_> {
    PixelSource::new(width, height, stride, PixelOrder::Rgba, bytes).unwrap()
}
/// Re-encodes R,G,B,A texels as the A,B,G,R bytes of little-endian `VG_sRGBA_8888` words.
fn abgr(bytes: &[u8]) -> Vec<u8> { bytes.chunks_exact(4).flat_map(|texel| [texel[3], texel[2], texel[1], texel[0]]).collect() }
fn render_images(commands: &[DrawCommand<'_>]) -> Vec<u8> {
    let pack = AssetPack::parse(EMPTY_PACK, AssetProvenance::Original { author: "test vectors" }).unwrap();
    let mut bytes = vec![0; BYTES];
    Surface::new(SurfaceId::Osd, &mut bytes).unwrap().render(DrawList::new(SurfaceId::Osd, commands), &mut RasterResources::coverage8(&pack)).unwrap();
    bytes
}
fn render_shapes(commands: &[DrawCommand<'_>]) -> Result<Vec<u8>, RasterError> {
    let pack = AssetPack::parse(EMPTY_PACK, AssetProvenance::Original { author: "test vectors" }).unwrap();
    let mut vertices = [Vertex::EMPTY; 64];
    let mut strokes = [StrokePrimitive::EMPTY; 64];
    let workspace = VectorWorkspace { vertices: &mut vertices, strokes: &mut strokes, dash_budget: DashBudget::new(64), accuracy: CurveAccuracy::new(0.25).unwrap(), samples: SampleGrid::One };
    let mut resources = RasterResources::new(&pack, FontRegistry::Coverage8, &[], Some(workspace))?;
    let mut bytes = vec![0; BYTES];
    Surface::new(SurfaceId::Osd, &mut bytes)?.render(DrawList::new(SurfaceId::Osd, commands), &mut resources)?;
    Ok(bytes)
}

struct Item<'a> { mode: u32, stroke: u32, fill: u32, fill_rule: u32, cap: u32, width: f32, segments: &'a [u8], coordinates: &'a [f32] }
impl<'a> Item<'a> {
    fn fill(paint: u32, fill_rule: u32, segments: &'a [u8], coordinates: &'a [f32]) -> Self {
        Self { mode: 2, stroke: 0, fill: paint, fill_rule, cap: BUTT, width: 1.0, segments, coordinates }
    }
    fn stroke(paint: u32, width: f32, cap: u32, segments: &'a [u8], coordinates: &'a [f32]) -> Self {
        Self { mode: 1, stroke: paint, fill: 0, fill_rule: NON_ZERO, cap, width, segments, coordinates }
    }
}
/// GVFF layout of `Gvff::parse`: 56-byte header (bounds at +40), 36-byte solid paints, then each 52-byte item record
/// followed by its segment bytes, padding to the next multiple of four plus four, and its coordinates.
fn gvff(bounds: [f32; 4], paints: &[[f32; 4]], items: &[Item<'_>]) -> Vec<u8> {
    let mut bytes = vec![0u8; 56];
    bytes[..4].copy_from_slice(b"GVFF");
    bytes[20..24].copy_from_slice(&(items.len() as u32).to_le_bytes());
    bytes[24..28].copy_from_slice(&(paints.len() as u32).to_le_bytes());
    for (index, value) in bounds.iter().enumerate() { bytes[40 + index * 4..44 + index * 4].copy_from_slice(&value.to_le_bytes()); }
    for paint in paints {
        let mut record = [0u8; 36];
        record[4..8].copy_from_slice(&0x1b00u32.to_le_bytes());
        for (index, value) in paint.iter().enumerate() { record[8 + index * 4..12 + index * 4].copy_from_slice(&value.to_le_bytes()); }
        bytes.extend_from_slice(&record);
    }
    for item in items {
        let mut record = [0u8; 52];
        let words = [
            (0, item.mode), (8, 1), (12, item.stroke), (16, item.fill), (20, 0x29a), (24, item.fill_rule), (28, 0x1800), (32, item.cap),
            (36, item.width.to_bits()), (40, 4.0f32.to_bits()), (44, item.segments.len() as u32), (48, item.coordinates.len() as u32 * 4),
        ];
        for (offset, value) in words { record[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
        bytes.extend_from_slice(&record);
        bytes.extend_from_slice(item.segments);
        bytes.extend(core::iter::repeat(0).take((item.segments.len() & !3) + 4 - item.segments.len()));
        for value in item.coordinates { bytes.extend_from_slice(&value.to_le_bytes()); }
    }
    bytes
}
const SQUARE_SEGMENTS: &[u8] = &[MOVE, LINE, LINE, LINE, CLOSE];
const TWO_SQUARES: &[u8] = &[MOVE, LINE, LINE, LINE, CLOSE, MOVE, LINE, LINE, LINE, CLOSE];
/// Outer square [0,4]² and inner square [1,3]², both counter-clockwise, so the inner square has winding two.
const NESTED: &[f32] = &[0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0, 1.0, 1.0, 3.0, 1.0, 3.0, 3.0, 1.0, 3.0];
fn recolour_block(pairs: [[f32; 8]; 3]) -> RecolourBlock {
    let mut block = [0u8; 96];
    for (index, value) in pairs.iter().flatten().enumerate() { block[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes()); }
    RecolourBlock(block)
}

#[test]
fn identity_scale_copies_texels_exactly_in_both_byte_orders() {
    let texels = [
        10, 20, 30, 255, 8, 16, 32, 128, 1, 2, 3, 0, 99, 99, 99, 99,
        200, 100, 50, 255, 0, 0, 0, 255, 255, 255, 255, 1, 99, 99, 99, 99,
    ];
    let swapped = abgr(&texels);
    for source in [rgba_source(3, 2, 16, &texels), PixelSource::new(3, 2, 16, PixelOrder::Abgr, &swapped).unwrap()] {
        let bytes = render_images(&[DrawCommand::ScaledImage { dest: scene(10, 20, 3, 2), source }]);
        assert_eq!(pixel(&bytes, 10, 20), [10, 20, 30, 255]);
        assert_eq!(pixel(&bytes, 11, 20), [8, 16, 32, 128]);
        assert_eq!(pixel(&bytes, 12, 20), [0, 0, 0, 0]);
        assert_eq!(pixel(&bytes, 10, 21), [200, 100, 50, 255]);
        assert_eq!(pixel(&bytes, 11, 21), [0, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 12, 21), [255, 255, 255, 1]);
        assert_eq!(pixel(&bytes, 9, 20), [0, 0, 0, 0]);
        assert_eq!(pixel(&bytes, 13, 21), [0, 0, 0, 0]);
    }
}

#[test]
fn doubled_width_and_height_interpolate_between_texel_centres() {
    // Ws = 2, Wt = 4: centres 0.5, 1.5, 2.5, 3.5 map to u = -0.25 (clamped to 0), 0.25, 0.75, 1.25 (clamped to 1).
    let texels = [0, 0, 0, 255, 200, 0, 0, 255];
    let bytes = render_images(&[
        DrawCommand::ScaledImage { dest: scene(0, 0, 4, 1), source: rgba_source(2, 1, 8, &texels) },
        DrawCommand::ScaledImage { dest: scene(0, 2, 1, 4), source: rgba_source(1, 2, 4, &texels) },
    ]);
    for (offset, red) in [0, 50, 150, 200].into_iter().enumerate() {
        assert_eq!(pixel(&bytes, offset, 0), [red, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 0, 2 + offset), [red, 0, 0, 255]);
    }
}

#[test]
fn stretch_weights_colour_by_alpha_so_transparent_texels_do_not_bleed() {
    // Weights as in the doubled-width case; alpha 0.75·255 and 0.25·255 round to 191 and 64, colour stays opaque red's.
    let texels = [255, 0, 0, 255, 0, 0, 255, 0];
    let swapped = abgr(&texels);
    for source in [rgba_source(2, 1, 8, &texels), PixelSource::new(2, 1, 8, PixelOrder::Abgr, &swapped).unwrap()] {
        let bytes = render_images(&[DrawCommand::ScaledImage { dest: scene(0, 0, 4, 1), source }]);
        assert_eq!(pixel(&bytes, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 1, 0), [255, 0, 0, 191]);
        assert_eq!(pixel(&bytes, 2, 0), [255, 0, 0, 64]);
        assert_eq!(pixel(&bytes, 3, 0), [0, 0, 0, 0]);
    }
}

#[test]
fn nine_eighths_stretch_follows_the_declared_texel_mapping() {
    // Ws = 8, Wt = 9: u = (2k + 1)·4/9 − 1/2. Red is 36·i, so inside [0, 7] the sample is 36·u = 32k − 2.
    let texels: Vec<u8> = (0..8u8).flat_map(|index| [36 * index, 0, 0, 255]).collect();
    let bytes = render_images(&[DrawCommand::ScaledImage { dest: scene(0, 0, 9, 1), source: rgba_source(8, 1, 32, &texels) }]);
    for (offset, red) in [0, 30, 62, 94, 126, 158, 190, 222, 252].into_iter().enumerate() {
        assert_eq!(pixel(&bytes, offset, 0), [red, 0, 0, 255], "column {offset}");
    }
}

#[test]
fn translucent_image_composites_source_over_an_existing_pixel() {
    // Straight source-over of red at 128/255 on opaque blue: red 255·128/255, blue 255·127/255, alpha 1.
    let blue = [0, 0, 255, 255];
    let red = [255, 0, 0, 128];
    let bytes = render_images(&[
        DrawCommand::ScaledImage { dest: scene(5, 5, 1, 1), source: rgba_source(1, 1, 4, &blue) },
        DrawCommand::ScaledImage { dest: scene(5, 5, 1, 1), source: rgba_source(1, 1, 4, &red) },
    ]);
    assert_eq!(pixel(&bytes, 5, 5), [128, 0, 127, 255]);
}

#[test]
fn images_clip_to_the_current_clip_and_the_surface_edges() {
    let row = [1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255];
    let block: Vec<u8> = row.iter().chain(row.iter()).copied().collect();
    let bytes = render_images(&[
        DrawCommand::Clip(panel_rect(1, 0, 2, 1)),
        DrawCommand::ScaledImage { dest: scene(0, 0, 4, 1), source: rgba_source(4, 1, 16, &row) },
        DrawCommand::ResetClip,
        DrawCommand::ScaledImage { dest: scene(718, 479, 4, 2), source: rgba_source(4, 2, 16, &block) },
        DrawCommand::ScaledImage { dest: scene(716, 0, 40, 30), source: rgba_source(4, 2, 16, &block) },
    ]);
    assert_eq!(pixel(&bytes, 0, 0), [0, 0, 0, 0]);
    assert_eq!(pixel(&bytes, 1, 0), [2, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 2, 0), [3, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 3, 0), [0, 0, 0, 0]);
    assert_eq!(pixel(&bytes, 718, 479), [1, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 719, 479), [2, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 716, 0), [1, 0, 0, 255]);
}

#[test]
fn negative_destination_origin_skips_the_texels_left_of_and_above_the_panel() {
    let texels: Vec<u8> = (0..8u8).flat_map(|index| [10 * index + 10, 0, 0, 255]).collect();
    let bytes = render_images(&[DrawCommand::ScaledImage { dest: scene(-2, -1, 4, 2), source: rgba_source(4, 2, 16, &texels) }]);
    assert_eq!(pixel(&bytes, 0, 0), [70, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 1, 0), [80, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 2, 0), [0, 0, 0, 0]);
    assert_eq!(pixel(&bytes, 0, 1), [0, 0, 0, 0]);
}

#[test]
fn gvff_square_bounds_map_onto_the_destination_rectangle() {
    let square = [0.0, 0.0, 2.0, 0.0, 2.0, -2.0, 0.0, -2.0];
    let shape = gvff([0.0, 0.0, 2.0, -2.0], &[RED], &[Item::fill(0, NON_ZERO, SQUARE_SEGMENTS, &square)]);
    let bytes = render_shapes(&[DrawCommand::ScaledShape { dest: scene(10, 20, 6, 4), gvff: &shape, recolour: None }]).unwrap();
    for y in 20..24 { for x in 10..16 { assert_eq!(pixel(&bytes, x, y), [255, 0, 0, 255], "({x}, {y})"); } }
    for (x, y) in [(9, 20), (16, 20), (10, 19), (10, 24), (15, 24)] { assert_eq!(pixel(&bytes, x, y), [0, 0, 0, 0], "({x}, {y})"); }
    let clipped = render_shapes(&[
        DrawCommand::Clip(panel_rect(12, 21, 2, 2)),
        DrawCommand::ScaledShape { dest: scene(10, 20, 6, 4), gvff: &shape, recolour: None },
    ]).unwrap();
    assert_eq!(pixel(&clipped, 12, 21), [255, 0, 0, 255]);
    assert_eq!(pixel(&clipped, 13, 22), [255, 0, 0, 255]);
    assert_eq!(pixel(&clipped, 11, 21), [0, 0, 0, 0]);
    assert_eq!(pixel(&clipped, 14, 22), [0, 0, 0, 0]);
}

#[test]
fn recolour_block_replaces_an_exactly_matching_solid_paint() {
    let square = [0.0, 0.0, 2.0, 0.0, 2.0, -2.0, 0.0, -2.0];
    let shape = gvff([0.0, 0.0, 2.0, -2.0], &[[0.0, 0.0, 0.0, 1.0]], &[Item::fill(0, NON_ZERO, SQUARE_SEGMENTS, &square)]);
    let block = recolour_block([
        [0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
    ]);
    let recoloured = render_shapes(&[DrawCommand::ScaledShape { dest: scene(0, 0, 2, 2), gvff: &shape, recolour: Some(block) }]).unwrap();
    assert_eq!(pixel(&recoloured, 1, 1), [0, 255, 0, 255]);
    let original = render_shapes(&[DrawCommand::ScaledShape { dest: scene(0, 0, 2, 2), gvff: &shape, recolour: None }]).unwrap();
    assert_eq!(pixel(&original, 1, 1), [0, 0, 0, 255]);
    let mut malformed = block;
    malformed.0[..4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert_eq!(render_shapes(&[DrawCommand::ScaledShape { dest: scene(0, 0, 2, 2), gvff: &shape, recolour: Some(malformed) }]), Err(RasterError::InvalidRecolour(ShapeError::NonFinite)));
}

#[test]
fn stored_fill_rule_decides_the_doubly_wound_overlap() {
    // Stock sets 0x1101 from each record's +0x18 (I4:6da508-6da510); pixel (2, 2) samples local (2.5, 1.5).
    for (rule, overlap) in [(EVEN_ODD, [0, 0, 0, 0]), (NON_ZERO, [255, 0, 0, 255])] {
        let shape = gvff([0.0, 4.0, 4.0, 0.0], &[RED], &[Item::fill(0, rule, TWO_SQUARES, NESTED)]);
        let bytes = render_shapes(&[DrawCommand::ScaledShape { dest: scene(0, 0, 4, 4), gvff: &shape, recolour: None }]).unwrap();
        assert_eq!(pixel(&bytes, 2, 2), overlap);
        assert_eq!(pixel(&bytes, 0, 0), [255, 0, 0, 255]);
    }
}

#[test]
fn fill_rule_carries_over_while_consecutive_items_share_the_fill_paint() {
    // 0x1101 is reissued only when the fill paint changes (flag I4:6dab90-6dabb0, test I4:6da4ec-6da504).
    let corner = [0.0, 3.0, 1.0, 3.0, 1.0, 4.0, 0.0, 4.0];
    for (second_paint, overlap) in [(0, [255, 0, 0, 255]), (1, [0, 0, 0, 0])] {
        let shape = gvff([0.0, 4.0, 4.0, 0.0], &[RED, RED], &[
            Item::fill(0, NON_ZERO, SQUARE_SEGMENTS, &corner),
            Item::fill(second_paint, EVEN_ODD, TWO_SQUARES, NESTED),
        ]);
        let bytes = render_shapes(&[DrawCommand::ScaledShape { dest: scene(0, 0, 4, 4), gvff: &shape, recolour: None }]).unwrap();
        assert_eq!(pixel(&bytes, 2, 2), overlap, "second fill paint {second_paint}");
    }
}

#[test]
fn line_cap_carries_over_while_stroke_paint_dash_and_width_match() {
    // Cap 0x1111 is reissued only when stroke paint, dash index or width change (flag I4:6dabd0-6dac24, test I4:6da520-6da53c).
    // The second line runs local x 2..4 at y 1 (panel y 3), width 2; a square cap reaches pixel (1, 2), a butt cap does not.
    let first = [6.0, 3.0, 7.0, 3.0];
    let second = [2.0, 1.0, 4.0, 1.0];
    for (first_paint, cap_pixel) in [(0, [0, 0, 0, 0]), (1, [255, 0, 0, 255])] {
        let shape = gvff([0.0, 4.0, 8.0, 0.0], &[RED, RED], &[
            Item::stroke(first_paint, 2.0, BUTT, &[MOVE, LINE], &first),
            Item::stroke(0, 2.0, SQUARE, &[MOVE, LINE], &second),
        ]);
        let bytes = render_shapes(&[DrawCommand::ScaledShape { dest: scene(0, 0, 8, 4), gvff: &shape, recolour: None }]).unwrap();
        assert_eq!(pixel(&bytes, 1, 2), cap_pixel, "first stroke paint {first_paint}");
        assert_eq!(pixel(&bytes, 2, 2), [255, 0, 0, 255]);
    }
}

#[test]
fn stroke_width_scales_independently_on_each_axis() {
    // Bounds 4 × 2 onto 12 × 4: x scale 3, y scale 2. The width-1 butt line along local y = 1 from x = 1 to 3 covers
    // local [1, 3] × [0.5, 1.5], i.e. panel x [3, 9] and y [1, 3].
    let line = [1.0, 1.0, 3.0, 1.0];
    let shape = gvff([0.0, 2.0, 4.0, 0.0], &[RED], &[Item::stroke(0, 1.0, BUTT, &[MOVE, LINE], &line)]);
    let bytes = render_shapes(&[DrawCommand::ScaledShape { dest: scene(0, 0, 12, 4), gvff: &shape, recolour: None }]).unwrap();
    for y in 1..3 { for x in 3..9 { assert_eq!(pixel(&bytes, x, y), [255, 0, 0, 255], "({x}, {y})"); } }
    for (x, y) in [(2, 1), (9, 1), (3, 0), (3, 3), (8, 3)] { assert_eq!(pixel(&bytes, x, y), [0, 0, 0, 0], "({x}, {y})"); }
}

#[test]
fn malformed_or_unplaceable_scaled_shapes_are_typed_errors() {
    let square = [0.0, 0.0, 2.0, 0.0, 2.0, -2.0, 0.0, -2.0];
    let mut bad_magic = gvff([0.0, 0.0, 2.0, -2.0], &[RED], &[Item::fill(0, NON_ZERO, SQUARE_SEGMENTS, &square)]);
    bad_magic[0] = b'X';
    assert_eq!(render_shapes(&[DrawCommand::ScaledShape { dest: scene(0, 0, 2, 2), gvff: &bad_magic, recolour: None }]), Err(RasterError::Shape(ShapeError::Magic)));
    let flat = gvff([0.0, 0.0, 0.0, -2.0], &[RED], &[Item::fill(0, NON_ZERO, SQUARE_SEGMENTS, &square)]);
    assert_eq!(render_shapes(&[DrawCommand::ScaledShape { dest: scene(0, 0, 2, 2), gvff: &flat, recolour: None }]), Err(RasterError::DegenerateShapeBounds));
    let valid = gvff([0.0, 0.0, 2.0, -2.0], &[RED], &[Item::fill(0, NON_ZERO, SQUARE_SEGMENTS, &square)]);
    let pack = AssetPack::parse(EMPTY_PACK, AssetProvenance::Original { author: "test vectors" }).unwrap();
    let mut bytes = vec![0; BYTES];
    let commands = [DrawCommand::ScaledShape { dest: scene(0, 0, 2, 2), gvff: &valid, recolour: None }];
    assert_eq!(Surface::new(SurfaceId::Osd, &mut bytes).unwrap().render(DrawList::new(SurfaceId::Osd, &commands), &mut RasterResources::coverage8(&pack)), Err(RasterError::MissingVectorWorkspace));
}

fn render_scene(commands: &[DrawCommand<'_>]) -> Result<Vec<u8>, RasterError> {
    let mut vertices = [Vertex::EMPTY; 64];
    let mut strokes = [StrokePrimitive::EMPTY; 64];
    let mut workspace = VectorWorkspace { vertices: &mut vertices, strokes: &mut strokes, dash_budget: DashBudget::new(64), accuracy: CurveAccuracy::new(0.25).unwrap(), samples: SampleGrid::One };
    let mut bytes = vec![7; BYTES];
    Surface::new(SurfaceId::Osd, &mut bytes)?.render_scene(DrawList::new(SurfaceId::Osd, commands), &mut workspace)?;
    Ok(bytes)
}

#[test]
fn scene_render_matches_full_render_for_clips_and_scaled_draws() {
    let square = [0.0, 0.0, 2.0, 0.0, 2.0, -2.0, 0.0, -2.0];
    let shape = gvff([0.0, 0.0, 2.0, -2.0], &[RED], &[Item::fill(0, NON_ZERO, SQUARE_SEGMENTS, &square)]);
    let texels: Vec<u8> = (0..16u8).flat_map(|value| [value, 255 - value, 3, 255]).collect();
    let commands = [
        DrawCommand::Clip(panel_rect(1, 1, 6, 6)),
        DrawCommand::ScaledImage { dest: scene(0, 0, 8, 8), source: rgba_source(4, 4, 16, &texels) },
        DrawCommand::ResetClip,
        DrawCommand::ScaledShape { dest: scene(10, 0, 4, 4), gvff: &shape, recolour: None },
    ];
    assert_eq!(render_scene(&commands).unwrap(), render_shapes(&commands).unwrap());
}

#[test]
fn scene_render_refuses_commands_that_need_assets() {
    let fill = DrawCommand::Fill { rect: panel_rect(0, 0, 2, 2), colour: Colour { palette: PaletteIndex(1), alpha: Alpha::OPAQUE } };
    assert_eq!(render_scene(&[DrawCommand::ResetClip, fill]), Err(RasterError::UnsupportedSceneCommand));
}

#[test]
fn fractional_image_edges_scale_alpha_by_covered_area() {
    let red = [200, 0, 0, 255];
    let bytes = render_images(&[
        DrawCommand::ScaledImage { dest: fractional(0.0, 0.0, 2.5, 1.0), source: rgba_source(1, 1, 4, &red) },
        DrawCommand::ScaledImage { dest: fractional(10.25, 10.0, 1.5, 0.5), source: rgba_source(1, 1, 4, &red) },
    ]);
    assert_eq!(pixel(&bytes, 1, 0), [200, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 2, 0), [200, 0, 0, 128]);
    assert_eq!(pixel(&bytes, 3, 0), [0, 0, 0, 0]);
    // Covered areas 0.75 x 0.5 at both columns.
    assert_eq!(pixel(&bytes, 10, 10), [200, 0, 0, 96]);
    assert_eq!(pixel(&bytes, 11, 10), [200, 0, 0, 96]);
    assert_eq!(pixel(&bytes, 12, 10), [0, 0, 0, 0]);
    assert_eq!(pixel(&bytes, 10, 11), [0, 0, 0, 0]);
}

#[test]
fn edges_within_half_a_coverage_step_of_a_pixel_boundary_render_as_that_boundary() {
    let texels = [30, 60, 90, 255];
    let square = [0.0, 0.0, 414.0, 0.0, 414.0, -2.0, 0.0, -2.0];
    let shape = gvff([0.0, 0.0, 414.0, -2.0], &[RED], &[Item::fill(0, NON_ZERO, SQUARE_SEGMENTS, &square)]);
    let exact_image = render_images(&[DrawCommand::ScaledImage { dest: scene(3, 4, 22, 2), source: rgba_source(1, 1, 4, &texels) }]);
    let exact_shape = render_scene(&[DrawCommand::ScaledShape { dest: scene(3, 4, 22, 2), gvff: &shape, recolour: None }]).unwrap();
    // One f32 step either side of 22; the Run-3 shooting screen lowers a 414-unit shape to 22.000002.
    for width in [f32::from_bits(22.0f32.to_bits() - 1), f32::from_bits(22.0f32.to_bits() + 1)] {
        let image = render_images(&[DrawCommand::ScaledImage { dest: fractional(3.0, 4.0, width, 2.0), source: rgba_source(1, 1, 4, &texels) }]);
        assert_eq!(image, exact_image, "image width {width}");
        let shape = render_scene(&[DrawCommand::ScaledShape { dest: fractional(3.0, 4.0, width, 2.0), gvff: &shape, recolour: None }]).unwrap();
        assert_eq!(shape, exact_shape, "shape width {width}");
    }
}

#[test]
fn fractional_shape_edges_cover_the_samples_inside_them() {
    let square = [0.0, 0.0, 2.0, 0.0, 2.0, -2.0, 0.0, -2.0];
    let shape = gvff([0.0, 0.0, 2.0, -2.0], &[RED], &[Item::fill(0, NON_ZERO, SQUARE_SEGMENTS, &square)]);
    // Sample centres at x + 0.5: the edge at 2.6 contains column 2's, the edge at 2.4 does not.
    let wide = render_scene(&[DrawCommand::ScaledShape { dest: fractional(0.0, 0.0, 2.6, 2.0), gvff: &shape, recolour: None }]).unwrap();
    let narrow = render_scene(&[DrawCommand::ScaledShape { dest: fractional(0.0, 0.0, 2.4, 2.0), gvff: &shape, recolour: None }]).unwrap();
    assert_eq!(pixel(&wide, 2, 1), [255, 0, 0, 255]);
    assert_eq!(pixel(&narrow, 2, 1), [0, 0, 0, 0]);
    assert_eq!(pixel(&narrow, 1, 1), [255, 0, 0, 255]);
}
