use crate::layers::*;
use crate::live_profile::*;
use crate::live_view::{Application, Condition, Effect, MotionSetting, ReplyStatus, RestoreOptions, StopMode, StopReason, ViewRequest};
use crate::replies::{Replies, ReplyCommand as R, Values};
use std::{vec, vec::Vec};
use xm1_saclac_interface::{
    stock, Command, CommandKind as K, Completion as C, CompletionStatus, ImgDc, ImgDcOperation as Op, ImgDcRequest, RecordHandle,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum Seen {
    Write(Field, u32),
    Block(Block, Vec<u32>),
    ClearSelection,
    Descriptor(u32),
    DescriptorPair(u32, u32),
    OwnerNotified,
    Flag(GeometryFlag, bool),
    ResetSelection,
    Hold(bool),
    Claim,
    Publish(Publication),
    DisplayReset,
    Mirror(Mirror),
    Convert,
    BeginRecordMode,
    EndRecordMode,
    RangeUpdate,
    ClearCalculations,
    TakeOffset,
    Request(Op, Vec<u8>, Vec<C>),
    Reply(R, i32, Vec<u32>),
}

#[derive(Default)]
struct Fake {
    fields: Vec<(Field, u32)>,
    changes: Vec<Change>,
    blocks: Vec<(Block, Vec<u32>)>,
    outgrown: Vec<GeometryCheck>,
    refuse: bool,
    alternate: bool,
    lens_follows: bool,
    record_mode_initialized: bool,
    record_mode_available: bool,
    values: ConfigurationValues,
    seen: Vec<Seen>,
}

impl Fake {
    fn with(fields: &[(Field, u32)]) -> Self { Self { fields: fields.to_vec(), ..Self::default() } }
    fn field(&self, field: Field) -> u32 {
        self.fields.iter().rev().find(|(known, _)| *known == field).map_or(0, |(_, value)| *value)
    }
    fn take(&mut self) -> Vec<Seen> { core::mem::take(&mut self.seen) }
    fn requests(&self) -> Vec<(Op, Vec<u8>, Vec<C>)> {
        self.seen.iter().filter_map(|seen| match seen {
            Seen::Request(op, bytes, completions) => Some((*op, bytes.clone(), completions.clone())),
            _ => None,
        }).collect()
    }
}

impl ProfileRecord for Fake {
    fn read(&mut self, record: RecordHandle, field: Field) -> u32 { assert_eq!(record, self::record()); self.field(field) }
    fn write(&mut self, _: RecordHandle, field: Field, value: u32) {
        self.fields.push((field, value));
        self.seen.push(Seen::Write(field, value));
    }
    fn changed(&mut self, _: RecordHandle, change: Change) -> bool { self.changes.contains(&change) }
    fn read_block(&mut self, _: RecordHandle, block: Block, out: &mut [u32]) {
        if let Some((_, words)) = self.blocks.iter().find(|(known, _)| *known == block) {
            out.copy_from_slice(&words[..out.len()]);
        }
    }
    fn write_block(&mut self, _: RecordHandle, block: Block, words: &[u32]) { self.seen.push(Seen::Block(block, words.to_vec())) }
    fn clear_selection(&mut self, _: RecordHandle) { self.seen.push(Seen::ClearSelection) }
    fn select_descriptor(&mut self, _: RecordHandle, descriptor: u32) { self.seen.push(Seen::Descriptor(descriptor)) }
    fn set_descriptor_pair(&mut self, _: RecordHandle, first: u32, second: u32) { self.seen.push(Seen::DescriptorPair(first, second)) }
    fn notify_selection_owner(&mut self, _: RecordHandle) { self.seen.push(Seen::OwnerNotified) }
}

impl Geometry for Fake {
    fn value(&mut self, _: RecordHandle, value: GeometryValue) -> u32 {
        match value { GeometryValue::StartValue => 0x51, GeometryValue::SelectionMode => 0x40, GeometryValue::Adjustment => 0x77 }
    }
    fn motion(&mut self) -> u32 { 9 }
    fn outgrown(&mut self, _: RecordHandle, check: GeometryCheck) -> bool { self.outgrown.contains(&check) }
    fn admits(&mut self, _: u32, _: u32) -> bool { !self.refuse }
    fn set(&mut self, flag: GeometryFlag, on: bool) { self.seen.push(Seen::Flag(flag, on)) }
    fn alternate(&mut self) -> bool { self.alternate }
    fn lens_follows(&mut self) -> bool { self.lens_follows }
    fn reset_selection(&mut self) { self.seen.push(Seen::ResetSelection) }
}

impl Configuration for Fake {
    fn hold(&mut self, held: bool) { self.seen.push(Seen::Hold(held)) }
    fn claim_selection(&mut self) { self.seen.push(Seen::Claim) }
    fn publish(&mut self, _: RecordHandle, publication: Publication) { self.seen.push(Seen::Publish(publication)) }
    fn search_property(&mut self) -> u32 { 0 }
    fn combine_lens_pair(&mut self, first: u32, second: u32, third: u32) -> u32 { first + second + third }
}

impl DisplayRecords for Fake {
    fn reset(&mut self) { self.seen.push(Seen::DisplayReset) }
    fn mirror(&mut self, _: RecordHandle, mirror: Mirror) { self.seen.push(Seen::Mirror(mirror)) }
}

impl RecordMode for Fake {
    fn initialize(&mut self) -> bool { self.record_mode_initialized }
    fn available(&mut self, channel: RecordChannel) -> bool {
        assert_eq!(channel, RecordChannel::LiveView);
        self.record_mode_available
    }
    fn begin(&mut self, channel: RecordChannel) {
        assert_eq!(channel, RecordChannel::LiveView);
        self.seen.push(Seen::BeginRecordMode)
    }
    fn finish(&mut self) { self.seen.push(Seen::EndRecordMode) }
}

impl SharedHelpers for Fake {
    fn update_selection_range(&mut self, _: RecordHandle) { self.seen.push(Seen::RangeUpdate) }
    fn clear_calculations(&mut self) { self.seen.push(Seen::ClearCalculations) }
    fn searched_lens_pair(&mut self, _: RecordHandle) -> u32 { 500 }
    fn convert_resource(&mut self, _: RecordHandle) { self.seen.push(Seen::Convert) }
}

impl Pipeline for Fake {
    fn stop_queried(&mut self) -> bool { true }
}

impl ConfigurationSource for Fake {
    fn values(&mut self, _: RecordHandle) -> ConfigurationValues { self.values }
    fn take_offset(&mut self, _: RecordHandle) -> u16 { self.seen.push(Seen::TakeOffset); 0x0102 }
}

impl ImgDc for Fake {
    type Error = ();
    fn request(&mut self, request: ImgDcRequest<'_>) -> Result<(), ()> {
        self.seen.push(Seen::Request(request.operation, request.parameters.bytes().to_vec(), request.completions.to_vec()));
        Ok(())
    }
}

impl Replies for Fake {
    fn reply(&mut self, command: R, status: CompletionStatus, values: Values<'_>) {
        let Values::Words(words) = values else { panic!("the live view's replies carry words") };
        self.seen.push(Seen::Reply(command, status.0, words.to_vec()))
    }
}

impl Layers for Fake {
    type Profile = Self;
    type Geometry = Self;
    type Configuration = Self;
    type Display = Self;
    type RecordMode = Self;
    type Helpers = Self;
    type Pipeline = Self;
    type Source = Self;
    type ImgDc = Self;
    type Replies = Self;
    fn profile(&mut self) -> &mut Self { self }
    fn geometry(&mut self) -> &mut Self { self }
    fn configuration(&mut self) -> &mut Self { self }
    fn display(&mut self) -> &mut Self { self }
    fn record_mode(&mut self) -> &mut Self { self }
    fn helpers(&mut self) -> &mut Self { self }
    fn pipeline(&mut self) -> &mut Self { self }
    fn source(&mut self) -> &mut Self { self }
    fn imgdc(&mut self) -> &mut Self { self }
    fn replies(&mut self) -> &mut Self { self }
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16a0_0490) }

