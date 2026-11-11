use super::*;
use crate::storage_shutdown_load::DefaultBank;
use std::vec::Vec;

#[derive(Clone, Copy)]
struct CacheRead {
    block: IndexedBlock,
    status: MediumStatus,
    copied: Option<(u32, usize)>,
}

struct Medium {
    section: Result<DefaultMediaOffset, MediumStatus>,
    lock_status: MediumStatus,
    cached_status: MediumStatus,
    direct_status: MediumStatus,
    cached: Vec<CacheRead>,
    direct: Vec<(IndexedBlock, u32, usize)>,
    unlocks: usize,
    pending_copy: Option<(u32, usize)>,
    lock_calls: usize,
}
impl Medium {
    fn new(section: u32) -> Self {
        Self {
            section: Ok(DefaultMediaOffset(section)),
            lock_status: MediumStatus(0),
            cached_status: MediumStatus(0),
            direct_status: MediumStatus(0),
            cached: Vec::new(),
            direct: Vec::new(),
            unlocks: 0,
            pending_copy: None,
            lock_calls: 0,
        }
    }
}
impl DefaultStorageIo for Medium {
    fn section_offset(&mut self) -> Result<DefaultMediaOffset, MediumStatus> { self.section }
    fn lock_cache(&mut self) -> MediumStatus {
        self.lock_calls += 1;
        self.lock_status
    }
    fn unlock_cache(&mut self) { self.unlocks += 1 }
    fn read_cached(&mut self, first: IndexedBlock) -> MediumStatus {
        self.pending_copy = Some((first.0, 0));
        self.cached_status
    }
    fn copy_cached(&mut self, offset: u32, destination: &mut [u8]) {
        for (index, byte) in destination.iter_mut().enumerate() { *byte = (offset as usize + index) as u8; }
        let (block, _) = self.pending_copy.unwrap();
        self.cached.push(CacheRead { block: IndexedBlock(block), status: self.cached_status, copied: Some((offset, destination.len())) });
    }
    fn read_direct(&mut self, first: IndexedBlock, count: u32, destination: &mut [u8]) -> MediumStatus {
        self.direct.push((first, count, destination.len()));
        self.direct_status
    }
}

fn reads(medium: &Medium) -> Vec<(u32, u32, usize)> {
    medium.cached.iter().map(|read| (read.block.0, read.copied.unwrap().0, read.copied.unwrap().1)).collect()
}

#[test]
fn primary_bank_starts_one_page_into_the_defaults_table() {
    let mut medium = Medium::new(0x1000);
    let mut destination = std::vec![0u8; 0x9200];
    assert_eq!(read_defaults(&mut medium, DefaultBank::Primary, &mut destination), MediumStatus(0));
    assert_eq!(reads(&medium), std::vec![(0, 0x16e00, 0x9200)]);
    assert!(medium.direct.is_empty());
    assert_eq!((medium.lock_calls, medium.unlocks), (1, 1));
    assert_eq!(destination[0], 0);
    assert_eq!(destination[0x91ff], (0x16e00usize + 0x91ff) as u8);
}

#[test]
fn extended_bank_starts_at_the_section_offset() {
    let mut medium = Medium::new(0x1000);
    let mut destination = std::vec![0u8; 0x9200];
    assert_eq!(read_defaults(&mut medium, DefaultBank::Extended, &mut destination), MediumStatus(0));
    assert_eq!(reads(&medium), std::vec![(0, 0x1000, 0x9200)]);
}

#[test]
fn whole_pages_are_read_directly_and_the_tail_is_cached() {
    let mut medium = Medium::new(0x40000);
    let mut destination = std::vec![0u8; 0x20000 + 0x800];
    assert_eq!(read_defaults(&mut medium, DefaultBank::Extended, &mut destination), MediumStatus(0));
    assert_eq!(medium.direct, std::vec![(IndexedBlock(2), 1, 0x20000)]);
    assert_eq!(reads(&medium), std::vec![(3, 0, 0x800)]);
}

