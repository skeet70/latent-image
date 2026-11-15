//! Synthetic X-M1 1.50 stock-memory fixtures. No stock instructions or devices execute here.

use core::cell::RefCell;
use xm1_saclac_calc::scene_recognition::SceneRecognition;
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

struct NoHardware;
impl xm1_saclac_imaging::driver::Isp for NoHardware {
    fn read(&mut self, _: xm1_saclac_imaging::driver::Register) -> u32 { panic!("unexpected ISP read") }
    fn write(&mut self, _: xm1_saclac_imaging::driver::Register, _: u32) { panic!("unexpected ISP write") }
    fn record(&mut self, _: xm1_saclac_imaging::driver::Unit) -> xm1_saclac_imaging::driver::Record { panic!("unexpected ISP record") }
    fn set_record(&mut self, _: xm1_saclac_imaging::driver::Unit, _: xm1_saclac_imaging::driver::Record) { panic!("unexpected ISP record write") }
    fn note(&mut self, _: xm1_saclac_imaging::driver::Unit, _: xm1_saclac_imaging::driver::Operation) { panic!("unexpected ISP operation") }
    fn log(&mut self, _: xm1_saclac_imaging::driver::Event) { panic!("unexpected ISP log") }
    fn exclusive<V>(&mut self, _: impl FnOnce(&mut Self) -> V) -> V { panic!("unexpected ISP lock") }
}
impl xm1_saclac_calc::exposure::ports::Placement for NoHardware {
    fn address(&mut self, _: xm1_saclac_calc::exposure::ports::InPlace) -> xm1_saclac_interface::calc::SharedAddress { panic!("unexpected placement") }
}

fn unused_callbacks(gate: &crate::statistics_results::ReportingGate) -> crate::shot_services::Callbacks<'_> {
    use crate::shot_services::*;
    use xm1_saclac_interface::stock::StockCallbackWord as Callback;
    Callbacks {
        exposure: crate::exposure_services::Callbacks { evaluation: Callback(0), correction: Callback(0) },
        calculation: Callback(0), statistics: Callback(0), capture_selection_hook: Callback(0),
        capture_calculations: CaptureCalculationCallbacks { first: Callback(0), second: Callback(0), third: Callback(0), fourth: Callback(0), area_evaluation: [Callback(0); 2] },
        moves: LensMoveCallbacks { finish_relative: Callback(0), finish_absolute: Callback(0), capture_relative: Callback(0), capture_absolute: Callback(0) },
        capture_statistics: CaptureStatisticsCallbacks { setup: Callback(0), setting: Callback(0), primary_setting: Callback(0) },
        measurement: MeasurementCallbacks { finished: Callback(0), ready: Callback(0), transferred: Callback(0) },
        capture_image: CaptureImageCallbacks { end: Callback(0), outputs: Callback(0), readout: Callback(0), delivery: Callback(0), complete: Callback(0), notice: Callback(0), command_c: Callback(0), command_28: Callback(0) },
        pipeline: PipelineCallbacks {
            raw_calculated: Callback(0), tone_calculated: Callback(0), gradation_calculated: Callback(0), raw_checked: Callback(0),
            frame_prepared: Callback(0), tone_progress: Callback(0), tone_finished: Callback(0), output: Callback(0), commit: Callback(0),
            statistics: Callback(0), statistics_commit: Callback(0), statistics_progress: Callback(0), progress_gate: gate,
        },
    }
}

