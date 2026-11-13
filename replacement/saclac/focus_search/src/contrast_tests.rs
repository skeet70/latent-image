use crate::contrast::settings::Setting;
use crate::contrast::{
    travel_limits, Context, CorrectedPosition, CorrectionQuery, FitPoint, FocusLog, FocusLogEntry, FocusOffer, FocusStatus, LensIdentity, LimitKind,
    LimitQuery, MonitorStepQuery, PeakSearchTrace, Phase, PinnedStatus, Platform, RetainedFocus, ScanPlan, ScanQuery, StageRecord, Step,
    StepQuery, TableOffset, TransitionTrace, TravelLimits,
};
use crate::stock;
use std::vec::Vec;
use xm1_saclac_controllers::search::LensPosition;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Effect {
    Stage(StageRecord),
    Status(FocusStatus),
    ClaimStatistics,
    ResetLog,
    ClaimOffer,
    Trace(LensPosition, LensPosition),
    Outer(LimitQuery),
    Offset(TableOffset),
}

struct Fake {
    effects: Vec<Effect>,
    position: LensPosition,
    lens: LensIdentity,
    selection: u8,
    offer: FocusOffer,
    retained: RetainedFocus,
    selected: u32,
    profile: u32,
    kind: LimitKind,
    pinned: bool,
    tracing: bool,
}

impl Fake {
    fn new() -> Self {
        Self {
            effects: Vec::new(),
            position: LensPosition(-30),
            lens: LensIdentity(0),
            selection: 1,
            offer: FocusOffer(0),
            retained: RetainedFocus { valid: 1, spread: 0, position: LensPosition(100), profile: 9 },
            selected: 0,
            profile: 9,
            kind: LimitKind(0),
            pinned: false,
            tracing: false,
        }
    }
}

