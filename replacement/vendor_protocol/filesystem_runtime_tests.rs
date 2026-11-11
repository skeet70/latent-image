use super::*;
use crate::filesystem_format::{FormatClass, Geometry, SectorCount};
use core::num::NonZeroU16;
use std::vec;
use std::vec::Vec;

struct Medium { bytes: Vec<u8>, sector_bytes: u16, writes: usize, fail_write: Option<usize> }
impl Medium {
    fn new(sectors: usize) -> Self { Self { bytes: vec![0xa5; sectors * 512], sector_bytes: 512, writes: 0, fail_write: None } }
    fn sector(&self, sector: u32) -> &[u8] { &self.bytes[sector as usize * 512..(sector as usize + 1) * 512] }
}
impl SectorIo for Medium {
    fn sector_bytes(&self) -> u16 { self.sector_bytes }
    fn read(&mut self, sector: Sector, output: &mut [u8]) -> Result<(), FormatError> {
        let start = sector.0 as usize * usize::from(self.sector_bytes);
        let source = self.bytes.get(start..start+output.len()).ok_or(FormatError::Device(3))?;
        output.copy_from_slice(source); Ok(())
    }
    fn write(&mut self, sector: Sector, bytes: &[u8]) -> Result<(), FormatError> {
        if self.fail_write == Some(self.writes) { return Err(FormatError::Device(3)); }
        let start = sector.0 as usize * usize::from(self.sector_bytes);
        let target = self.bytes.get_mut(start..start+bytes.len()).ok_or(FormatError::Device(3))?;
        target.copy_from_slice(bytes); self.writes += 1; Ok(())
    }
    fn flush(&mut self) -> Result<(), FormatError> { Ok(()) }
}
fn volume() -> VolumeDescriptor {
    VolumeDescriptor { sectors: SectorCount(4096), geometry: Geometry { cylinders: 32,
        heads: NonZeroU16::new(4).unwrap(), sectors_per_track: NonZeroU16::new(32).unwrap() },
        format: FormatClass::Capacity, write_state: WriteState(0), metadata_status: MetadataStatus(1),
        sector_bytes: 512, layout: None, next_free: Cluster(0), free_count: 0 }
}
#[test]
fn formats_both_fats_and_publishes_scanned_capacity() {
    let mut medium = Medium::new(4096);
    let mut volume = volume();
    format(&mut medium, &mut volume, &FormatControl::new(), None).unwrap();
    let layout = volume.layout.unwrap();
    assert_eq!(&medium.sector(0)[510..512], &[0x55, 0xaa]);
    assert_eq!(&medium.sector(layout.partition.0)[54..62], b"FAT12   ");
    assert_eq!(&medium.sector(layout.fat_start.0)[..3], &[0xf8, 0xff, 0xff]);
    assert_eq!(medium.sector(layout.fat_start.0), medium.sector(layout.fat_start.0 + layout.fat_sectors.0));
    assert_eq!(volume.free_count, layout.clusters);
    assert_eq!(volume.next_free, Cluster(2));
    assert_eq!(volume.write_state, WriteState(0));
    assert!(medium.sector(layout.data_start.0).iter().all(|byte| *byte == 0xa5));
}
#[test]
fn cancellation_prevents_metadata_writes() {
    let mut medium = Medium::new(4096);
    let mut volume = volume();
    let control = FormatControl::new(); control.cancel();
    assert_eq!(format(&mut medium, &mut volume, &control, None), Err(FormatError::Cancelled));
    assert!(medium.bytes.iter().all(|byte| *byte == 0xa5));
}
#[test]
fn write_failure_preserves_dirty_state_and_does_not_publish_success() {
    let mut medium = Medium::new(4096); medium.fail_write = Some(0);
    let mut volume = volume();
    assert_eq!(format(&mut medium, &mut volume, &FormatControl::new(), None), Err(FormatError::Write));
    assert_ne!(volume.write_state.0 & 8, 0);
    assert_eq!(volume.layout, None);
}
#[test]
fn write_protection_prevents_mutation() {
    let mut medium = Medium::new(4096);
    let mut volume = volume(); volume.write_state = WriteState(0x10);
    assert_eq!(format(&mut medium, &mut volume, &FormatControl::new(), None), Err(FormatError::WriteProtected));
    assert!(medium.bytes.iter().all(|byte| *byte == 0xa5));
}
#[test]
fn scan_reads_disk_entries_instead_of_assuming_empty_media() {
    let mut medium = Medium::new(4096);
    let mut volume = volume();
    format(&mut medium, &mut volume, &FormatControl::new(), None).unwrap();
    let layout = volume.layout.unwrap();
    let offset = layout.fat_start.0 as usize * 512 + 3;
    medium.bytes[offset] = 0xff; medium.bytes[offset+1] = 0x0f;
    scan_free(&mut medium, &mut volume, &FormatControl::new()).unwrap();
    assert_eq!(volume.free_count, layout.clusters - 1);
    assert_eq!(volume.next_free, Cluster(3));
}

