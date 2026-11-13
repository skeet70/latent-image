use crate::classification::classify_subjects;
use crate::grid_boundary::GridSetting;
use crate::grid_fakes::{area, cell, index, kind, put_area, Layers, Setup};
use crate::state::ShotState;
use crate::state_tests::Memory;
use xm1_saclac_interface::calculation::{AreaAt, AreaHalf, ResultPass, TrackedKind};
use xm1_saclac_interface::selection::{ClassifyStage, SubjectQuery, SubjectShare};

const QUERY: SubjectQuery = SubjectQuery { kind: 1, source: 2, area: 3 };

/// An area whose level scales to 100 and whose total is 150 (band 1); band 1 sorts levels at
/// 20, 40 and 60; kinds 0 and 1 total 250 (eligibility band 1, minimum 30).
fn world() -> (ShotState<Memory>, Layers) {
    let mut state = Setup { areas: 1, kinds: 0, grid: false, columns: 1, rows: 1 }.state();
    put_area(&mut state, AreaAt::Cell { pass: ResultPass::Plain, cell: cell(0, 0), index: index(0) }, &area(0, &[(AreaHalf::Level, 300), (AreaHalf::At0c, 150)]));
    for tracked in [0, 1] {
        put_area(&mut state, AreaAt::Tracked { pass: ResultPass::Plain, kind: kind(tracked), index: index(0) }, &area(0, &[(AreaHalf::At0c, 250)]));
    }
    let mut layers = Layers::default();
    for (step, threshold) in [20, 40, 60].into_iter().enumerate() { layers.set(GridSetting::KindThreshold { band: 1, step: step as u8 }, threshold); }
    layers.set(GridSetting::EligibilityThreshold { band: 1 }, 30);
    (state, layers)
}

fn classify(state: &mut ShotState<Memory>, layers: &mut Layers, stage: ClassifyStage, mode: u32, percent: u16) -> Option<TrackedKind> {
    classify_subjects(stage, SubjectShare { mode, percent }, QUERY, state, layers);
    state.subject_kind(stage, index(0))
}

#[test]
fn a_share_of_half_sorts_the_scaled_level_into_a_kind_and_keeps_it() {
    let (mut state, mut layers) = world();
    assert_eq!(classify(&mut state, &mut layers, ClassifyStage::Absorb, 0, 50), Some(kind(1)));
    assert_eq!(state.subject_level(ClassifyStage::Absorb, index(0)), 50);
    assert_eq!(state.subject_mode(ClassifyStage::Absorb), 0);
    assert_eq!(classify(&mut state, &mut layers, ClassifyStage::Absorb, 0, 100), Some(kind(2)));
    assert_eq!(classify(&mut state, &mut layers, ClassifyStage::Absorb, 0, 10), None);
}

#[test]
fn a_zero_share_leaves_every_area_untracked_and_keeps_the_mode() {
    let (mut state, mut layers) = world();
    assert_eq!(classify(&mut state, &mut layers, ClassifyStage::Round, 5, 0), None);
    assert_eq!(state.subject_mode(ClassifyStage::Round), 5);
}

#[test]
fn an_eligible_subject_moves_up_three_kinds() {
    let (mut state, mut layers) = world();
    layers.eligible = true;
    assert_eq!(classify(&mut state, &mut layers, ClassifyStage::Absorb, 0, 50), Some(kind(4)));
    layers.set(GridSetting::EligibilityThreshold { band: 1 }, 51);
    assert_eq!(classify(&mut state, &mut layers, ClassifyStage::Absorb, 0, 50), Some(kind(1)));
}

#[test]
fn share_mode_3_caps_the_kind_at_the_configured_class() {
    let (mut state, mut layers) = world();
    layers.set(GridSetting::KindCap, 1);
    layers.set(GridSetting::KindCapClass, 100);
    assert_eq!(classify(&mut state, &mut layers, ClassifyStage::Absorb, 3, 50), Some(kind(0)));
    assert_eq!(classify(&mut state, &mut layers, ClassifyStage::Absorb, 2, 50), Some(kind(1)));
    layers.set(GridSetting::KindCapClass, 150);
    assert_eq!(classify(&mut state, &mut layers, ClassifyStage::Absorb, 3, 50), None);
}

#[test]
fn each_stage_keeps_its_own_kinds() {
    let (mut state, mut layers) = world();
    classify(&mut state, &mut layers, ClassifyStage::Round, 0, 50);
    assert_eq!(state.subject_kind(ClassifyStage::Round, index(0)), Some(kind(1)));
    assert_eq!(state.subject_kind(ClassifyStage::CaptureJob, index(0)), Some(kind(0)));
    classify(&mut state, &mut layers, ClassifyStage::CaptureJob, 0, 0);
    assert_eq!(state.subject_kind(ClassifyStage::Round, index(0)), Some(kind(1)));
    assert_eq!(state.subject_kind(ClassifyStage::RecordLoad, index(0)), Some(kind(0)));
}
