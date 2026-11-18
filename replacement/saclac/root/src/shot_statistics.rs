#![forbid(unsafe_code)]

use xm1_saclac_calc::exposure::ports::{InPlace, Placement};
use xm1_saclac_interface::{Completion, ImgDc, ImgDcOperation, ImgDcRequest, exposure::StatisticsRequest, stock::{exposure::statistics_request, parameters}};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::StatisticsPort, subjects::SubjectList};
use crate::shot_services::Libraries;

/// fea824 stages its220-byte record here before posting callback/address to imgdc.
const STAGED_BLOCK: u32 = 0x016f_0c90;

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A: Placement, Q: ImgDc> StatisticsPort for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn request_statistics(&mut self, request: &StatisticsRequest, _subjects: &SubjectList) {
        // ExposureWork supplies its setup subjects, already bound into this SAME context.
        // BoundSubjects has native104-byte records; do not stage a second active subject list.
        let subjects = self.placement.borrow_mut().address(InPlace::BoundSubjects);
        let bytes = statistics_request(request, subjects.0);
        let mut machine = self.services.borrow().machine.clone();
        for (index, word) in bytes.chunks_exact(4).enumerate() {
            machine.write_u32(STAGED_BLOCK + index as u32 * 4, u32::from_le_bytes(word.try_into().expect("statistics word")));
        }
        let mut payload = [0; 8];
        payload[..4].copy_from_slice(&self.callbacks.statistics.0.to_le_bytes());
        payload[4..].copy_from_slice(&STAGED_BLOCK.to_le_bytes());
        assert!(self.peers.request(ImgDcRequest { operation: ImgDcOperation::Request1a, parameters: parameters(&payload), completions: &[Completion::Image4d, Completion::Image4c] }).is_ok(), "exposure statistics enqueue failed");
    }
    fn release_statistics(&mut self) {
        assert!(self.peers.request(ImgDcRequest { operation: ImgDcOperation::Request1b, parameters: parameters(&[]), completions: &[Completion::Image4c] }).is_ok(), "exposure statistics release failed");
    }
}