fn command(kind: K, words: &[u32]) -> (K, Vec<u8>) {
    (kind, words.iter().flat_map(|word| word.to_le_bytes()).collect())
}

fn entered((kind, bytes): &(K, Vec<u8>)) -> Command<'_> { Command { kind: *kind, parameters: stock::parameters(bytes) } }

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes.chunks_exact(4).map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])).collect()
}

#[test]
fn view_start_carries_the_start_value_and_whether_it_is_measured() {
    let mut fake = Fake::default();
    LiveProfile::new().view(ViewRequest::Start { measured: true }, record(), &mut fake);
    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, Op::Request13);
    assert_eq!(words(&requests[0].1)[1..], [0x51, 1]);
    assert_eq!(requests[0].2, vec![C::Image3f]);
}

#[test]
fn restore_and_refresh_share_the_restart_request_with_their_own_words() {
    let mut fake = Fake::with(&[(Field::RefreshValue, 0x44)]);
    let mut live = LiveProfile::new();
    live.view(ViewRequest::Restore(RestoreOptions { first: true, second: false, negative: true }), record(), &mut fake);
    live.view(ViewRequest::Refresh(MotionSetting(5)), record(), &mut fake);
    let requests = fake.requests();
    assert_eq!(requests.iter().map(|request| request.0).collect::<Vec<_>>(), vec![Op::Request16, Op::Request16]);
    assert_eq!(words(&requests[0].1)[1..], [0, 0, 0x8000_0001]);
    assert_eq!(words(&requests[1].1)[1..], [0x44, 5, 3]);
    assert!(requests.iter().all(|request| request.2 == vec![C::Image40]));
}

