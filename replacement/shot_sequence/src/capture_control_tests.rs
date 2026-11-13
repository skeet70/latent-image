use crate::boundary::{ClientRef, Frame, LensReading};
use crate::capture_boundary::*;
use crate::capture_control::CaptureControl;
use crate::capture_fakes::{Call, CaptureFake};
use crate::capture_stock;
use crate::lens_return::{ReturnPoint, SavedSlots};
use crate::subjects::{Candidates, Subject, SubjectId, SubjectList};
use std::vec::Vec;
use xm1_saclac_interface::statistics::{SettingOverrides, StatisticsSetting};
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};

const RECORD: u32 = 0x2000;
const FRAME: Frame = Frame(0x8000_0000);

fn record() -> RecordHandle { stock::record_handle_from_a2(RECORD) }

fn parameters(options: u32, mode: u32) -> CaptureParameters {
    CaptureParameters { options: CaptureOptions(options), mode, sources: ClientRef(0x200), profile: ClientRef(0x280), slots: ClientRef(0x300), selection: ClientRef(0x400) }
}

fn bound() -> CaptureControl {
    let mut control = CaptureControl::new();
    control.bind(record());
    control
}

fn slots(fake: &mut CaptureFake, requested: u8, count: u8) {
    fake.selection = SlotSelection { requested, count };
    for index in 0..count { fake.add_slot(0x51 + u32::from(index), 0x10 + u32::from(index), index as i16); }
}

#[test]
fn the_statistics_setup_carries_the_option_bit() {
    for (options, enabled) in [(0x40, true), (0xbf, false)] {
        let mut fake = CaptureFake::new();
        bound().request_statistics_setup(&parameters(options, 0), &mut fake);
        assert_eq!(fake.calls, std::vec![Call::StatisticsSetup { enabled, completion: Completion::Statistics83 }]);
    }
}

#[test]
fn a_setting_request_lays_the_command_over_the_last_primary_record() {
    let mut fake = CaptureFake::new();
    let mut control = bound();
    control.setting_template = StatisticsSetting::from_bytes([0x5a; StatisticsSetting::BYTES]);
    let overrides = SettingOverrides { selector: 1, words: [2, 3, 4, 5, 6], window: [7; 6], option: 8 };
    control.request_setting(&overrides, &mut fake);
    let mut expected = control.setting_template;
    expected.apply(&overrides);
    assert_eq!(fake.calls, std::vec![Call::StatisticsSetting { setting: expected, completion: Completion::Statistics85 }]);
}

#[test]
fn committing_a_setting_stores_what_its_completion_delivered() {
    let mut fake = CaptureFake::new();
    let mut control = bound();
    let mut payload = [0u8; 24];
    for index in 0..6 { payload[4 * index..4 * index + 4].copy_from_slice(&(index as u32 + 1).to_le_bytes()); }
    control.receive(Completed { completion: Completion::Statistics85, parameters: stock::parameters(&payload) });
    control.receive(Completed { completion: Completion::Statistics83, parameters: stock::parameters(&[9; 24]) });
    control.commit_setting(&mut fake);
    assert_eq!(fake.calls, std::vec![Call::StoreSettingResult(record(), SettingResult([2, 3, 4, 5])), Call::RefreshSetting]);
}

#[test]
fn a_job_starts_from_an_invalid_unrefined_focus_result() {
    let mut fake = CaptureFake::new();
    fake.focus.extent = 128;
    bound().prepare_record(&mut fake);
    let Some(Call::StoreFocusResult(_, stored)) = fake.calls.first().cloned() else { panic!("no store") };
    assert_eq!((stored.valid, stored.refined, stored.kept, stored.extent), (0, 0, 1, 128));
}

#[test]
fn the_release_rebinds_the_device_monitor_and_traces() {
    let mut fake = CaptureFake::new();
    bound().release(&mut fake);
    assert_eq!(fake.calls, std::vec![Call::MonitorDeviceCommands(true), Call::Trace(CaptureTrace::Released)]);
}