macro_rules! acquisition {
    ($root:ident, $recognition:ident, $cx:ident) => {
        let owner = RefCell::new($root);
        let machine = owner.borrow().machine.clone();
        let geometry = RefCell::new(xm1_saclac_profile_geometry::controller::ProfileGeometry::new(
            crate::stock::geometry::Memory(machine.clone()),
            crate::geometry::GeometryBindings {
                stages: crate::stages::StageDriver { isp: NoHardware, arguments: crate::stock::stages::StageMemory, machine: machine.clone() },
                peers: fake::peers(&$cx),
            },
            xm1_saclac_profile_geometry::profile::Profiles::new(xm1_saclac_profile_geometry::stock::StockTables),
        ));
        let exposure = crate::exposure_services::ExposureState::default();
        let placement = RefCell::new(NoHardware);
        let subjects = RefCell::new(crate::shot_subjects::Workspace::default());
        let capture = RefCell::new(crate::shot_capture_workspace::Workspace::default());
        let motion = RefCell::new(None);
        let record = xm1_saclac_interface::stock::record_handle_from_a2(u32::MAX);
        let binding = xm1_saclac_search_bindings::RecordBinding::new(record);
        let progress = crate::statistics_results::ReportingGate::new(false);
        let library = crate::shot_services::Libraries {
            services: &owner, geometry: &geometry, recognition: &$recognition, placement: &placement, exposure: &exposure,
            peers: (), record, live_search_record: &binding, subjects: &subjects, capture: &capture, motion: &motion,
            callbacks: unused_callbacks(&progress),
        };
        let shot = RefCell::new(xm1_shot_sequence::shot_ports::ShotPorts::new(
            crate::stock::shot::ShotMemory(machine, &exposure.context), library,
        ));
        let mut $root = crate::acquisition::Acquisition::new(&owner, &$recognition, &shot);
    };
}

fn put(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
fn half(bytes: &mut [u8], at: usize, value: u16) { bytes[at..at + 2].copy_from_slice(&value.to_le_bytes()); }
fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
fn load_half(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) }
fn memory(fake: &Fake, address: u32, bytes: &[u8]) {
    for (offset, &byte) in bytes.iter().enumerate() { fake.set_byte(address + offset as u32, byte); }
}
fn completion<'a>(kind: Completion, bytes: &'a [u8]) -> Completed<'a> { Completed { completion: kind, parameters: parameters(bytes) } }


#[test]
fn live_primary_round_publishes_completion_levels_and_releases_statistics_for_its_consumers() {
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default();
    let record = fake::open(&mut bytes, &[0, 4, 5, 6, 13, 14, 17, 20, 22, 26, 27, 31, 33, 37, 39, 41]);
    let (machine, root) = fake::root_with(fake::peers(&cx), bytes, Entries::default());
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);
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
    let (machine, root) = fake::root_with(fake::peers(&cx), bytes, Entries { header_31: 47, ..Entries::default() });
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);
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

    root.mark_selection_changed();
    assert!(root.with_selection(|memory| memory.value_changed()));
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
    assert_eq!(child.handle(control(AcquisitionControl::Stop), &mut root), ChildOutcome::Complete);
    assert!(!root.with_selection(|memory| memory.value_changed()));
}

#[test]
fn exposure_mirror_updates_are_gated_and_leave_other_consumers_bytes_unchanged() {
    for mode in [0, 1, 2] {
        let kernel = fake::saclac_kernel(); let cx = kernel.task().unwrap();
        let mut bytes = Bytes::default();
        let record = fake::open(&mut bytes, &[33]);
        bytes.shared[0x7a0..0x7ec].fill(0xa5);
        let (_, root) = fake::root_with(fake::peers(&cx), bytes, Entries { header_63: mode, ..Entries::default() });
        let recognition = RefCell::new(SceneRecognition::new());
        acquisition!(root, recognition, cx);
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
    let (_, root) = fake::root_with(fake::peers(&cx), Bytes::default(), Entries::default());
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);
    assert_eq!(root.select_program(&ProgramInputs { kind: 2, option: 1, percent: 31, ..ProgramInputs::default() }), None);
}

#[test]
fn controller_reuse_is_cleared_only_under_the_record_named_by_its_reply_block() {
    let kernel = fake::saclac_kernel(); let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default(); let record = fake::open(&mut bytes, &[27]);
    put(&mut bytes.shared, 0x744, 1);
    let (machine, root) = fake::root_with(fake::peers(&cx), bytes, Entries::default());
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);
    machine.set_word(0x16be01c, u32::MAX);
    root.clear_controller_reuse();
    assert_eq!(root.record_view(record).reuse_blocked, 1);
    machine.set_word(0x16be01c, record_handle_word(record));
    root.clear_controller_reuse();
    assert_eq!(root.record_view(record).reuse_blocked, 0);
}

