#![forbid(unsafe_code)]

//! The subject classification (X-M1 1.50 id5 0xf1c6b4): each area of the selected pass gets a
//! scaled level and the tracked kind that level and the area's total sort it into, kept in the
//! caller's bank.

use crate::grid::selected_cell;
use crate::grid_boundary::{GridLayers, GridSetting};
use crate::selection::areas;
use crate::state::{ShotState, StateMemory};
use xm1_saclac_interface::calculation::{AreaAt, AreaHalf, AreaIndex, ResultPass, TrackedKind};
use xm1_saclac_interface::selection::{ClassifyStage, SubjectQuery, SubjectShare};

/// The share mode that lets the configuration cap the kinds.
const CAPPED_MODE: u32 = 3;
/// The level an area's own level is measured down from before scaling.
const SCALE_ORIGIN: i16 = 500;
/// The kinds an eligible subject moves up to.
const ELIGIBLE_KINDS: u8 = 3;

pub fn classify_subjects<M: StateMemory>(stage: ClassifyStage, share: SubjectShare, query: SubjectQuery, state: &mut ShotState<M>, layers: &mut impl GridLayers) {
    state.set_subject_mode(stage, share.mode);
    let classification = Classification {
        stage,
        percent: share.percent,
        query,
        pass: state.selected_pass(),
        capped: share.mode == CAPPED_MODE && layers.setting(GridSetting::KindCap) & 1 != 0,
    };
    for index in areas(state.area_count()) {
        let kind = if share.percent == 0 { None } else { classification.kind(index, state, layers) };
        state.set_subject_kind(stage, index, kind);
    }
}

struct Classification {
    stage: ClassifyStage,
    percent: u16,
    query: SubjectQuery,
    pass: ResultPass,
    capped: bool,
}

impl Classification {
    fn kind<M: StateMemory>(&self, index: AreaIndex, state: &mut ShotState<M>, layers: &mut impl GridLayers) -> Option<TrackedKind> {
        let pass = self.pass;
        let cell = selected_cell(pass, index, state).cell();
        let area = state.area(AreaAt::Cell { pass, cell, index });
        let scale = layers.level_scale(SCALE_ORIGIN.wrapping_sub(area.half(AreaHalf::Level)));
        let level = u64::from(scale) * u64::from(self.percent) / 100;
        state.set_subject_level(self.stage, index, level);
        let mut kind = level_kind(level, area.total(), layers)?;
        if self.capped { kind = kind.min(capped_kind(layers)?); }
        let tracked = state.area(AreaAt::Tracked { pass, kind: TrackedKind::new(kind)?, index });
        if eligible(level, tracked.total(), self.query, layers) { kind += ELIGIBLE_KINDS; }
        TrackedKind::new(kind)
    }
}

/// Kind 0, 1 or 2 by the three configured thresholds of the area's total band; `None` below the
/// first (0xf1a408).
fn level_kind(level: u64, total: i16, layers: &mut impl GridLayers) -> Option<u8> {
    let band = (2..=6).position(|step| total < layers.lens_threshold(step)).unwrap_or(5) as u8;
    let mut threshold = |step| u64::from(layers.setting(GridSetting::KindThreshold { band, step }) as u8);
    if level < threshold(0) { return None; }
    Some(if level < threshold(1) { 0 } else if level < threshold(2) { 1 } else { 2 })
}

/// The kind the configuration caps classes at: 100, 200 or 300 name kinds 0, 1 and 2; any other
/// value leaves every area untracked (0xf1a3cc).
fn capped_kind(layers: &mut impl GridLayers) -> Option<u8> {
    match layers.setting(GridSetting::KindCapClass) {
        100 => Some(0),
        200 => Some(1),
        300 => Some(2),
        _ => None,
    }
}

/// The subject is eligible, its kind's area total lies from lens threshold 2 up to threshold 7,
/// and the level reaches the configured minimum of the total's band (0xf1a4f4).
fn eligible(level: u64, total: i16, query: SubjectQuery, layers: &mut impl GridLayers) -> bool {
    if !layers.subject_eligible(query) || total < layers.lens_threshold(2) { return false; }
    let Some(band) = (3..=7).position(|step| total < layers.lens_threshold(step)) else { return false };
    level >= u64::from(layers.setting(GridSetting::EligibilityThreshold { band: band as u8 }) as u8)
}