#[test]
fn view_stop_records_the_pipeline_query_and_sends_the_stop_mode() {
    let mut fake = Fake::default();
    let mut live = LiveProfile::new();
    for mode in [StopMode::Plain, StopMode::Secondary, StopMode::SecondaryNegative] {
        live.view(ViewRequest::Stop(mode), record(), &mut fake);
    }
    assert!(fake.seen.contains(&Seen::Write(Field::StopQueried, 1)));
    let modes: Vec<u32> = fake.requests().iter().map(|request| words(&request.1)[1]).collect();
    assert_eq!(modes, vec![0, 1, 6]);
    assert!(fake.requests().iter().all(|request| request.0 == Op::Request27 && request.2 == vec![C::Image42]));
}

#[test]
fn suspend_marks_the_suspension_block_only_when_the_selection_callback_is_armed() {
    let mut idle = Fake::default();
    LiveProfile::new().view(ViewRequest::Suspend, record(), &mut idle);
    assert!(!idle.seen.iter().any(|seen| matches!(seen, Seen::Block(..))));

    let mut armed = Fake::with(&[(Field::SelectionCallbackArmed, 1)]);
    armed.blocks.push((Block::Suspension, (0..19).collect()));
    LiveProfile::new().view(ViewRequest::Suspend, record(), &mut armed);
    let mut marked: Vec<u32> = (0..19).collect();
    marked[17] = 4;
    assert_eq!(armed.seen[0], Seen::Block(Block::Suspension, marked));
    assert_eq!(armed.requests()[0].0, Op::Request28);
}

#[test]
fn resume_resets_the_selection_buffers_unless_the_profile_is_selected() {
    let mut unselected = Fake::default();
    LiveProfile::new().view(ViewRequest::Resume, record(), &mut unselected);
    assert!(matches!(unselected.seen[0], Seen::Request(Op::Request29, _, _)));
    assert_eq!(unselected.seen[1], Seen::ResetSelection);

    let mut selected = Fake::with(&[(Field::Selection, 1)]);
    LiveProfile::new().view(ViewRequest::Resume, record(), &mut selected);
    assert_eq!(selected.seen.len(), 1);
}

#[test]
fn adjustment_carries_the_selection_word_only_when_asked() {
    let mut fake = Fake::with(&[(Field::ViewActive, 1), (Field::SelectionPending, 2), (Field::AdjustmentFirst, 7), (Field::AdjustmentSecond, 8)]);
    let mut live = LiveProfile::new();
    live.view(ViewRequest::Adjust { selection: true }, record(), &mut fake);
    live.view(ViewRequest::Adjust { selection: false }, record(), &mut fake);
    let requests = fake.requests();
    assert_eq!(words(&requests[0].1)[1..], [3, 0x40, 7, 8]);
    assert_eq!(words(&requests[1].1)[1..], [0, 0x40, 7, 8]);
    assert!(requests.iter().all(|request| request.0 == Op::Request34 && request.1.len() == 20 && request.2 == vec![C::Image63]));
}

