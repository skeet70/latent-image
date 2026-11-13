#![forbid(unsafe_code)]

//! What every evaluation does before its phase step (0xf44fec).

use super::settings::Setting;
use super::{Context, CorrectionQuery, Platform};
use xm1_saclac_interface::search::Evaluation;

impl Context {
    /// 0xf44fec: takes in the evaluation and moves the position history on by one sample.
    pub(crate) fn accept(&mut self, platform: &mut impl Platform, evaluation: &Evaluation) {
        let [[first, second], [third, fourth], [fifth, sixth]] = evaluation.pairs;
        let values = &mut self.values;
        if platform.setting(Setting::PairedEvaluation) == 1 {
            let first = first.max(1);
            values.level = second.wrapping_mul(10) / first;
            values.combined = [first.wrapping_add(second), third.wrapping_add(fourth), fifth.wrapping_add(sixth)];
            if second == 0 {
                values.level = platform.setting(Setting::LevelThousands).wrapping_mul(1000);
            }
            if self.frame.selected == 1 {
                let level = self.selected_level(platform);
                self.values.level = level;
            }
        } else {
            values.combined = [first, third, fifth];
            values.level = platform.setting(Setting::LevelUnits);
        }
        let values = &mut self.values;
        values.seconds = [second, fourth, sixth];
        values.levels = evaluation.levels;
        values.reference = evaluation.reference;
        values.scan_pairs = evaluation.scan_pairs;
        values.scan_levels = evaluation.scan_levels;
        values.scan_reference = evaluation.reference;
        let [raw, focus] = evaluation.focus_values;
        values.focus_raw = raw;
        values.focus_value = focus;
        values.unsettled = evaluation.settled == 0;
        values.flag = evaluation.flag;
        self.hold = false;
        self.positions = [self.reported, self.positions[0], self.positions[1]];
        self.previous_direction = self.direction;
        self.classes = [self.reported_class, self.classes[0], self.classes[1]];
        if platform.scan_refresh() && self.frame.corrects_position {
            self.corrected = platform.corrected_position(self.frame.mark);
            self.positions = [platform.lens_position(), self.corrected.frame, self.corrected.frame];
            self.classes[2] = self.frame.correction_class;
        }
        let correction = self.focus_correction(platform);
        self.values.focus_correction = correction;
        self.values.focus_value = self.values.focus_value.wrapping_add(correction as u16);
        if self.frame.pinned == 1 {
            self.search.focus_marks[0] = self.limits.offset;
            self.frame.pinned_setting = platform.setting(Setting::PinnedAmount) as u16;
        }
        if platform.scan_refresh() {
            self.positions[2] = self.corrected.frame;
        }
        self.log_evaluation(platform);
        self.accepted = true;
    }

    /// A selected record's level: the units level once it has taken `SelectedLevelSamples`,
    /// else one below the tens level while its selection holds or the previous level reached
    /// the units level.
    fn selected_level(&self, platform: &mut impl Platform) -> u32 {
        if u32::from(self.frame.selected_samples) >= platform.setting(Setting::SelectedLevelSamples) {
            return platform.setting(Setting::LevelUnits);
        }
        if self.frame.previous_selected == self.frame.selected || self.values.previous_level >= platform.setting(Setting::LevelUnits) {
            return platform.setting(Setting::LevelTens).wrapping_mul(10).wrapping_sub(1);
        }
        platform.setting(Setting::LevelUnits)
    }

    /// 0xf44f58.
    fn focus_correction(&self, platform: &mut impl Platform) -> i16 {
        if platform.setting(Setting::FocusCorrection) == 0 {
            return 0;
        }
        platform.focus_correction(CorrectionQuery { position: self.positions[2], class: self.classes[2], lens: self.lens, parameter: self.frame.parameter })
    }
}
