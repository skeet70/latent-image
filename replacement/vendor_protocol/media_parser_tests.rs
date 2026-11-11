use super::*;

fn jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut output = vec![0xff, 0xd8, 0xff, 0xe0, 0, 7, b'J', b'F', b'I', b'F', 0,
        0xff, 0xc0, 0, 17, 8];
    output.extend_from_slice(&height.to_be_bytes());
    output.extend_from_slice(&width.to_be_bytes());
    output.extend_from_slice(&[3, 1, 0x21, 0, 2, 0x11, 1, 3, 0x11, 1, 0xff, 0xd9]);
    output
}
fn half(value: u16, big: bool) -> [u8; 2] { if big { value.to_be_bytes() } else { value.to_le_bytes() } }
fn word(value: u32, big: bool) -> [u8; 4] { if big { value.to_be_bytes() } else { value.to_le_bytes() } }
fn entry(output: &mut Vec<u8>, tag: u16, kind: u16, value: u32, big: bool) {
    output.extend_from_slice(&half(tag, big));
    output.extend_from_slice(&half(kind, big));
    output.extend_from_slice(&word(1, big));
    if kind == 3 { output.extend_from_slice(&half(value as u16, big)); output.extend_from_slice(&[0, 0]); }
    else { output.extend_from_slice(&word(value, big)); }
}
fn exif(big: bool) -> Vec<u8> {
    let thumbnail = jpeg(160, 120);
    let mut tiff = if big { b"MM\0*".to_vec() } else { b"II*\0".to_vec() };
    tiff.extend_from_slice(&word(8, big));
    tiff.extend_from_slice(&half(2, big));
    entry(&mut tiff, 0x112, 3, 6, big);
    entry(&mut tiff, 0x8769, 4, 68, big);
    tiff.extend_from_slice(&word(38, big));
    tiff.extend_from_slice(&half(2, big));
    entry(&mut tiff, 0x201, 4, 98, big);
    entry(&mut tiff, 0x202, 4, thumbnail.len() as u32, big);
    tiff.extend_from_slice(&word(0, big));
    tiff.extend_from_slice(&half(2, big));
    entry(&mut tiff, 0xa002, 4, 4896, big);
    entry(&mut tiff, 0xa003, 4, 3264, big);
    tiff.extend_from_slice(&word(0, big));
    tiff.extend_from_slice(&thumbnail);
    let mut image = vec![0xff, 0xd8, 0xff, 0xe1];
    image.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
    image.extend_from_slice(b"Exif\0\0");
    image.extend_from_slice(&tiff);
    image.extend_from_slice(&jpeg(4896, 3264)[11..]);
    image
}
#[test]
fn baseline_dimensions_and_absent_thumbnail_are_distinct() {
    let image = jpeg(640, 480);
    let metadata = parse(&image, MediaFormat::Jpeg).unwrap();
    assert_eq!((metadata.image_width, metadata.image_height), (640, 480));
    assert_eq!(metadata.thumbnail, None);
    assert_eq!((metadata.primary_width, metadata.primary_height), (0, 0));
    assert_eq!(metadata.bit_depth, 24);
    assert_eq!(metadata.components, 6);
    let thumb = parse_thumbnail(&image).unwrap();
    assert_eq!((thumb.width, thumb.height, thumb.precision, thumb.components), (640, 480, 8, 3));
}
#[test]
fn both_tiff_orders_preserve_dimensions_orientation_and_thumbnail_bytes() {
    for big in [false, true] {
        let image = exif(big);
        let metadata = parse(&image, MediaFormat::ExifJpeg).unwrap();
        assert_eq!((metadata.primary_width, metadata.primary_height), (160, 120));
        assert_eq!((metadata.secondary_width, metadata.secondary_height), (4896, 3264));
        assert_eq!((metadata.image_width, metadata.image_height), (4896, 3264));
        assert_eq!(metadata.orientation, 6);
        let thumbnail = metadata.thumbnail.unwrap();
        assert_eq!(thumbnail.offset, 110);
        assert_eq!(&image[thumbnail.offset as usize..(thumbnail.offset + thumbnail.bytes) as usize], &jpeg(160, 120));
        assert_eq!((metadata.data_size, metadata.secondary_size), (110, jpeg(160, 120).len() as u32));
    }
}
#[test]
fn truncated_and_malformed_headers_never_publish_dimensions() {
    let image = jpeg(640, 480);
    for length in 0..image.len() - 2 { assert!(parse(&image[..length], MediaFormat::Jpeg).is_err()); }
    assert_eq!(parse(&jpeg(0, 480), MediaFormat::Jpeg), Err(MediaError::MissingDimensions));
    assert_eq!(parse(b"not a jpeg", MediaFormat::Jpeg), Err(MediaError::InvalidSignature));
    let mut invalid = image.clone(); invalid[4..6].fill(0);
    assert_eq!(parse(&invalid, MediaFormat::Jpeg), Err(MediaError::Malformed));
    let mut progressive = image; progressive[12] = 0xc2;
    assert_eq!(parse(&progressive, MediaFormat::Jpeg), Err(MediaError::UnsupportedFormat));
    let mut cycle = exif(false); cycle[12 + 30..12 + 34].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(parse(&cycle, MediaFormat::Jpeg), Err(MediaError::Malformed));
}
fn raf() -> Vec<u8> {
    let preview = exif(false);
    let mut output = vec![0; 0x90];
    output[..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
    output[0x54..0x58].copy_from_slice(&0x90u32.to_be_bytes());
    output[0x58..0x5c].copy_from_slice(&(preview.len() as u32 + 10000).to_be_bytes());
    output.extend_from_slice(&preview);
    output
}
#[test]
fn raf_partial_input_retains_preview_and_thumbnail_file_offsets() {
    let image = raf();
    for format in [MediaFormat::Raf, MediaFormat::FujiRaf] {
        let metadata = parse(&image, format).unwrap();
        assert_eq!(metadata.thumbnail.unwrap().offset, 0x90 + 110);
        assert_eq!(metadata.embedded_image.unwrap().offset, 0x90);
        assert_eq!((metadata.secondary_width, metadata.secondary_height), (4896, 3264));
        assert_eq!(metadata.components, 3);
    }
    let mut invalid = image; invalid[0x54..0x58].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(parse(&invalid, MediaFormat::Raf), Err(MediaError::SizeOverflow));
}
fn atom(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut output = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    output.extend_from_slice(tag); output.extend_from_slice(payload); output
}
fn movie() -> Vec<u8> {
    let mut header = vec![0; 100];
    header[12..16].copy_from_slice(&1000u32.to_be_bytes());
    header[16..20].copy_from_slice(&2500u32.to_be_bytes());
    let mut sample = vec![0; 78];
    sample[24..26].copy_from_slice(&1920u16.to_be_bytes());
    sample[26..28].copy_from_slice(&1080u16.to_be_bytes());
    sample[74..76].copy_from_slice(&24u16.to_be_bytes());
    let mut description = vec![0, 0, 0, 0, 0, 0, 0, 1];
    description.extend_from_slice(&atom(b"avc1", &sample));
    let mut sizes = vec![0; 12];
    sizes[4..8].copy_from_slice(&1000u32.to_be_bytes());
    sizes[8..12].copy_from_slice(&60u32.to_be_bytes());
    let mut table = atom(b"stsd", &description); table.extend_from_slice(&atom(b"stsz", &sizes));
    let track = atom(b"trak", &atom(b"mdia", &atom(b"minf", &atom(b"stbl", &table))));
    let mut root = atom(b"mvhd", &header); root.extend_from_slice(&track);
    let mut output = atom(b"ftyp", b"qt  \0\0\0\0qt  "); output.extend_from_slice(&atom(b"moov", &root)); output
}
#[test]
fn movie_tracks_duration_and_sample_sizes_come_from_bytes() {
    let image = movie();
    let metadata = parse(&image, MediaFormat::Movie).unwrap();
    assert_eq!((metadata.image_width, metadata.image_height), (1920, 1080));
    assert_eq!((metadata.secondary_width, metadata.secondary_height), (1920, 1080));
    assert_eq!((metadata.duration, metadata.average_size), (1000, 2));
    assert_eq!(metadata.bit_depth, 24);
    assert_eq!(metadata.stream_flags, 240000);
    assert_eq!(metadata.thumbnail, None);
    let mut malformed = image; malformed[..4].copy_from_slice(&4u32.to_be_bytes());
    assert_eq!(parse(&malformed, MediaFormat::Movie), Err(MediaError::Malformed));
}
#[test]
fn mvtg_metadata_uses_its_tiff_relative_thumbnail() {
    let image = exif(false);
    let tiff = &image[12..12 + 98 + jpeg(160, 120).len()];
    let mut payload = vec![0; 8];
    payload.extend_from_slice(&((tiff.len() - 8 + 4) as u32).to_le_bytes());
    payload.extend_from_slice(&1u32.to_le_bytes());
    let mut bare = tiff[8..].to_vec();
    bare[22..26].copy_from_slice(&60u32.to_le_bytes());
    bare[26..30].copy_from_slice(&30u32.to_le_bytes());
    bare[40..44].copy_from_slice(&90u32.to_le_bytes());
    payload.extend_from_slice(&bare);
    let metadata = parse(&atom(b"MVTG", &payload), MediaFormat::Movie).unwrap();
    assert_eq!((metadata.image_width, metadata.image_height), (4896, 3264));
    assert_eq!((metadata.primary_width, metadata.primary_height), (160, 120));
    assert_eq!(metadata.thumbnail.unwrap().offset, 114);
}

#[test]
fn canonical_sector_file_caller_matches_byte_parser() {
    use crate::filesystem_catalog::{Drive, MediaPath};
    use crate::filesystem_format::{Cluster, FatKind, FormatClass, Geometry, Layout, MetadataStatus, Sector, SectorCount, VolumeDescriptor, WriteState};
    use crate::filesystem_runtime::HostVolume;
    use core::num::NonZeroU16;
    let volume = VolumeDescriptor { sectors: SectorCount(8192), geometry: Geometry {
        cylinders: 16, heads: NonZeroU16::new(4).unwrap(), sectors_per_track: NonZeroU16::new(32).unwrap() },
        format: FormatClass::Capacity, write_state: WriteState(0), metadata_status: MetadataStatus(0),
        sector_bytes: 512, layout: None, next_free: Cluster(2), free_count: 0 };
    let mut layout = Layout::select(&volume).unwrap();
    layout.kind = FatKind::Fat16; layout.partition = Sector(0); layout.volume_sectors = SectorCount(1024);
    layout.cluster_sectors = 1; layout.fat_start = Sector(1); layout.fat_sectors = SectorCount(4);
    layout.data_start = Sector(10); layout.root_entries = 16; layout.clusters = 400;
    let bytes = exif(false);
    let mut image = vec![0; 1024 * 512];
    for start in [512, 5 * 512] { image[start + 10..start + 12].copy_from_slice(&0xffffu16.to_le_bytes()); }
    let directory = &mut image[9 * 512..9 * 512 + 32];
    directory[..11].copy_from_slice(b"PHOTO   JPG"); directory[11] = 0x20;
    directory[26..28].copy_from_slice(&5u16.to_le_bytes());
    directory[28..32].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    image[13 * 512..13 * 512 + bytes.len()].copy_from_slice(&bytes);
    let mut io = HostVolume::adopt(&mut image, 512).unwrap();
    let mut catalog = Catalog::new(&mut io, layout, Drive::parse(b'A').unwrap()).unwrap();
    let entry = catalog.resolve(&MediaPath::parse("A:/PHOTO.JPG").unwrap()).unwrap();
    let mut files = FileCpu::new(layout, None).unwrap();
    assert_eq!(metadata(&mut files, &mut catalog, entry, MediaFormat::Jpeg, &mut [0; 32]), parse(&bytes, MediaFormat::Jpeg));
    assert_eq!(metadata(&mut files, &mut catalog, entry, MediaFormat::Jpeg, &mut [0; 31]), Err(MediaError::ScratchTooSmall));
}