impl Platform for Fake {
    fn lens_position(&mut self) -> LensPosition { self.position }
    fn lens_identity(&mut self) -> LensIdentity { self.lens }
    fn table_count(&mut self) -> u32 { 4 }
    fn limit_kind(&mut self) -> LimitKind { self.kind }
    fn pinned_table(&mut self) -> bool { self.pinned }
    fn table_position(&mut self) -> u16 { 7 }
    fn lens_flags(&mut self) -> u32 { 0 }
    fn record_auxiliary(&mut self) -> u32 { 0 }
    fn lens_state(&mut self) -> u32 { 2 }
    fn lens_bias(&mut self, _: LensIdentity) -> u16 { 5 }
    fn outer_limits(&mut self, query: LimitQuery) -> (LensPosition, LensPosition) {
        self.effects.push(Effect::Outer(query));
        (LensPosition(-200), LensPosition(90))
    }
    fn inner_limits(&mut self, _: LimitQuery) -> (LensPosition, LensPosition) { (LensPosition(-150), LensPosition(60)) }
    fn table_offset(&mut self, _: LensIdentity, kind: TableOffset) -> u16 {
        self.effects.push(Effect::Offset(kind));
        40
    }
    fn focus_offer(&mut self) -> FocusOffer { self.offer }
    fn claim_focus_offer(&mut self) { self.effects.push(Effect::ClaimOffer); }
    fn retained_focus(&mut self) -> RetainedFocus { self.retained }
    fn measurement_selection(&mut self) -> u8 { self.selection }
    fn record_selected(&mut self) -> u32 { self.selected }
    fn record_profile(&mut self) -> u32 { self.profile }
    fn publish_status(&mut self, status: FocusStatus) { self.effects.push(Effect::Status(status)); }
    fn store_stage(&mut self, stage: StageRecord) { self.effects.push(Effect::Stage(stage)); }
    fn claim_image_statistics(&mut self) { self.effects.push(Effect::ClaimStatistics); }
    fn reset_focus_log(&mut self) { self.effects.push(Effect::ResetLog); }
    fn tracing(&mut self) -> bool { self.tracing }
    fn trace_begin(&mut self, retained: LensPosition, focus: LensPosition) { self.effects.push(Effect::Trace(retained, focus)); }
    fn setting(&mut self, _: Setting) -> u32 { 0 }
    fn lens_mode(&mut self) -> u32 { 0 }
    fn lens_busy(&mut self) -> bool { false }
    fn lens_ready(&mut self) -> bool { true }
    fn area_levels(&mut self) -> [u16; 2] { [0; 2] }
    fn focus_correction(&mut self, _: CorrectionQuery) -> i16 { 0 }
    fn profile_period(&mut self, _: u32) -> u32 { 0 }
    fn record_class(&mut self) -> u32 { 0 }
    fn step(&mut self, _: StepQuery) -> Step { Step { size: 0, rate: 0 } }
    fn monitor_step(&mut self, query: MonitorStepQuery) -> u16 { query.step }
    fn request_scan_refresh(&mut self, _: bool) {}
    fn scan_refresh(&mut self) -> bool { false }
    fn plan_scan(&mut self, query: ScanQuery) -> ScanPlan { ScanPlan { limits: [query.limits.low, query.limits.high], window: [0; 2], lead: 0, rate: query.rate } }
    fn corrected_position(&mut self, _: u16) -> CorrectedPosition { CorrectedPosition { frame: LensPosition(0), drive: LensPosition(0) } }
    fn drive_target(&mut self, _: LensIdentity, _: u16, low: LensPosition, _: LensPosition) -> LensPosition { low }
    fn below_threshold(&mut self, _: u32, _: u32, _: u32, _: u32) -> bool { false }
    fn passed_peak(&mut self, _: &[u32], _: u32, _: bool) -> (bool, Option<u32>) { (false, None) }
    fn fell_from_peak(&mut self, _: &[u32]) -> bool { false }
    fn contrast_log(&mut self, _: u32) -> u32 { 0 }
    fn fit_peak(&mut self, points: [FitPoint; 3]) -> LensPosition { LensPosition(points[1].position) }
    fn measurement_history(&mut self) -> Option<[Option<[u16; 2]>; 5]> { None }
    fn profile_focus(&mut self) -> i16 { 0 }
    fn detailed_tracing(&mut self) -> bool { false }
    fn trace_peak_search(&mut self, _: PeakSearchTrace) {}
    fn focus_tracing(&mut self) -> bool { false }
    fn trace_focus(&mut self, _: crate::contrast::FocusTrace) {}
    fn minimum_step(&mut self, minimum: u32) -> u32 { minimum }
    fn profile_mode(&mut self) -> u32 { 0 }
    fn parameter_label(&mut self, _: u32) -> u16 { 0 }
    fn ticks(&mut self) -> u32 { 0 }
    fn append_focus_log(&mut self, _: &FocusLog) {}
    fn newest_focus_log(&mut self) -> FocusLogEntry { FocusLogEntry::default() }
    fn show_debug_text(&mut self, _: &[u8], _: u8, _: u8) {}
    fn angular_rate(&mut self) -> u32 { 0 }
    fn request_angular_rate(&mut self) {}
    fn stage(&mut self) -> StageRecord { StageRecord::default() }
    fn status_snapshot(&mut self) -> [u32; 2] { [0; 2] }
    fn publish_pinned_status(&mut self, _: PinnedStatus) {}
    fn trace_transition(&mut self, _: TransitionTrace) {}
}

const INACTIVE: FocusStatus = FocusStatus { active: 0, mode: 4, position: LensPosition(0), snapshot: [0; 2], flag: 0, stage: 2 };

#[test]
fn initialization_clears_the_stage_and_publishes_an_inactive_status() {
    let mut fake = Fake::new();
    Context::new().initialize(&mut fake);
    assert_eq!(fake.effects, [Effect::Stage(StageRecord::default()), Effect::Status(INACTIVE), Effect::ClaimStatistics]);
}

#[test]
fn initialization_resets_the_focus_log_unless_the_record_selects_measurement() {
    let mut fake = Fake::new();
    fake.selection = 0;
    Context::new().initialize(&mut fake);
    assert_eq!(fake.effects.last(), Some(&Effect::ResetLog));
}