#[test]
fn window_request_carries_the_window_word() {
    let mut fake = Fake::default();
    LiveProfile::new().view(ViewRequest::Window(0x1234), record(), &mut fake);
    let requests = fake.requests();
    assert_eq!((requests[0].0, words(&requests[0].1)[1], requests[0].2.clone()), (Op::Request18, 0x1234, vec![C::Image65]));
}

#[test]
fn configuration_lays_out_the_profile_values_and_the_preparation_it_follows() {
    let mut fake = Fake::with(&[(Field::Selection, 1), (Field::ViewMode, 3), (Field::ViewLocked, 1), (Field::SelectionNotifies, 1), (Field::ViewActive, 1)]);
    fake.values = ConfigurationValues {
        head: [1, 2, 3],
        group: ValueGroup { first: 0x10, seventh: 0x20, eleventh: 0x30, ..ValueGroup::default() },
        converted: [0x111, 0x222],
        properties: [0xa, 0xb, 0xc, 0xd],
        classes: [9, 2],
        restarts: 0x99,
        ..ConfigurationValues::default()
    };
    let mut live = LiveProfile::new();
    let primary = command(K::Command02, &[0, 0, 0, 2, 0]);
    live.prepare(Some(entered(&primary)), record(), &mut fake);
    fake.take();
    live.view(ViewRequest::Configure(MotionSetting(0)), record(), &mut fake);
    let requests = fake.requests();
    let (operation, bytes, completions) = &requests[0];
    assert_eq!((*operation, bytes.len()), (Op::Request26, 232));
    assert_eq!(completions, &vec![C::Image41, C::Accessor4a, C::Accessor4b]);
    let payload = words(bytes);
    assert_eq!(payload[3..7], [1, 1, 2, 3]);
    assert_eq!(payload[0x44 / 4], 3);
    assert_eq!((payload[0x48 / 4], payload[0x6c / 4]), (0x10, 0x10));
    assert_eq!((bytes[0x90], bytes[0x91], bytes[0x9e], bytes[0x9f]), (16, 16, 7, 7));
    assert_eq!(u16::from_le_bytes([bytes[0xa0], bytes[0xa1]]), 0xb);
    assert_eq!(u16::from_le_bytes([bytes[0xa2], bytes[0xa3]]), 0xa);
    assert_eq!(payload[0xa8 / 4], 0x0222_0111);
    assert_eq!(payload[0xb0 / 4], 0x40);
    assert_eq!((bytes[0xc9], bytes[0xca]), (6, 2));
    assert_eq!(payload[0xd0 / 4], 1);
    assert_eq!(u16::from_le_bytes([bytes[0xd8], bytes[0xd9]]), 0x0102);
    assert_eq!((bytes[0xda], bytes[0xdc], bytes[0xdd], bytes[0xdf]), (9, 1, 3, 0));
    assert_eq!(payload[0xe4 / 4], 0x99);
    assert!(fake.seen.contains(&Seen::Flag(GeometryFlag::Configuring, true)));
    assert!(fake.seen.contains(&Seen::Mirror(Mirror::Configured)));
}

#[test]
fn primary_preparation_asks_for_the_live_sequence_when_its_fourth_word_is_zero() {
    let mut fake = Fake::default();
    let mut live = LiveProfile::new();
    let preparation = live.prepare(Some(entered(&command(K::Command02, &[0, 0, 0, 0, 0]))), record(), &mut fake);
    assert!(preparation.sequence);
    let seen = fake.take();
    assert_eq!(seen[..3], [Seen::Claim, Seen::DisplayReset, Seen::RangeUpdate]);
    assert!(seen.contains(&Seen::Write(Field::PreparationSecond, 1)));
    assert!(seen.contains(&Seen::ResetSelection));
    assert!(seen.contains(&Seen::Write(Field::PendingOutput, 0)));

    let preparation = live.prepare(Some(entered(&command(K::Command02, &[0, 0, 0, 2, 0]))), record(), &mut fake);
    assert!(!preparation.sequence);
    assert!(!fake.take().contains(&Seen::ResetSelection));
}

