use crate::image::{ImageChannel, ImageClient, ImageEffect, ImageJob, ImageOutcome, ImagePeer, ImageReply, ImageRequest, ImageSignal, Interruption, InterruptionAnswer, ReplyValue};
use crate::fakes::{Outbound, Ports};
use crate::image::{deliver, ChannelCount, LensReport};
use xm1_saclac_interface::{stock, ClientCallback, CommandKind, CompletionStatus, ReplyTarget};
use std::vec::Vec;

fn callback(word: u32) -> Option<ClientCallback> { stock::optional_callback(word) }

fn client(word: u32, channel: u32, value: u32) -> ImageClient {
    ImageClient { callback: callback(word), channel: ImageChannel(channel), value: ReplyValue(value) }
}

fn request(kind: ImageRequest, client: ImageClient) -> ImageSignal<'static> {
    ImageSignal::Request { kind, parameters: stock::parameters(&[]), client }
}

fn interrupt(interruption: Interruption, word: u32) -> ImageSignal<'static> {
    ImageSignal::Interrupt { interruption, parameters: stock::parameters(&[]), callback: callback(word) }
}

fn run(job: &mut ImageJob, signal: ImageSignal<'static>) -> (ImageOutcome, Vec<ImageEffect<'static>>) {
    let mut effects = Vec::new();
    let outcome = job.signal(signal, &mut |effect| effects.push(effect));
    (outcome, effects)
}

fn enabled() -> ImageJob {
    let mut job = ImageJob::new();
    assert_eq!(run(&mut job, ImageSignal::Enable), (ImageOutcome::Complete, std::vec![ImageEffect::Peer(ImagePeer::Initialize)]));
    job
}

#[test]
fn an_image_request_replies_to_its_client_when_the_image_completes() {
    let mut job = enabled();
    let (outcome, effects) = run(&mut job, request(ImageRequest::Image, client(0x100, 3, 7)));
    assert_eq!(outcome, ImageOutcome::Pending);
    assert_eq!(effects, [ImageEffect::Peer(ImagePeer::PrepareGeometry), ImageEffect::Peer(ImagePeer::Request { kind: ImageRequest::Image, parameters: stock::parameters(&[]) })]);
    let (outcome, effects) = run(&mut job, ImageSignal::Imaged { kind: ImageRequest::Image, failed: true });
    assert_eq!(outcome, ImageOutcome::Complete);
    assert_eq!(effects, [ImageEffect::Reply(ImageReply::Finished { kind: ImageRequest::Image, failed: true, callback: callback(0x100), channel: ImageChannel(3), value: ReplyValue(7) })]);
    assert_eq!(run(&mut job, request(ImageRequest::Snapshot, client(0x200, 4, 8))).0, ImageOutcome::Pending);
}

#[test]
fn a_reply_takes_its_command_client_and_the_latest_channel() {
    let mut job = enabled();
    run(&mut job, request(ImageRequest::Image, client(0x100, 3, 7)));
    assert_eq!(run(&mut job, request(ImageRequest::Snapshot, client(0x200, 9, 8))).0, ImageOutcome::Fault);
    let (_, effects) = run(&mut job, ImageSignal::Imaged { kind: ImageRequest::Image, failed: false });
    assert_eq!(effects, [ImageEffect::Reply(ImageReply::Finished { kind: ImageRequest::Image, failed: false, callback: callback(0x100), channel: ImageChannel(9), value: ReplyValue(7) })]);
}

