use crate::filesystem_catalog::{Catalog, Drive, Entry, MediaPath};
use crate::filesystem_file_cpu::{CloseOptions, ExfatAllocation, FileCpu, FileError, OpenMode, SeekFrom, WriteOutcome};
use crate::filesystem_format::{Cluster, FatKind, FormatClass, FormatError, Geometry, Layout, MetadataStatus, Sector, SectorCount, VolumeDescriptor, WriteState};
use crate::filesystem_runtime::{HostVolume, SectorIo};
use core::num::NonZeroU16;

fn layout(kind: FatKind) -> Layout {
    let volume = VolumeDescriptor { sectors: SectorCount(8192), geometry: Geometry {
        cylinders: 16, heads: NonZeroU16::new(4).unwrap(), sectors_per_track: NonZeroU16::new(32).unwrap() },
        format: FormatClass::Capacity, write_state: WriteState(0), metadata_status: MetadataStatus(0),
        sector_bytes: 512, layout: None, next_free: Cluster(2), free_count: 0 };
    let mut layout = Layout::select(&volume).unwrap();
    layout.kind = kind;
    layout.partition = Sector(0);
    layout.volume_sectors = SectorCount(1024);
    layout.sector_bytes = 512;
    layout.cluster_sectors = 1;
    layout.fat_start = Sector(1);
    layout.fat_sectors = SectorCount(4);
    layout.data_start = Sector(10);
    layout.root = if matches!(kind, FatKind::Fat12 | FatKind::Fat16) { Cluster(9) } else { Cluster(2) };
    layout.root_entries = 16;
    layout.clusters = 400;
    layout
}
fn put_fat(image: &mut [u8], layout: &Layout, cluster: u32, value: u32) {
    let copies = if layout.kind == FatKind::ExFat { 1 } else { 2 };
    for copy in 0..copies {
        let start = (layout.fat_start.0 + copy * layout.fat_sectors.0) as usize * 512;
        match layout.kind {
            FatKind::Fat12 => {
                let at = start + cluster as usize * 3 / 2;
                if cluster & 1 == 0 { image[at] = value as u8; image[at + 1] = (image[at + 1] & 0xf0) | ((value >> 8) as u8 & 15); }
                else { image[at] = (image[at] & 15) | (value << 4) as u8; image[at + 1] = (value >> 4) as u8; }
            }
            FatKind::Fat16 => { let at = start + cluster as usize * 2; image[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes()); }
            FatKind::Fat32 | FatKind::ExFat => { let at = start + cluster as usize * 4; image[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
        }
    }
}
fn fat_image(kind: FatKind, readonly: bool) -> (Layout, Vec<u8>) {
    let layout = layout(kind);
    let mut image = vec![0; 1024 * 512];
    if kind == FatKind::Fat32 { put_fat(&mut image, &layout, 2, 0x0fff_ffff); }
    put_fat(&mut image, &layout, 5, 9);
    put_fat(&mut image, &layout, 9, 0x0fff_ffff);
    let root = if kind == FatKind::Fat32 { 10 } else { 9 };
    let entry = &mut image[root * 512..root * 512 + 32];
    entry[..11].copy_from_slice(b"PHOTO   JPG");
    entry[11] = if readonly { 0x21 } else { 0x20 };
    entry[26..28].copy_from_slice(&5u16.to_le_bytes());
    entry[28..32].copy_from_slice(&700u32.to_le_bytes());
    image[13 * 512..14 * 512].fill(0x31);
    image[17 * 512..18 * 512].fill(0x72);
    (layout, image)
}
fn resolve(io: &mut impl SectorIo, layout: Layout) -> Entry {
    Catalog::new(io, layout, Drive::parse(b'A').unwrap()).unwrap()
        .resolve(&MediaPath::parse("A:/PHOTO.JPG").unwrap()).unwrap()
}

#[test]
fn fragmented_fat_data_seek_permissions_and_close_are_observable() {
    for kind in [FatKind::Fat12, FatKind::Fat16, FatKind::Fat32] {
        let (layout, mut image) = fat_image(kind, false);
        let mut io = HostVolume::adopt(&mut image, 512).unwrap();
        let entry = resolve(&mut io, layout);
        let mut files = FileCpu::new(layout, None).unwrap();
        let handle = files.open(&mut io, entry, OpenMode::Read).unwrap();
        let mut bytes = [0; 700];
        assert_eq!(files.read(&mut io, handle, &mut bytes), Ok(700));
        assert_eq!(&bytes[..512], &[0x31; 512]);
        assert_eq!(&bytes[512..], &[0x72; 188]);
        assert_eq!(files.read(&mut io, handle, &mut [0]), Err(FileError::EndOfFile));
        assert_eq!(files.seek(&mut io, handle, SeekFrom::End(-190)), Ok(510));
        assert_eq!(files.read(&mut io, handle, &mut bytes[..4]), Ok(4));
        assert_eq!(&bytes[..4], &[0x31, 0x31, 0x72, 0x72]);
        assert_eq!(files.seek(&mut io, handle, SeekFrom::End(1)), Err(FileError::InvalidSeek));
        assert_eq!(files.write(&mut io, handle, &[1]).error, Some(FileError::WrongMode));
        files.close(&mut io, handle).unwrap();
        assert_eq!(files.stat(handle), Err(FileError::InvalidHandle));
    }
}

#[test]
fn append_overwrite_and_truncate_publish_bytes_to_a_reopened_catalog() {
    for kind in [FatKind::Fat12, FatKind::Fat16, FatKind::Fat32] {
        let (layout, mut image) = fat_image(kind, false);
        let mut io = HostVolume::adopt(&mut image, 512).unwrap();
        let entry = resolve(&mut io, layout);
        let mut files = FileCpu::new(layout, None).unwrap();
        let handle = files.open(&mut io, entry, OpenMode::Append).unwrap();
        assert_eq!(files.stat(handle).unwrap().position, 700);
        assert_eq!(files.write(&mut io, handle, &[0x93; 800]), WriteOutcome { bytes: 800, error: None });
        files.seek(&mut io, handle, SeekFrom::Start(511)).unwrap();
        assert_eq!(files.write(&mut io, handle, &[0xa4, 0xb5]).bytes, 2);
        files.close_with_options(&mut io, handle, CloseOptions { final_size: Some(1100), modified: None }).unwrap();
        let entry = resolve(&mut io, layout);
        assert_eq!(entry.size.0, 1100);
        let reader = files.open(&mut io, entry, OpenMode::Read).unwrap();
        let mut bytes = [0; 1100];
        files.read(&mut io, reader, &mut bytes).unwrap();
        assert_eq!(&bytes[..511], &[0x31; 511]);
        assert_eq!(&bytes[511..513], &[0xa4, 0xb5]);
        assert_eq!(&bytes[513..700], &[0x72; 187]);
        assert_eq!(&bytes[700..], &[0x93; 400]);
        files.close(&mut io, reader).unwrap();
    }
}

#[test]
fn write_open_releases_the_old_chain_and_publishes_new_content() {
    let (layout, mut image) = fat_image(FatKind::Fat12, false);
    let mut io = HostVolume::adopt(&mut image, 512).unwrap();
    let entry = resolve(&mut io, layout);
    let mut files = FileCpu::new(layout, None).unwrap();
    let handle = files.open(&mut io, entry, OpenMode::Write).unwrap();
    assert_eq!(files.stat(handle).unwrap().bytes, 0);
    assert_eq!(files.read(&mut io, handle, &mut [0]), Err(FileError::WrongMode));
    assert_eq!(files.write(&mut io, handle, b"new bytes").bytes, 9);
    files.close(&mut io, handle).unwrap();
    let entry = resolve(&mut io, layout);
    assert_eq!(entry.size.0, 9);
    let reader = files.open(&mut io, entry, OpenMode::Read).unwrap();
    let mut output = [0; 9];
    files.read(&mut io, reader, &mut output).unwrap();
    assert_eq!(&output, b"new bytes");
}

#[test]
fn cancellation_and_readonly_refuse_without_writing_data() {
    let (layout, mut image) = fat_image(FatKind::Fat16, true);
    let before = image.clone();
    let mut io = HostVolume::adopt(&mut image, 512).unwrap();
    let entry = resolve(&mut io, layout);
    let mut files = FileCpu::new(layout, None).unwrap();
    assert_eq!(files.open(&mut io, entry, OpenMode::Append), Err(FileError::ReadOnly));
    let handle = files.open(&mut io, entry, OpenMode::Read).unwrap();
    files.cancel(handle).unwrap();
    assert_eq!(files.read(&mut io, handle, &mut [0]), Err(FileError::Cancelled));
    files.close(&mut io, handle).unwrap();
    assert_eq!(io.bytes(), before);
}

#[test]
fn fat12_sector_boundary_mutation_preserves_neighbor_and_both_copies() {
    let layout = layout(FatKind::Fat12);
    let mut image = vec![0; 1024 * 512];
    put_fat(&mut image, &layout, 340, 0x456);
    put_fat(&mut image, &layout, 341, 0x789);
    let mut io = HostVolume::adopt(&mut image, 512).unwrap();
    crate::filesystem_file_cpu::set_fat_entry(&mut io, &layout, Cluster(341), 0xabc).unwrap();
    for start in [512usize, 5 * 512] {
        assert_eq!(&io.bytes()[start + 510..start + 513], &[0x56, 0xc4, 0xab]);
    }
}

fn exfat_image() -> (Layout, Vec<u8>, ExfatAllocation) {
    let layout = layout(FatKind::ExFat);
    let mut image = vec![0; 1024 * 512];
    put_fat(&mut image, &layout, 2, u32::MAX);
    put_fat(&mut image, &layout, 3, u32::MAX);
    let root = 10 * 512;
    image[root] = 0x81;
    image[root + 20..root + 24].copy_from_slice(&3u32.to_le_bytes());
    image[root + 24..root + 32].copy_from_slice(&50u64.to_le_bytes());
    let primary = root + 32;
    image[primary] = 0x85;
    image[primary + 1] = 2;
    image[primary + 4] = 0x20;
    let stream = primary + 32;
    image[stream] = 0xc0;
    image[stream + 1] = 3;
    image[stream + 3] = 9;
    image[stream + 8..stream + 16].copy_from_slice(&600u64.to_le_bytes());
    image[stream + 20..stream + 24].copy_from_slice(&4u32.to_le_bytes());
    image[stream + 24..stream + 32].copy_from_slice(&700u64.to_le_bytes());
    let name = stream + 32;
    image[name] = 0xc1;
    for (index, unit) in "PHOTO.JPG".encode_utf16().enumerate() { image[name + 2 + index * 2..name + 4 + index * 2].copy_from_slice(&unit.to_le_bytes()); }
    let checksum = image[primary..primary + 96].iter().enumerate().filter(|(index, _)| *index != 2 && *index != 3)
        .fold(0u16, |sum, (_, byte)| sum.rotate_right(1).wrapping_add(u16::from(*byte)));
    image[primary + 2..primary + 4].copy_from_slice(&checksum.to_le_bytes());
    image[11 * 512] = 0x0f;
    image[12 * 512..13 * 512].fill(0x31);
    image[13 * 512..14 * 512].fill(0x72);
    (layout, image, ExfatAllocation { bitmap_first: Cluster(3), bitmap_bytes: 50 })
}

#[test]
fn exfat_contiguous_tail_zeros_and_extension_survive_reopen() {
    let (layout, mut image, bitmap) = exfat_image();
    let mut io = HostVolume::adopt(&mut image, 512).unwrap();
    let entry = resolve(&mut io, layout);
    let mut files = FileCpu::new(layout, Some(bitmap)).unwrap();
    let handle = files.open(&mut io, entry, OpenMode::Append).unwrap();
    files.seek(&mut io, handle, SeekFrom::Start(590)).unwrap();
    let mut tail = [0xff; 110];
    files.read(&mut io, handle, &mut tail).unwrap();
    assert_eq!(&tail[..10], &[0x72; 10]);
    assert_eq!(&tail[10..], &[0; 100]);
    assert_eq!(files.write(&mut io, handle, &[0xa9; 500]), WriteOutcome { bytes: 500, error: None });
    files.close(&mut io, handle).unwrap();
    let entry = resolve(&mut io, layout);
    assert_eq!(entry.size.0, 1200);
    assert_eq!(entry.valid_size.0, 1200);
    assert!(!entry.contiguous);
    let handle = files.open(&mut io, entry, OpenMode::Read).unwrap();
    files.seek(&mut io, handle, SeekFrom::Start(590)).unwrap();
    let mut output = [0; 610];
    files.read(&mut io, handle, &mut output).unwrap();
    assert_eq!(&output[..10], &[0x72; 10]);
    assert_eq!(&output[10..110], &[0; 100]);
    assert_eq!(&output[110..], &[0xa9; 500]);
}

struct FaultVolume<'a> { inner: HostVolume<'a>, fail_sector: Option<Sector>, flush_error: bool }
impl SectorIo for FaultVolume<'_> {
    fn sector_bytes(&self) -> u16 { self.inner.sector_bytes() }
    fn read(&mut self, sector: Sector, output: &mut [u8]) -> Result<(), FormatError> { self.inner.read(sector, output) }
    fn write(&mut self, sector: Sector, bytes: &[u8]) -> Result<(), FormatError> {
        if self.fail_sector == Some(sector) { return Err(FormatError::Device(0x101)); }
        self.inner.write(sector, bytes)
    }
    fn flush(&mut self) -> Result<(), FormatError> { if self.flush_error { Err(FormatError::Flush) } else { self.inner.flush() } }
}

