use super::{decode_spare, encode_spare, FtlError, FtlGeometry, LogicalBlock, SpareRecord};
use crate::application_nand::PhysicalBlock;

#[test]
fn native_volume_geometry_matches_selected_b_drive() {
    let geometry = FtlGeometry::from_mapped_xm1().unwrap();
    assert_eq!(geometry.base().index(), 314);
    assert_eq!(geometry.physical_blocks(), 582);
    assert_eq!(geometry.logical_blocks(), 562);
    assert_eq!(geometry.sector_bytes(), 2048);
    assert_eq!(geometry.sector_count(), 35_968);
}

#[test]
fn volume_capacity_uses_requested_sector_unit() {
    let geometry = FtlGeometry::new(PhysicalBlock::new(32).unwrap(), 128, 100).unwrap();
    assert_eq!(geometry.sector_count(), 25_600);
    assert_eq!(geometry.with_sector_bytes(2048).unwrap().sector_count(), 6400);
    assert_eq!(geometry.with_sector_bytes(1024), Err(FtlError::Geometry));
}

#[test]
fn geometry_rejects_ranges_without_replacement_space_or_outside_chip() {
    let first = PhysicalBlock::new(1000).unwrap();
    assert_eq!(FtlGeometry::new(first, 25, 20), Err(FtlError::Geometry));
    assert_eq!(FtlGeometry::new(first, 24, 24), Err(FtlError::Geometry));
    assert_eq!(FtlGeometry::new(first, 24, 0), Err(FtlError::Geometry));
    assert!(FtlGeometry::new(first, 24, 23).is_ok());
    assert_eq!(LogicalBlock::new(1000), Err(FtlError::Range));
}

#[test]
fn mapped_tag_bytes_are_big_endian_repeated_and_even_parity() {
    let mut expected = [0xff; 16];
    expected[..4].copy_from_slice(&[0x10, 2, 0x10, 2]);
    assert_eq!(encode_spare(LogicalBlock::new(1).unwrap()), expected);
    expected[..4].copy_from_slice(&[0x17, 0xcf, 0x17, 0xcf]);
    assert_eq!(encode_spare(LogicalBlock::new(999).unwrap()), expected);
}

#[test]
fn erased_and_bad_records_do_not_create_logical_mappings() {
    assert_eq!(decode_spare(&[0xff; 16]), SpareRecord::Erased);
    for bad_offset in [4, 5] {
        let mut spare = encode_spare(LogicalBlock::new(1).unwrap());
        spare[bad_offset] = 0xfe;
        assert_eq!(decode_spare(&spare), SpareRecord::Bad);
    }
}

#[test]
fn tag_redundancy_accepts_one_valid_copy_but_not_disagreement() {
    let mut spare = [0xff; 16];
    spare[..4].copy_from_slice(&[0, 3, 0, 2]);
    assert_eq!(decode_spare(&spare), SpareRecord::Mapped(Some(LogicalBlock::new(1).unwrap())));
    spare[..4].copy_from_slice(&[0, 2, 0, 3]);
    assert_eq!(decode_spare(&spare), SpareRecord::Mapped(Some(LogicalBlock::new(1).unwrap())));
    spare[..4].copy_from_slice(&[0, 3, 0, 5]);
    assert_eq!(decode_spare(&spare), SpareRecord::Mapped(None));
    spare[..4].copy_from_slice(&[0, 2, 0, 2]);
    assert_eq!(decode_spare(&spare), SpareRecord::Mapped(None));
}

extern crate std;
use std::vec;
use std::vec::Vec;
use super::{FilesystemFtl, FtlCacheBlock};
use crate::application_nand::memory::{BlockFailures, FailureCount, HostConfiguration, ImageProvenance, MemoryNand};
use crate::application_nand::{Timeout, BLOCK_BYTES, PAGE_BYTES, PAGES_PER_BLOCK};
use crate::filesystem_format::Sector;

const CONFIGURATIONS: [HostConfiguration; 2] = [
    HostConfiguration::G1StrictRejectBeforeMutation,
    HostConfiguration::LegacyAndCommitFailedStatus,
];

/// The image model charges one timeout tick per 512 transferred bytes, and retiring a
/// block programs PAGES_PER_BLOCK pages of PAGE_BYTES plus spare.
const TIMEOUT: Timeout = Timeout::new(1000);

