//! Synthetic X-M1 1.50 stock-memory fixtures. No stock instructions or devices execute here.

use xm1_focus_measurement_round::acquisition::live::LiveWork;
use xm1_focus_measurement_round::acquisition::ports::{Branch, ExposureRecordUpdate, Ports, ProgramInputs};
use xm1_focus_measurement_round::children::S1Acquisition;
use xm1_focus_measurement_round::pipeline::GainStep;
use xm1_saclac_controllers::children::{AcquisitionControl, ChildEvent, ChildOutcome};
use xm1_saclac_interface::stock::{parameters, record_handle_word};
use xm1_saclac_interface::{Completed, Completion};
use xm1_saclac_profile_records::Bytes;
use xm1_saclac_shared::stock::Machine;

use crate::fake::{self, Entries, Fake};

fn put(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
fn half(bytes: &mut [u8], at: usize, value: u16) { bytes[at..at + 2].copy_from_slice(&value.to_le_bytes()); }
fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
fn load_half(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) }
fn memory(fake: &Fake, address: u32, bytes: &[u8]) {
    for (offset, &byte) in bytes.iter().enumerate() { fake.set_byte(address + offset as u32, byte); }
}
fn returning(fake: &Fake, address: u32, value: u32) { for _ in 0..100 { fake.answer(address, value); } }
fn completion<'a>(kind: Completion, bytes: &'a [u8]) -> Completed<'a> { Completed { completion: kind, parameters: parameters(bytes) } }

#[test]
fn fresh_live_acquisition_publishes_the_retained_target_and_range_through_owned_records() {
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default();
    let record = fake::open(&mut bytes, &[0, 4, 5, 6, 13, 14, 15, 17, 20, 27, 31, 33, 37, 39, 41]);
    put(&mut bytes.shared, 0x140, 1);
    put(&mut bytes.shared, 0x734, 3);
    half(&mut bytes.records[0], 0x1c, 40);
    let (machine, mut root) = fake::root_with(fake::peers(&cx), bytes, Entries { header_63: 1, ..Entries::default() });
    returning(&machine, 0x76d9c8, 9);
    returning(&machine, 0xfa7edc, 3);
    returning(&machine, 0x772f28, 12);
    returning(&machine, 0xef5310, 7);
    returning(&machine, 0xef74e4, 70);
    let mut target = [0; 18];
    half(&mut target, 0, 300); half(&mut target, 2, 280); put(&mut target, 4, 3);
    half(&mut target, 8, 12); half(&mut target, 10, 90); half(&mut target, 16, 21);
    machine.buffer(0xf79b94, 0, &target);
    let mut range = [0; 10];
    half(&mut range, 0, 250); half(&mut range, 2, 80); half(&mut range, 4, 20);
    half(&mut range, 6, 0x1122); range[8] = 2;
    machine.buffer(0xf7a0cc, 0, &range);
    returning(&machine, 0xf7a0cc, 0x56);

    let mut live = LiveWork::new();
    live.initialize(record, &mut root);
    live.enable(record, &mut root);
    let published = root.published_exposure(record);
    assert_eq!((live.timing().0, published.timing.0), (0x56, 0x56));
    assert_eq!((published.exposure.0, published.level, published.second, published.offset), (3, 250, 80, 20));
    assert_eq!((published.reference, published.combined, published.reference_level, published.aspect), (40, 150, 190, 7));
    assert_eq!((published.code, published.selected, published.gain_step.0, published.lens.0), (0x1122, 2, 3, 9));
    assert_eq!(root.acquisition_word(record), 1);
    let mirror = &root.records().memory().shared[0x7a0..0x7ec];
    assert_eq!((word(mirror, 0x34), word(mirror, 0x38), word(mirror, 0x3c)), (3, 9, 0));
    let block = root.acquisition_block(record);
    assert_eq!((block.collected, block.selected, block.lens.0, block.exposure.0), (0, 0, 9, 3));
    assert_eq!((block.level, block.offset, block.gain_offset), (250, 100, 12));
    assert_eq!(fake::posts(&kernel), vec![]);
}

