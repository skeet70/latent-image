use super::*;
use super::super::filesystem_format::{FormatClass, Geometry, SectorCount};
use core::num::NonZeroU16;
use std::collections::BTreeMap;

struct Medium {
    sectors: BTreeMap<u32, [u8; 512]>,
    fail_write: Option<Sector>,
    fail_flush: bool,
    fail_read: bool,
}
impl Medium {
    fn new() -> Self { Self { sectors: BTreeMap::new(), fail_write: None, fail_flush: false, fail_read: false } }
    fn sector(&self, at: u32) -> [u8; 512] { self.sectors.get(&at).copied().unwrap_or([0; 512]) }
}
impl SectorIo for Medium {
    fn sector_bytes(&self) -> u16 { 512 }
    fn write(&mut self, at: Sector, bytes: &[u8]) -> Result<(), FormatError> {
        if self.fail_write == Some(at) { return Err(FormatError::Device(7)); }
        let data: [u8; 512] = bytes.try_into().unwrap();
        if data.iter().any(|&b| b != 0) { self.sectors.insert(at.0, data); }
        else { self.sectors.remove(&at.0); }
        Ok(())
    }
    fn read(&mut self, at: Sector, output: &mut [u8]) -> Result<(), FormatError> {
        if self.fail_read { return Err(FormatError::Device(8)); }
        output.copy_from_slice(&self.sector(at.0));
        Ok(())
    }
    fn flush(&mut self) -> Result<(), FormatError> {
        if self.fail_flush { Err(FormatError::Device(9)) } else { Ok(()) }
    }
}
fn volume() -> VolumeDescriptor {
    VolumeDescriptor {
        sectors: SectorCount(0x10000000),
        geometry: Geometry { cylinders: 1024, heads: NonZeroU16::new(255).unwrap(), sectors_per_track: NonZeroU16::new(63).unwrap() },
        format: FormatClass::Capacity,
        write_state: WriteState(4),
        metadata_status: MetadataStatus(11),
        sector_bytes: 512,
        layout: None,
        next_free: Cluster(0),
        free_count: 0,
    }
}
fn synthetic_tables<'a>(upcase: &'a [u8; UPCASE_BYTES], parameters: &'a [u8; 16]) -> ExfatTables<'a> {
    ExfatTables { upcase, parameters }
}
fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }

#[test]
fn formatted_medium_contains_redundant_boot_and_allocated_system_files() {
    let mut io = Medium::new();
    let mut volume = volume();
    let layout = Layout::select(&volume).unwrap();
    let mut upcase = [0u8; UPCASE_BYTES];
    for (i, byte) in upcase.iter_mut().enumerate() { *byte = (i % 251) as u8; }
    let parameters = [0x31; 16];
    format_exfat(&mut io, &mut volume, layout, &FormatControl::new(), synthetic_tables(&upcase, &parameters)).unwrap();
    let boot = io.sector(layout.partition.0);
    assert_eq!(boot, io.sector(layout.partition.0 + 12));
    assert_eq!(&boot[3..11], b"EXFAT   ");
    assert_eq!(word(&boot, 96), 4);
    let mut sum = 0u32;
    for offset in 0..11 {
        let bytes = io.sector(layout.partition.0 + offset);
        for (i, byte) in bytes.iter().enumerate() {
            if offset != 0 || !matches!(i, 106 | 107 | 112) { sum = sum.rotate_right(1).wrapping_add(u32::from(*byte)); }
        }
    }
    let sums = io.sector(layout.partition.0 + 11);
    for i in (0..512).step_by(4) { assert_eq!(word(&sums, i), sum); }
    assert_eq!(sums, io.sector(layout.partition.0 + 23));
    let fat = io.sector(layout.fat_start.0);
    assert_eq!(word(&fat, 0), 0xfffffff8);
    for offset in [4, 8, 12, 16] { assert_eq!(word(&fat, offset), u32::MAX); }
    let root = io.sector(layout.data_start.0 + 2 * u32::from(layout.cluster_sectors));
    assert_eq!(root[32], 0x81);
    assert_eq!(root[64], 0x82);
    assert_eq!(word(&root, 52), 2);
    assert_eq!(word(&root, 84), 3);
    let upcase_sector = layout.data_start.0 + u32::from(layout.cluster_sectors);
    for (page, bytes) in upcase.chunks(512).enumerate() {
        let actual = io.sector(upcase_sector + page as u32);
        assert_eq!(&actual[..bytes.len()], bytes);
        assert!(actual[bytes.len()..].iter().all(|&b| b == 0));
    }
    assert_eq!(io.sector(layout.data_start.0)[0], 7);
    assert_eq!(volume.next_free, Cluster(5));
    assert_eq!(volume.free_count, layout.clusters - 3);
    assert_eq!(volume.write_state, WriteState(0));
    assert_eq!(volume.metadata_status, MetadataStatus(0));
}

