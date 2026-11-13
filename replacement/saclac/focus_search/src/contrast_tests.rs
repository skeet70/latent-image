use crate::contrast::{travel_limits, Context, FocusOffer, FocusStatus, LensIdentity, LimitKind, LimitQuery, Phase, StageRecord, TableOffset, TravelLimits};
use crate::stock;
use crate::test_platform::{Effect, Fake};
use xm1_saclac_controllers::search::LensPosition;

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