#[test]
fn live_primary_round_publishes_completion_levels_and_releases_statistics_for_its_consumers() {
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default();
    let record = fake::open(&mut bytes, &[0, 4, 5, 6, 13, 14, 17, 20, 22, 26, 27, 31, 33, 37, 39, 41]);
    let (machine, mut root) = fake::root_with(fake::peers(&cx), bytes, Entries::default());
    let mut snapshot = [0; 0x5c4];
    put(&mut snapshot, 0, 5); put(&mut snapshot, 0xec, 0x1234);
    put(&mut snapshot, 0xf8, 6); put(&mut snapshot, 0x110, 2);
    half(&mut snapshot, 0xfc, 400); half(&mut snapshot, 0xfe, 100); half(&mut snapshot, 0x100, 20);
    machine.buffer(0xfea7ac, 0, &snapshot);
    machine.adjustment(0x4a07, 1);
    machine.adjustment(0x64a1, 20);
    let mut live = LiveWork::new();
    live.enable(record, &mut root);
    live.submit_primary(record, &mut root);
    let posts = fake::posts(&kernel);
    assert_eq!((posts[0].0, posts[0].1), (45, 0x3d00));
    assert_eq!((word(&posts[0].2, 0xc), word(&posts[0].2, 0x28), load_half(&posts[0].2, 0x2c)), (1, 6, 400));
    let published = root.published_exposure(record);
    assert_eq!((published.exposure.0, published.lens.0, published.level, published.second), (6, 2, 400, 100));

    let mut result = [0; 0x50];
    half(&mut result, 0, 200);
    for (index, value) in [190, 180, 170, 160, 150, 140, 130].into_iter().enumerate() { half(&mut result, 2 + 2 * index, value); }
    put(&mut result, 0x10, 0x76543210);
    for bank in 0..4 {
        let base = 0x41000000 + bank * 0x1000;
        put(&mut result, 0x18 + 4 * bank as usize, base);
        for index in 0..256 { machine.set_word(base + 2 * index, 100 + bank); }
    }
    put(&mut result, 0x28, 6); half(&mut result, 0x2c, 200);
    half(&mut result, 0x32, 15); half(&mut result, 0x38, 5);
    put(&mut result, 0x3c, 2); put(&mut result, 0x44, 1);
    memory(&machine, 0x42000000, &result);
    machine.set_word(0x5ffc6340, 90 | (91 << 16));
    machine.set_word(0x5ffec1b4, 0x12345678);
    machine.set_word(0x5ffec1b4 + 0x114c, 0xaabbccdd);
    let mut reply = [0; 8]; put(&mut reply, 4, 0x42000000);
    live.collect_primary(record, completion(Completion::Calculation87, &reply), &mut root);
    assert_eq!(root.published_exposure(record).primary_levels, [190, 180, 170, 160]);
    assert_eq!((root.acquisition_block(record).collected, root.acquisition_block(record).eligible), (1, 0));
    live.release(record, &mut root);
    let block = root.acquisition_block(record);
    assert_eq!((block.offsets.0, block.classes.0, block.copy.0, block.extra.0, block.release_offset), (0x5ffbece0, 0x5ffbf930, 0x5ffb6120, 0x5ffbf4e0, 20));
    assert_eq!((machine.read_u16(0x5ffbece0), machine.read_u16(0x5ffbf2e0)), (90, 93));
    assert_eq!((machine.read_u16(0x5ffbf4e0), machine.read_u16(0x5ffbf4e2)), (80, 81));
    assert!((0..256).all(|index| machine.byte(0x5ffbf930 + index) == 4));
    assert_eq!((machine.read_u32(0x5ffb6120), machine.read_u32(0x5ffb6120 + 0x114c)), (0x12345678, 0xaabbccdd));
    assert_eq!(root.selection_record(record).tracked, 0);
    live.submit_primary(record, &mut root);
    let posts = fake::posts(&kernel);
    let requests: Vec<_> = posts.iter().filter(|post| post.1 == 0x3d00).collect();
    assert_eq!(word(&requests[1].2, 0xc), 0);
    assert!(machine.take_entered().contains(&(0x740e74, vec![0x42000000, 0x50])));
}