#[test]
fn only_a_first_slot_capture_decides_the_lens_return() {
    let untouched = ReturnPoint { saved: Some(SavedSlots::default()), armed: true, pending: true };
    for (requested, count, mode) in [(2, 3, 0), (1, 2, 3), (2, 2, 0)] {
        let mut fake = CaptureFake::new();
        slots(&mut fake, requested, count);
        fake.return_request = true;
        let mut point = untouched;
        assert!(!bound().needs_lens_return(&parameters(0, mode), &mut point, &mut fake));
        assert_eq!(point, untouched);
        assert!(!fake.return_request, "the half-press request is consumed");
    }
}

#[test]
fn a_lens_off_the_fifth_reference_arms_the_return() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 1, 2);
    fake.set(RecordField::ResourceKind, 2);
    fake.pairs[4].reference = 9;
    fake.set_lens(LensReading::Reference, 8);
    let mut point = ReturnPoint { pending: true, ..ReturnPoint::default() };
    assert!(bound().needs_lens_return(&parameters(0, 0), &mut point, &mut fake));
    assert_eq!(point, ReturnPoint { saved: None, armed: true, pending: false });
}

#[test]
fn a_half_press_request_asks_for_the_return_without_saving_slots() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 1, 2);
    fake.return_request = true;
    let mut point = ReturnPoint::default();
    assert!(bound().needs_lens_return(&parameters(0, 0), &mut point, &mut fake));
    assert_eq!(point.saved, None);
}

#[test]
fn slots_whose_reference_moves_with_the_lens_are_saved_converted() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 1, 2);
    fake.set_lens(LensReading::Reference, 4);
    let mut point = ReturnPoint::default();
    assert!(bound().needs_lens_return(&parameters(0, 0), &mut point, &mut fake));
    // Converted references are slot + 4; the second slot (0x11) also maps unchanged.
    assert_eq!(point.saved, Some(SavedSlots { references: [0x14, 0x15, 0], offsets: [0x10, 0x11, 0] }));
}

#[test]
fn a_slot_that_maps_differently_drops_the_saved_positions() {
    let mut fake = CaptureFake::new();
    fake.selection = SlotSelection { requested: 1, count: 1 };
    fake.add_slot(0x51, 1, 0);
    fake.set_lens(LensReading::Reference, 1);
    let mut point = ReturnPoint::default();
    assert!(bound().needs_lens_return(&parameters(0, 0), &mut point, &mut fake));
    assert_eq!(point.saved, None);
}

#[test]
fn unchanged_slots_need_no_return() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 1, 3);
    let mut point = ReturnPoint { armed: true, ..ReturnPoint::default() };
    assert!(!bound().needs_lens_return(&parameters(0, 0), &mut point, &mut fake));
    assert_eq!(point, ReturnPoint::default());
}

#[test]
fn the_secondary_job_follows_the_diagnostic_style_or_the_record() {
    let decide = |setup: &dyn Fn(&mut CaptureFake), mode: u32| {
        let mut fake = CaptureFake::new();
        setup(&mut fake);
        bound().secondary_job(&parameters(0, mode), &mut fake)
    };
    assert!(!decide(&|fake| fake.secondary_enabled = true, 3));
    for style in 0..9u8 {
        let expected = matches!(style, 1 | 4 | 5 | 6 | 7);
        assert_eq!(decide(&|fake| { fake.set_adjustment(CaptureAdjustment::Diagnostics, 1); fake.set_adjustment(CaptureAdjustment::SecondaryStyle, style) }, 0), expected, "style {style}");
    }
    assert!(decide(&|fake| fake.secondary_enabled = true, 0));
    assert!(!decide(&|fake| { fake.secondary_enabled = true; fake.set(RecordField::SecondaryBlocked, 1) }, 0));
    assert!(!decide(&|fake| { fake.secondary_enabled = true; fake.set(RecordField::SecondaryMode, 7) }, 0));
    assert!(!decide(&|_| {}, 0));
}

