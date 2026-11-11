use super::*;

#[test]
fn storage_identifiers_do_not_accept_bare_volume_numbers() {
    assert_eq!(StorageSelector::parse(1), Err(ObjectResponse::InvalidStorage));
    assert_eq!(StorageSelector::parse(2), Err(ObjectResponse::InvalidStorage));
    assert_eq!(StorageSelector::parse(0x10000002), Ok(StorageSelector::Secondary));
    assert_eq!(StorageSelector::parse(u32::MAX), Ok(StorageSelector::All));
}

#[test]
fn object_handles_preserve_secondary_volume_bit() {
    let handle = ObjectHandle::from_ordinal(ObjectVolume::Secondary, 19);
    assert_eq!(handle.wire(), 0x80000013);
    assert_eq!(ObjectHandle::from_wire(0x80000013).ordinal(), 19);
    assert_eq!(ObjectHandle::from_wire(0x80000013).volume(), ObjectVolume::Secondary);
}

#[test]
fn root_and_unfiltered_parent_are_distinct() {
    assert!(ParentFilter::from_wire(0).matches(23));
    assert!(!ParentFilter::from_wire(u32::MAX).matches(23));
    assert!(ParentFilter::from_wire(u32::MAX).matches(0));
    assert!(ParentFilter::from_wire(0x80000017).matches(23));
}

#[test]
fn image_filter_observes_source_format_flag() {
    assert!(FormatFilter::Images.matches(0x80003801, false));
    assert!(!FormatFilter::Images.matches(0x300d, false));
    assert!(FormatFilter::Exact(0x3801).matches(0x80003801, false));
    assert!(FormatFilter::Any.matches(0x80000000, false));
    assert!(!FormatFilter::Any.matches(0x80000000, true));
}

#[test]
fn partial_read_clips_end_and_preserves_seek_mode() {
    let window = ReadWindow::partial(ObjectLength(90), ObjectOffset(70), ObjectLength(40), ObjectOffset(60), false).unwrap();
    assert_eq!(window.length, ObjectLength(20));
    assert_eq!(window.mode, ReadMode::Seek);
    assert!(window.reaches_end);
    assert_eq!(ReadWindow::partial(ObjectLength(90), ObjectOffset(90), ObjectLength(1), ObjectOffset(0), false), Err(ObjectResponse::InvalidParameter));
    assert_eq!(ReadWindow::partial(ObjectLength(90), ObjectOffset(0), ObjectLength(0), ObjectOffset(0), false), Err(ObjectResponse::InvalidParameter));
}

#[test]
fn partial_read_preserves_source_word_overflow_comparison() {
    let window = ReadWindow::partial(ObjectLength(90), ObjectOffset(70), ObjectLength(u32::MAX), ObjectOffset(80), false).unwrap();
    assert_eq!(window.length, ObjectLength(u32::MAX));
    assert_eq!(window.mode, ReadMode::Continue);
    assert!(!window.reaches_end);
}

#[test]
fn delete_failure_codes_preserve_distinct_responses() {
    assert_eq!(DeleteFailure::from_device(0), None);
    assert_eq!(DeleteFailure::from_device(0x80000200).unwrap().response(), 0x200d);
    assert_eq!(DeleteFailure::from_device(0x80000100).unwrap().response(), 0x2009);
    assert_eq!(DeleteFailure::from_device(0x80000002).unwrap().response(), 0x2019);
    assert_eq!(DeleteFailure::from_device(0x80000000).unwrap().response(), 0x2002);
}

#[test]
fn object_info_decodes_unaligned_wire_fields() {
    let mut bytes = [0; 52];
    bytes[0..4].copy_from_slice(&0x10000001u32.to_le_bytes());
    bytes[4..6].copy_from_slice(&0xb103u16.to_le_bytes());
    bytes[14..18].copy_from_slice(&0x12345678u32.to_le_bytes());
    bytes[38..42].copy_from_slice(&0x80000017u32.to_le_bytes());
    bytes[48..52].copy_from_slice(&0x87654321u32.to_le_bytes());
    let info = ObjectInfoHeader::decode(&bytes).unwrap();
    assert_eq!(info.thumbnail_size, ObjectLength(0x12345678));
    assert_eq!(info.sequence, 0x87654321);
    assert_eq!(info.parent, ParentFilter::Object(ObjectHandle::from_wire(0x80000017)));
    assert_eq!(info.encode(), bytes);
}

fn disk_fixture() -> (crate::filesystem_format::Layout, Vec<u8>) {
    use crate::filesystem_format::*;
    use core::num::NonZeroU16;
    let volume = VolumeDescriptor {
        sectors: SectorCount(8192),
        geometry: Geometry { cylinders: 16, heads: NonZeroU16::new(4).unwrap(), sectors_per_track: NonZeroU16::new(32).unwrap() },
        format: FormatClass::Capacity, write_state: WriteState(0), metadata_status: MetadataStatus(0),
        sector_bytes: 512, layout: None, next_free: Cluster(2), free_count: 0,
    };
    let mut layout = Layout::select(&volume).unwrap();
    layout.kind = FatKind::Fat16;
    layout.partition = Sector(0);
    layout.volume_sectors = SectorCount(1024);
    layout.sector_bytes = 512;
    layout.cluster_sectors = 1;
    layout.fat_start = Sector(1);
    layout.fat_sectors = SectorCount(4);
    layout.data_start = Sector(10);
    layout.root = Cluster(9);
    layout.root_entries = 16;
    layout.clusters = 400;
    let mut bytes = vec![0; 1024 * 512];
    for fat in [512, 5 * 512] {
        bytes[fat + 10..fat + 12].copy_from_slice(&9u16.to_le_bytes());
        bytes[fat + 18..fat + 20].copy_from_slice(&u16::MAX.to_le_bytes());
    }
    let entry = &mut bytes[9 * 512..9 * 512 + 32];
    entry[..11].copy_from_slice(b"PHOTO   JPG");
    entry[11] = 0x20;
    entry[26..28].copy_from_slice(&5u16.to_le_bytes());
    entry[28..32].copy_from_slice(&700u32.to_le_bytes());
    bytes[13 * 512..14 * 512].fill(0x31);
    bytes[17 * 512..18 * 512].fill(0x72);
    (layout, bytes)
}

