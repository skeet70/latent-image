use std::collections::HashMap;

use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::{ImageProcessor, ImageProcessorOperation, ImageProcessorRequest};
use xm1_saclac_shared::image_registers::{RegisterGroup, RegisterSnapshot, WindowLimits};

use crate::face::{submit_triplet, NotPosted};
use crate::registers::*;
use crate::scan::*;
use crate::services::*;

#[derive(Default)]
struct Blocks {
    fields: HashMap<String, u32>,
    flags: [u8; 19],
    applied: AppliedParts,
    window: Option<[u16; 3]>,
    cleared: bool,
}

impl ScanBlocks for Blocks {
    fn clear_configuration(&mut self) {
        self.cleared = true;
        self.fields.clear();
    }
    fn set(&mut self, field: ScanField, value: u32) { self.fields.insert(format!("{field:?}"), value); }
    fn configuration(&self, field: ScanField) -> u32 { self.fields.get(&format!("{field:?}")).copied().unwrap_or(0) }
    fn request_flag(&self, index: u8) -> u8 { self.flags[usize::from(index)] }
    fn applied_parts(&self) -> AppliedParts { self.applied }
    fn mark_applied(&mut self, parts: AppliedParts) { self.applied.0 |= parts.0; }
    fn clear_applied_parts(&mut self) { self.applied = AppliedParts(0); }
    fn set_request_window(&mut self, limits: [u16; 3]) { self.window = Some(limits); }
}

#[derive(Default)]
struct Library(Vec<StockPart>, usize);

impl ScanLibrary<Blocks> for Library {
    fn apply(&mut self, part: StockPart, _: &mut Blocks) { self.0.push(part); }
    fn finish_defaults(&mut self, _: &mut Blocks) { self.1 += 1; }
}

#[test]
fn a_part_applies_only_when_its_request_byte_enables_it_and_marks_its_bit() {
    let mut blocks = Blocks::default();
    let mut library = Library::default();
    assert!(apply(Part::Geometry, &mut blocks, &mut library), "always applied");
    assert!(!apply(Part::Second, &mut blocks, &mut library));
    blocks.flags[1] = 2;
    assert!(!apply(Part::Second, &mut blocks, &mut library), "enabled only by 1");
    blocks.flags[1] = 1;
    assert!(apply(Part::Second, &mut blocks, &mut library));
    blocks.flags[16] = 5;
    assert!(apply(Part::Seventeenth, &mut blocks, &mut library), "any nonzero byte");
    assert_eq!(library.0, [StockPart::Geometry, StockPart::Second, StockPart::Seventeenth]);
    assert_eq!(blocks.applied, AppliedParts(1 << 16 | 1 << 17 | 1 << 29));
}

#[test]
fn the_window_part_copies_the_configured_window_into_the_request() {
    let mut blocks = Blocks::default();
    set_window(&mut blocks, [3, 4, 5]);
    blocks.flags[2] = 1;
    let mut library = Library::default();
    assert!(apply(Part::Window, &mut blocks, &mut library));
    assert_eq!((blocks.window, blocks.applied, library.0.len()), (Some([3, 4, 5]), AppliedParts(1 << 18), 0));
}

#[test]
fn setters_clamp_as_the_scan_library_expects() {
    let mut blocks = Blocks::default();
    set_140(&mut blocks, 12);
    set_13c(&mut blocks, 12);
    set_148(&mut blocks, 12);
    set_1dc(&mut blocks, 5);
    set_144(&mut blocks, true);
    assert_eq!(
        [ScanField::Word140, ScanField::Word13c, ScanField::Word148, ScanField::Word1dc, ScanField::Word144].map(|field| blocks.configuration(field)),
        [8, 9, 3, 0, 1]
    );
    set_1dc(&mut blocks, 4);
    assert_eq!(blocks.configuration(ScanField::Word1dc), 4);
}

#[test]
fn defaults_set_the_configuration_and_let_the_library_finish() {
    let mut blocks = Blocks::default();
    let mut library = Library::default();
    set_defaults(&mut blocks, &mut library);
    assert_eq!(blocks.configuration(ScanField::Word11c), 600);
    assert_eq!(blocks.configuration(ScanField::Half1a0), 0x2ba);
    assert_eq!(blocks.configuration(ScanField::Half1a4), 0x7e);
    assert_eq!(blocks.configuration(ScanField::WordF4), 2);
    assert_eq!(library.1, 1);
    clear(&mut blocks);
    assert!(blocks.cleared);
}