fn prepared(fake: &mut CaptureFake, parameters: CaptureParameters) -> (CaptureControl, Candidates) {
    let mut control = bound();
    let mut candidates = Candidates::default();
    control.prepare_job(&parameters, &mut candidates, fake);
    (control, candidates)
}

#[test]
fn a_job_loads_the_profile_and_selects_the_active_slot_as_the_options_ask() {
    for (options, expected) in [
        (0x02, std::vec![Call::LoadProfile(record(), ClientRef(0x280)), Call::SelectSlot(record(), SlotHandle(0x52))]),
        (0x01, std::vec![Call::SelectSlot(record(), SlotHandle(0x52))]),
        (0x40, Vec::new()),
    ] {
        let mut fake = CaptureFake::new();
        slots(&mut fake, 2, 3);
        prepared(&mut fake, parameters(options, 0));
        let selected: Vec<Call> = fake.calls.iter().filter(|call| matches!(call, Call::LoadProfile(..) | Call::SelectSlot(..))).cloned().collect();
        assert_eq!(selected, expected, "options {options:#x}");
    }
}

#[test]
fn a_first_slot_job_refreshes_rates_from_the_lens_and_hands_the_slots_out() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 1, 2);
    fake.set(RecordField::SlotMode, 1);
    for (reading, value) in [(LensReading::Reference, 1), (LensReading::State, 2), (LensReading::Position, 3), (LensReading::Kind, 4), (LensReading::Control, 5)] {
        fake.set_lens(reading, value);
    }
    prepared(&mut fake, parameters(0, 0));
    assert_eq!(fake.slot_writes(SlotField::Rate), std::vec![(0, 15), (1, 15), (2, 15)]);
    assert_eq!(fake.slot_writes(SlotField::Direction), std::vec![(0, 12), (1, 12), (2, 12)]);
    let handed: Vec<SlotHandle> = fake.calls.iter().filter_map(|call| match call { Call::StoreSlotEntry(handle, _) => Some(*handle), _ => None }).collect();
    assert_eq!(handed, std::vec![SlotHandle(0x51), SlotHandle(0x52), SlotHandle(0x51), SlotHandle(0x52)]);
}

#[test]
fn each_slot_keeps_its_distance_up_to_the_limit_and_steps_the_rest() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 1, 1);
    let mut entry = SlotEntry([0; SlotEntry::BYTES]);
    entry.0[0x8..0xa].copy_from_slice(&10i16.to_le_bytes());
    entry.0[0xc..0xe].copy_from_slice(&20i16.to_le_bytes());
    entry.0[0xe..0x10].copy_from_slice(&30i16.to_le_bytes());
    fake.entries.insert(0, entry);
    fake.split_limit = 45;
    prepared(&mut fake, parameters(0, 0));
    assert_eq!(fake.slot_writes(SlotField::Step), std::vec![(0, 15)]);
    assert_eq!(fake.slot_writes(SlotField::Residual), std::vec![(0, 35)]);
    let Some(Call::StoreSlotEntry(SlotHandle(0x51), handed)) = fake.calls.iter().rev().find(|call| matches!(call, Call::StoreSlotEntry(..))).cloned() else { panic!("not handed") };
    assert_eq!((handed.step(), handed.residual(), handed.offset()), (15, 35, 10));
}

#[test]
fn a_job_ends_with_the_hook_the_prepared_mark_and_the_secondary_pending() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 2, 2);
    let (control, _) = prepared(&mut fake, parameters(0, 3));
    assert!(control.secondary_pending());
    assert!(!control.held());
    let tail: Vec<Call> = fake.calls.iter().rev().take(3).rev().cloned().collect();
    assert_eq!(tail, std::vec![Call::MonitorDeviceCommands(false), Call::SelectionHook(ClientRef(0x400)), Call::SetField(record(), RecordField::JobPrepared, 1)]);
    assert!(!fake.has(&Call::ResetSecondary), "a reduced capture leaves the secondary state");
    assert_eq!(fake.calls.first(), Some(&Call::Trace(CaptureTrace::PrepareJob)));
}