#[test]
fn partial_write_reports_completed_bytes_and_close_failure_releases_handle() {
    let (layout, mut image) = fat_image(FatKind::Fat16, false);
    let mut io = FaultVolume { inner: HostVolume::adopt(&mut image, 512).unwrap(), fail_sector: None, flush_error: false };
    let entry = resolve(&mut io, layout);
    let mut files = FileCpu::new(layout, None).unwrap();
    let handle = files.open(&mut io, entry, OpenMode::Append).unwrap();
    files.seek(&mut io, handle, SeekFrom::Start(0)).unwrap();
    io.fail_sector = Some(Sector(17));
    assert_eq!(files.write(&mut io, handle, &[0x8b; 700]), WriteOutcome { bytes: 512, error: Some(FileError::Write) });
    assert_eq!(files.stat(handle).unwrap().position, 512);
    io.fail_sector = None;
    files.close(&mut io, handle).unwrap();
    let entry = resolve(&mut io, layout);
    let handle = files.open(&mut io, entry, OpenMode::Read).unwrap();
    let mut output = [0; 700];
    files.read(&mut io, handle, &mut output).unwrap();
    assert_eq!(&output[..512], &[0x8b; 512]);
    assert_eq!(&output[512..], &[0x72; 188]);
    io.flush_error = true;
    assert_eq!(files.close(&mut io, handle), Err(FileError::Flush));
    assert_eq!(files.stat(handle), Err(FileError::InvalidHandle));
}

