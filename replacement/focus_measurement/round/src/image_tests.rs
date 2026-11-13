use crate::fakes::{Outbound, Ports};
use crate::image::{ChannelCount, ImageClient, ImageCommand, ImageJob, ImageLog, ImageLogging, ImageOutcome, ImageRequest, ImageSignal, ImageWork, Interruption, LensReport, ReplyValue};
use crate::profile::StatisticsInUse;
use xm1_saclac_interface::round_image::{CancelArgument, ImageArea, ImageArguments, ImageChannel, ImageLaneRequest, SnapshotArgument};
use xm1_saclac_interface::stock::{self, StockCallbackWord};
use xm1_saclac_interface::{ClientCallback, CommandKind, CompletionStatus, RecordHandle, ReplyTarget};

fn callback(word: u32) -> Option<ClientCallback> { stock::optional_callback(word) }

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16c0000) }

fn client(word: u32, channel: u32, value: u32) -> ImageClient {
    ImageClient { callback: callback(word), channel: ImageChannel(channel), value: ReplyValue(value) }
}

fn image(client: ImageClient) -> ImageSignal { ImageSignal::Request(ImageCommand { client, work: ImageWork::Image(ImageArguments([0x11, 0x22])) }) }

fn snapshot(client: ImageClient) -> ImageSignal { ImageSignal::Request(ImageCommand { client, work: ImageWork::Snapshot(SnapshotArgument(-4)) }) }

fn cancel(word: u32) -> ImageSignal { ImageSignal::Interrupt { interruption: Interruption::Cancel(CancelArgument(0x33)), callback: callback(word) } }

fn interrupt_stop(word: u32) -> ImageSignal { ImageSignal::Interrupt { interruption: Interruption::Stop, callback: callback(word) } }

fn imaged(kind: ImageRequest, failed: bool) -> ImageSignal { ImageSignal::Imaged { kind, failed } }

fn reply(kind: CommandKind, word: u32, status: i32, values: [u32; 3]) -> [Outbound; 2] {
    let target = ReplyTarget::Command(kind);
    [Outbound::ClearPending(target), Outbound::Reply { target, callback: callback(word).unwrap(), status: CompletionStatus(status), values }]
}

/// An enabled lane whose area configuration adds the channel to the first halfword it is given.
fn enabled() -> (ImageJob, Ports) {
    let mut ports = Ports::new(None, ());
    ports.channel_count = ChannelCount(16);
    ports.image_area = |channel, area| ImageArea([area.0[0] + channel.0 as i16, 7]);
    let mut job = ImageJob::new();
    assert_eq!(job.signal(ImageSignal::Enable, record(), &mut ports), ImageOutcome::Complete);
    ports.take();
    (job, ports)
}

#[test]
fn starting_the_lane_tells_the_registered_image_client_and_keeps_the_search_statistics_in_use() {
    for signal in [ImageSignal::Initialize, ImageSignal::Enable] {
        let mut ports = Ports::new(None, ());
        ports.records.in_use = StatisticsInUse { primary: true, search: false, auxiliary: true };
        ports.image_client = Some(StockCallbackWord(0x00e0_1000));
        assert_eq!(ImageJob::new().signal(signal, record(), &mut ports), ImageOutcome::Complete);
        assert_eq!(ports.take(), [Outbound::NotifyImageClient(StockCallbackWord(0x00e0_1000))]);
        assert_eq!(ports.records.in_use, StatisticsInUse { primary: true, search: true, auxiliary: true });
    }
    let mut ports = Ports::new(None, ());
    ImageJob::new().signal(ImageSignal::Enable, record(), &mut ports);
    assert!(ports.take().is_empty());
    assert!(ports.records.in_use.search);
}

#[test]
fn an_image_command_prepares_the_area_for_its_channel_then_asks_imgdc_with_the_command_words() {
    let (mut job, mut ports) = enabled();
    assert_eq!(job.signal(image(client(0x100, 3, 7)), record(), &mut ports), ImageOutcome::Pending);
    assert_eq!(ports.take(), [
        Outbound::PrepareImageArea(ImageChannel(3), ImageArea([0, 0])),
        Outbound::Image(ImageLaneRequest::Image { arguments: ImageArguments([0x11, 0x22]), area: ImageArea([3, 7]), channel: ImageChannel(3) }),
    ]);
    assert_eq!(job.signal(imaged(ImageRequest::Image, true), record(), &mut ports), ImageOutcome::Complete);
    assert_eq!(ports.take(), reply(CommandKind::Command14, 0x100, -94, [3, 7, 0]));
}