#[test]
fn write_flush_and_scan_failures_do_not_publish_success() {
    let upcase = [0; UPCASE_BYTES];
    let parameters = [0; 16];
    for expected in [FormatError::Write, FormatError::Flush, FormatError::Scan] {
        let mut volume = volume();
        let before = volume;
        let layout = Layout::select(&volume).unwrap();
        let mut io = Medium::new();
        match expected {
            FormatError::Write => io.fail_write = Some(Sector(0)),
            FormatError::Flush => io.fail_flush = true,
            FormatError::Scan => io.fail_read = true,
            _ => unreachable!(),
        }
        assert_eq!(format_exfat(&mut io, &mut volume, layout, &FormatControl::new(), synthetic_tables(&upcase, &parameters)), Err(expected));
        assert_eq!(volume, before);
    }
}

#[test]
fn cancellation_preserves_medium_and_descriptor() {
    let mut io = Medium::new();
    let mut volume = volume();
    let before = volume;
    let layout = Layout::select(&volume).unwrap();
    let control = FormatControl::new();
    control.cancel();
    assert_eq!(format_exfat(&mut io, &mut volume, layout, &control, synthetic_tables(&[0; UPCASE_BYTES], &[0; 16])), Err(FormatError::Cancelled));
    assert!(io.sectors.is_empty());
    assert_eq!(volume, before);
}

#[test]
fn missing_or_unrecognized_private_tables_are_rejected() {
    assert!(matches!(ExfatTables::from_image4(&[]), Err(FormatError::Layout)));
    assert!(matches!(ExfatTables::from_image4(&vec![0; IMAGE_BYTES]), Err(FormatError::Layout)));
}

fn mounted_fragmented_bitmap() -> (Medium, VolumeDescriptor) {
    let mut io = Medium::new();
    let mut volume = volume();
    let mut layout = Layout::select(&volume).unwrap();
    layout.cluster_sectors = 1;
    layout.clusters = 4100;
    layout.fat_start = Sector(50);
    layout.fat_sectors = SectorCount(40);
    layout.data_start = Sector(100);
    layout.root = Cluster(10);
    volume.layout = Some(layout);
    let mut fat = [0; 512];
    put32(&mut fat, 10 * 4, 11);
    put32(&mut fat, 11 * 4, u32::MAX);
    put32(&mut fat, 20 * 4, 35);
    put32(&mut fat, 35 * 4, u32::MAX);
    io.sectors.insert(50, fat);
    let mut first_root = [0; 512];
    for entry in first_root.chunks_exact_mut(32) { entry[0] = 0x85; }
    io.sectors.insert(108, first_root);
    let mut second_root = [0; 512];
    second_root[0] = 0x81;
    second_root[1] = 1;
    second_root[32] = 0x81;
    put32(&mut second_root, 52, 20);
    put64(&mut second_root, 56, 513);
    io.sectors.insert(109, second_root);
    let mut first_bitmap = [0xff; 512];
    first_bitmap[0] &= !(1 << 3);
    io.sectors.insert(118, first_bitmap);
    let mut second_bitmap = [0xff; 512];
    second_bitmap[0] = 5;
    io.sectors.insert(133, second_bitmap);
    (io, volume)
}

#[test]
fn mounted_free_scan_follows_root_and_bitmap_chains_without_writes() {
    let (mut io, mut volume) = mounted_fragmented_bitmap();
    let medium_before = io.sectors.clone();
    let layout_before = volume.layout;
    scan_free(&mut io, &mut volume, &FormatControl::new()).unwrap();
    assert_eq!(volume.next_free, Cluster(5));
    assert_eq!(volume.free_count, 3);
    assert_eq!(volume.layout, layout_before);
    assert_eq!(io.sectors, medium_before);
}

#[test]
fn malformed_bitmap_chains_do_not_publish_free_capacity() {
    for next in [20, u32::MAX, 1] {
        let (mut io, mut volume) = mounted_fragmented_bitmap();
        let before = volume;
        put32(io.sectors.get_mut(&50).unwrap(), 20 * 4, next);
        assert_eq!(scan_free(&mut io, &mut volume, &FormatControl::new()), Err(FormatError::Scan));
        assert_eq!(volume, before);
    }
}

#[test]
fn mounted_scan_propagates_cancellation_and_read_failure() {
    let (mut io, mut volume) = mounted_fragmented_bitmap();
    let before = volume;
    let control = FormatControl::new();
    control.cancel();
    assert_eq!(scan_free(&mut io, &mut volume, &control), Err(FormatError::Cancelled));
    control.reset();
    io.fail_read = true;
    assert_eq!(scan_free(&mut io, &mut volume, &control), Err(FormatError::Scan));
    assert_eq!(volume, before);
}