#[test]
fn handle_exhaustion_and_reuse_do_not_revive_closed_handles() {
    let (layout, mut image) = fat_image(FatKind::Fat16, false);
    let mut io = HostVolume::adopt(&mut image, 512).unwrap();
    let entry = resolve(&mut io, layout);
    let mut files = FileCpu::new(layout, None).unwrap();
    let handles: Vec<_> = (0..16).map(|_| files.open(&mut io, entry, OpenMode::Read).unwrap()).collect();
    assert_eq!(files.open(&mut io, entry, OpenMode::Read), Err(FileError::TooManyOpenFiles));
    assert_eq!(files.read(&mut io, handles[0], &mut []), Err(FileError::EmptyTransfer));
    files.close(&mut io, handles[0]).unwrap();
    let replacement = files.open(&mut io, entry, OpenMode::Read).unwrap();
    assert_ne!(replacement, handles[0]);
    assert_eq!(files.close(&mut io, handles[0]), Err(FileError::InvalidHandle));
    assert_eq!(files.stat(replacement).unwrap().bytes, 700);
}

#[test]
fn no_space_preflight_preserves_the_original_file_and_position() {
    let (layout, mut image) = fat_image(FatKind::Fat16, false);
    for cluster in 2..layout.clusters + 2 {
        if cluster != 5 { put_fat(&mut image, &layout, cluster, 0xffff); }
    }
    let original = image.clone();
    let mut io = HostVolume::adopt(&mut image, 512).unwrap();
    let entry = resolve(&mut io, layout);
    let mut files = FileCpu::new(layout, None).unwrap();
    let handle = files.open(&mut io, entry, OpenMode::Append).unwrap();
    assert_eq!(files.write(&mut io, handle, &[0xa6; 400]), WriteOutcome { bytes: 0, error: Some(FileError::NoSpace) });
    assert_eq!(files.stat(handle).unwrap().position, 700);
    assert_eq!(io.bytes(), original);
}

