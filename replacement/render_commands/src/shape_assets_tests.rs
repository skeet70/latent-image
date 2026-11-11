use super::*;
use super::super::{ImageId, resources::DataRegion};

fn put(bytes: &mut [u8], offset: usize, value: u32) { bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
fn float(bytes: &mut [u8], offset: usize, value: f32) { put(bytes, offset, value.to_bits()); }
fn fixture() -> [u8; 324] {
    let mut bytes = [0; 324];
    put(&mut bytes, 0, 0x46465647);
    put(&mut bytes, 20, 1);
    put(&mut bytes, 24, 3);
    put(&mut bytes, 28, 1);
    put(&mut bytes, 32, 1);
    float(&mut bytes, 48, 8.0);
    float(&mut bytes, 52, -6.0);
    put(&mut bytes, 56, 2);
    float(&mut bytes, 64, 1.0);
    float(&mut bytes, 76, 1.0);
    float(&mut bytes, 80, 1.0);
    float(&mut bytes, 88, 1.0);
    float(&mut bytes, 96, 0.5);
    for (offset, kind) in [(100, 0x1b00), (136, 0x1b01), (172, 0x1b02)] {
        put(&mut bytes, offset + 4, kind);
    }
    float(&mut bytes, 108, 0.25);
    float(&mut bytes, 112, 0.5);
    float(&mut bytes, 116, 0.75);
    float(&mut bytes, 120, 1.0);
    float(&mut bytes, 152, 8.0);
    float(&mut bytes, 180, 4.0);
    float(&mut bytes, 184, -3.0);
    float(&mut bytes, 188, 3.0);
    float(&mut bytes, 192, -2.0);
    float(&mut bytes, 196, 5.0);
    put(&mut bytes, 208, 3);
    put(&mut bytes, 216, 1);
    put(&mut bytes, 224, 1);
    put(&mut bytes, 232, 0x1900);
    put(&mut bytes, 236, 0x1802);
    put(&mut bytes, 240, 0x1701);
    float(&mut bytes, 244, 2.0);
    float(&mut bytes, 248, 4.0);
    put(&mut bytes, 252, 4);
    put(&mut bytes, 256, 24);
    bytes[260..264].copy_from_slice(&[2, 4, 5, 0]);
    for (index, value) in [1.0, -1.0, 7.0, -1.0, 0.0, -4.0].iter().enumerate() { float(&mut bytes, 268 + index * 4, *value); }
    put(&mut bytes, 292, 2);
    float(&mut bytes, 296, 1.5);
    float(&mut bytes, 300, 3.0);
    float(&mut bytes, 304, 2.0);
    bytes
}

#[test]
fn gvff_retains_paint_geometry_path_modes_and_dash_data() {
    let bytes = fixture();
    let file = Gvff::parse(&bytes).unwrap();
    assert_eq!(file.bounds().map(Scalar::value), [0.0, 0.0, 8.0, -6.0]);
    assert_eq!(file.consumed_bytes(), 324);
    let mut paints = file.paints();
    let Paint::Solid { rgba } = paints.next().unwrap() else { panic!() };
    assert_eq!(rgba.iter().map(Scalar::value).collect::<std::vec::Vec<_>>(), [0.25, 0.5, 0.75, 1.0]);
    let Paint::Linear { endpoints, ramp } = paints.next().unwrap() else { panic!() };
    assert_eq!(ramp.value(), 0);
    assert_eq!(endpoints.get(2).unwrap().value(), 8.0);
    let Paint::Radial { circle, .. } = paints.next().unwrap() else { panic!() };
    assert_eq!(circle.iter().map(Scalar::value).collect::<std::vec::Vec<_>>(), [4.0, -3.0, 3.0, -2.0, 5.0]);
    let ramp = file.ramps().next().unwrap();
    let stops = ramp.stops().collect::<std::vec::Vec<_>>();
    assert_eq!(stops[0].offset.value(), 0.0);
    assert_eq!(stops[0].rgba.map(Scalar::value), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(stops[1].offset.value(), 1.0);
    assert_eq!(stops[1].rgba.map(Scalar::value), [0.0, 1.0, 0.0, 0.5]);
    let item = file.items().next().unwrap().unwrap();
    assert_eq!(item.mode, PaintMode::StrokeAndFill);
    assert_eq!(item.fill_rule, FillRule::EvenOdd);
    assert_eq!(item.cap, LineCap::Round);
    assert_eq!(item.join, LineJoin::Bevel);
    assert_eq!(item.stroke.unwrap().value(), 0);
    assert_eq!(item.fill.unwrap().value(), 1);
    assert_eq!(item.stroke_width.value(), 2.0);
    assert_eq!(item.miter_limit.value(), 4.0);
    assert!(item.source_over);
    let dash = item.dash.unwrap();
    assert_eq!(dash.phase.value(), 1.5);
    assert_eq!(dash.lengths.iter().map(Scalar::value).collect::<std::vec::Vec<_>>(), [3.0, 2.0]);
    let segments = item.path.segments().collect::<std::vec::Vec<_>>();
    assert_eq!(segments.len(), 4);
    assert_eq!(segments[0].kind, SegmentKind::Move);
    assert_eq!(segments[0].coordinates.get(1).unwrap().value(), -1.0);
    assert_eq!(segments[2].mode, CoordinateMode::Relative);
    assert_eq!(segments[2].coordinates.get(1).unwrap().value(), -4.0);
    assert_eq!(segments[3].kind, SegmentKind::Close);
}

#[test]
fn aligned_segment_stream_keeps_extra_padding_and_ignores_absent_dash_table() {
    let mut bytes = fixture();
    put(&mut bytes, 32, 0x030f0000);
    put(&mut bytes, 228, 0x29a);
    let file = Gvff::parse(&bytes[..292]).unwrap();
    assert_eq!(file.consumed_bytes(), 292);
    let item = file.items().next().unwrap().unwrap();
    assert!(item.dash.is_none());
    assert_eq!(item.path.segments().next().unwrap().coordinates.get(0).unwrap().value(), 1.0);
    put(&mut bytes, 32, 1);
    put(&mut bytes, 228, 1);
    assert!(Gvff::parse(&bytes[..292]).unwrap().items().next().unwrap().unwrap().dash.is_none());
}

#[test]
fn malformed_and_truncated_data_never_produces_a_partial_asset() {
    let bytes = fixture();
    for end in 0..bytes.len() { assert!(Gvff::parse(&bytes[..end]).is_err(), "prefix {end}"); }
    let mut invalid = bytes;
    put(&mut invalid, 164, 1);
    assert_eq!(Gvff::parse(&invalid).unwrap_err(), ShapeError::RampIndex);
    invalid = bytes;
    put(&mut invalid, 224, 3);
    assert_eq!(Gvff::parse(&invalid).unwrap_err(), ShapeError::PaintIndex);
    invalid = bytes;
    invalid[260] = 0xfe;
    assert_eq!(Gvff::parse(&invalid).unwrap_err(), ShapeError::InvalidSegment(0xfe));
    invalid = bytes;
    float(&mut invalid, 268, f32::NAN);
    assert_eq!(Gvff::parse(&invalid).unwrap_err(), ShapeError::NonFinite);
    invalid = bytes;
    put(&mut invalid, 292, 7);
    assert_eq!(Gvff::parse(&invalid).unwrap_err(), ShapeError::DashCount);
    invalid = bytes;
    put(&mut invalid, 256, 20);
    assert_eq!(Gvff::parse(&invalid).unwrap_err(), ShapeError::CoordinateCount);
}

#[test]
fn styled_records_advance_separate_segment_and_float_streams() {
    let mut values = [0; 120];
    for (index, value) in [2.0, 4.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 3.0, 1.0, 2.0, 3.0, 4.0].iter().enumerate() { float(&mut values, index * 4, *value); }
    for (index, value) in [1.0, 6.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0].iter().enumerate() { float(&mut values, 52 + index * 4, *value); }
    let paths = StyledPaths::parse(b"ESRMMLEF-BBMC", &values).unwrap();
    assert_eq!(paths.consumed_value_bytes(), 120);
    let mut items = paths.items();
    let first = items.next().unwrap();
    assert_eq!(first.mode, PaintMode::StrokeAndFill);
    assert_eq!(first.fill_rule, FillRule::EvenOdd);
    assert_eq!(first.cap, LineCap::Round);
    assert_eq!(first.join, LineJoin::Miter);
    assert_eq!(first.stroke_rgb.map(Scalar::value), [1.0, 0.0, 0.0]);
    assert_eq!(first.fill_rgb.map(Scalar::value), [0.0, 1.0, 0.0]);
    let second = items.next().unwrap();
    assert_eq!(second.mode, PaintMode::Fill);
    assert_eq!(second.cap, LineCap::Butt);
    assert_eq!(second.join, LineJoin::Bevel);
    let cubic = second.path.segments().nth(1).unwrap();
    assert_eq!(cubic.kind, SegmentKind::Cubic);
    assert_eq!(cubic.coordinates.iter().map(Scalar::value).collect::<std::vec::Vec<_>>(), [4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    assert!(items.next().is_none());
    assert!(StyledPaths::parse(b"ESRMMLEF-BBMC", &values[..119]).is_err());
}

#[test]
fn both_catalog_kinds_decode_the_data_pointer_not_a_request() {
    let bytes = fixture();
    let regions = [DataRegion { base: DataAddress(0x130000), bytes: &bytes }];
    let memory = ResourceMemory::new(&regions).unwrap();
    for kind in [BitmapKind::ShapeKindOne, BitmapKind::ShapeKindTwo] {
        let entry = BitmapEntry { image: ImageId::new(1).unwrap(), width: 8, height: 6, kind, data: DataAddress(0x130000) };
        let asset = ShapeAsset::decode(entry, &memory).unwrap();
        assert_eq!(asset.entry, entry);
        let ShapeContent::Gradient { file, recolour } = asset.content else { panic!() };
        assert!(recolour.is_none());
        assert_eq!(file.item_count(), 1);
        let missing = BitmapEntry { data: DataAddress(0x140000), ..entry };
        assert_eq!(ShapeAsset::decode(missing, &memory).unwrap_err(), ShapeError::Resource(ResourceError::MissingData(DataAddress(0x140000))));
    }
}

#[test]
fn request_parameter_selects_gradient_and_exact_colour_mapping() {
    let bytes = fixture();
    let regions = [DataRegion { base: DataAddress(0x2000), bytes: &bytes }];
    let memory = ResourceMemory::new(&regions).unwrap();
    let entry = BitmapEntry { image: ImageId::new(2).unwrap(), width: 8, height: 6, kind: BitmapKind::ShapeKindOne, data: DataAddress(0x8000) };
    let mut request = [0; 116];
    put(&mut request, 8, 0x2000);
    put(&mut request, 16, 2);
    for (index, value) in [0.25, 0.5, 0.75, 1.0, 1.0, 0.0, 0.0, 0.5].iter().enumerate() { float(&mut request, 20 + index * 4, *value); }
    let asset = ShapeAsset::from_request(entry, &request, &memory).unwrap();
    let ShapeContent::Gradient { file, recolour: Some(recolour) } = asset.content else { panic!() };
    let Paint::Solid { rgba } = file.paints().next().unwrap() else { panic!() };
    let colour = core::array::from_fn(|index| rgba.get(index).unwrap());
    assert_eq!(recolour.map(colour).map(Scalar::value), [1.0, 0.0, 0.0, 0.5]);
    let unmatched = [Scalar(0.7); 4];
    assert_eq!(recolour.map(unmatched), unmatched);
    put(&mut request, 16, 1);
    assert!(matches!(ShapeAsset::from_request(entry, &request[..20], &memory).unwrap().content, ShapeContent::Gradient { recolour: None, .. }));
}

extern crate std;

#[test]
fn natural_placement_maps_asset_bounds_to_top_left_panel_coordinates() {
    let origin = super::super::PanelPoint::new(30, 40).unwrap();
    let placement = NaturalPlacement::natural(origin, [Scalar(2.0), Scalar(8.0), Scalar(12.0), Scalar(-4.0)]);
    assert_eq!(placement.map_x(2.0), 30.0);
    assert_eq!(placement.map_y(8.0), 40.0);
    assert_eq!(placement.map_x(12.0), 40.0);
    assert_eq!(placement.map_y(-4.0), 52.0);
    assert_eq!(placement.unmap_x(35.0), 7.0);
    assert_eq!(placement.unmap_y(50.0), -2.0);
}

#[test]
fn styled_placement_uses_constructor_height_without_tight_bounds_rebasing() {
    let origin = super::super::PanelPoint::new(30, 40).unwrap();
    let entry = BitmapEntry { image: ImageId::new(2).unwrap(), width: 80, height: 60, kind: BitmapKind::ShapeKindOne, data: DataAddress(0x2000) };
    let placement = NaturalPlacement::styled(origin, entry);
    assert_eq!(placement.map_x(7.0), 37.0);
    assert_eq!(placement.map_y(9.0), 91.0);
    assert_eq!(placement.map_y(60.0), 40.0);
    assert_eq!(placement.unmap_y(91.0), 9.0);
}

#[test]
fn native_palette_recolours_only_exact_black_red_and_white_sources() {
    use super::super::commands::{Rgba, ShapePalette, ShapeSecondarySource};
    let palette = ShapePalette {
        primary: Rgba { red: 255, green: 0, blue: 128, alpha: 64 },
        secondary: Rgba { red: 0, green: 255, blue: 0, alpha: 255 },
        secondary_source: ShapeSecondarySource::Red,
    };
    let black = [Scalar(0.0), Scalar(0.0), Scalar(0.0), Scalar(1.0)];
    let red = [Scalar(1.0), Scalar(0.0), Scalar(0.0), Scalar(1.0)];
    let white = [Scalar(1.0); 4];
    let transparent = [0.0; 4];
    let recolour = Recolour::from_palette(palette);
    assert_eq!(recolour.map(black).map(Scalar::value), [1.0, 0.0, 0.5019608, 0.2509804]);
    assert_eq!(recolour.map(red).map(Scalar::value), [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(recolour.map(white).map(Scalar::value), transparent);
    let recolour = Recolour::from_palette(ShapePalette { secondary_source: ShapeSecondarySource::White, ..palette });
    assert_eq!(recolour.map(red).map(Scalar::value), transparent);
    assert_eq!(recolour.map(white).map(Scalar::value), [0.0, 1.0, 0.0, 1.0]);
    let translucent_black = [Scalar(0.0), Scalar(0.0), Scalar(0.0), Scalar(0.5)];
    assert_eq!(recolour.map(translucent_black), translucent_black);
}