struct NandFixture {
    data: Vec<u8>,
    spare: Vec<[u8; 16]>,
    failures: Vec<BlockFailures>,
    configuration: HostConfiguration,
}
impl NandFixture {
    fn erased(blocks: usize, configuration: HostConfiguration) -> Self {
        Self {
            data: vec![0xff; blocks * BLOCK_BYTES as usize],
            spare: vec![[0xff; 16]; blocks * PAGES_PER_BLOCK as usize],
            failures: vec![BlockFailures::default(); blocks],
            configuration,
        }
    }
    fn mapped(&mut self, physical: usize, logical: u16, value: u8) {
        self.data[physical * BLOCK_BYTES as usize..(physical + 1) * BLOCK_BYTES as usize].fill(value);
        let tag = encode_spare(LogicalBlock::new(logical).unwrap());
        self.spare[physical * PAGES_PER_BLOCK as usize..(physical + 1) * PAGES_PER_BLOCK as usize].fill(tag);
    }
    fn image(&mut self) -> MemoryNand<'_> {
        MemoryNand::adopt(PhysicalBlock::new(0).unwrap(), &mut self.data, &mut self.spare,
            &mut self.failures, ImageProvenance::Synthetic, self.configuration).unwrap()
    }
}

fn fixture_geometry() -> FtlGeometry {
    FtlGeometry::new(PhysicalBlock::new(0).unwrap(), 8, 4).unwrap()
}

#[test]
fn mounted_sectors_follow_oob_map_and_unmapped_sectors_are_erased() { for configuration in CONFIGURATIONS {
    let mut fixture = NandFixture::erased(8, configuration);
    fixture.mapped(5, 1, 0x3c);
    fixture.data[BLOCK_BYTES as usize..2 * BLOCK_BYTES as usize].fill(0x77);
    let mut image = fixture.image();
    let mut cache = [FtlCacheBlock::new()];
    let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
    let mut sector = [0; 512];
    ftl.read(Sector(256), &mut sector).unwrap();
    assert_eq!(sector, [0x3c; 512]);
    ftl.read(Sector(0), &mut sector).unwrap();
    assert_eq!(sector, [0xff; 512]);
    assert_eq!(ftl.map().physical(LogicalBlock::new(1).unwrap()).unwrap().unwrap().index(), 5);
}}

#[test]
fn mount_preserves_first_committed_mapping_and_reclaims_duplicate_and_torn_blocks() { for configuration in CONFIGURATIONS {
    let mut fixture = NandFixture::erased(8, configuration);
    fixture.mapped(2, 0, 0x22);
    fixture.mapped(3, 0, 0x33);
    fixture.mapped(4, 1, 0x44);
    fixture.spare[5 * PAGES_PER_BLOCK as usize - 1] = encode_spare(LogicalBlock::new(2).unwrap());
    fixture.spare[0][5] = 0xe0;
    {
        let mut image = fixture.image();
        let mut cache = [FtlCacheBlock::new()];
        let ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
        assert_eq!(ftl.map().physical(LogicalBlock::new(0).unwrap()).unwrap().unwrap().index(), 2);
        assert_eq!(ftl.map().physical(LogicalBlock::new(1).unwrap()).unwrap(), None);
        assert_eq!(ftl.map().bad_blocks(), 1);
    }
    assert!(fixture.data[3 * BLOCK_BYTES as usize..5 * BLOCK_BYTES as usize].iter().all(|&byte| byte == 0xff));
    assert_eq!(fixture.spare[0][5], 0xe0);
    assert_eq!(fixture.data[2 * BLOCK_BYTES as usize], 0x22);
}}

#[test]
fn dirty_eviction_and_remount_preserve_neighbor_sectors() { for configuration in CONFIGURATIONS {
    let mut fixture = NandFixture::erased(8, configuration);
    fixture.mapped(3, 0, 0x11);
    {
        let mut image = fixture.image();
        let mut cache = [FtlCacheBlock::new()];
        let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
        ftl.write(Sector(1), &[0xa5; 512]).unwrap();
        let mut read = [0; 512];
        ftl.read(Sector(256), &mut read).unwrap();
        ftl.write(Sector(257), &[0x5a; 512]).unwrap();
        ftl.flush().unwrap();
    }
    {
        let mut image = fixture.image();
        let mut cache = [FtlCacheBlock::new()];
        let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
        let mut sectors = [0; 1536];
        ftl.read(Sector(0), &mut sectors).unwrap();
        assert_eq!(&sectors[..512], &[0x11; 512]);
        assert_eq!(&sectors[512..1024], &[0xa5; 512]);
        assert_eq!(&sectors[1024..], &[0x11; 512]);
        ftl.read(Sector(256), &mut sectors).unwrap();
        assert_eq!(&sectors[..512], &[0xff; 512]);
        assert_eq!(&sectors[512..1024], &[0x5a; 512]);
        assert_eq!(&sectors[1024..], &[0xff; 512]);
    }
}}

