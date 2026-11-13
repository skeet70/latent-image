use crate::replies::{deliver, Effect, Effects, Reply};
use std::vec::Vec;
use xm1_saclac_interface::{stock, CommandKind, CompletionStatus, ReplyTarget};

#[derive(Default)]
struct Output(Vec<(bool, ReplyTarget, Option<CompletionStatus>, Vec<u8>)>);
impl Effects for Output {
    fn emit(&mut self, effect: Effect<'_>) {
        self.0.push(match effect {
            Effect::ClearPending(target) => (true, target, None, Vec::new()),
            Effect::Deliver { target, status, values, .. } => (false, target, Some(status), values.bytes().to_vec()),
        });
    }
}

#[test]
fn final_replies_clear_pending_before_entering_the_client() {
    let target = ReplyTarget::Command(CommandKind::Autofocus);
    let callback = stock::client_callback(stock::parameters(&[1, 0, 0, 0])).unwrap();
    for status in [i32::MIN, -95, -1, 0, 20, 21] {
        let mut output = Output::default();
        deliver(Reply { target, callback: Some(callback), status: CompletionStatus(status), values: stock::parameters(&[7, 8]) }, &mut output);
        assert_eq!(output.0, std::vec![(true, target, None, std::vec![]), (false, target, Some(CompletionStatus(status)), std::vec![7, 8])]);
    }
}

#[test]
fn progress_replies_leave_the_command_pending() {
    let target = ReplyTarget::Command(CommandKind::HalfPress);
    let callback = stock::client_callback(stock::parameters(&[1, 0, 0, 0])).unwrap();
    for status in [1, 2, 19, 22, i32::MAX] {
        let mut output = Output::default();
        deliver(Reply { target, callback: Some(callback), status: CompletionStatus(status), values: stock::parameters(&[]) }, &mut output);
        assert_eq!(output.0, std::vec![(false, target, Some(CompletionStatus(status)), std::vec![])]);
    }
}

#[test]
fn absent_client_still_releases_a_finished_command() {
    let target = ReplyTarget::Command(CommandKind::Stop);
    let mut output = Output::default();
    deliver(Reply { target, callback: None, status: CompletionStatus(0), values: stock::parameters(&[]) }, &mut output);
    assert_eq!(output.0, std::vec![(true, target, None, std::vec![])]);
}

#[test]
fn preparation_notice_uses_subscription_instead_of_command_callback() {
    struct Callbacks(Vec<xm1_saclac_interface::ClientCallback>);
    impl Effects for Callbacks {
        fn emit(&mut self, effect: Effect<'_>) {
            if let Effect::Deliver { callback, .. } = effect { self.0.push(callback); }
        }
    }
    let command = stock::optional_callback(1);
    let subscriber = stock::optional_callback(2);
    let mut output = Callbacks(Vec::new());
    crate::replies::prepare(command, crate::replies::PreparationNotice::Requested(subscriber), CompletionStatus(19), stock::parameters(&[]), &mut output);
    assert_eq!(output.0, [subscriber.unwrap()]);
    output.0.clear();
    crate::replies::prepare(command, crate::replies::PreparationNotice::Suppressed, CompletionStatus(19), stock::parameters(&[]), &mut output);
    assert!(output.0.is_empty());
    crate::replies::prepare(command, crate::replies::PreparationNotice::Requested(subscriber), CompletionStatus(0), stock::parameters(&[]), &mut output);
    assert_eq!(output.0, [command.unwrap()]);
}
