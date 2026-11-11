extern crate std;
use super::*;
use super::super::shape_assets::{Gvff, StyledPaths};
use std::vec;
use std::vec::Vec;

struct Mask { pixels: [u8; 144] }
impl Mask {
    fn new() -> Self { Self { pixels: [0; 144] } }
    fn at(&self, x: usize, y: usize) -> u8 { self.pixels[y * 12 + x] }
}
impl CoverageSink for Mask {
    fn cover(&mut self, x: i32, y: i32, coverage: u8) {
        assert!((0..12).contains(&x) && (0..12).contains(&y));
        self.pixels[y as usize * 12 + x as usize] = coverage;
    }
}
fn clip() -> ClipBounds { ClipBounds::new(0, 0, 12, 12).unwrap() }
fn accuracy() -> CurveAccuracy { CurveAccuracy::new(0.01).unwrap() }
fn styled(commands: &[u8], coordinates: &[f32]) -> (Vec<u8>, Vec<u8>) {
    let mut segments = b"FSBM".to_vec();
    segments.extend_from_slice(commands);
    let mut values = Vec::new();
    for value in [2.0, 4.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, commands.len() as f32].into_iter().chain(coordinates.iter().copied()) {
        values.extend_from_slice(&value.to_le_bytes());
    }
    (segments, values)
}
fn style(cap: LineCap, join: LineJoin) -> StrokeStyle<'static> {
    StrokeStyle { metrics: StrokeMetrics::new(2.0, 4.0).unwrap(), cap, join, dash: None }
}

#[test]
fn implicit_close_fills_only_triangle_in_clip() {
    let (segments, values) = styled(b"MLL", &[1.0, 1.0, 9.0, 1.0, 1.0, 9.0]);
    let item = StyledPaths::parse(&segments, &values).unwrap().items().next().unwrap();
    let mut workspace = [Vertex::EMPTY; 16];
    let path = flatten_path(item.path, &mut workspace, accuracy()).unwrap();
    let mut mask = Mask::new();
    fill(path, FillRule::NonZero, ClipBounds::new(2, 2, 6, 6).unwrap(), SampleGrid::One, &mut mask);
    assert_eq!(mask.at(2, 2), 255);
    assert_eq!(mask.at(5, 5), 0);
    assert_eq!(mask.at(1, 2), 0);
    assert_eq!(mask.at(2, 6), 0);
}

#[test]
fn nested_same_direction_contours_distinguish_fill_rules() {
    let (segments, values) = styled(b"MLLLEMLLLE", &[1.0,1.0, 10.0,1.0, 10.0,10.0, 1.0,10.0, 3.0,3.0, 8.0,3.0, 8.0,8.0, 3.0,8.0]);
    let item = StyledPaths::parse(&segments, &values).unwrap().items().next().unwrap();
    let mut workspace = [Vertex::EMPTY; 16];
    let path = flatten_path(item.path, &mut workspace, accuracy()).unwrap();
    let mut nonzero = Mask::new();
    let mut evenodd = Mask::new();
    fill(path, FillRule::NonZero, clip(), SampleGrid::One, &mut nonzero);
    fill(path, FillRule::EvenOdd, clip(), SampleGrid::One, &mut evenodd);
    assert_eq!(nonzero.at(5, 5), 255);
    assert_eq!(evenodd.at(5, 5), 0);
    assert_eq!(evenodd.at(2, 2), 255);
    assert_eq!(evenodd.at(10, 2), 0);
}

#[test]
fn uniform_grid_reports_fractional_edge_coverage() {
    let (segments, values) = styled(b"MLLLE", &[1.5,1.0, 4.0,1.0, 4.0,4.0, 1.5,4.0]);
    let item = StyledPaths::parse(&segments, &values).unwrap().items().next().unwrap();
    let mut workspace = [Vertex::EMPTY; 16];
    let path = flatten_path(item.path, &mut workspace, accuracy()).unwrap();
    let mut mask = Mask::new();
    fill(path, FillRule::NonZero, clip(), SampleGrid::Four, &mut mask);
    assert_eq!(mask.at(1, 2), 128);
    assert_eq!(mask.at(2, 2), 255);
    assert_eq!(mask.at(0, 2), 0);
}

#[test]
fn cubic_encloses_area_below_its_apex_not_its_control_hull() {
    let (segments, values) = styled(b"MCE", &[1.0,1.0, 1.0,7.0, 7.0,7.0, 7.0,1.0]);
    let item = StyledPaths::parse(&segments, &values).unwrap().items().next().unwrap();
    let mut workspace = [Vertex::EMPTY; 128];
    let path = flatten_path(item.path, &mut workspace, accuracy()).unwrap();
    let mut mask = Mask::new();
    fill(path, FillRule::NonZero, clip(), SampleGrid::Four, &mut mask);
    assert_eq!(mask.at(4, 3), 255);
    assert_eq!(mask.at(4, 6), 0);
    assert_eq!(mask.at(0, 3), 0);
}