#[test]
fn autofocus_preparation_stops_with_the_first_changed_group_in_order() {
    let mut fake = Fake::default();
    fake.changes = vec![Change::StopFour, Change::StopFive];
    let preparation = LiveProfile::new().prepare(Some(entered(&command(K::Autofocus, &[]))), record(), &mut fake);
    assert!(matches!(preparation.stop, StopReason::Change(code) if code.get() == 5));
}

#[test]
fn command22_preparation_with_its_change_flag_stops_without_touching_the_modes() {
    let mut fake = Fake::default();
    let preparation = LiveProfile::new().prepare(Some(entered(&command(K::Command22, &[0, 1]))), record(), &mut fake);
    assert!(matches!(preparation.stop, StopReason::Change(code) if code.get() == 2));
    assert!(fake.take().is_empty());
}

#[test]
fn preparation_without_a_command_leaves_the_default_modes() {
    let mut fake = Fake::default();
    LiveProfile::new().prepare(None, record(), &mut fake);
    assert_eq!(fake.take(), vec![
        Seen::Write(Field::PreparationFirst, 2), Seen::Write(Field::PreparationSecond, 2), Seen::Write(Field::PreparationThird, 1),
    ]);
}

#[test]
fn command17_is_refused_restarts_or_applies_and_reports_its_selection() {
    let mut live = LiveProfile::new();
    let mut refusing = Fake { refuse: true, ..Fake::default() };
    let change = command(K::Command17, &[0, 2, 1, 0x10, 0, 0, 0, 0, 5, 6]);
    assert_eq!(live.apply(entered(&change), record(), &mut refusing), Application::Refused);

    let mut fake = Fake { outgrown: vec![GeometryCheck::Command17], ..Fake::default() };
    let selecting = command(K::Command17, &[0, 1, 1, 0x10, 0, 0, 0, 0, 5, 6]);
    assert_eq!(live.apply(entered(&selecting), record(), &mut fake), Application::Applied);
    assert!(fake.seen.contains(&Seen::Write(Field::Command17First, 0x10)));
    assert!(live.condition(Condition::SelectionActive, None, record(), &mut fake));

    let clearing = command(K::Command17, &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut fresh = Fake { outgrown: vec![GeometryCheck::Command17], ..Fake::default() };
    assert_eq!(live.apply(entered(&clearing), record(), &mut fresh), Application::Restart);
    assert!(!live.condition(Condition::SelectionActive, None, record(), &mut fresh));
}

#[test]
fn prepare_parameters_set_or_clear_the_enabled_flags() {
    let mut fake = Fake::with(&[(Field::SelectionEnables, 0b1100)]);
    let prepare = command(K::Prepare, &[0, 0b0011, 0b0001, 7, 8]);
    assert_eq!(LiveProfile::new().apply(entered(&prepare), record(), &mut fake), Application::Applied);
    assert_eq!(fake.take(), vec![
        Seen::Write(Field::SelectionEnables, 0b0011),
        Seen::Write(Field::SelectionFlags, 0b1101),
        Seen::Write(Field::PrepareFirst, 7),
        Seen::Write(Field::PrepareSecond, 8),
    ]);
}

#[test]
fn profile_commands_restart_the_view_as_their_geometry_requires() {
    let mut live = LiveProfile::new();
    let mut fake = Fake::default();
    assert_eq!(live.apply(entered(&command(K::Command21, &[0, 4])), record(), &mut fake), Application::Restart);
    assert_eq!(live.apply(entered(&command(K::Command20, &[0, 4])), record(), &mut fake), Application::Applied);
    let mut locked = Fake::with(&[(Field::ViewMode, 3)]);
    assert_eq!(live.apply(entered(&command(K::Command23, &[0, 1, 2])), record(), &mut locked), Application::Restart);
    assert_eq!(locked.take()[..2], [Seen::Write(Field::SelectionTaken, 1), Seen::Write(Field::Command23Second, 2)]);
}

#[test]
fn primary_and_secondary_profiles_replace_the_selection_and_record_which_applied() {
    let mut live = LiveProfile::new();
    let mut fake = Fake::default();
    live.apply(entered(&command(K::Command02, &[0, 0, 0xffff_ffff])), record(), &mut fake);
    assert_eq!(fake.take(), vec![Seen::ClearSelection, Seen::Mirror(Mirror::Primary)]);
    assert!(live.condition(Condition::PrimaryApplied, None, record(), &mut fake));
    live.apply(entered(&command(K::Command03, &[0, 0, 0, 9])), record(), &mut fake);
    assert_eq!(fake.take(), vec![Seen::ClearSelection, Seen::Descriptor(9)]);
    assert!(!live.condition(Condition::PrimaryApplied, None, record(), &mut fake));
}

#[test]
fn command1d_restarts_when_the_view_mode_and_the_requested_mode_agree() {
    let mut live = LiveProfile::new();
    for (view_mode, requested, restart) in [(3, 3, true), (3, 1, false), (1, 3, false), (1, 1, true)] {
        let mut fake = Fake::with(&[(Field::ViewMode, view_mode)]);
        let change = command(K::Command1d, &[0, requested]);
        assert_eq!(live.condition(Condition::Command1dRestart, Some(entered(&change)), record(), &mut fake), restart);
    }
    let mut forced = Fake::with(&[(Field::Command1dRestarts, 1)]);
    assert!(live.condition(Condition::Command1dRestart, None, record(), &mut forced));
}

#[test]
fn direct_start_is_refused_only_in_the_locked_third_view_mode() {
    let mut live = LiveProfile::new();
    assert!(live.condition(Condition::DirectStart, None, record(), &mut Fake::with(&[(Field::ViewMode, 3)])));
    assert!(!live.condition(Condition::DirectStart, None, record(), &mut Fake::with(&[(Field::ViewMode, 3), (Field::ViewLocked, 1)])));
}

#[test]
fn record_mode_begins_only_when_idle_and_its_channel_is_ready() {
    let mut live = LiveProfile::new();
    let mut busy = Fake { record_mode_initialized: true, record_mode_available: true, ..Fake::default() };
    live.effect(Effect::BeginRecordMode, record(), &mut busy);
    assert!(busy.take().is_empty());
    let mut idle = Fake { record_mode_available: true, ..Fake::default() };
    live.effect(Effect::BeginRecordMode, record(), &mut idle);
    live.effect(Effect::FinishRecordMode, record(), &mut idle);
    assert_eq!(idle.take(), vec![Seen::BeginRecordMode, Seen::EndRecordMode]);
}

#[test]
fn statistics_preparation_sets_and_clears_the_first_selection_flag() {
    let mut live = LiveProfile::new();
    let mut fake = Fake::with(&[(Field::SelectionFlags, 0b110)]);
    live.effect(Effect::StatisticsPreparation(true), record(), &mut fake);
    live.effect(Effect::StatisticsPreparation(false), record(), &mut fake);
    assert_eq!(fake.take(), vec![Seen::Write(Field::SelectionFlags, 0b111), Seen::Write(Field::SelectionFlags, 0b110)]);
}

#[test]
fn autofocus_change_decided_by_the_view_mode_restarts_only_in_mode_three() {
    let mut live = LiveProfile::new();
    let autofocus = command(K::Autofocus, &[]);
    let mut third = Fake { changes: vec![Change::ModeDecides], ..Fake::with(&[(Field::ViewMode, 3)]) };
    assert_eq!(live.apply(entered(&autofocus), record(), &mut third), Application::Restart);
    let mut first = Fake { changes: vec![Change::ModeDecides], ..Fake::default() };
    assert_eq!(live.apply(entered(&autofocus), record(), &mut first), Application::Applied);
}

#[test]
fn autofocus_area_change_restarts_unless_the_profile_is_selected() {
    let mut live = LiveProfile::new();
    let autofocus = command(K::Autofocus, &[]);
    let mut unselected = Fake { changes: vec![Change::AutofocusArea], ..Fake::default() };
    assert_eq!(live.apply(entered(&autofocus), record(), &mut unselected), Application::Restart);
    let mut selected = Fake { changes: vec![Change::AutofocusArea], ..Fake::with(&[(Field::Selection, 1)]) };
    assert_eq!(live.apply(entered(&autofocus), record(), &mut selected), Application::Applied);
}

#[test]
fn autofocus_change_that_outgrows_the_view_restarts_it_without_mirroring() {
    let mut fake = Fake { changes: vec![Change::Geometry, Change::Shift], outgrown: vec![GeometryCheck::Autofocus], ..Fake::default() };
    let autofocus = command(K::Autofocus, &[]);
    assert_eq!(LiveProfile::new().apply(entered(&autofocus), record(), &mut fake), Application::Restart);
    let seen = fake.take();
    assert!(!seen.iter().any(|seen| matches!(seen, Seen::Mirror(_))));
    assert!(seen.contains(&Seen::Publish(Publication::Window)));
}

#[test]
fn autofocus_change_kept_running_mirrors_and_publishes_what_changed() {
    let mut fake = Fake { changes: vec![Change::SelectionMode, Change::Shift], ..Fake::default() };
    let autofocus = command(K::Autofocus, &[]);
    assert_eq!(LiveProfile::new().apply(entered(&autofocus), record(), &mut fake), Application::Applied);
    let seen = fake.take();
    let mirrored = AutofocusMirror { selection_mode: true, shift: true, ..AutofocusMirror::default() };
    assert!(seen.contains(&Seen::Mirror(Mirror::Autofocus(mirrored))));
    let published: Vec<_> = seen.iter().filter_map(|seen| match seen { Seen::Publish(p) => Some(*p), _ => None }).collect();
    assert_eq!(published, vec![Publication::SelectionMode, Publication::Position, Publication::Range]);
    assert!(seen.contains(&Seen::Write(Field::AutofocusRelease, 0)));
}

#[test]
fn autofocus_lens_range_change_rewrites_both_lens_slots_when_the_pair_moved() {
    let mut parameters = vec![0u32; 40];
    parameters[38] = 10;
    let autofocus = command(K::Autofocus, &parameters);
    let mut moved = Fake { changes: vec![Change::LensRange], lens_follows: true, ..Fake::with(&[(Field::LensRange, 100)]) };
    moved.blocks.push((Block::Lens(LensSlot::Current), vec![1, 2]));
    LiveProfile::new().apply(entered(&autofocus), record(), &mut moved);
    let seen = moved.take();
    assert!(seen.contains(&Seen::Block(Block::Lens(LensSlot::Current), vec![10, 120])));
    assert!(seen.contains(&Seen::Block(Block::Lens(LensSlot::Next), vec![10, 120])));

    let mut held = Fake { changes: vec![Change::LensRange], lens_follows: true, ..Fake::with(&[(Field::LensRange, 13)]) };
    held.blocks.push((Block::Lens(LensSlot::Current), vec![1, 2]));
    LiveProfile::new().apply(entered(&autofocus), record(), &mut held);
    assert!(!held.take().iter().any(|seen| matches!(seen, Seen::Block(Block::Lens(_), _))));
}

fn replied(command: Option<K>, status: ReplyStatus, fake: &mut Fake) -> Vec<Seen> {
    LiveProfile::new().reply(command, status, record(), fake);
    fake.take()
}

#[test]
fn finished_commands_are_answered_through_their_own_reply_with_the_outcome() {
    for (kind, answered) in [
        (K::HalfRelease, R::HalfRelease),
        (K::Command1d, R::Command1d),
        (K::Command1e, R::Command1e),
        (K::Command20, R::Command20),
        (K::Command21, R::Command21),
        (K::Command22, R::Command22),
        (K::Command23, R::Command23),
        (K::Prepare26, R::Prepare26),
        (K::Command29, R::Command29),
    ] {
        let mut fake = Fake::default();
        assert_eq!(replied(Some(kind), ReplyStatus::Accepted, &mut fake), vec![Seen::Reply(answered, 0, vec![])]);
        assert_eq!(replied(Some(kind), ReplyStatus::Rejected, &mut fake), vec![Seen::Reply(answered, 21, vec![])]);
    }
}

#[test]
fn prepare_is_answered_with_the_selection_flags() {
    let mut fake = Fake::with(&[(Field::SelectionFlags, 0b101)]);
    assert_eq!(replied(Some(K::Prepare), ReplyStatus::Accepted, &mut fake), vec![Seen::Reply(R::Prepare, 0, vec![0b101])]);
}

#[test]
fn capture_changes_are_answered_through_command09() {
    let mut fake = Fake::default();
    for status in [ReplyStatus::Accepted, ReplyStatus::Rejected] {
        assert_eq!(replied(Some(K::Command09), status, &mut fake), vec![Seen::Reply(R::Command09, 15, vec![0])]);
    }
    assert_eq!(replied(Some(K::Command0a), ReplyStatus::Accepted, &mut fake), vec![Seen::Reply(R::Command09, 0, vec![0])]);
    assert_eq!(replied(Some(K::Command0a), ReplyStatus::Rejected, &mut fake), vec![Seen::Reply(R::Command09, 21, vec![0])]);
}

fn focus_blocks() -> Fake {
    let mut fake = Fake::default();
    fake.blocks = vec![
        (Block::Release, vec![0x40; 33]),
        (Block::FocusConfirmation, vec![0x80; 8]),
        (Block::MeasuredWindow, vec![0x50; 5]),
        (Block::FocusHistory, vec![0x20; 20]),
    ];
    fake
}

fn cleared(block: Block, length: usize, indices: &[usize], fill: u32) -> Seen {
    let mut words = vec![fill; length];
    indices.iter().for_each(|&index| words[index] = 0);
    Seen::Block(block, words)
}

fn focus_cleared() -> Vec<Seen> {
    vec![
        cleared(Block::Release, 33, &[1, 24], 0x40),
        cleared(Block::FocusConfirmation, 8, &[0], 0x80),
        cleared(Block::MeasuredWindow, 5, &[3], 0x50),
    ]
}

#[test]
fn secondary_profile_and_autofocus_clear_focus_state_without_an_answer() {
    let mut fake = focus_blocks();
    let mut expected = focus_cleared();
    expected.push(cleared(Block::FocusHistory, 20, &[0, 1], 0x20));
    assert_eq!(replied(Some(K::Command03), ReplyStatus::Accepted, &mut fake), expected);
    assert_eq!(replied(Some(K::Autofocus), ReplyStatus::Accepted, &mut fake), vec![cleared(Block::FocusHistory, 20, &[0], 0x20)]);
}

#[test]
fn command17_clears_focus_state_when_nothing_stays_selected() {
    let mut fake = focus_blocks();
    let mut expected = focus_cleared();
    expected.push(cleared(Block::FocusHistory, 20, &[0], 0x20));
    expected.push(cleared(Block::FocusHistory, 20, &[0, 1], 0x20));
    expected.push(Seen::Reply(R::Command17, 0, vec![]));
    assert_eq!(replied(Some(K::Command17), ReplyStatus::Accepted, &mut fake), expected);

    let mut selected = Fake { fields: vec![(Field::Command17Selection, 2)], ..focus_blocks() };
    assert_eq!(replied(Some(K::Command17), ReplyStatus::Accepted, &mut selected), vec![Seen::Reply(R::Command17, 0, vec![])]);
}

#[test]
fn rejected_command17_is_answered_with_its_own_status_and_clears_nothing() {
    let mut fake = focus_blocks();
    assert_eq!(replied(Some(K::Command17), ReplyStatus::Rejected, &mut fake), vec![Seen::Reply(R::Command17, -96, vec![])]);
}

#[test]
fn commands_the_live_view_does_not_answer_and_no_command_leave_no_trace() {
    let mut fake = focus_blocks();
    for command in [None, Some(K::Command02), Some(K::Command11), Some(K::Command1f), Some(K::Command13)] {
        assert_eq!(replied(command, ReplyStatus::Accepted, &mut fake), vec![]);
    }
}
