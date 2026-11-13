use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::{ImageProcessor, ImageProcessorOperation, ImageProcessorRequest};
use xm1_saclac_shared::image_registers::{RegisterGroup, RegisterSnapshot, WindowLimits};

use crate::compiled::Optioned;
use crate::face::{submit_triplet, NotPosted};
use crate::registers::*;
use crate::scan::*;
use crate::services::*;

#[derive(Default)]
struct Blocks {
    flags: [u8; 19],
    applied: AppliedParts,
    configured: [u16; 3],
    requested: Option<[u16; 3]>,
    cleared: bool,
}

impl ScanBlocks for Blocks {
    fn clear_configuration(&mut self) { self.cleared = true; }
    fn request_flag(&self, index: u8) -> u8 { self.flags[usize::from(index)] }
    fn applied_parts(&self) -> AppliedParts { self.applied }
    fn mark_applied(&mut self, parts: AppliedParts) { self.applied.0 |= parts.0; }
    fn clear_applied_parts(&mut self) { self.applied = AppliedParts(0); }
    fn configured_window(&self) -> [u16; 3] { self.configured }
    fn set_configured_window(&mut self, limits: [u16; 3]) { self.configured = limits; }
    fn set_request_window(&mut self, limits: [u16; 3]) { self.requested = Some(limits); }
}

#[derive(Default)]
struct Library(Vec<StockPart>, usize);

impl ScanLibrary<Blocks> for Library {
    type Output = ();
    fn apply(&mut self, part: StockPart, _: &mut Blocks) { self.0.push(part); }
    fn finish_defaults(&mut self, _: &mut Blocks) { self.1 += 1; }
    fn fill_settings(&mut self, _: &Blocks, _: &mut ()) {}
    fn fill_geometry(&mut self, _: &Blocks, _: &mut ()) {}
    fn nine(&mut self, _: &Blocks) -> Optioned<9> { Optioned { option: 7, values: [1, 2, 3, 4, 5, 6, 7, 8, 9] } }
    fn four(&mut self, _: &Blocks) -> Optioned<4> { Optioned { option: 9, values: [4, 3, 2, 1] } }
}

#[test]
fn table_reads_give_the_values_without_their_option() {
    let (blocks, mut library) = (Blocks::default(), Library::default());
    assert_eq!(nine_values(&mut library, &blocks), [1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(four_values(&mut library, &blocks), [4, 3, 2, 1]);
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
    assert_eq!((blocks.requested, blocks.applied, library.0.len()), (Some([3, 4, 5]), AppliedParts(1 << 18), 0));
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
fn the_cached_alias_clears_bit_thirty() {
    assert_eq!(cached(0x4012_3456), 0x0012_3456);
}

#[derive(Default)]
struct Clock(Vec<u32>);

impl KernelDelay for Clock {
    fn delay(&mut self, ticks: u32) -> u32 {
        self.0.push(ticks);
        7
    }
}

#[test]
fn a_delay_asks_the_kernel_for_the_adjusted_ticks_and_returns_its_status() {
    let mut clock = Clock::default();
    assert_eq!(delay(&mut clock, 20), 7);
    delay(&mut clock, 21);
    delay(&mut clock, 0x8000_0005);
    assert_eq!(clock.0, [21, 21, 5]);
}

struct Lens(Option<u8>);

impl LensStatus for Lens {
    fn status(&mut self) -> Option<u8> { self.0 }
}

#[test]
fn the_lens_status_is_bit_six_of_the_read_byte() {
    assert!(lens_status(&mut Lens(Some(0x40))));
    assert!(!lens_status(&mut Lens(Some(0xbf))));
    assert!(!lens_status(&mut Lens(None)), "a failed read reads as clear");
}

#[derive(Default)]
struct Requests(Vec<StockCallbackWord>, Vec<(u8, StockCallbackWord)>, u32);

impl LensRequests for Requests {
    fn store_callback(&mut self, callback: StockCallbackWord) { self.0.push(callback); }
    fn request(&mut self, number: u8, callback: StockCallbackWord) -> u32 {
        self.1.push((number, callback));
        self.2
    }
}

#[test]
fn a_lens_request_stores_the_callback_then_requests_the_mapped_number() {
    let (first, second, third) = (StockCallbackWord(0xaa), StockCallbackWord(0xbb), StockCallbackWord(0xcc));
    let mut requests = Requests::default();
    assert_eq!(lens_request(&mut requests, 11, first), 0);
    assert_eq!(lens_request(&mut requests, 15, second), REFUSED_INDEX);
    requests.2 = 1;
    assert_eq!(lens_request(&mut requests, 3, third), NOT_POSTED);
    assert_eq!(requests.0, [first, second, third], "the callback is stored even for a refused index");
    assert_eq!(requests.1, [(12, first), (3, third)]);
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