#[test]
fn a_retained_focus_on_the_same_lens_and_profile_starts_in_focus() {
    let mut fake = Fake::new();
    let mut context = Context::new();
    context.initialize(&mut fake);
    assert_eq!(context.choose_start(&mut fake), Phase::Focus);
    fake.lens = LensIdentity(4);
    assert_eq!(context.choose_start(&mut fake), Phase::Direction);
}

#[test]
fn a_retained_focus_from_another_profile_starts_in_direction_search_for_a_selected_record() {
    let mut fake = Fake::new();
    let mut context = Context::new();
    context.initialize(&mut fake);
    fake.selection = 0;
    fake.selected = 1;
    fake.profile = 8;
    assert_eq!(context.choose_start(&mut fake), Phase::Direction);
    fake.profile = 9;
    assert_eq!(context.choose_start(&mut fake), Phase::Focus);
}

#[test]
fn an_offer_already_taken_or_an_invalid_retained_focus_starts_in_direction_search() {
    let mut fake = Fake::new();
    let mut context = Context::new();
    context.initialize(&mut fake);
    fake.effects.clear();
    fake.offer = FocusOffer(2);
    assert_eq!(context.choose_start(&mut fake), Phase::Direction);
    assert!(fake.effects.is_empty());
    fake.offer = FocusOffer(0);
    fake.retained.valid = 0;
    assert_eq!(context.choose_start(&mut fake), Phase::Direction);
    assert_eq!(fake.effects, [Effect::ClaimOffer]);
}

#[test]
fn a_search_started_in_focus_keeps_the_lens_position_it_started_from() {
    let mut fake = Fake::new();
    fake.tracing = true;
    let mut context = Context::new();
    context.initialize(&mut fake);
    context.choose_start(&mut fake);
    context.initialize(&mut fake);
    fake.effects.clear();
    context.choose_start(&mut fake);
    assert_eq!(fake.effects.last(), Some(&Effect::Trace(LensPosition(100), LensPosition(-30))));
}

#[test]
fn travel_limits_take_the_records_table_unless_pinned_or_forced() {
    let mut fake = Fake::new();
    fake.lens = LensIdentity(3);
    fake.kind = LimitKind(2);
    let limits = travel_limits(&mut fake, false);
    assert_eq!(
        limits,
        Some(TravelLimits {
            low: LensPosition(-200),
            high: LensPosition(90),
            inner_low: LensPosition(-150),
            inner_high: LensPosition(60),
            offset: 45,
        })
    );
    assert!(matches!(fake.effects[..], [Effect::Outer(LimitQuery { table: Some(LimitKind(2)), .. }), Effect::Offset(TableOffset::Second)]));
    fake.effects.clear();
    fake.lens = LensIdentity(4);
    travel_limits(&mut fake, true);
    assert!(matches!(fake.effects[..], [Effect::Outer(LimitQuery { table: None, .. }), Effect::Offset(TableOffset::First)]));
    fake.effects.clear();
    fake.pinned = true;
    travel_limits(&mut fake, false);
    assert!(matches!(fake.effects[..], [Effect::Outer(LimitQuery { table: None, .. }), Effect::Offset(TableOffset::Default)]));
    fake.lens = LensIdentity(-1);
    assert_eq!(travel_limits(&mut fake, false), None);
}

#[test]
fn the_status_and_stage_layouts_are_the_records() {
    let status = FocusStatus { active: 1, mode: 3, position: LensPosition(-2), snapshot: [5, 6], flag: 7, stage: 8 };
    let bytes = stock::focus_status(status);
    assert_eq!(bytes[..10], [1, 0, 0, 0, 3, 0, 0, 0, 0xfe, 0xff]);
    assert_eq!(bytes[12..], [5, 0, 0, 0, 6, 0, 0, 0, 2, 0, 0, 0, 7, 0, 0, 0, 8, 0, 0, 0]);
    let stage = StageRecord { stage: 1, first_pass: 2, words: [3, 4], halves: [5, 6], last: 7 };
    assert_eq!(stock::stage_record(stage), [1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 5, 0, 0, 0, 4, 0, 0, 0, 6, 0, 7, 0]);
}