#[test]
fn stroke_caps_extend_only_their_selected_geometry() {
    let (segments, values) = styled(b"ML", &[3.0,4.0, 7.0,4.0]);
    let item = StyledPaths::parse(&segments, &values).unwrap().items().next().unwrap();
    let mut vertices = [Vertex::EMPTY; 8];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut primitives = [StrokePrimitive::EMPTY; 16];
    let mut butt = Mask::new();
    let mut square = Mask::new();
    stroke(path, style(LineCap::Butt, LineJoin::Bevel), &mut primitives, DashBudget::new(64), clip(), SampleGrid::One, &mut butt).unwrap();
    stroke(path, style(LineCap::Square, LineJoin::Bevel), &mut primitives, DashBudget::new(64), clip(), SampleGrid::One, &mut square).unwrap();
    assert_eq!(butt.at(3, 3), 255);
    assert_eq!(butt.at(2, 3), 0);
    assert_eq!(square.at(2, 3), 255);
    assert_eq!(square.at(7, 4), 255);
    assert_eq!(square.at(4, 5), 0);
}

#[test]
fn miter_limit_selects_bevel_at_the_outer_corner() {
    let (segments, values) = styled(b"MLL", &[2.0,7.0, 6.0,7.0, 6.0,2.0]);
    let item = StyledPaths::parse(&segments, &values).unwrap().items().next().unwrap();
    let mut vertices = [Vertex::EMPTY; 8];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut primitives = [StrokePrimitive::EMPTY; 16];
    let mut miter = Mask::new();
    let mut bevel = Mask::new();
    stroke(path, style(LineCap::Butt, LineJoin::Miter), &mut primitives, DashBudget::new(64), clip(), SampleGrid::Four, &mut miter).unwrap();
    let limited = StrokeStyle { metrics: StrokeMetrics::new(2.0, 1.0).unwrap(), ..style(LineCap::Butt, LineJoin::Miter) };
    stroke(path, limited, &mut primitives, DashBudget::new(64), clip(), SampleGrid::Four, &mut bevel).unwrap();
    assert_eq!(miter.at(6, 7), 255);
    assert_eq!(bevel.at(6, 7), 159);
}

fn dashed_asset(lengths: &[f32], phase: f32) -> Vec<u8> {
    path_asset(&[2, 4], &[1.0, 4.0, 11.0, 4.0], lengths, phase)
}

fn path_asset(commands: &[u8], coordinates: &[f32], lengths: &[f32], phase: f32) -> Vec<u8> {
    let mut bytes = vec![0; 56];
    bytes[0..4].copy_from_slice(&0x46465647u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&1u32.to_le_bytes());
    bytes[32..36].copy_from_slice(&1u32.to_le_bytes());
    let mut paint = [0u8; 36];
    paint[..4].copy_from_slice(&0x1b00u32.to_le_bytes());
    bytes.extend_from_slice(&paint);
    let mut item = [0u8; 52];
    for (offset, value) in [(0,1u32),(20,0),(24,0x1901),(28,0x1802),(32,0x1700),(36,2.0f32.to_bits()),(40,4.0f32.to_bits()),(44,commands.len() as u32),(48,coordinates.len() as u32 * 4)] {
        item[offset..offset+4].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&item);
    bytes.extend_from_slice(commands);
    bytes.resize(bytes.len() + (commands.len() & !3) + 4 - commands.len(), 0);
    for number in coordinates { bytes.extend_from_slice(&number.to_le_bytes()); }
    let mut dash = [0u8; 32];
    dash[..4].copy_from_slice(&(lengths.len() as u32).to_le_bytes());
    dash[4..8].copy_from_slice(&phase.to_le_bytes());
    for (index, value) in lengths.iter().enumerate() { dash[8+index*4..12+index*4].copy_from_slice(&value.to_le_bytes()); }
    bytes.extend_from_slice(&dash);
    bytes
}

#[test]
fn odd_dash_pattern_ignores_final_element_and_applies_phase() {
    let bytes = dashed_asset(&[2.0,3.0,7.0], 1.0);
    let item = Gvff::parse(&bytes).unwrap().items().next().unwrap().unwrap();
    let mut vertices = [Vertex::EMPTY; 8];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut primitives = [StrokePrimitive::EMPTY; 32];
    let mut mask = Mask::new();
    stroke(path, StrokeStyle { dash: item.dash, ..style(LineCap::Butt, LineJoin::Bevel) }, &mut primitives, DashBudget::new(64), clip(), SampleGrid::One, &mut mask).unwrap();
    assert_eq!(mask.at(1, 4), 255);
    assert_eq!(mask.at(2, 4), 0);
    assert_eq!(mask.at(3, 4), 0);
    assert_eq!(mask.at(4, 4), 0);
    assert_eq!(mask.at(5, 4), 255);
    assert_eq!(mask.at(6, 4), 255);
    assert_eq!(mask.at(7, 4), 0);
    assert_eq!(mask.at(10, 4), 255);
}