#[test]
fn record_view_uses_owned_capture_offset_and_constant_restore_adjustment() {
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default();
    let record = fake::open(&mut bytes, &[0, 27]);
    bytes.records[0][0x69] = 1;
    half(&mut bytes.records[0], 0x2c, 500);
    half(&mut bytes.records[0], 0x22, (-12i16) as u16);
    half(&mut bytes.records[0], 0x2e, 0x1234);
    put(&mut bytes.shared, 0x740, 2);
    let (machine, root) = fake::root_with(fake::peers(&cx), bytes, Entries { header_31: 7, ..Entries::default() });
    machine.adjustment(0x48a3, 25);
    machine.adjustment(0x4a24, 0xfd);
    machine.adjustment(0x4a25, 0xff);
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);

    let view = root.record_view(record);
    assert_eq!(
        (
            view.lens_limit,
            view.restore_adjustment,
            view.converted_level,
            view.decade,
            view.mode.0,
            view.updated_position,
            view.position_halfword,
        ),
        (525, 0, (-12i32) as u32, (-30i16) as u16, 7, 7, 0x1234),
    );
    let entered = machine.take_entered();
    assert!(!entered.iter().any(|(address, _)| {
        [0xfa_7634, 0xfa_7954, 0xfa_7984, 0xfa_6bb4, 0xf5_1b94, 0xf5_f9fc, 0xef_74e4].contains(address)
    }));
}

#[test]
fn selected_record_level_uses_the_owned_zone_fallback() {
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default();
    let record = fake::open(&mut bytes, &[0]);
    let (machine, root) = fake::root_with(fake::peers(&cx), bytes, Entries {
        selection: xm1_saclac_profile_records::MeasurementSelection::Record,
        header_31: 9,
        header_63: 1,
        ..Entries::default()
    });
    machine.answer(0x77_2f28, (-300i32) as u32);
    memory(&machine, 0x0091_9788, &0x1234_5678u32.to_le_bytes());
    memory(&machine, 0x0091_978c, &0xabcd_u16.to_le_bytes());
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);

    let view = root.record_view(record);
    assert_eq!(
        (view.converted_level, view.mode.0, view.updated_position, view.position_halfword),
        ((-300i32) as u32, 0x1234_5678, 0x1234_5678, 0xabcd),
    );
    let entered = machine.take_entered();
    assert!(entered.contains(&(0x77_2f28, vec![3])));
    assert!(!entered.iter().any(|(address, _)| {
        [0xfa_7634, 0xfa_7954, 0xfa_7984, 0xfa_6bb4].contains(address)
    }));
}