#[test]
fn unaligned_section_offset_splits_the_partial_and_tail_pages() {
    let mut medium = Medium::new(0x100);
    let mut destination = std::vec![0u8; 0x30000];
    assert_eq!(read_defaults(&mut medium, DefaultBank::Extended, &mut destination), MediumStatus(0));
    assert_eq!(reads(&medium), std::vec![(0, 0x100, 0x1ff00), (1, 0, 0x10100)]);
    assert!(medium.direct.is_empty());
    assert_eq!((medium.lock_calls, medium.unlocks), (2, 2));
}

#[test]
fn aligned_destination_reads_only_whole_pages() {
    let mut medium = Medium::new(0x40000);
    let mut destination = std::vec![0u8; 0x40000];
    assert_eq!(read_defaults(&mut medium, DefaultBank::Extended, &mut destination), MediumStatus(0));
    assert_eq!(medium.direct, std::vec![(IndexedBlock(2), 2, 0x40000)]);
    assert!(medium.cached.is_empty());
    assert_eq!(medium.lock_calls, 0);
}

#[test]
fn empty_destination_reads_nothing() {
    let mut medium = Medium::new(0x40000);
    let mut destination: [u8; 0] = [];
    assert_eq!(read_defaults(&mut medium, DefaultBank::Extended, &mut destination), MediumStatus(0));
    assert!(medium.direct.is_empty());
    assert!(medium.cached.is_empty());
    assert_eq!(medium.lock_calls, 0);
}

#[test]
fn section_offset_failure_stops_before_any_read() {
    let mut medium = Medium::new(0);
    medium.section = Err(MediumStatus(-30));
    let mut destination = std::vec![0u8; 0x100];
    assert_eq!(read_defaults(&mut medium, DefaultBank::Primary, &mut destination), MediumStatus(-30));
    assert!(medium.cached.is_empty());
    assert!(medium.direct.is_empty());
    assert_eq!(medium.lock_calls, 0);
}

#[test]
fn cache_lock_failure_reports_an_unspecified_failure_without_reading() {
    let mut medium = Medium::new(0x100);
    medium.lock_status = MediumStatus(-5);
    let mut destination = std::vec![0u8; 0x100];
    assert_eq!(read_defaults(&mut medium, DefaultBank::Extended, &mut destination), MediumStatus(-255));
    assert!(medium.cached.is_empty());
    assert_eq!(medium.unlocks, 0);
}

#[test]
fn cached_read_failure_is_returned_after_the_copy_and_unlock() {
    let mut medium = Medium::new(0x100);
    medium.cached_status = MediumStatus(-30);
    let mut destination = std::vec![0u8; 0x100];
    assert_eq!(read_defaults(&mut medium, DefaultBank::Extended, &mut destination), MediumStatus(-30));
    assert_eq!(reads(&medium), std::vec![(0, 0x100, 0x100)]);
    assert_eq!(medium.unlocks, 1);
}

#[test]
fn direct_read_failure_skips_the_tail_read() {
    let mut medium = Medium::new(0x40000);
    medium.direct_status = MediumStatus(-30);
    let mut destination = std::vec![0u8; 0x20000 + 0x800];
    assert_eq!(read_defaults(&mut medium, DefaultBank::Extended, &mut destination), MediumStatus(-30));
    assert_eq!(medium.direct, std::vec![(IndexedBlock(2), 1, 0x20000)]);
    assert!(medium.cached.is_empty());
}

#[test]
fn default_banks_are_distinct_table_offsets() {
    let mut primary = Medium::new(0x20000);
    let mut extended = Medium::new(0x20000);
    let mut destination = std::vec![0u8; 0x800];
    read_defaults(&mut primary, DefaultBank::Primary, &mut destination);
    read_defaults(&mut extended, DefaultBank::Extended, &mut destination);
    assert_eq!(reads(&primary), std::vec![(1, 0x15e00, 0x800)]);
    assert_eq!(reads(&extended), std::vec![(1, 0, 0x800)]);
}