#[test]
fn program_failure_rolls_over_preserving_data_and_law_specific_oob() { for configuration in CONFIGURATIONS {
    let mut fixture = NandFixture::erased(8, configuration);
    fixture.failures[0].program = FailureCount::new(1);
    {
        let mut image = fixture.image();
        let mut cache = [FtlCacheBlock::new()];
        let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
        ftl.write(Sector(0), &[0x55; 512]).unwrap();
        ftl.flush().unwrap();
        assert_eq!(ftl.map().physical(LogicalBlock::new(0).unwrap()).unwrap().unwrap().index(), 1);
        assert_eq!(ftl.map().bad_blocks(), 1);
        let mut sector = [0; 512];
        ftl.read(Sector(0), &mut sector).unwrap();
        assert_eq!(sector, [0x55; 512]);
    }
    let expected = match configuration {
        HostConfiguration::G1StrictRejectBeforeMutation => SpareRecord::Erased,
        HostConfiguration::LegacyAndCommitFailedStatus => SpareRecord::Bad,
    };
    assert_eq!(decode_spare(&fixture.spare[0]), expected);
    assert_eq!(&fixture.data[..512], &[0x55; 512]);
    assert_eq!(decode_spare(&fixture.spare[PAGES_PER_BLOCK as usize]), SpareRecord::Mapped(Some(LogicalBlock::new(0).unwrap())));
}}

#[test]
fn exhausted_reserve_returns_error_and_keeps_dirty_reads_available() { for configuration in CONFIGURATIONS {
    let mut fixture = NandFixture::erased(8, configuration);
    for failure in &mut fixture.failures[..4] { failure.program = FailureCount::new(1); }
    let mut image = fixture.image();
    let mut cache = [FtlCacheBlock::new()];
    let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
    ftl.write(Sector(0), &[0x66; 512]).unwrap();
    assert_eq!(ftl.flush(), Err(FtlError::NoReplacement));
    assert_eq!(ftl.map().physical(LogicalBlock::new(0).unwrap()).unwrap(), None);
    let mut sector = [0; 512];
    ftl.read(Sector(0), &mut sector).unwrap();
    assert_eq!(sector, [0x66; 512]);
}}

#[test]
fn previous_erase_failure_keeps_replacement_and_law_specific_oob() { for configuration in CONFIGURATIONS {
    let mut fixture = NandFixture::erased(8, configuration);
    fixture.mapped(5, 0, 0x77);
    fixture.failures[5].erase = FailureCount::new(1);
    {
        let mut image = fixture.image();
        let mut cache = [FtlCacheBlock::new()];
        let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
        ftl.write(Sector(0), &[0x88; 512]).unwrap();
        ftl.flush().unwrap();
        assert_eq!(ftl.map().bad_blocks(), 1);
    }
    let expected = match configuration {
        HostConfiguration::G1StrictRejectBeforeMutation => SpareRecord::Mapped(Some(LogicalBlock::new(0).unwrap())),
        HostConfiguration::LegacyAndCommitFailedStatus => SpareRecord::Bad,
    };
    assert_eq!(decode_spare(&fixture.spare[5 * PAGES_PER_BLOCK as usize]), expected);
    let mut image = fixture.image();
    let mut cache = [FtlCacheBlock::new()];
    let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
    let mut sector = [0; 1024];
    ftl.read(Sector(0), &mut sector).unwrap();
    assert_eq!(&sector[..512], &[0x88; 512]);
    assert_eq!(&sector[512..], &[0x77; 512]);
}}

#[test]
fn invalid_ranges_have_no_sector_effects_and_page_sized_sectors_round_trip() { for configuration in CONFIGURATIONS {
    let mut fixture = NandFixture::erased(8, configuration);
    let geometry = fixture_geometry().with_sector_bytes(2048).unwrap();
    let mut image = fixture.image();
    let mut cache = [FtlCacheBlock::new()];
    let mut ftl = FilesystemFtl::mount_image(&mut image, geometry, &mut cache, TIMEOUT).unwrap();
    assert_eq!(ftl.write(Sector(geometry.sector_count()), &[0x33; PAGE_BYTES]), Err(FtlError::Range));
    assert_eq!(ftl.write(Sector(0), &[0x33; 512]), Err(FtlError::Range));
    let mut untouched = [0; PAGE_BYTES];
    ftl.read(Sector(0), &mut untouched).unwrap();
    assert_eq!(untouched, [0xff; PAGE_BYTES]);
    ftl.write(Sector(63), &[0x99; PAGE_BYTES * 2]).unwrap();
    ftl.flush().unwrap();
    let mut output = [0; PAGE_BYTES * 2];
    ftl.read(Sector(63), &mut output).unwrap();
    assert_eq!(output, [0x99; PAGE_BYTES * 2]);
}}