#[test]
fn owned_lens_services_preserve_record_calibration_and_class_boundaries() {
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default();
    let record = fake::open(&mut bytes, &[0]);
    bytes.records[0][0x69] = 1;
    half(&mut bytes.records[0], 0x2c, 0x4567);
    let (machine, mut root) = fake::root_with(fake::peers(&cx), bytes, Entries::default());
    machine.adjustment(0x3e0f82, 2);
    machine.set_word(0x0102_2fbc, 0x500);
    machine.set_word(0x0102_2fc0, 0x600);
    machine.adjustment(0x500, 6);
    machine.adjustment(0x501, 0);
    machine.adjustment(0x600, 1);
    machine.adjustment(0x601, 0);
    let lens_steps = [
        xm1_saclac_calc::adjust::profile_adjustment::LensSteps::lens_step(&mut root, 0),
        xm1_saclac_calc::adjust::profile_adjustment::LensSteps::lens_step(&mut root, 31),
        xm1_saclac_calc::adjust::profile_adjustment::LensSteps::lens_step(&mut root, 2),
    ];
    let ratio = xm1_saclac_calc::adjust::profile_adjustment::StepRatios::step_ratio(&mut root, 2, 1);
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);

    assert_eq!(lens_steps, [31, 31, 0]);
    assert_eq!(ratio, 17);
    assert_eq!(root.lens_word(record, None), 0x4567);
    assert_eq!(root.acquisition_gain_step(900).0, 3);
    assert_eq!([2, 3, 4, 5].map(|kind| root.kind_decade(kind)), [0, 1, 1, 0]);
    let entered = machine.take_entered();
    assert!(!entered.iter().any(|(address, _)| {
        [0xf4_f16c, 0xfa_7edc, 0xef_7820, 0x74_6804, 0x100_7e44].contains(address)
    }));
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
    let (machine, root) = fake::root_with(fake::peers(&cx), bytes, Entries::default());
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);
    machine.set_word(xm1_saclac_calc::stock::adjust::SHARED_WORD.0, 1);
    let mut live = LiveWork::new();
    live.submit_secondary(record, &mut root);
    let (current, saved) = root.with_selection(|memory| memory.presets());
    assert_eq!((current.word, current.halfwords, saved.word), (8, [77, 66], 0));
    root.mark_selection_changed();
    let mut s1 = S1Acquisition::new();
    let control = |control| ChildEvent::Control { control, parameters: parameters(&[]), record };
    assert_eq!(s1.handle(control(AcquisitionControl::Initialize), &mut root), ChildOutcome::Complete);
    assert_eq!(root.with_selection(|memory| memory.presets()).0, current);
    assert!(root.with_selection(|memory| memory.value_changed()));
    assert_eq!(s1.handle(control(AcquisitionControl::Stop), &mut root), ChildOutcome::Complete);
    assert_eq!(root.with_selection(|memory| memory.presets()).0, current);
    assert!(!root.with_selection(|memory| memory.value_changed()));
    let posts = fake::posts(&kernel);
    assert_eq!(posts.iter().map(|post| (post.0, post.1)).collect::<Vec<_>>(), vec![(45, 0x3d01)]);
}

#[test]
fn empty_s1_evaluation_preserves_both_persistent_selection_grids() {
    use xm1_focus_measurement_round::acquisition::ports::EvaluationGrid;
    use xm1_saclac_interface::round_acquisition::AcquisitionRequest;
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let (machine, root) = fake::root_with(fake::peers(&cx), Bytes::default(), Entries::default());
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);
    root.set_grid_count(Branch::Live, 7);
    root.set_grid_count(Branch::S1, 9);
    root.set_grid_configuration(Branch::Live, 0x12);
    root.set_grid_configuration(Branch::S1, 0x34);
    let grid = crate::stock::acquisition::CALC1_GRID.0;
    let request = AcquisitionRequest::default();
    root.request_acquisition(Branch::S1, &request, EvaluationGrid::Empty);
    assert!((0..1236).all(|offset| machine.byte(grid + offset) == 0));
    root.request_acquisition(Branch::S1, &request, EvaluationGrid::Current);
    assert_eq!((machine.word(grid), machine.byte(grid + 0x458)), (9, 0x34));
    root.request_acquisition(Branch::Live, &request, EvaluationGrid::Current);
    assert_eq!((machine.word(grid), machine.byte(grid + 0x458)), (7, 0x12));
    root.request_acquisition(Branch::S1, &request, EvaluationGrid::Current);
    assert_eq!((machine.word(grid), machine.byte(grid + 0x458)), (9, 0x34));
}

#[test]
fn search_refresh_and_acquisition_observe_one_configuration() {
    use core::cell::RefCell;
    use xm1_saclac_focus_search::contrast::Platform;
    use xm1_saclac_search_bindings::{RecordBinding, Session};
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let (machine, root) = fake::root_with(fake::peers(&cx), Bytes::default(), Entries::default());
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);
    let record = RecordBinding::new(xm1_saclac_interface::stock::record_handle_from_a2(0));
    {
        let bindings = RefCell::new(root.search_bindings());
        Session::new(&bindings, &record).request_scan_refresh(true);
    }
    assert_eq!(root.adjustment_option(), 1);
    machine.adjustment(0x87fd, 1);
    {
        let bindings = RefCell::new(root.search_bindings());
        let mut search = Session::new(&bindings, &record);
        assert!(search.scan_refresh());
        search.request_scan_refresh(true);
    }
    assert_eq!(root.adjustment_option(), 0);
}