#[test]
fn interruptions_answer_whether_an_image_was_pending() {
    let mut job = enabled();
    assert_eq!(run(&mut job, interrupt(Interruption::Cancel, 0x300)), (ImageOutcome::Complete, std::vec![
        ImageEffect::Reply(ImageReply::Interrupted { interruption: Interruption::Cancel, callback: callback(0x300), answer: InterruptionAnswer::NothingPending }),
    ]));
    run(&mut job, request(ImageRequest::Snapshot, client(0x200, 4, 8)));
    assert_eq!(run(&mut job, interrupt(Interruption::Cancel, 0x300)), (ImageOutcome::Pending, std::vec![
        ImageEffect::Peer(ImagePeer::Cancel(stock::parameters(&[]))),
        ImageEffect::Reply(ImageReply::Interrupted { interruption: Interruption::Cancel, callback: callback(0x300), answer: InterruptionAnswer::Interrupted }),
    ]));
    assert_eq!(run(&mut job, interrupt(Interruption::Stop, 0x400)), (ImageOutcome::Pending, std::vec![
        ImageEffect::Peer(ImagePeer::Stop),
        ImageEffect::Reply(ImageReply::Interrupted { interruption: Interruption::Stop, callback: callback(0x400), answer: InterruptionAnswer::Interrupted }),
    ]));
}

#[test]
fn stopping_waits_for_the_pending_image() {
    let mut job = enabled();
    run(&mut job, request(ImageRequest::Image, client(0x100, 3, 7)));
    assert_eq!(run(&mut job, ImageSignal::Stop), (ImageOutcome::Pending, std::vec![ImageEffect::Peer(ImagePeer::Stop)]));
    assert_eq!(run(&mut job, ImageSignal::Imaged { kind: ImageRequest::Image, failed: false }), (ImageOutcome::Complete, std::vec![]));
    assert_eq!(run(&mut job, ImageSignal::Initialize).0, ImageOutcome::Complete);
}

#[test]
fn the_calculation_request_never_becomes_ready() {
    let mut job = enabled();
    assert_eq!(run(&mut job, ImageSignal::Run), (ImageOutcome::Idle, std::vec![]));
    assert_eq!(run(&mut job, ImageSignal::Calculated), (ImageOutcome::Ignored, std::vec![]));
}

#[test]
fn requests_need_an_enabled_lane() {
    let mut job = ImageJob::new();
    assert_eq!(run(&mut job, request(ImageRequest::Image, client(0x100, 3, 7))), (ImageOutcome::Fault, std::vec![]));
    assert_eq!(run(&mut job, ImageSignal::Imaged { kind: ImageRequest::Image, failed: false }), (ImageOutcome::Ignored, std::vec![]));
}

#[test]
fn interruption_replies_answer_twenty_or_twenty_one_to_the_cancel_or_stop_client() {
    let mut ports = Ports::new(None, ());
    deliver(ImageReply::Interrupted { interruption: Interruption::Cancel, callback: callback(0x300), answer: InterruptionAnswer::Interrupted }, &mut ports);
    deliver(ImageReply::Interrupted { interruption: Interruption::Stop, callback: None, answer: InterruptionAnswer::NothingPending }, &mut ports);
    let cancel = ReplyTarget::Command(CommandKind::Command15);
    assert_eq!(ports.take(), [
        Outbound::ClearPending(cancel),
        Outbound::Reply { target: cancel, callback: callback(0x300).unwrap(), status: CompletionStatus(20), values: [0; 3] },
        Outbound::ClearPending(ReplyTarget::Command(CommandKind::Command16)),
    ]);
}

#[test]
fn a_snapshot_reply_rejects_a_negative_channel_and_reports_the_lens_queries() {
    let mut ports = Ports::new(None, ());
    ports.channel_count = ChannelCount(100);
    ports.lens = LensReport { first: false, second: true, third: false };
    let finished = |channel| ImageReply::Finished { kind: ImageRequest::Snapshot, failed: false, callback: callback(0x500), channel: ImageChannel(channel), value: ReplyValue(3) };
    deliver(finished(u32::MAX), &mut ports);
    deliver(finished(99), &mut ports);
    let target = ReplyTarget::Command(CommandKind::Command2b);
    let reply = |channel, value| Outbound::Reply { target, callback: callback(0x500).unwrap(), status: CompletionStatus(0), values: [channel, value, 1] };
    assert_eq!(ports.take(), [
        Outbound::ClearPending(target), reply(u32::MAX, 0x80_0000),
        Outbound::ClearPending(target), reply(99, 3),
    ]);
}