#[test]
fn caller_owned_host_image_is_the_formatted_medium() {
    let mut bytes = vec![0xa5; 4096 * 512];
    let mut io = HostVolume::adopt(&mut bytes, 512).unwrap();
    let mut runtime = FilesystemRuntime::new(volume(), filesystem_service::VolumeIdentifier([0; 16]));
    runtime.format(&mut io, None).unwrap();
    let mut free = [0u8; 4];
    runtime.service().query(filesystem_service::SnapshotSelector::parse(8).unwrap(), &mut free).unwrap();
    assert_eq!(u32::from_le_bytes(free), runtime.volume().free_count);
    assert_eq!(&io.bytes()[510..512], &[0x55, 0xaa]);
}
#[test]
fn cancellation_during_the_transaction_leaves_the_medium_dirty() {
    struct CancelAfterWrite<'a> { image: HostVolume<'a>, control: &'a FormatControl }
    impl SectorIo for CancelAfterWrite<'_> {
        fn sector_bytes(&self) -> u16 { self.image.sector_bytes() }
        fn read(&mut self, sector: Sector, output: &mut [u8]) -> Result<(), FormatError> { self.image.read(sector, output) }
        fn write(&mut self, sector: Sector, bytes: &[u8]) -> Result<(), FormatError> {
            self.image.write(sector, bytes)?; self.control.cancel(); Ok(())
        }
        fn flush(&mut self) -> Result<(), FormatError> { self.image.flush() }
    }
    let mut bytes = vec![0xa5; 4096 * 512];
    let control = FormatControl::new();
    let mut io = CancelAfterWrite { image: HostVolume::adopt(&mut bytes, 512).unwrap(), control: &control };
    let mut volume = volume();
    assert_eq!(format(&mut io, &mut volume, &control, None), Err(FormatError::Cancelled));
    assert_ne!(volume.write_state.0 & 8, 0);
    assert!(io.image.bytes()[..512].iter().all(|byte| *byte == 0));
    assert!(io.image.bytes()[512..].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn allocation_capacity_reads_the_mounted_fat_from_the_provider() {
    let mut medium = Medium::new(4096);
    medium.bytes[10 * 512..11 * 512].fill(0);
    medium.bytes[10 * 512 + 4..10 * 512 + 6].fill(0xff);
    let mut volume = volume();
    let mut layout = Layout::select(&volume).unwrap();
    layout.kind = FatKind::Fat16;
    layout.fat_start = Sector(10);
    layout.fat_sectors = SectorCount(1);
    layout.data_start = Sector(1024);
    layout.cluster_sectors = 64;
    layout.clusters = 16;
    volume.layout = Some(layout);
    let mut runtime = FilesystemRuntime::new(volume, filesystem_service::VolumeIdentifier([0; 16]));
    runtime.refresh_allocation_capacity(&mut medium, &mut [0; 512]).unwrap();
    let mut output = [0; 4];
    runtime.service().query(filesystem_service::SnapshotSelector::parse(36).unwrap(), &mut output).unwrap();
    assert_eq!(u32::from_le_bytes(output), 0x40000);
}