#[test]
fn a_snapshot_command_asks_imgdc_with_its_halfword_and_replies_to_the_snapshot_client() {
    let (mut job, mut ports) = enabled();
    assert_eq!(job.signal(snapshot(client(0x200, 4, 8)), record(), &mut ports), ImageOutcome::Pending);
    assert_eq!(ports.take(), [
        Outbound::PrepareImageArea(ImageChannel(4), ImageArea([0, 0])),
        Outbound::Image(ImageLaneRequest::Snapshot { argument: SnapshotArgument(-4), area: ImageArea([4, 7]), channel: ImageChannel(4) }),
    ]);
    job.signal(imaged(ImageRequest::Snapshot, false), record(), &mut ports);
    assert_eq!(ports.take(), reply(CommandKind::Command2b, 0x200, 0, [4, 8, 0]));
}

#[test]
fn the_prepared_area_carries_to_the_next_request_until_the_lane_starts_again() {
    let (mut job, mut ports) = enabled();
    job.signal(image(client(0x100, 3, 7)), record(), &mut ports);
    job.signal(imaged(ImageRequest::Image, false), record(), &mut ports);
    ports.take();
    job.signal(snapshot(client(0x200, 4, 8)), record(), &mut ports);
    assert_eq!(ports.take()[0], Outbound::PrepareImageArea(ImageChannel(4), ImageArea([3, 7])));
    job.signal(imaged(ImageRequest::Snapshot, false), record(), &mut ports);
    job.signal(ImageSignal::Stop, record(), &mut ports);
    job.signal(ImageSignal::Enable, record(), &mut ports);
    ports.take();
    job.signal(image(client(0x100, 5, 7)), record(), &mut ports);
    assert_eq!(ports.take()[0], Outbound::PrepareImageArea(ImageChannel(5), ImageArea([0, 0])));
}

#[test]
fn a_reply_takes_its_command_client_and_the_latest_channel() {
    let (mut job, mut ports) = enabled();
    job.signal(image(client(0x100, 3, 7)), record(), &mut ports);
    assert_eq!(job.signal(snapshot(client(0x200, 9, 8)), record(), &mut ports), ImageOutcome::Fault);
    ports.take();
    job.signal(imaged(ImageRequest::Image, false), record(), &mut ports);
    assert_eq!(ports.take(), reply(CommandKind::Command14, 0x100, 0, [9, 7, 0]));
}

#[test]
fn interruptions_ask_imgdc_only_while_an_image_is_pending() {
    let (mut job, mut ports) = enabled();
    assert_eq!(job.signal(cancel(0x300), record(), &mut ports), ImageOutcome::Complete);
    assert_eq!(ports.take(), reply(CommandKind::Command15, 0x300, 21, [0; 3]));
    assert_eq!(job.signal(interrupt_stop(0x400), record(), &mut ports), ImageOutcome::Complete);
    assert_eq!(ports.take(), reply(CommandKind::Command16, 0x400, 21, [0; 3]));

    job.signal(snapshot(client(0x200, 4, 8)), record(), &mut ports);
    ports.take();
    assert_eq!(job.signal(cancel(0x300), record(), &mut ports), ImageOutcome::Pending);
    let [clear, answer] = reply(CommandKind::Command15, 0x300, 20, [0; 3]);
    assert_eq!(ports.take(), [Outbound::Image(ImageLaneRequest::Cancel(CancelArgument(0x33))), clear, answer]);
    assert_eq!(job.signal(interrupt_stop(0x400), record(), &mut ports), ImageOutcome::Pending);
    let [clear, answer] = reply(CommandKind::Command16, 0x400, 20, [0; 3]);
    assert_eq!(ports.take(), [Outbound::Image(ImageLaneRequest::Stop), clear, answer]);
}