#[test]
fn sector_2048_partial_writes_preserve_surrounding_bytes() {
    let mut layout = layout(FatKind::Fat16);
    layout.sector_bytes = 2048;
    let mut image = vec![0; 1024 * 2048];
    for start in [2048usize, 5 * 2048] {
        image[start + 10..start + 12].copy_from_slice(&9u16.to_le_bytes());
        image[start + 18..start + 20].copy_from_slice(&0xffffu16.to_le_bytes());
    }
    let entry = &mut image[9 * 2048..9 * 2048 + 32];
    entry[..11].copy_from_slice(b"PHOTO   JPG");
    entry[11] = 0x20;
    entry[26..28].copy_from_slice(&5u16.to_le_bytes());
    entry[28..32].copy_from_slice(&3000u32.to_le_bytes());
    image[13 * 2048..14 * 2048].fill(0x31);
    image[17 * 2048..18 * 2048].fill(0x72);
    let mut io = HostVolume::adopt(&mut image, 2048).unwrap();
    let entry = resolve(&mut io, layout);
    let mut files = FileCpu::new(layout, None).unwrap();
    let handle = files.open(&mut io, entry, OpenMode::Append).unwrap();
    files.seek(&mut io, handle, SeekFrom::Start(2047)).unwrap();
    assert_eq!(files.write(&mut io, handle, &[0xac, 0xbd]).bytes, 2);
    files.close(&mut io, handle).unwrap();
    assert_eq!(&io.bytes()[13 * 2048 + 2046..14 * 2048], &[0x31, 0xac]);
    assert_eq!(&io.bytes()[17 * 2048..17 * 2048 + 2], &[0xbd, 0x72]);
}