#[test]
fn zero_length_dash_with_round_caps_produces_separate_dots() {
    let bytes = dashed_asset(&[0.0,4.0], 0.0);
    let item = Gvff::parse(&bytes).unwrap().items().next().unwrap().unwrap();
    let mut vertices = [Vertex::EMPTY; 8];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut primitives = [StrokePrimitive::EMPTY; 32];
    let mut mask = Mask::new();
    stroke(path, StrokeStyle { dash: item.dash, ..style(LineCap::Round, LineJoin::Bevel) }, &mut primitives, DashBudget::new(64), clip(), SampleGrid::One, &mut mask).unwrap();
    assert_eq!(mask.at(1, 4), 255);
    assert_eq!(mask.at(3, 4), 0);
    assert_eq!(mask.at(5, 4), 255);
}

#[test]
fn rejected_geometry_never_emits_partial_coverage() {
    let (segments, values) = styled(b"MCE", &[1.0,1.0, 1.0,7.0, 7.0,7.0, 7.0,1.0]);
    let item = StyledPaths::parse(&segments, &values).unwrap().items().next().unwrap();
    assert_eq!(flatten_path(item.path, &mut [Vertex::EMPTY; 1], accuracy()).unwrap_err(), VectorError::WorkspaceTooSmall);
    let mut vertices = [Vertex::EMPTY; 128];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut mask = Mask::new();
    assert_eq!(stroke(path, style(LineCap::Round, LineJoin::Round), &mut [], DashBudget::new(64), clip(), SampleGrid::Four, &mut mask), Err(VectorError::WorkspaceTooSmall));
    assert!(mask.pixels.iter().all(|pixel| *pixel == 0));
    assert_eq!(StrokeMetrics::new(-1.0, 4.0), Err(VectorError::InvalidStroke));
    assert!(CurveAccuracy::new(f32::NAN).is_none());
    assert!(CurveAccuracy::new(0.0).is_none());
}

#[test]
fn natural_placement_reflects_y_before_fractional_pixel_sampling() {
    let (segments, values) = styled(b"MLLLE", &[1.5,1.0, 4.0,1.0, 4.0,4.0, 1.5,4.0]);
    let item = StyledPaths::parse(&segments, &values).unwrap().items().next().unwrap();
    let mut asset = dashed_asset(&[2.0], 0.0);
    asset[40..44].copy_from_slice(&1.0f32.to_le_bytes());
    let bounds = Gvff::parse(&asset).unwrap().bounds();
    let placement = NaturalPlacement::natural(super::super::PanelPoint::new(2, 8).unwrap(), bounds);
    let mut vertices = [Vertex::EMPTY; 16];
    let path = flatten_path_placed(item.path, &mut vertices, accuracy(), placement).unwrap();
    let mut mask = Mask::new();
    fill(path, FillRule::NonZero, clip(), SampleGrid::Four, &mut mask);
    assert_eq!(mask.at(2, 5), 128);
    assert_eq!(mask.at(3, 5), 255);
    assert_eq!(mask.at(3, 2), 0);
    assert_eq!(mask.at(3, 8), 0);
}

#[test]
fn finite_large_coordinates_refuse_unrepresentable_dash_progress_before_coverage() {
    let bytes = path_asset(&[2, 4], &[f32::MAX, 0.0, 0.0, 0.0], &[1.0, 1.0], 0.0);
    let item = Gvff::parse(&bytes).unwrap().items().next().unwrap().unwrap();
    let mut vertices = [Vertex::EMPTY; 4];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut mask = Mask::new();
    let result = stroke(path, StrokeStyle { dash: item.dash, ..style(LineCap::Butt, LineJoin::Miter) }, &mut [], DashBudget::new(1), clip(), SampleGrid::One, &mut mask);
    assert_eq!(result, Err(VectorError::SubdivisionLimit));
    assert!(mask.pixels.iter().all(|pixel| *pixel == 0));
}

#[test]
fn dash_interval_budget_is_independent_of_primitive_capacity() {
    let bytes = dashed_asset(&[0.25, 0.25], 0.0);
    let item = Gvff::parse(&bytes).unwrap().items().next().unwrap().unwrap();
    let mut vertices = [Vertex::EMPTY; 4];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut primitives = [StrokePrimitive::EMPTY; 64];
    let mut mask = Mask::new();
    let result = stroke(path, StrokeStyle { dash: item.dash, ..style(LineCap::Butt, LineJoin::Miter) }, &mut primitives, DashBudget::new(2), clip(), SampleGrid::One, &mut mask);
    assert_eq!(result, Err(VectorError::DashBudgetExceeded));
    assert!(mask.pixels.iter().all(|pixel| *pixel == 0));
}