fn command(operation: u16, parameters: [u32; 5]) -> CommandPacket {
    CommandPacket { operation: super::super::packet::OperationCode(operation), transaction: super::super::packet::TransactionId(27), parameters, parameter_count: 3 }
}

#[test]
fn remote_partial_read_sends_actual_fragmented_file_bytes_and_count() {
    use crate::filesystem_catalog::{Drive, MediaPath};
    use crate::filesystem_runtime::HostVolume;
    use super::super::super::interface_modes::catalog::{ObjectFormat, ObjectMetadata};
    use super::super::executor::SessionIdentifier;
    let (layout, mut disk) = disk_fixture();
    let mut io = HostVolume::adopt(&mut disk, 512).unwrap();
    let mut fs = Catalog::new(&mut io, layout, Drive::parse(b'A').unwrap()).unwrap();
    let path = MediaPath::parse("A:/PHOTO.JPG").unwrap();
    let entry = fs.resolve(&path).unwrap();
    let metadata = ObjectMetadata::from_entry(&MediaPath::root(Drive::parse(b'A').unwrap()), entry, ObjectFormat::Jpeg, ObjectHandle::from_wire(1), None).unwrap();
    let mut slots = [None; 1];
    let mut scratch = [None; 1];
    let mut catalog = CatalogRuntime::new(&mut slots, &mut scratch);
    catalog.insert(metadata).unwrap();
    let mut files = FileCpu::new(layout, None).unwrap();
    let mut operations = ConcreteRemoteObjectOperations::new();
    let mut state = PtpState::new();
    state.session_id = Some(SessionIdentifier(1));
    let mut output = PtpOutput::new();
    let command = command(0x101b, [1, 510, 4, 0, 0]);
    operations.execute(OperationRequest { phase: PtpPhase::Command, command, data: &[] }, &mut state, &mut output, &mut catalog, &mut fs, &mut files, &mut [], &InterfaceModes::new(), InterfaceMode::Mode0).unwrap();
    assert_eq!(output.queued_bytes(), &[16, 0, 0, 0, 2, 0, 0x1b, 0x10, 27, 0, 0, 0, 0x31, 0x31, 0x72, 0x72]);
    output.acknowledge(12).unwrap();
    assert_eq!(output.queued_bytes(), &[0x31, 0x31, 0x72, 0x72]);
    output.acknowledge(4).unwrap();
    operations.execute(OperationRequest { phase: PtpPhase::Data, command, data: &[] }, &mut state, &mut output, &mut catalog, &mut fs, &mut files, &mut [], &InterfaceModes::new(), InterfaceMode::Mode0).unwrap();
    assert_eq!(output.queued_bytes(), &[16, 0, 0, 0, 3, 0, 1, 0x20, 27, 0, 0, 0, 4, 0, 0, 0]);
}

#[test]
fn upload_info_waits_for_its_transaction_then_reports_installed_null_creator() {
    use crate::filesystem_catalog::Drive;
    use crate::filesystem_runtime::HostVolume;
    use super::super::executor::SessionIdentifier;
    let (layout, mut disk) = disk_fixture();
    let original = disk.clone();
    let mut io = HostVolume::adopt(&mut disk, 512).unwrap();
    let mut fs = Catalog::new(&mut io, layout, Drive::parse(b'A').unwrap()).unwrap();
    let mut objects = [];
    let mut scratch = [];
    let mut catalog = CatalogRuntime::new(&mut objects, &mut scratch);
    let mut files = FileCpu::new(layout, None).unwrap();
    let mut operations = ConcreteRemoteObjectOperations::new();
    let mut state = PtpState::new();
    state.session_id = Some(SessionIdentifier(1));
    let mut output = PtpOutput::new();
    let command = command(0x100c, [0; 5]);
    operations.execute(OperationRequest { phase: PtpPhase::Command, command, data: &[] }, &mut state, &mut output, &mut catalog, &mut fs, &mut files, &mut [], &InterfaceModes::new(), InterfaceMode::Mode0).unwrap();
    assert!(output.is_empty());
    let mut dataset = [0; 57];
    dataset[..4].copy_from_slice(&0x10000001u32.to_le_bytes());
    dataset[4..6].copy_from_slice(&0xb003u16.to_le_bytes());
    dataset[52] = 2;
    dataset[53] = b'A';
    let mut wrong = command;
    wrong.transaction.0 += 1;
    assert!(matches!(operations.execute(OperationRequest { phase: PtpPhase::Data, command: wrong, data: &dataset }, &mut state, &mut output, &mut catalog, &mut fs, &mut files, &mut [], &InterfaceModes::new(), InterfaceMode::Mode0), Err(PtpError::InvalidState)));
    assert!(output.is_empty());
    operations.execute(OperationRequest { phase: PtpPhase::Data, command, data: &dataset }, &mut state, &mut output, &mut catalog, &mut fs, &mut files, &mut [], &InterfaceModes::new(), InterfaceMode::Mode0).unwrap();
    assert_eq!(output.queued_bytes(), &[12, 0, 0, 0, 3, 0, 0x20, 0x20, 27, 0, 0, 0]);
    drop(fs);
    drop(io);
    assert_eq!(disk, original);
}
