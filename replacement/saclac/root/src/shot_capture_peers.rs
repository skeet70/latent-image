#![forbid(unsafe_code)]

use xm1_saclac_interface::{statistics::{StatisticsPayload, StatisticsSetting}, stock, Completion, Statistics, StatisticsOperation, StatisticsRequest};
use xm1_shot_sequence::capture_boundary::CapturePeers;
use crate::shot_services::Libraries;

impl<M, R, E, P, I, T, A, Q: Statistics> CapturePeers for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn statistics_setup(&mut self, enabled: bool, completion: Completion) {
        assert_eq!(completion, Completion::Statistics83);
        let (operation, bytes) = stock::statistics::statistics_payload(&StatisticsPayload::Stop {
            pause: enabled, shot_idle: true, callback: self.callbacks.capture_statistics.setup,
        });
        assert!(Statistics::request(&mut self.peers, StatisticsRequest {
            operation, parameters: stock::parameters(bytes.bytes()), completions: &[completion],
        }).is_ok(), "capture statistics setup enqueue");
    }
    fn statistics_setting(&mut self, setting: &StatisticsSetting, completion: Completion) {
        let callback = match completion {
            Completion::Statistics85 => self.callbacks.capture_statistics.setting,
            Completion::Statistics7d => self.callbacks.capture_statistics.primary_setting,
            _ => panic!("capture setting completion outside its two native callbacks"),
        };
        let bytes = crate::stock::capture_statistics::setting(setting, callback);
        assert!(Statistics::request(&mut self.peers, StatisticsRequest {
            operation: StatisticsOperation::Request05, parameters: stock::parameters(&bytes), completions: &[completion],
        }).is_ok(), "capture statistics setting enqueue");
    }
}