#[test]
fn unflushed_owner_cache_survives_same_medium_remount() { for configuration in CONFIGURATIONS {
    let mut fixture = NandFixture::erased(8, configuration);
    let mut cache = [FtlCacheBlock::new()];
    {
        let mut image = fixture.image();
        let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
        ftl.write(Sector(7), &[0x42; 512]).unwrap();
    }
    assert!(fixture.data.iter().all(|&byte| byte == 0xff));
    {
        let mut image = fixture.image();
        let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
        let mut output = [0; 512];
        ftl.read(Sector(7), &mut output).unwrap();
        assert_eq!(output, [0x42; 512]);
        ftl.flush().unwrap();
    }
    assert_eq!(&fixture.data[7 * 512..8 * 512], &[0x42; 512]);
}}

#[test]
fn cancelled_program_keeps_previous_map_and_dirty_data_for_explicit_retry() { for configuration in CONFIGURATIONS {
    let mut fixture = NandFixture::erased(8, configuration);
    fixture.mapped(5, 0, 0x11);
    fixture.failures[0].program_cancel = FailureCount::new(1);
    let mut image = fixture.image();
    let mut cache = [FtlCacheBlock::new()];
    let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
    ftl.write(Sector(0), &[0x22; 512]).unwrap();
    assert!(matches!(ftl.flush(), Err(FtlError::Nand(crate::application_nand::Error::Image(_)))));
    assert_eq!(ftl.map().physical(LogicalBlock::new(0).unwrap()).unwrap().unwrap().index(), 5);
    let mut output = [0; 1024];
    ftl.read(Sector(0), &mut output).unwrap();
    assert_eq!(&output[..512], &[0x22; 512]);
    assert_eq!(&output[512..], &[0x11; 512]);
    ftl.flush().unwrap();
    assert_ne!(ftl.map().physical(LogicalBlock::new(0).unwrap()).unwrap().unwrap().index(), 5);
}}

#[test]
fn retirement_reports_status_and_complete_data_and_oob_under_both_laws() {
    for configuration in CONFIGURATIONS {
        let mut fixture = NandFixture::erased(1, configuration);
        fixture.data[..512].fill(0x55);
        let expected_data = fixture.data.clone();
        let mut bad = [0xff; 16];
        bad[5] = 0xe0;
        let mut expected_spare = vec![bad; PAGES_PER_BLOCK as usize];
        if configuration == HostConfiguration::G1StrictRejectBeforeMutation {
            expected_spare[0] = [0xff; 16];
        }
        let mut image = fixture.image();
        assert_eq!(image.configuration(), configuration);
        assert_eq!(image.provenance(), ImageProvenance::Synthetic);
        let status = image.mark_bad(PhysicalBlock::new(0).unwrap(), TIMEOUT);
        assert_eq!(status, Ok(()), "{configuration:?}");
        assert_eq!(image.bytes(), expected_data.as_slice(), "{configuration:?}");
        assert_eq!(image.spare_records(), expected_spare.as_slice(), "{configuration:?}");
    }
}

#[test]
fn dirty_cache_cannot_cross_host_law_configuration() {
    let mut fixture = NandFixture::erased(8, HostConfiguration::G1StrictRejectBeforeMutation);
    let mut cache = [FtlCacheBlock::new()];
    {
        let mut image = fixture.image();
        let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
        ftl.write(Sector(0), &[0x22; 512]).unwrap();
    }
    fixture.configuration = HostConfiguration::LegacyAndCommitFailedStatus;
    {
        let mut image = fixture.image();
        let result = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT);
        assert!(matches!(result, Err(FtlError::Geometry)));
    }
    assert!(fixture.data.iter().all(|&byte| byte == 0xff));
    fixture.configuration = HostConfiguration::G1StrictRejectBeforeMutation;
    let mut image = fixture.image();
    let mut ftl = FilesystemFtl::mount_image(&mut image, fixture_geometry(), &mut cache, TIMEOUT).unwrap();
    let mut bytes = [0; 512];
    ftl.read(Sector(0), &mut bytes).unwrap();
    assert_eq!(bytes, [0x22; 512]);
}