#[test]
fn s1_child_drives_four_bound_passes_then_restores_selection_and_publishes_the_focus_table() {
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default();
    let record = fake::open(&mut bytes, &[0, 4, 5, 6, 13, 14, 15, 17, 20, 22, 26, 27, 31, 33, 37, 39, 41]);
    half(&mut bytes.shared, 0x206, 150); half(&mut bytes.shared, 0x208, 30);
    let (machine, mut root) = fake::root_with(fake::peers(&cx), bytes, Entries { header_31: 47, ..Entries::default() });
    let mut sample = [0; 0x6c];
    put(&mut sample, 0x18, 0x55); put(&mut sample, 0x58, 5);
    half(&mut sample, 0x20, 210); half(&mut sample, 0x22, 80);
    machine.buffer(0xfeaadc, 0, &sample);
    let mut child = S1Acquisition::new();
    let control = |control| ChildEvent::Control { control, parameters: parameters(&[]), record };
    assert_eq!(child.handle(control(AcquisitionControl::Initialize), &mut root), ChildOutcome::Complete);
    assert_eq!(root.records().memory().shared[0x970..0x972], 120i16.to_le_bytes());
    let mut primary = [0; 0x50]; half(&mut primary, 0, 133);
    memory(&machine, 0x42000000, &primary);
    let mut reply = [0; 8]; put(&mut reply, 4, 0x42000000);
    for _ in 0..4 {
        assert_eq!(child.handle(control(AcquisitionControl::Start), &mut root), ChildOutcome::Pending);
        assert_eq!(child.handle(ChildEvent::Completed { completed: completion(Completion::Calculation9b, &reply), record }, &mut root), ChildOutcome::Pending);
    }
    let posts = fake::posts(&kernel);
    assert_eq!(posts.iter().map(|p| (p.0, p.1)).collect::<Vec<_>>(), vec![(45, 0x3d00), (45, 0x3d00), (45, 0x3d00), (45, 0x3d00), (45, 0x3d01)]);
    assert_eq!(posts[..4].iter().map(|p| word(&p.2, 0xc)).collect::<Vec<_>>(), vec![1, 0, 0, 0]);
    assert_eq!(root.records().memory().shared[0x970..0x972], 133i16.to_le_bytes());
    assert_eq!((root.setting_echo(Branch::S1).shot_word, machine.read_u32(0x41717c28 + 0x118)), (0, 0x41717c44));

    root.mark_selection_changed();
    assert!(root.selection_memory().value_changed());
    let mut result = [0; 0x114];
    put(&mut result, 0x88, 4); put(&mut result, 0xfc, 7);
    half(&mut result, 0x8c, 222); half(&mut result, 0x8e, 333);
    half(&mut result, 0xec, 11); half(&mut result, 0xee, 12); half(&mut result, 0xf0, 13); half(&mut result, 0xf2, 14);
    half(&mut result, 0xf8, 55); half(&mut result, 0x9c, 66); result[0x100] = 1;
    put(&mut result, 0xc4, 0x43000000); put(&mut result, 0xc8, 0x44000000); put(&mut result, 0xd0, 1);
    put(&mut result, 0x58, 8); half(&mut result, 0x62, 321);
    put(&mut result, 0x34, 9); half(&mut result, 0x3e, 432);
    memory(&machine, 0x42001000, &result);
    let mut saved = [0; 0x44]; put(&mut saved, 8, 10); half(&mut saved, 0xe, 444); half(&mut saved, 0x3e, 55); half(&mut saved, 0x40, 66);
    memory(&machine, 0x44000000, &saved);
    put(&mut reply, 4, 0x42001000);
    assert_eq!(child.handle(ChildEvent::Completed { completed: completion(Completion::Calculation9c, &reply), record }, &mut root), ChildOutcome::Pending);
    let selection = root.selection_record(record);
    assert_eq!((selection.position, selection.word, selection.level, selection.target, selection.levels), (7, 4, 222, 333, [11, 12, 13, 14]));
    let published = root.published_exposure(record);
    assert_eq!((published.exposure.0, published.level, published.second, published.offset), (10, 444, 55, 66));
    let slot = root.selection_slot(record);
    assert_eq!((slot.word, slot.levels, slot.limit_level), (4, [11, 12, 13, 14], 55));
    let posts = fake::posts(&kernel);
    assert_eq!((posts.last().unwrap().0, posts.last().unwrap().1), (47, 0x3f45));
    assert_eq!((machine.read_u32(0x16f224c), machine.read_u32(0x16f224c + 12)), (0x55, 8));
    assert!(machine.take_entered().contains(&(0xf17af8, vec![1])));
    assert_eq!(child.handle(control(AcquisitionControl::Stop), &mut root), ChildOutcome::Complete);
    assert!(!root.selection_memory().value_changed());
}