struct CancellingVolume<'a, 'c> {
    inner: HostVolume<'a>,
    cancellation: &'c crate::filesystem_file_cpu::FileCancellation,
    cancel_after: Sector,
}
impl SectorIo for CancellingVolume<'_, '_> {
    fn sector_bytes(&self) -> u16 { self.inner.sector_bytes() }
    fn read(&mut self, sector: Sector, output: &mut [u8]) -> Result<(), FormatError> { self.inner.read(sector, output) }
    fn write(&mut self, sector: Sector, bytes: &[u8]) -> Result<(), FormatError> {
        self.inner.write(sector, bytes)?;
        if sector == self.cancel_after { self.cancellation.cancel(); }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), FormatError> { self.inner.flush() }
}

#[test]
fn in_flight_cancellation_publishes_only_completed_write_sectors() {
    let (layout, mut image) = fat_image(FatKind::Fat16, false);
    let cancellation = crate::filesystem_file_cpu::FileCancellation::new();
    let mut io = CancellingVolume { inner: HostVolume::adopt(&mut image, 512).unwrap(),
        cancellation: &cancellation, cancel_after: Sector(13) };
    let entry = resolve(&mut io, layout);
    let mut files = FileCpu::new(layout, None).unwrap();
    let handle = files.open(&mut io, entry, OpenMode::Append).unwrap();
    files.seek(&mut io, handle, SeekFrom::Start(0)).unwrap();
    assert_eq!(files.write_controlled(&mut io, handle, &[0x9c; 700], &cancellation),
        WriteOutcome { bytes: 512, error: Some(FileError::Cancelled) });
    files.close(&mut io, handle).unwrap();
    let entry = resolve(&mut io, layout);
    let reader = files.open(&mut io, entry, OpenMode::Read).unwrap();
    let mut output = [0; 700];
    files.read(&mut io, reader, &mut output).unwrap();
    assert_eq!(&output[..512], &[0x9c; 512]);
    assert_eq!(&output[512..], &[0x72; 188]);
}
