use crate::boundary::{ClientRef, Frame, LensReading};
use crate::capture_boundary::*;
use crate::capture_fakes::{Call, CaptureFake};
use crate::lens_return::{LensReturn, ReturnPoint, SavedSlots};
use crate::ports::{LensMove, ReturnEffect, ReturnQuery};
use std::vec::Vec;
use xm1_saclac_interface::calculation::{ReferenceLimits, ReferenceRequest};
use xm1_saclac_interface::{stock, Completion, RecordHandle};

const RECORD: u32 = 0x1000;

fn record() -> RecordHandle { stock::record_handle_from_a2(RECORD) }

fn parameters() -> CaptureParameters {
    CaptureParameters { options: CaptureOptions(0), mode: 0, sources: ClientRef(0x200), reference: 0, slots: ClientRef(0x300), selection: ClientRef(0x400) }
}

fn begun(fake: &mut CaptureFake) -> LensReturn {
    let mut lens_return = LensReturn::new();
    lens_return.effect(ReturnEffect::Begin(record()), &parameters(), fake);
    lens_return
}

fn two_slots(fake: &mut CaptureFake) {
    fake.selection = SlotSelection { requested: 1, count: 2 };
    fake.add_slot(0x51, 70, -4);
    fake.add_slot(0x52, 71, 9);
}

#[test]
fn preparing_the_records_publishes_the_capture_into_the_bound_record() {
    let mut fake = CaptureFake::new();
    fake.frame = Frame(0x8000_0000);
    two_slots(&mut fake);
    let mut lens_return = begun(&mut fake);
    lens_return.effect(ReturnEffect::PrepareRecords, &parameters(), &mut fake);
    assert_eq!(fake.record_writes(), std::vec![
        Call::SetField(record(), RecordField::CaptureFrame, 0x8000_0000),
        Call::SetField(record(), RecordField::CaptureSlotCount, 2),
        Call::SetField(record(), RecordField::CaptureSlots, 0x300),
    ]);
}

#[test]
fn an_armed_point_asks_for_a_calculation_and_becomes_pending() {
    let mut fake = CaptureFake::new();
    fake.set(RecordField::ResourceKind, 13);
    let mut lens_return = begun(&mut fake);
    let mut point = ReturnPoint { saved: Some(SavedSlots::default()), armed: true, pending: false };
    assert!(lens_return.query(ReturnQuery::NeedsCalculation, &mut point, &parameters(), &mut fake));
    assert!(point.pending);
    assert!(fake.record_writes().is_empty());

    point.armed = false;
    assert!(!lens_return.query(ReturnQuery::NeedsCalculation, &mut point, &parameters(), &mut fake));
    assert!(!point.pending);
}

#[test]
fn only_a_kind_13_resource_without_saved_positions_needs_a_calculation() {
    for (kind, saved, needs) in [(2, None, false), (13, None, true), (13, Some(SavedSlots::default()), false)] {
        let mut fake = CaptureFake::new();
        fake.set(RecordField::ResourceKind, kind);
        let mut lens_return = begun(&mut fake);
        let mut point = ReturnPoint { saved, ..ReturnPoint::default() };
        assert_eq!(lens_return.query(ReturnQuery::NeedsCalculation, &mut point, &parameters(), &mut fake), needs, "kind {kind} saved {saved:?}");
    }
}

#[test]
fn saved_positions_go_back_into_each_slot_of_the_record() {
    let mut fake = CaptureFake::new();
    fake.set(RecordField::ResourceKind, 13);
    fake.selection = SlotSelection { requested: 1, count: 2 };
    let mut lens_return = begun(&mut fake);
    let saved = SavedSlots { references: [31, 32, 33], offsets: [-2, 5, 7] };
    let mut point = ReturnPoint { saved: Some(saved), ..ReturnPoint::default() };
    assert!(!lens_return.query(ReturnQuery::NeedsCalculation, &mut point, &parameters(), &mut fake));
    assert_eq!(fake.record_writes(), std::vec![
        Call::SetSlotField(record(), 0, SlotField::Reference, 31),
        Call::SetSlotField(record(), 0, SlotField::Offset, (-2i32) as u32),
        Call::SetSlotField(record(), 1, SlotField::Reference, 32),
        Call::SetSlotField(record(), 1, SlotField::Offset, 5),
    ]);
}

#[test]
fn more_slots_than_were_saved_restore_only_the_saved_ones() {
    let mut fake = CaptureFake::new();
    fake.set(RecordField::ResourceKind, 13);
    fake.selection = SlotSelection { requested: 1, count: 5 };
    let mut lens_return = begun(&mut fake);
    let mut point = ReturnPoint { saved: Some(SavedSlots { references: [1, 2, 3], offsets: [4, 5, 6] }), ..ReturnPoint::default() };
    lens_return.query(ReturnQuery::NeedsCalculation, &mut point, &parameters(), &mut fake);
    let slots: Vec<u8> = fake.record_writes().iter().filter_map(|call| match call { Call::SetSlotField(_, slot, SlotField::Reference, _) => Some(*slot), _ => None }).collect();
    assert_eq!(slots, std::vec![0, 1, 2]);
}