#[test]
fn logging_marks_each_request_before_imgdc_receives_it() {
    let (mut job, mut ports) = enabled();
    ports.image_logging = ImageLogging { image: true, snapshot: false };
    job.signal(image(client(0x100, 3, 7)), record(), &mut ports);
    assert_eq!(&ports.take()[1..], [
        Outbound::ImageLog(ImageLog::Image),
        Outbound::Image(ImageLaneRequest::Image { arguments: ImageArguments([0x11, 0x22]), area: ImageArea([3, 7]), channel: ImageChannel(3) }),
    ]);
    job.signal(cancel(0x300), record(), &mut ports);
    assert_eq!(ports.take()[..2], [Outbound::ImageLog(ImageLog::Cancel), Outbound::Image(ImageLaneRequest::Cancel(CancelArgument(0x33)))]);
    job.signal(ImageSignal::Stop, record(), &mut ports);
    assert_eq!(ports.take(), [Outbound::ImageLog(ImageLog::Stop), Outbound::Image(ImageLaneRequest::Stop)]);

    let (mut job, mut ports) = enabled();
    ports.image_logging = ImageLogging { image: false, snapshot: true };
    job.signal(snapshot(client(0x200, 4, 8)), record(), &mut ports);
    assert_eq!(ports.take()[1], Outbound::ImageLog(ImageLog::Snapshot));
    job.signal(cancel(0x300), record(), &mut ports);
    assert_eq!(ports.take()[0], Outbound::Image(ImageLaneRequest::Cancel(CancelArgument(0x33))));
}

#[test]
fn stopping_asks_imgdc_to_stop_and_waits_for_the_pending_image() {
    let (mut job, mut ports) = enabled();
    job.signal(image(client(0x100, 3, 7)), record(), &mut ports);
    ports.take();
    assert_eq!(job.signal(ImageSignal::Stop, record(), &mut ports), ImageOutcome::Pending);
    assert_eq!(ports.take(), [Outbound::Image(ImageLaneRequest::Stop)]);
    assert_eq!(job.signal(ImageSignal::Calculated, record(), &mut ports), ImageOutcome::Pending);
    assert_eq!(job.signal(imaged(ImageRequest::Image, false), record(), &mut ports), ImageOutcome::Complete);
    assert!(ports.take().is_empty());
    assert_eq!(job.signal(ImageSignal::Initialize, record(), &mut ports), ImageOutcome::Complete);
}

#[test]
fn a_snapshot_finishing_while_stopping_faults() {
    let (mut job, mut ports) = enabled();
    job.signal(snapshot(client(0x200, 4, 8)), record(), &mut ports);
    job.signal(ImageSignal::Stop, record(), &mut ports);
    assert_eq!(job.signal(imaged(ImageRequest::Snapshot, false), record(), &mut ports), ImageOutcome::Fault);
}

#[test]
fn the_calculation_request_never_becomes_ready() {
    let (mut job, mut ports) = enabled();
    assert_eq!(job.signal(ImageSignal::Run, record(), &mut ports), ImageOutcome::Idle);
    assert_eq!(job.signal(ImageSignal::Calculated, record(), &mut ports), ImageOutcome::Ignored);
    assert!(ports.take().is_empty());
}

#[test]
fn requests_need_an_enabled_lane() {
    let mut ports = Ports::new(None, ());
    let mut job = ImageJob::new();
    assert_eq!(job.signal(image(client(0x100, 3, 7)), record(), &mut ports), ImageOutcome::Fault);
    assert_eq!(job.signal(imaged(ImageRequest::Image, false), record(), &mut ports), ImageOutcome::Ignored);
    assert!(ports.take().is_empty());
}

#[test]
fn a_reply_rejects_a_negative_or_unknown_channel_and_reports_the_lens_queries() {
    let (mut job, mut ports) = enabled();
    ports.channel_count = ChannelCount(100);
    ports.lens = LensReport { first: false, second: true, third: false };
    for (channel, value) in [(u32::MAX, 0x80_0000), (100, 0x80_0000), (99, 3)] {
        job.signal(snapshot(client(0x500, channel, 3)), record(), &mut ports);
        job.signal(imaged(ImageRequest::Snapshot, false), record(), &mut ports);
        assert_eq!(ports.take()[2..], reply(CommandKind::Command2b, 0x500, 0, [channel, value, 1]));
    }
}

#[test]
fn the_logging_setting_logs_image_requests_on_bit_eight_and_snapshots_on_bit_nine() {
    assert_eq!(crate::stock::image_logging(0x0100), ImageLogging { image: true, snapshot: false });
    assert_eq!(crate::stock::image_logging(0x0200), ImageLogging { image: false, snapshot: true });
    assert_eq!(crate::stock::image_logging(0xfcff), ImageLogging::default());
}
