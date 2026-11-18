#![forbid(unsafe_code)]

use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::primary_boundary::{Calibration, CalibrationSeed, CaptureJobCalc};
use crate::shot_services::Libraries;

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> CaptureJobCalc for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn default_calibration(&mut self, seed: &CalibrationSeed) -> (Calibration, u32) {
        // f2cc64..80: column is the second halfword, level the first; the second
        // curve word is copied by the native caller but not read by f4d4cc.
        let selected = xm1_saclac_calc::gains::default_calibration(&mut *self.services.borrow_mut(), &xm1_saclac_calc::gains::CalibrationSeed {
            record: seed.record.expect("primary default calibration requires its admitted record"),
            custom: seed.base.0, column: seed.pair[1] as i16, level: seed.pair[0] as i16, variant_set: seed.words[0],
        });
        (Calibration(selected.gains), selected.class.0)
    }
    fn output_scale(&mut self, index: i16, distance: u32, reference: u32, words: [u32; 2]) -> i32 {
        self.services.borrow().machine.enter(0x74_9be4, &[index as i32 as u32 as usize, distance as usize, reference as usize, words[0] as usize, words[1] as usize]) as i32
    }
}