#[test]
fn the_calculation_request_moves_the_template_to_the_lens_and_carries_the_record_limits() {
    let mut fake = CaptureFake::new();
    fake.set(RecordField::ReferenceFirstLimit, (-7i16) as u16 as u32);
    fake.set(RecordField::ReferenceSecondLimit, 300);
    fake.set(RecordField::ReferenceOption, 9);
    fake.set_lens(LensReading::Reference, 20);
    fake.pairs[4] = ReferencePair { reference: 6, detail: 4 };
    let mut template = [0u8; ReferenceRequest::BYTES];
    template[0x38..0x3a].copy_from_slice(&100i16.to_le_bytes());
    fake.template = ReferenceRequest::from_bytes(template);
    let mut lens_return = begun(&mut fake);
    lens_return.effect(ReturnEffect::RequestCalculation, &parameters(), &mut fake);

    // Offset (20 - 6) * 2 = 28; rounding (4 + 1) - ((6 + 4 + 20) + 1) = -26; shift 28 + 26.
    let mut expected = fake.template;
    expected.shift_positions(54);
    expected.set_limits(ReferenceLimits { first: -7, second: 300, option: 9 });
    assert_eq!(fake.templates_for, std::vec![20]);
    assert_eq!(fake.calls, std::vec![Call::RequestReference { request: expected, result: CalculationResult::LensReturn, completion: Completion::Calculation90 }]);
    assert_eq!(lens_return.request().position(), 154);
}

#[test]
fn the_calculation_restores_into_the_return_result_and_installs_it_before_absorbing() {
    let mut fake = CaptureFake::new();
    let mut lens_return = begun(&mut fake);
    for effect in [ReturnEffect::MarkActive(true), ReturnEffect::PrepareCalculation, ReturnEffect::AbsorbCalculation, ReturnEffect::ConcludeExposure] {
        lens_return.effect(effect, &parameters(), &mut fake);
    }
    assert_eq!(fake.calls, std::vec![
        Call::SetReturnActive(true),
        Call::RestoreReturnCalculation,
        Call::InstallReturnCalculation,
        Call::AbsorbCalculation,
        Call::LoadReturnRecords,
    ]);
}

#[test]
fn the_slots_are_copied_to_the_client_only_when_the_first_slot_is_selected() {
    for (requested, copied) in [(1, true), (2, false)] {
        let mut fake = CaptureFake::new();
        two_slots(&mut fake);
        fake.selection.requested = requested;
        let mut lens_return = begun(&mut fake);
        lens_return.effect(ReturnEffect::PrepareMoves, &parameters(), &mut fake);
        let expected = if copied {
            std::vec![Call::StoreSlotEntry(SlotHandle(0x51), CaptureFake::entry(0)), Call::StoreSlotEntry(SlotHandle(0x52), CaptureFake::entry(1))]
        } else {
            Vec::new()
        };
        assert_eq!(fake.record_writes(), expected, "requested {requested}");
    }
}

#[test]
fn the_lens_moves_back_to_the_first_slot_reference_when_it_stands_elsewhere() {
    for (state, moves) in [(70, false), (69, true)] {
        let mut fake = CaptureFake::new();
        two_slots(&mut fake);
        fake.set_lens(LensReading::State, state);
        let mut lens_return = begun(&mut fake);
        let mut point = ReturnPoint::default();
        assert_eq!(lens_return.query(ReturnQuery::NeedsAbsoluteMove, &mut point, &parameters(), &mut fake), moves);
        lens_return.lens_move(LensMove::ReturnAbsolute, &mut fake);
        assert_eq!(fake.calls, std::vec![
            Call::Trace(CaptureTrace::ReturnAbsolute),
            Call::Absolute { target: 70, profile: MoveProfile::LensReturn, completion: Completion::Lens3a },
        ]);
    }
}

#[test]
fn the_relative_move_covers_the_remapped_distance_at_the_lens_speed() {
    for (remap_mode, delta) in [(1, 3 * 70 + 8 + 100 - 500), (0, 3 * 70 + 8 - 500)] {
        let mut fake = CaptureFake::new();
        two_slots(&mut fake);
        fake.set(RecordField::RemapMode, remap_mode);
        fake.set_lens(LensReading::Position, 500);
        fake.set_lens(LensReading::State, 8);
        fake.set_lens(LensReading::Kind, 2);
        let mut lens_return = begun(&mut fake);
        lens_return.query(ReturnQuery::NeedsAbsoluteMove, &mut ReturnPoint::default(), &parameters(), &mut fake);
        lens_return.effect(ReturnEffect::PrepareRelative, &parameters(), &mut fake);
        lens_return.lens_move(LensMove::ReturnRelative, &mut fake);
        assert_eq!(fake.calls.last(), Some(&Call::Relative { delta, speed: 2, profile: MoveProfile::LensReturn, completion: Completion::Lens35 }));
        assert!(fake.has(&Call::Trace(CaptureTrace::ReturnRelative)));
    }
}

#[test]
fn moves_of_other_steps_are_not_started_by_the_lens_return() {
    let mut fake = CaptureFake::new();
    let lens_return = begun(&mut fake);
    for lens_move in [LensMove::FinishAbsolute, LensMove::FinishRelative, LensMove::CaptureAbsolute, LensMove::CaptureRelative] {
        lens_return.lens_move(lens_move, &mut fake);
    }
    assert!(fake.calls.is_empty());
}