#[derive(Default)]
struct Snapshot(Vec<(char, usize)>);

impl RegisterSnapshot for Snapshot {
    type Configuration = ();
    fn restore(&mut self, group: RegisterGroup) { self.0.push(('r', group.index())); }
    fn window_limits(&self, _: ()) -> WindowLimits { WindowLimits([1, 2, 3]) }
    fn window_words(&self) -> [u32; 2] { [0, 0] }
    fn set_window_words(&mut self, _: [u32; 2]) { self.0.push(('w', 2)); }
    fn pack_retained(&mut self, group: RegisterGroup, _: ()) { self.0.push(('p', group.index())); }
}

#[derive(Default)]
struct Hardware(Vec<(u32, u32)>);

impl PipelineHardware for Hardware {
    fn acquire(&mut self, client: u32) -> u32 {
        self.0.push((client, 0));
        42
    }
    fn release(&mut self, client: u32, grant: u32) { self.0.push((client, grant)); }
}

#[test]
fn register_groups_follow_the_applied_parts_with_group_zero_for_any_of_them() {
    let selected: Vec<usize> = groups(AppliedParts(1 << 18 | 1 << 30)).map(|group| group.index()).collect();
    assert_eq!(selected, [0, 2, 14]);
    assert_eq!(groups(AppliedParts(1 << 16)).count(), 0);
    let mut snapshot = Snapshot::default();
    pack_groups(AppliedParts(1 << 17 | 1 << 18), (), &mut snapshot);
    assert_eq!(snapshot.0, [('p', 0), ('p', 1), ('w', 2)]);
}

#[test]
fn a_restore_holds_the_hardware_around_the_groups() {
    let (mut snapshot, mut hardware) = (Snapshot::default(), Hardware::default());
    restore_groups(AppliedParts(1 << 19), &mut snapshot, &mut hardware);
    assert_eq!(snapshot.0, [('r', 0), ('r', 3)]);
    assert_eq!(hardware.0, [(RESTORE_CLIENT, 0), (RESTORE_CLIENT, 42)]);
}

#[test]
fn services_clear_the_alias_bit_and_adjust_the_delay() {
    assert_eq!(cached(0x4012_3456), 0x0012_3456);
    assert_eq!(adjusted_delay(20), 21);
    assert_eq!(adjusted_delay(21), 21);
    assert_eq!(adjusted_delay(0x8000_0005), 5);
    assert!(lens_status_bit(0x40));
    assert!(!lens_status_bit(0xbf));
}

#[derive(Default)]
struct Requests(Vec<u32>, Vec<(u8, u32)>, u32);

impl LensRequests for Requests {
    fn store_callback(&mut self, callback: u32) { self.0.push(callback); }
    fn request(&mut self, number: u8, callback: u32) -> u32 {
        self.1.push((number, callback));
        self.2
    }
}

#[test]
fn a_lens_request_stores_the_callback_then_requests_the_mapped_number() {
    let mut requests = Requests::default();
    assert_eq!(lens_request(&mut requests, 11, 0xaa), 0);
    assert_eq!(lens_request(&mut requests, 15, 0xbb), REFUSED_INDEX);
    requests.2 = 1;
    assert_eq!(lens_request(&mut requests, 3, 0xcc), NOT_POSTED);
    assert_eq!(requests.0, [0xaa, 0xbb, 0xcc], "the callback is stored even for a refused index");
    assert_eq!(requests.1, [(12, 0xaa), (3, 0xcc)]);
}

struct Processor(Vec<Vec<u8>>, bool);

impl ImageProcessor for Processor {
    type Error = ();
    fn request(&mut self, request: ImageProcessorRequest<'_>) -> Result<(), ()> {
        assert_eq!(request.operation, ImageProcessorOperation::Request03);
        self.0.push(request.parameters.bytes().to_vec());
        if self.1 { Ok(()) } else { Err(()) }
    }
}

#[test]
fn a_triplet_posts_its_completion_and_values() {
    let mut processor = Processor(Vec::new(), true);
    let completion = StockCallbackWord(0x00ee_cd04);
    assert_eq!(submit_triplet(&mut processor, [1, 2, 3], completion), Ok(()));
    assert_eq!(processor.0[0], [4, 0xcd, 0xee, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]);
    processor.1 = false;
    assert_eq!(submit_triplet(&mut processor, [1, 2, 3], completion), Err(NotPosted));
}
