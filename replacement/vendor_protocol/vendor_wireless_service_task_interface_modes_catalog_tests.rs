use super::*;
use crate::filesystem_catalog::Drive;
use crate::filesystem_format::{Cluster, FatKind, FormatClass, FormatError, Geometry, Layout, MetadataStatus, Sector, SectorCount, VolumeDescriptor, WriteState};
use crate::filesystem_runtime::HostVolume;
use core::num::NonZeroU16;

fn layout() -> Layout {
    let volume = VolumeDescriptor {
        sectors: SectorCount(4096), geometry: Geometry { cylinders: 32, heads: NonZeroU16::new(4).unwrap(), sectors_per_track: NonZeroU16::new(32).unwrap() },
        format: FormatClass::Capacity, write_state: WriteState(0), metadata_status: MetadataStatus(0), sector_bytes: 512, layout: None, next_free: Cluster(0), free_count: 0,
    };
    let mut layout = Layout::select(&volume).unwrap();
    layout.kind = FatKind::Fat16;
    layout.fat_start = Sector(10);
    layout.fat_sectors = SectorCount(1);
    layout.data_start = Sector(20);
    layout.cluster_sectors = 1;
    layout.root_entries = 16;
    layout.clusters = 100;
    layout
}
fn entry(medium: &mut [u8], sector: usize, slot: usize, name: &[u8; 11], directory: bool, cluster: u16, size: u32) {
    let at = sector * 512 + slot * 32;
    medium[at..at + 11].copy_from_slice(name);
    medium[at + 11] = if directory { 16 } else { 0 };
    medium[at + 26..at + 28].copy_from_slice(&cluster.to_le_bytes());
    medium[at + 28..at + 32].copy_from_slice(&size.to_le_bytes());
}
fn medium() -> std::vec::Vec<u8> {
    let mut medium = std::vec![0; 128 * 512];
    entry(&mut medium, 12, 0, b"DCIM       ", true, 2, 0);
    entry(&mut medium, 20, 0, b"101FUJI_   ", true, 3, 0);
    entry(&mut medium, 20, 1, b"100FUJI_   ", true, 4, 0);
    entry(&mut medium, 21, 0, b"DSCF0003JPG", false, 5, 103);
    entry(&mut medium, 22, 0, b"DSCF0002JPG", false, 6, 102);
    entry(&mut medium, 22, 1, b"DSCF0001JPG", false, 7, 101);
    entry(&mut medium, 22, 2, b"DSCF0001RAF", false, 8, 201);
    entry(&mut medium, 22, 3, b"DSCF0004MOV", false, 9, 304);
    medium
}

#[test]
fn primary_walk_publishes_actual_entries_in_numeric_and_collision_order() {
    let mut bytes = medium();
    let mut io = HostVolume::adopt(&mut bytes, 512).unwrap();
    let mut fs = Catalog::new(&mut io, layout(), Drive::parse(b'A').unwrap()).unwrap();
    let mut objects = [None; 16];
    let mut scratch = [None; 16];
    let mut catalog = CatalogRuntime::new(&mut objects, &mut scratch);
    let publication = catalog.scan_primary(&mut fs).unwrap();
    assert_eq!(publication.count, 5);
    let observed: std::vec::Vec<_> = catalog.objects().map(|object| (object.handle.ordinal(), object.size.0, object.format)).collect();
    assert_eq!(observed, [(1, 102, ObjectFormat::Jpeg), (2, 304, ObjectFormat::Movie), (3, 101, ObjectFormat::Jpeg), (4, 201, ObjectFormat::Raw), (5, 103, ObjectFormat::Jpeg)]);
    for object in catalog.objects() { assert_eq!(fs.resolve(&object.path).unwrap().size, object.size); }
}

#[test]
fn mode3_initialization_walks_reverse_and_does_not_include_raw_files() {
    let mut bytes = medium();
    let mut io = HostVolume::adopt(&mut bytes, 512).unwrap();
    let mut fs = Catalog::new(&mut io, layout(), Drive::parse(b'A').unwrap()).unwrap();
    let mut objects = [None; 16];
    let mut scratch = [None; 16];
    let mut catalog = CatalogRuntime::new(&mut objects, &mut scratch);
    assert_eq!(catalog.initialize_mode3(&mut fs).unwrap().count, 4);
    assert_eq!(catalog.objects().map(|object| object.size.0).collect::<std::vec::Vec<_>>(), [103, 304, 102, 101]);
    assert_eq!(catalog.start_mode3().unwrap().count, 4);
}

