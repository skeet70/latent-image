#![forbid(unsafe_code)]

use xm1_power_lifecycle::{record_image_command_results::{self, PreviewConfiguration}};
use xm1_saclac_interface::{Completion, ImgDc, ImgDcOperation, ImgDcRequest, Statistics, StatisticsOperation, StatisticsRequest, pipeline::{RawFrameRequest, FrameRequest, ScanRequest, SelectionRequest, StatisticsSetup, StatisticsWeights}, stock::{parameters, pipeline as wire}};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::pipeline_work::{PipelinePeers, PlanBlock, SampleSnapshot};
use crate::{capture_samples, shot_services::Libraries};

fn word(payload: &mut [u8], at: usize, value: u32) { payload[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
fn image<Q: ImgDc>(peers: &mut Q, operation: ImgDcOperation, payload: &[u8], completions: &[Completion]) {
    assert!(peers.request(ImgDcRequest { operation, parameters: parameters(payload), completions }).is_ok(), "pipeline image enqueue failed");
}
fn statistics<Q: Statistics>(peers: &mut Q, operation: StatisticsOperation, payload: &[u8], completions: &[Completion]) {
    assert!(peers.request(StatisticsRequest { operation, parameters: parameters(payload), completions }).is_ok(), "pipeline statistics enqueue failed");
}

impl<M: Machine + Clone, R, E, P, I, T, A, Q: ImgDc + Statistics> PipelinePeers for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn request_raw_frame(&mut self, request: &RawFrameRequest, completion: Completion) {
        assert_eq!(completion, Completion::Image4e);
        let mut payload = [0; 52];
        payload[..48].copy_from_slice(&wire::raw_frame_request(request));
        word(&mut payload, 48, self.callbacks.pipeline.raw_checked.0);
        image(&mut self.peers, ImgDcOperation::Request1d, &payload, &[completion]);
    }
    fn request_frame(&mut self, request: &FrameRequest, completion: Completion) {
        assert_eq!(completion, Completion::Image4f);
        let mut payload = [0; 56];
        payload[..52].copy_from_slice(&wire::frame_request(request));
        word(&mut payload, 52, self.callbacks.pipeline.frame_prepared.0);
        image(&mut self.peers, ImgDcOperation::Request1e, &payload, &[completion]);
    }
    fn request_scan(&mut self, request: &ScanRequest, progress: Completion, status: Completion) {
        assert_eq!((progress, status), (Completion::Image50, Completion::Image51));
        let mut payload = [0; 128];
        payload[..120].copy_from_slice(&wire::scan_request(request));
        // fea950 appends status51 before progress50.
        word(&mut payload, 120, self.callbacks.pipeline.tone_finished.0);
        word(&mut payload, 124, self.callbacks.pipeline.tone_progress.0);
        image(&mut self.peers, ImgDcOperation::Request1f, &payload, &[status, progress]);
    }
    fn request_selection(&mut self, request: SelectionRequest) {
        image(&mut self.peers, ImgDcOperation::Request20, &wire::selection_request(request), &[]);
    }
    fn request_scan_selection(&mut self) { image(&mut self.peers, ImgDcOperation::Request21, &[], &[]); }
    fn request_output(&mut self, completion: Completion) {
        assert_eq!(completion, Completion::Image52);
        image(&mut self.peers, ImgDcOperation::Request22, &self.callbacks.pipeline.output.0.to_le_bytes(), &[completion]);
    }
    fn request_commit(&mut self, completion: Completion) {
        assert_eq!(completion, Completion::Image53);
        image(&mut self.peers, ImgDcOperation::Request23, &self.callbacks.pipeline.commit.0.to_le_bytes(), &[completion]);
    }
    fn request_statistics(&mut self, setup: &StatisticsSetup, completion: Completion) {
        assert_eq!(completion, Completion::Statistics7a);
        let blocks = wire::statistics_blocks(setup);
        let mut payload = [0; 176];
        payload[..16].copy_from_slice(&blocks.selection);
        word(&mut payload, 16, blocks.format);
        payload[20..36].copy_from_slice(&blocks.area);
        payload[36..52].copy_from_slice(&blocks.levels);
        payload[52..108].copy_from_slice(&blocks.sampling);
        payload[108..164].copy_from_slice(&blocks.capture);
        word(&mut payload, 164, blocks.kind);
        // f20e74/80: progress f20518, then final f20604.
        word(&mut payload, 168, self.callbacks.pipeline.statistics_progress.0);
        word(&mut payload, 172, self.callbacks.pipeline.statistics.0);
        statistics(&mut self.peers, StatisticsOperation::Request03, &payload, &[completion]);
    }
    fn request_statistics_commit(&mut self, completion: Completion) {
        assert_eq!(completion, Completion::Statistics6b);
        let (operation, payload) = xm1_saclac_interface::stock::statistics::statistics_payload(
            &xm1_saclac_interface::statistics::StatisticsPayload::Stop { pause: false, shot_idle: true, callback: self.callbacks.pipeline.statistics_commit });
        statistics(&mut self.peers, operation, payload.bytes(), &[completion]);
    }
    fn store_statistics_weights(&mut self, target: u32, weights: &StatisticsWeights) {
        let mut machine = self.services.borrow().machine.clone();
        for (index, byte) in weights.weights.iter().enumerate() { machine.write_u8(capture_samples::REQUEST.get() + index as u32, *byte); }
        let [kind, width, height, x, y, first, second] = wire::statistics_weights_arguments(weights);
        record_image_command_results::helper_1042e9c(&mut crate::calibration_inputs::runtime(&machine), PreviewConfiguration {
            handle: target, table: capture_samples::REQUEST, kind, width, height, x, y, first, second,
        }).expect("statistics weights memory");
    }
    fn statistics_target(&mut self, lens: PlanBlock) -> u32 {
        assert_eq!(lens.0, crate::motion_plan::LENS);
        // fa36a0 points to base+978; f9e934/940 installs its sixth buffer.
        xm1_saclac_calc::stock::motion::IMAGE_BUFFERS[5].0
    }
    fn capture_idle(&mut self) -> bool { capture_samples::idle(&self.services.borrow().machine) }
    fn take_sample(&mut self, level: u16, targets: &[PlanBlock; 5]) -> Option<i16> {
        let machine = self.services.borrow().machine.clone();
        capture_samples::take(&machine, level, targets)
    }
    fn channel_sample(&mut self, index: u8) -> i16 { capture_samples::position(&self.services.borrow().machine, u32::from(index)) }
    fn sample_snapshot(&mut self) -> SampleSnapshot { capture_samples::snapshot(&self.services.borrow().machine) }
}
