use super::{select_samples, CaptureStatistics, SampleDimensions, FramePixels};
use super::super::profiles::{ProfileId, ProfileTables, FrameGeometry, PixelFormat};
use crate::backend::ConfigImages;

fn tables<'a>(entries: &'a mut [u8; 720], classes: &'a mut [u8; 816], configured: &'a [u8; 272], expanded: &'a mut [u8; 272]) -> ProfileTables<'a> {
    entries[0..4].copy_from_slice(&1u32.to_le_bytes());
    classes[0..2].copy_from_slice(&128u16.to_le_bytes()); classes[2..4].copy_from_slice(&128u16.to_le_bytes());
    for (index, key) in [0x10u32,0x12,0x14,0x16].into_iter().enumerate() { expanded[index * 4..index * 4 + 4].copy_from_slice(&key.to_le_bytes()); }
    ProfileTables::adopt(entries, classes, configured, expanded).unwrap()
}
#[test]
fn grid_quadrants_publish_spatially_selected_cells() {
    let mut entries = [0;720]; let mut classes = [0;816]; let configured = [0;272]; let mut expanded = [0;272];
    let profiles = tables(&mut entries, &mut classes, &configured, &mut expanded);
    let mut pixels = [0;768];
    for y in 0..6 { for x in 0..6 { let offset = y * 128 + x * 2; pixels[offset..offset + 2].copy_from_slice(&((y * 6 + x) as u16).to_le_bytes()); } }
    let raster = FramePixels { bytes: &pixels, format: PixelFormat::Unpacked16, geometry: FrameGeometry { pitch:128,size:768,width:6,height:6 } };
    let accumulators = [0;1032]; let dimensions = SampleDimensions {height:0,width:0};
    let statistics = CaptureStatistics {accumulators:&accumulators,primary:dimensions,auxiliary:dimensions};
    for (mode, expected) in [(0xa0,[0,1,2,6,7,8,12,13,14]),(0xa1,[3,4,5,9,10,11,15,16,17]),(0xa2,[18,19,20,24,25,26,30,31,32]),(0xa3,[21,22,23,27,28,29,33,34,35])] {
        let mut bytes = [0;256]; bytes[0x82] = mode; bytes[0x15] = 96; bytes[0x17] = 96;
        let mut configuration = ConfigImages::adopt(&[],&[],&[],&mut bytes,&[]);
        select_samples(&mut configuration,statistics,raster,&profiles,ProfileId::parse(0).unwrap()).unwrap();
        for (index,value) in expected.into_iter().enumerate() { assert_eq!(configuration.byte(0x83 + index as u32 * 2),Some(0)); assert_eq!(configuration.byte(0x84 + index as u32 * 2),Some(value)); }
    }
}
#[test]
fn total_samples_duplicate_the_four_plane_mean_into_both_outputs() {
    let mut entries = [0;720]; let mut classes = [0;816]; let configured = [0;272]; let mut expanded = [0;272];
    let profiles = tables(&mut entries,&mut classes,&configured,&mut expanded);
    let mut sums = [0;1032]; sums[0x404..].copy_from_slice(&[11,22,33,34]);
    let mut bytes = [0xff;256]; bytes[0x82] = 1;
    let mut configuration = ConfigImages::adopt(&[],&[],&[],&mut bytes,&[]);
    select_samples(&mut configuration,CaptureStatistics {accumulators:&sums,primary:SampleDimensions {height:0,width:0},auxiliary:SampleDimensions {height:2,width:5}},FramePixels {bytes:&[],format:PixelFormat::Unpacked16,geometry:FrameGeometry::default()},&profiles,ProfileId::parse(0).unwrap()).unwrap();
    for index in 0..8 { assert_eq!(configuration.byte(0x83 + index * 2),Some(0)); assert_eq!(configuration.byte(0x84 + index * 2),Some(10)); }
    assert_eq!(configuration.byte(0x93),Some(0)); assert_eq!(configuration.byte(0x94),Some(0));
}
#[test]
fn selected_window_and_wrapped_successor_use_distinct_hardware_sums() {
    let mut entries = [0;720]; let mut classes = [0;816]; let configured = [0;272]; let mut expanded = [0;272];
    let profiles = tables(&mut entries,&mut classes,&configured,&mut expanded);
    let mut sums = [0;1032];
    for plane in 0..4 { sums[plane * 256 + 255] = 90; sums[plane * 256] = 180; }
    let mut bytes = [0;256]; bytes[0x81] = 255;
    let mut configuration = ConfigImages::adopt(&[],&[],&[],&mut bytes,&[]);
    select_samples(&mut configuration,CaptureStatistics {accumulators:&sums,primary:SampleDimensions {height:3,width:3},auxiliary:SampleDimensions {height:0,width:0}},FramePixels {bytes:&[],format:PixelFormat::Unpacked16,geometry:FrameGeometry::default()},&profiles,ProfileId::parse(0).unwrap()).unwrap();
    for (index,value) in [18,45,18,45,36,90,36,90].into_iter().enumerate() { assert_eq!(configuration.byte(0x84 + index as u32 * 2),Some(value)); }
}