#[test]
fn deletion_preserves_remaining_handles_and_teardown_removes_visibility() {
    let mut bytes = medium();
    let mut io = HostVolume::adopt(&mut bytes, 512).unwrap();
    let mut fs = Catalog::new(&mut io, layout(), Drive::parse(b'A').unwrap()).unwrap();
    let mut objects = [None; 16];
    let mut scratch = [None; 16];
    let mut catalog = CatalogRuntime::new(&mut objects, &mut scratch);
    catalog.scan_primary(&mut fs).unwrap();
    let first = ObjectHandle::from_ordinal(ObjectVolume::Primary, 1);
    let generation = catalog.publication_generation();
    assert!(catalog.object(first).is_some());
    assert_eq!(catalog.publication_generation(), generation);
    assert!(catalog.remove(first));
    assert_ne!(catalog.publication_generation(), generation);
    assert!(catalog.object(first).is_none());
    assert_eq!(catalog.objects().next().unwrap().handle.ordinal(), 2);
    let next = catalog.next_handle(ObjectVolume::Primary).unwrap();
    assert_eq!(next.ordinal(), 6);
    let directory = MediaPath::parse("A:/DCIM/101FUJI_").unwrap();
    let entry = fs.resolve(&MediaPath::parse("A:/DCIM/101FUJI_/DSCF0003.JPG").unwrap()).unwrap();
    let inserted = ObjectMetadata::from_entry(&directory, entry, ObjectFormat::Jpeg, next, None).unwrap();
    catalog.insert(inserted).unwrap();
    assert_eq!(catalog.object(next).unwrap().size.0, 103);
    catalog.teardown();
    assert_eq!(catalog.objects().count(), 0);
    assert_eq!(catalog.state(), CatalogState::Empty);
}

#[test]
fn short_output_preserves_published_objects_and_reports_failure() {
    let mut bytes = medium();
    let mut io = HostVolume::adopt(&mut bytes, 512).unwrap();
    let mut fs = Catalog::new(&mut io, layout(), Drive::parse(b'A').unwrap()).unwrap();
    let mut objects = [None; 2];
    let mut scratch = [None; 16];
    let mut catalog = CatalogRuntime::new(&mut objects, &mut scratch);
    assert_eq!(catalog.scan_primary(&mut fs), Err(CatalogFailure::StorageCapacity));
    assert_eq!(catalog.objects().map(|object| object.size.0).collect::<std::vec::Vec<_>>(), [102, 304]);
    assert_eq!(catalog.state(), CatalogState::Failed);
}

struct CancelledMedium;
impl SectorIo for CancelledMedium {
    fn sector_bytes(&self) -> u16 { 512 }
    fn read(&mut self, _: Sector, _: &mut [u8]) -> Result<(), FormatError> { Err(FormatError::Cancelled) }
    fn write(&mut self, _: Sector, _: &[u8]) -> Result<(), FormatError> { Err(FormatError::WriteProtected) }
    fn flush(&mut self) -> Result<(), FormatError> { Ok(()) }
}
#[test]
fn media_cancellation_remains_a_cancelled_completion() {
    let mut io = CancelledMedium;
    let mut fs = Catalog::new(&mut io, layout(), Drive::parse(b'A').unwrap()).unwrap();
    let mut objects = [None; 1];
    let mut scratch = [None; 1];
    let mut catalog = CatalogRuntime::new(&mut objects, &mut scratch);
    assert_eq!(catalog.scan_primary(&mut fs), Err(CatalogFailure::Cancelled));
    assert_eq!(catalog.state(), CatalogState::Cancelled);
    assert_eq!(catalog.objects().count(), 0);
}

#[test]
fn mode3_start_publishes_zero_without_initialization() {
    let mut objects = [];
    let mut scratch = [];
    let catalog = CatalogRuntime::new(&mut objects, &mut scratch);
    assert_eq!(catalog.start_mode3(), Ok(CatalogPublication { count: 0, mode: CatalogMode::Mode3 }));
}