#[test]
fn selection_defaults_reflect_numbered_record_layout_and_preserve_pending_selection_change() {
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let mut bytes = Bytes::default();
    let record = fake::open(&mut bytes, &[0, 1, 27]);
    bytes.records[0][0x32] = 13;
    put(&mut bytes.records[0], 0x3c, 0x1234);
    let (_, root) = fake::root_with(fake::peers(&cx), bytes, Entries::default());
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);
    root.mark_selection_changed();
    let defaults = root.prepare_selection_defaults(record);
    assert_eq!((defaults.word, defaults.words), (1, [1, 2, 3]));
    assert!(root.with_selection(|selection| selection.value_changed()));
}

#[test]
fn restored_setting_keeps_saved_signed_positions_at_the_same_reference_and_names_both_references() {
    use xm1_focus_measurement_round::auxiliary::ResourceIndex;
    use xm1_saclac_interface::calculation::ReferenceRequest;
    let kernel = fake::saclac_kernel();
    let cx = kernel.task().unwrap();
    let (machine, root) = fake::root_with(fake::peers(&cx), Bytes::default(), Entries::default());
    let mut saved = [0; ReferenceRequest::BYTES];
    put(&mut saved, 0x100, 100);
    put(&mut saved, 8, 999);
    half(&mut saved, 0x38, (-17i16) as u16);
    let positions = [i16::MIN, -20, -1, 0, 1, 17, 200, 1000, i16::MAX];
    for (index, position) in positions.into_iter().enumerate() { half(&mut saved, 0x104 + 2 * index, position as u16); }
    memory(&machine, xm1_shot_sequence::stock::calculation::SAVED_INPUT, &saved);
    let recognition = RefCell::new(SceneRecognition::new());
    acquisition!(root, recognition, cx);
    root.restore_selection_setting(Branch::S1, ResourceIndex(100));
    let destination = crate::stock::acquisition::region(Branch::S1).setting.0;
    let answer = ReferenceRequest::from_bytes(core::array::from_fn(|index| machine.read_u8(destination + index as u32)));
    assert_eq!((answer.reference(), answer.reference_copy(), answer.position()), (100, 100, -17));
    assert_eq!(answer.area_positions(), positions);
}

#[test]
fn conditional_restore_preserves_work_when_disabled_and_restores_the_full_result_for_nonzero_flags() {
    use xm1_shot_sequence::stock::state::{RESULT, RESULT_BYTES, RETAINED_RESULT};
    for enabled in [0, 2] {
        let kernel = fake::saclac_kernel();
        let cx = kernel.task().unwrap();
        let mut bytes = Bytes::default();
        let record = fake::open(&mut bytes, &[0, 27]);
        bytes.records[0][0x62] = enabled;
        let entries = Entries { selection: xm1_saclac_profile_records::MeasurementSelection::Record, ..Entries::default() };
        let (machine, root) = fake::root_with(fake::peers(&cx), bytes, entries);
        machine.set_word(RESULT.get(), 0x12345678);
        machine.set_word(RETAINED_RESULT.get(), 0x87654321);
        let last = RESULT_BYTES as u32 - 1;
        machine.set_byte(RESULT.get() + last, 0x12);
        machine.set_byte(RETAINED_RESULT.get() + last, 0x87);
        let recognition = RefCell::new(SceneRecognition::new());
        acquisition!(root, recognition, cx);
        root.restore_selection_if_enabled(record, 0);
        let expected = if enabled == 0 { (0x12345678, 0x12) } else { (0x87654321, 0x87) };
        assert_eq!((machine.word(RESULT.get()), machine.byte(RESULT.get() + last)), expected);
    }
}

