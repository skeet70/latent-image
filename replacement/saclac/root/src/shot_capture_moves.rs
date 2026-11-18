#![forbid(unsafe_code)]

use xm1_saclac_interface::{shared::{MoveMode, RelativeMove}, stock, Completion, LensDc, LensDcOperation, LensDcRequest};
use xm1_shot_sequence::{capture_boundary::{LensMoves, MoveProfile}, capture_stock, pipeline_work::PipelinePlatform};
use crate::shot_services::Libraries;

impl<M, R, E, P, I, T, A, Q: LensDc> LensMoves for Libraries<'_, M, R, E, P, I, T, A, Q>
where Self: PipelinePlatform {
    fn absolute(&mut self, target: u32, profile: MoveProfile, completion: Completion) {
        assert_eq!(completion, Completion::Lens3a);
        let words = capture_stock::absolute_move_arguments(profile);
        let special = PipelinePlatform::load_test_state(self) != 0
            && PipelinePlatform::load_test_setting(self) == 0x70 && words[3] == 10;
        let operation = if special { LensDcOperation::Request1d } else { LensDcOperation::Request13 };
        let callback = if matches!(profile, MoveProfile::Finish { .. }) { self.callbacks.moves.finish_absolute } else { self.callbacks.moves.capture_absolute };
        let bytes = crate::stock::capture_moves::absolute(callback, target, words);
        assert!(LensDc::request(&mut self.peers, LensDcRequest {
            operation, parameters: stock::parameters(&bytes), completions: &[completion],
        }).is_ok(), "absolute capture lens enqueue");
    }
    fn relative(&mut self, delta: i16, speed: u32, profile: MoveProfile, completion: Completion) {
        assert_eq!(completion, Completion::Lens35);
        // 76e1d8..e0 returns0x81 without posting for a raw mode above2. Its caller
        // discards that return value; it does not truncate, enqueue or synthesize a completion.
        let mode = match speed { 0 => MoveMode::First, 1 => MoveMode::Second, 2 => MoveMode::Third, _ => return };
        assert!(xm1_saclac_shared::lens::move_relative(&RelativeMove {
            callback: if matches!(profile, MoveProfile::Finish { .. }) { self.callbacks.moves.finish_relative } else { self.callbacks.moves.capture_relative },
            step: delta, mode, words: capture_stock::relative_move_arguments(profile),
        }, &[completion], &mut self.peers).is_ok(), "relative capture lens enqueue");
    }
}