#[test]
fn the_record_decides_whether_the_secondary_job_is_enabled() {
    for (first, second, mode, enabled) in [(1, 1, 7, true), (1, 0, 7, false), (1, 1, 0, false)] {
        let mut fake = CaptureFake::new();
        slots(&mut fake, 2, 2);
        fake.set_slot(0, SlotField::SecondaryFirst, first);
        fake.set_slot(0, SlotField::SecondarySecond, second);
        fake.set(RecordField::SecondaryMode, mode);
        prepared(&mut fake, parameters(0, 0));
        assert_eq!(fake.secondary_enabled, enabled);
        assert!(fake.has(&Call::ResetSecondary));
        assert!(fake.has(&Call::StoreItem(record(), 0, ItemEntry::EMPTY)));
    }
}

#[test]
fn the_diagnostic_style_overrides_the_record_decision() {
    for (style, record_style, enabled) in [(3, 7, true), (5, 0, true), (2, 6, false)] {
        let mut fake = CaptureFake::new();
        slots(&mut fake, 2, 2);
        fake.set_adjustment(CaptureAdjustment::Diagnostics, 1);
        fake.set_adjustment(CaptureAdjustment::SecondaryStyle, style);
        prepared(&mut fake, parameters(0, 0));
        assert!(fake.has(&Call::SetField(record(), RecordField::SecondaryStyle, record_style)));
        assert_eq!(fake.secondary_enabled, enabled);
    }
}

#[test]
fn the_half_press_work_leaves_the_selection_result() {
    for (kind, active, expected) in [(1, 1u32, 4), (1, 0, 2), (5, 0, 5)] {
        let mut fake = CaptureFake::new();
        slots(&mut fake, 1, 1);
        fake.has_work = true;
        fake.set(RecordField::SubjectKind, kind);
        let mut entry = SlotEntry([0; SlotEntry::BYTES]);
        entry.0[0x24..0x28].copy_from_slice(&active.to_le_bytes());
        fake.entries.insert(0, entry);
        prepared(&mut fake, parameters(0, 0));
        assert!(fake.has(&Call::SelectionResult(ClientRef(0x400), expected)), "kind {kind}");
    }
}

fn subject(id: u32) -> Subject { Subject { id: SubjectId(id), ..Subject::default() } }

#[test]
fn moved_areas_replace_the_capture_candidates_and_republish_them() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 1, 1);
    fake.has_work = true;
    fake.subject_refresh = 1;
    fake.moved_areas = 2;
    fake.set_adjustment(CaptureAdjustment::Subjects, 1);
    let mut source = SubjectList { count: 1, ..SubjectList::default() };
    source.entries[0] = subject(77);
    fake.sources[1] = source;
    fake.frame = FRAME;
    let (_, candidates) = prepared(&mut fake, parameters(0, 0));
    assert!(fake.has(&Call::UpdateCaptureSelection(record())));
    assert!(fake.has(&Call::ExportSelection(FRAME)));
    let published: Vec<SubjectList> = fake.calls.iter().filter_map(|call| match call { Call::PublishSubjects(_, list) => Some(*list), _ => None }).collect();
    assert_eq!(published.len(), 1);
    assert_eq!(candidates.lists[3], published[0]);
    assert_eq!(published[0].count, 0, "the other lists hold none of the source's subjects");
}

#[test]
fn unmoved_areas_keep_the_candidates() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 1, 1);
    fake.has_work = true;
    fake.subject_refresh = 1;
    fake.set_adjustment(CaptureAdjustment::Subjects, 1);
    prepared(&mut fake, parameters(0, 0));
    assert!(fake.calls.iter().any(|call| matches!(call, Call::ClassifySubjects { .. })));
    assert!(!fake.calls.iter().any(|call| matches!(call, Call::UpdateCaptureSelection(_) | Call::PublishSubjects(..))));
}