#[test]
fn selected_reference_uses_the_live_bank_and_reclassification_can_replace_a_tracked_area() {
    use xm1_saclac_interface::{calculation::{AreaAt, AreaHalf, AreaIndex, AreaResult, GridCell, ResultPass, TrackedKind}, selection::{ClassifyStage, SelectionSlot}};
    use xm1_shot_sequence::stock::state;
    for (tracking, control, expected_level) in [(false, 0, 202), (false, 2, -101), (true, 0, 202), (true, 2, -101)] {
        let kernel = fake::saclac_kernel();
        let cx = kernel.task().unwrap();
        let mut bytes = Bytes::default();
        let record = fake::open(&mut bytes, &[0, 27]);
        bytes.records[0][0x51] = 2;
        let (machine, root) = fake::root_with(fake::peers(&cx), bytes, Entries::default());
        machine.adjustment(0x5257, control);
        machine.set_word(state::TRACKING.get(), u32::from(tracking));
        machine.set_word(state::AREA_COUNT.get(), 1);
        let index = AreaIndex::FIRST;
        let pass = ResultPass::Plain;
        memory(&machine, state::cell(SelectionSlot { kind: None, pass, index }).get(), &[1, 2]);
        machine.set_word(state::subject_kind(ClassifyStage::Absorb, index).get(), 1);
        machine.set_word(state::subject_kind(ClassifyStage::Round, index).get(), 2);
        for (at, level) in [
            (AreaAt::Cell { pass, cell: GridCell { column: 1, row: 2 }, index }, -101),
            (AreaAt::Tracked { pass, kind: TrackedKind::new(1).unwrap(), index }, 202),
        ] {
            let mut area = AreaResult::default();
            area.set_half(AreaHalf::Level, level);
            for (half, value) in [(AreaHalf::At0c, -300), (AreaHalf::At0e, 7), (AreaHalf::At10, 11), (AreaHalf::At1a, 13)] { area.set_half(half, value); }
            memory(&machine, state::area(at).get(), area.bytes());
        }
        let recognition = RefCell::new(SceneRecognition::new());
        acquisition!(root, recognition, cx);
        let answer = root.selected_grid_reference(record, Default::default());
        assert_eq!((answer.level, answer.second, answer.word, answer.levels),
            (expected_level as u16, (-269i16) as u16, 2, [(-300i16) as u16, 7, 11, 13]));
        if tracking && control != 0 {
            assert_eq!(machine.word(state::subject_kind(ClassifyStage::Round, index).get()), 7);
        }
    }
}

#[test]
fn selected_reference_mode_one_uses_the_areas_active_word_and_live_class_aspect_byte() {
    use xm1_saclac_interface::{calculation::{AreaAt, AreaIndex, AreaResult, AreaWord, GridCell, ResultPass}, selection::ClassifyStage};
    use xm1_shot_sequence::stock::state;
    for (active, aspect, expected) in [(0, 0, 2), (0, 1, 3), (9, 0, 4)] {
        let kernel = fake::saclac_kernel();
        let cx = kernel.task().unwrap();
        let mut bytes = Bytes::default();
        let record = fake::open(&mut bytes, &[0, 27]);
        bytes.records[0][0x51] = 1;
        let (machine, root) = fake::root_with(fake::peers(&cx), bytes, Entries::default());
        machine.set_word(state::subject_kind(ClassifyStage::Absorb, AreaIndex::FIRST).get(), 7);
        let mut area = AreaResult::default();
        area.set_word(AreaWord::At24, active);
        area.set_word(AreaWord::ClassIndex, 3);
        let at = AreaAt::Cell { pass: ResultPass::Plain, cell: GridCell { column: 0, row: 0 }, index: AreaIndex::FIRST };
        memory(&machine, state::area(at).get(), area.bytes());
        machine.set_byte(0xef6df2 + 3 * 20, aspect);
        let recognition = RefCell::new(SceneRecognition::new());
        acquisition!(root, recognition, cx);
        assert_eq!(root.selected_grid_reference(record, Default::default()).word, expected);
    }
}