#[test]
fn exposure_mirror_updates_are_gated_and_leave_other_consumers_bytes_unchanged() {
    for mode in [0, 1, 2] {
        let kernel = fake::saclac_kernel(); let cx = kernel.task().unwrap();
        let mut bytes = Bytes::default();
        let record = fake::open(&mut bytes, &[33]);
        bytes.shared[0x7a0..0x7ec].fill(0xa5);
        let (_, mut root) = fake::root_with(fake::peers(&cx), bytes, Entries { header_63: mode, ..Entries::default() });
        root.update_exposure_record(record, ExposureRecordUpdate::PrimaryPair([11, 22]));
        root.update_exposure_record(record, ExposureRecordUpdate::SelectionTail { tail: [33, 44, 55], last: 66 });
        root.update_exposure_record(record, ExposureRecordUpdate::GainStep(GainStep(7)));
        let actual = &root.records().memory().shared[0x7a0..0x7ec];
        let mut expected = [0xa5; 76];
        if mode == 1 {
            for (at, value) in [(0x28, 11), (0x2a, 22), (0x2c, 33), (0x2e, 44), (0x30, 55), (0x4a, 66)] { half(&mut expected, at, value); }
            put(&mut expected, 0x34, 7);
        }
        assert_eq!(actual, &expected, "mode {mode}");
    }
}

#[test]
fn an_unselected_banded_choice_without_an_entry_does_not_invent_a_program() {
    let kernel = fake::saclac_kernel(); let cx = kernel.task().unwrap();
    let (_, mut root) = fake::root_with(fake::peers(&cx), Bytes::default(), Entries::default());
    assert_eq!(root.select_program(&ProgramInputs { kind: 2, option: 1, percent: 31, ..ProgramInputs::default() }), None);
}

#[test]
fn controller_reuse_is_cleared_only_under_the_record_named_by_its_reply_block() {
    let kernel = fake::saclac_kernel(); let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default(); let record = fake::open(&mut bytes, &[27]);
    put(&mut bytes.shared, 0x744, 1);
    let (machine, mut root) = fake::root_with(fake::peers(&cx), bytes, Entries::default());
    machine.set_word(0x16be01c, u32::MAX);
    root.clear_controller_reuse();
    assert_eq!(root.record_view(record).reuse_blocked, 1);
    machine.set_word(0x16be01c, record_handle_word(record));
    root.clear_controller_reuse();
    assert_eq!(root.record_view(record).reuse_blocked, 0);
}

#[test]
fn live_and_s1_keep_one_selection_preset_and_s1_consumes_the_controllers_change() {
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default();
    let record = fake::open(&mut bytes, &[0, 4, 5, 6, 13, 14, 15, 17, 20, 22, 26, 27, 31, 33, 37, 39, 41]);
    bytes.records[0][0x44] = 8;
    half(&mut bytes.records[0], 0x1e, 77);
    half(&mut bytes.records[0], 0x1c, 66);
    let (machine, mut root) = fake::root_with(fake::peers(&cx), bytes, Entries::default());
    machine.set_word(xm1_saclac_calc::stock::adjust::SHARED_WORD.0, 1);
    let mut live = LiveWork::new();
    live.submit_secondary(record, &mut root);
    let (current, saved) = root.selection_memory().presets();
    assert_eq!((current.word, current.halfwords, saved.word), (8, [77, 66], 0));
    root.mark_selection_changed();
    let mut s1 = S1Acquisition::new();
    let control = |control| ChildEvent::Control { control, parameters: parameters(&[]), record };
    assert_eq!(s1.handle(control(AcquisitionControl::Initialize), &mut root), ChildOutcome::Complete);
    assert_eq!(root.selection_memory().presets().0, current);
    assert!(root.selection_memory().value_changed());
    assert_eq!(s1.handle(control(AcquisitionControl::Stop), &mut root), ChildOutcome::Complete);
    assert_eq!(root.selection_memory().presets().0, current);
    assert!(!root.selection_memory().value_changed());
    let posts = fake::posts(&kernel);
    assert_eq!(posts.iter().map(|post| (post.0, post.1)).collect::<Vec<_>>(), vec![(45, 0x3d01)]);
}