#[test]
fn the_dump_clears_the_trace_area_but_its_kept_words_and_exports() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 2, 2);
    fake.frame = FRAME;
    fake.set_adjustment(CaptureAdjustment::Diagnostics, 0x80);
    for (index, offset) in capture_stock::TRACE_KEPT.into_iter().enumerate() { fake.frame_words.insert(offset.0, index as u32 + 1); }
    prepared(&mut fake, parameters(0, 0));
    let fill = fake.calls.iter().position(|call| *call == Call::FillFrame(FRAME, capture_stock::TRACE_AREA, capture_stock::TRACE_AREA_BYTES, 0)).unwrap();
    for (index, offset) in capture_stock::TRACE_KEPT.into_iter().enumerate() {
        let restore = fake.calls.iter().position(|call| *call == Call::SetFrameWord(FRAME, offset, index as u32 + 1)).unwrap();
        assert!(restore > fill);
    }
    for export in [Call::ExportRecordSnapshot(FRAME), Call::ExportJointTables(FRAME), Call::ExportPipeline(FRAME)] { assert!(fake.has(&export)); }
}

#[test]
fn the_selection_map_is_marked_once_and_copied_while_present() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 2, 2);
    fake.frame = FRAME;
    fake.set_adjustment(CaptureAdjustment::Diagnostics, 0x80);
    fake.set_adjustment(CaptureAdjustment::Subjects, 1);
    fake.set(RecordField::MapPresent, 1);
    let mut control = bound();
    let mut candidates = Candidates::default();
    control.prepare_job(&parameters(0, 0), &mut candidates, &mut fake);
    control.prepare_job(&parameters(0, 0), &mut candidates, &mut fake);
    let prepared_areas = fake.calls.iter().filter(|call| matches!(call, Call::PrepareDumpArea(..))).count();
    let copies = fake.calls.iter().filter(|call| matches!(call, Call::CopyTable(_, _, FrameTable::SelectionMap))).count();
    assert_eq!((prepared_areas, copies), (1, 2));

    fake.set(RecordField::MapPresent, 0);
    control.prepare_job(&parameters(0, 0), &mut candidates, &mut fake);
    assert!(fake.has(&Call::FillFrame(FRAME, capture_stock::MAP_AREA, capture_stock::MAP_AREA_BYTES, 0)));
    fake.set(RecordField::MapPresent, 1);
    control.prepare_job(&parameters(0, 0), &mut candidates, &mut fake);
    assert_eq!(fake.calls.iter().filter(|call| matches!(call, Call::PrepareDumpArea(..))).count(), 2, "a cleared map is marked again");
}

#[test]
fn without_subjects_the_map_holds_only_unset_cells() {
    let mut fake = CaptureFake::new();
    slots(&mut fake, 2, 2);
    fake.frame = FRAME;
    fake.set_adjustment(CaptureAdjustment::Diagnostics, 0x80);
    prepared(&mut fake, parameters(0, 0));
    assert!(fake.has(&Call::FillFrame(FRAME, capture_stock::MAP_TABLE, capture_stock::MAP_TABLE_BYTES, 0)));
    for (offset, bytes) in capture_stock::MAP_UNSET { assert!(fake.has(&Call::FillFrame(FRAME, offset, bytes, 0xff))); }
}

#[test]
fn an_unset_second_limit_takes_the_default_when_the_options_ask() {
    for (options, limit, expected) in [(0x10, 0xffff_8000, true), (0x10, 3, false), (0, 0xffff_8000, false)] {
        let mut fake = CaptureFake::new();
        slots(&mut fake, 2, 2);
        fake.set(RecordField::ReferenceSecondLimit, limit);
        prepared(&mut fake, parameters(options, 3));
        assert_eq!(fake.slot_writes(SlotField::SecondLimit) == std::vec![(0, (-500i32) as u32)], expected);
    }
}