#[test]
fn zero_gap_keeps_distinct_dashes_capped_at_a_corner() {
    let bytes = path_asset(&[2, 4, 4], &[3.0, 4.0, 5.0, 4.0, 5.0, 2.0], &[2.0, 0.0], 0.0);
    let item = Gvff::parse(&bytes).unwrap().items().next().unwrap().unwrap();
    let mut vertices = [Vertex::EMPTY; 4];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut primitives = [StrokePrimitive::EMPTY; 16];
    let mut mask = Mask::new();
    stroke(path, StrokeStyle { dash: item.dash, ..style(LineCap::Butt, LineJoin::Miter) }, &mut primitives, DashBudget::new(16), clip(), SampleGrid::One, &mut mask).unwrap();
    assert_eq!(mask.at(4, 3), 255);
    assert_eq!(mask.at(5, 4), 0);
}

#[test]
fn zero_square_dash_follows_diagonal_tangent() {
    let bytes = path_asset(&[2, 4], &[4.25, 4.5, 8.25, 8.5], &[0.0, 4.0], 0.0);
    let item = Gvff::parse(&bytes).unwrap().items().next().unwrap().unwrap();
    let mut vertices = [Vertex::EMPTY; 4];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut primitives = [StrokePrimitive::EMPTY; 16];
    let mut mask = Mask::new();
    stroke(path, StrokeStyle { dash: item.dash, ..style(LineCap::Square, LineJoin::Miter) }, &mut primitives, DashBudget::new(16), clip(), SampleGrid::One, &mut mask).unwrap();
    assert_eq!(mask.at(5, 4), 255);
    assert_eq!(mask.at(3, 3), 0);
}

#[test]
fn zero_square_dash_at_vertex_uses_both_tangents_and_join() {
    let bytes = path_asset(&[2, 4, 4], &[2.0, 4.0, 6.0, 4.0, 6.0, 8.0], &[0.0, 4.0], 0.0);
    let item = Gvff::parse(&bytes).unwrap().items().next().unwrap().unwrap();
    let mut vertices = [Vertex::EMPTY; 4];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut primitives = [StrokePrimitive::EMPTY; 32];
    let mut mask = Mask::new();
    stroke(path, StrokeStyle { dash: item.dash, ..style(LineCap::Square, LineJoin::Miter) }, &mut primitives, DashBudget::new(16), clip(), SampleGrid::One, &mut mask).unwrap();
    assert_eq!(mask.at(5, 3), 255);
    assert_eq!(mask.at(6, 4), 255);
    assert_eq!(mask.at(6, 3), 255);
    assert_eq!(mask.at(7, 3), 0);
}

#[test]
fn zero_round_dash_at_vertex_retains_miter_outside_round_caps() {
    let bytes = path_asset(&[2, 4, 4], &[2.0, 4.0, 6.0, 4.0, 6.0, 8.0], &[0.0, 4.0], 0.0);
    let item = Gvff::parse(&bytes).unwrap().items().next().unwrap().unwrap();
    let mut vertices = [Vertex::EMPTY; 4];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut primitives = [StrokePrimitive::EMPTY; 32];
    let mut mask = Mask::new();
    stroke(path, StrokeStyle { dash: item.dash, ..style(LineCap::Round, LineJoin::Miter) }, &mut primitives, DashBudget::new(16), clip(), SampleGrid::Four, &mut mask).unwrap();
    assert_eq!(mask.at(6, 3), 255);
}

#[test]
fn terminal_zero_dash_preserves_phase_for_the_next_subpath() {
    let bytes = path_asset(&[2, 4, 2, 4], &[1.0, 1.0, 5.0, 1.0, 1.0, 5.0, 5.0, 5.0], &[0.0, 4.0], 0.0);
    let item = Gvff::parse(&bytes).unwrap().items().next().unwrap().unwrap();
    let mut vertices = [Vertex::EMPTY; 8];
    let path = flatten_path(item.path, &mut vertices, accuracy()).unwrap();
    let mut primitives = [StrokePrimitive::EMPTY; 16];
    let mut mask = Mask::new();
    stroke(path, StrokeStyle { dash: item.dash, ..style(LineCap::Round, LineJoin::Miter) }, &mut primitives, DashBudget::new(16), clip(), SampleGrid::One, &mut mask).unwrap();
    assert_eq!(mask.at(1, 1), 255);
    assert_eq!(mask.at(5, 1), 255);
    assert_eq!(mask.at(1, 5), 255);
    assert_eq!(mask.at(5, 5), 255);
    assert_eq!(mask.at(3, 1), 0);
    assert_eq!(mask.at(3, 5), 0);
}
