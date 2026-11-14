#![forbid(unsafe_code)]

//! The image lane: client image and snapshot commands, their interruption, the requests they
//! make of imgdc and the replies their clients receive.

use crate::profile::{ProfileRecords, StatisticsInUse};
use crate::replies;
use xm1_saclac_interface::round_image::{CancelArgument, ImageArea, ImageArguments, ImageChannel, ImageLaneRequest, SnapshotArgument};
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::{ClientCallback, CommandKind, CompletionStatus, RecordHandle, ReplyTarget};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageRequest { Image, Snapshot }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyValue(pub u32);

/// What an image or snapshot command leaves for its eventual reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageClient {
    pub callback: Option<ClientCallback>,
    pub channel: ImageChannel,
    pub value: ReplyValue,
}

/// What an image or snapshot command asks of imgdc.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageWork { Image(ImageArguments), Snapshot(SnapshotArgument) }

impl ImageWork {
    const fn kind(self) -> ImageRequest {
        match self { Self::Image(_) => ImageRequest::Image, Self::Snapshot(_) => ImageRequest::Snapshot }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageCommand { pub client: ImageClient, pub work: ImageWork }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Interruption { Cancel(CancelArgument), Stop }

/// Code the client of an interruption receives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterruptionAnswer { Interrupted, NothingPending }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageSignal {
    Initialize,
    Enable,
    Stop,
    Run,
    Request(ImageCommand),
    Interrupt { interruption: Interruption, callback: Option<ClientCallback> },
    Calculated,
    Imaged { kind: ImageRequest, failed: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageOutcome { Ignored, Complete, Pending, Idle, Fault }

/// Which requests leave an entry in the diagnostic log.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImageLogging {
    /// Image, cancel and stop requests.
    pub image: bool,
    pub snapshot: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageLog { Image, Snapshot, Cancel, Stop }

/// Lens-control queries an image reply reports; each is set when its query answers 2.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensReport { pub first: bool, pub second: bool, pub third: bool }

/// How many image channels a reply may name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelCount(pub u8);

pub trait ReplyPorts: replies::Effects {
    fn channel_count(&mut self) -> ChannelCount;
    fn lens_report(&mut self) -> LensReport;
}

/// imgdc, the area configuration, the image client and the records the image lane uses.
pub trait Ports: ReplyPorts + ProfileRecords {
    /// The callback the client offering the image measurement registered with its offer.
    fn image_client_callback(&mut self) -> Option<StockCallbackWord>;
    /// Tells the image client's callback that the lane started.
    fn notify_image_client(&mut self, callback: StockCallbackWord);
    /// Programs the area configuration for `channel` under the record the search lane holds and
    /// returns the halfwords it leaves; `area` holds those of the previous preparation.
    fn prepare_image_area(&mut self, channel: ImageChannel, area: ImageArea) -> ImageArea;
    fn image_logging(&mut self) -> ImageLogging;
    fn log_image(&mut self, entry: ImageLog);
    fn request_image(&mut self, request: ImageLaneRequest);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Disabled, Ready, Imaging, Stopping }

/// The image lane's machine. Its calculation request never becomes ready.
pub struct ImageJob {
    phase: Phase,
    image_pending: bool,
    channel: ImageChannel,
    image: Option<ImageClient>,
    snapshot: Option<ImageClient>,
    area: ImageArea,
}

impl Default for ImageJob { fn default() -> Self { Self::new() } }

impl ImageJob {
    pub const fn new() -> Self {
        Self { phase: Phase::Disabled, image_pending: false, channel: ImageChannel(0), image: None, snapshot: None, area: ImageArea([0; 2]) }
    }

    pub fn signal(&mut self, signal: ImageSignal, record: RecordHandle, ports: &mut impl Ports) -> ImageOutcome {
        if let ImageSignal::Request(command) = signal {
            self.channel = command.client.channel;
            *self.client(command.work.kind()) = Some(command.client);
        }
        match (self.phase, signal) {
            (Phase::Disabled, ImageSignal::Initialize) => { self.start(record, ports); ImageOutcome::Complete }
            (Phase::Disabled, ImageSignal::Enable) => {
                self.start(record, ports);
                self.phase = Phase::Ready;
                ImageOutcome::Complete
            }
            (Phase::Disabled, ImageSignal::Stop) => ImageOutcome::Complete,
            (Phase::Ready, ImageSignal::Stop) => { self.phase = Phase::Disabled; ImageOutcome::Complete }
            (Phase::Imaging, ImageSignal::Stop) => {
                ask(ports, ImageLog::Stop, ImageLaneRequest::Stop);
                self.phase = Phase::Stopping;
                ImageOutcome::Pending
            }
            (Phase::Disabled | Phase::Ready | Phase::Imaging, ImageSignal::Run) => ImageOutcome::Idle,
            (Phase::Ready, ImageSignal::Request(command)) => {
                self.request(command.work, ports);
                self.phase = Phase::Imaging;
                ImageOutcome::Pending
            }
            (Phase::Ready, ImageSignal::Interrupt { interruption, callback }) => {
                deliver(ImageReply::Interrupted { interruption, callback, answer: InterruptionAnswer::NothingPending }, ports);
                ImageOutcome::Complete
            }
            (Phase::Imaging, ImageSignal::Interrupt { interruption, callback }) => {
                match interruption {
                    Interruption::Cancel(argument) => ask(ports, ImageLog::Cancel, ImageLaneRequest::Cancel(argument)),
                    Interruption::Stop => ask(ports, ImageLog::Stop, ImageLaneRequest::Stop),
                }
                deliver(ImageReply::Interrupted { interruption, callback, answer: InterruptionAnswer::Interrupted }, ports);
                ImageOutcome::Pending
            }
            (Phase::Imaging, ImageSignal::Imaged { kind, failed }) => {
                self.image_pending = false;
                self.phase = Phase::Ready;
                let client = *self.client(kind);
                deliver(ImageReply::Finished {
                    kind,
                    failed,
                    callback: client.and_then(|client| client.callback),
                    channel: self.channel,
                    value: client.map_or(ReplyValue(0), |client| client.value),
                }, ports);
                ImageOutcome::Complete
            }
            (Phase::Stopping, ImageSignal::Imaged { kind: ImageRequest::Image, .. }) => {
                self.image_pending = false;
                self.stopped()
            }
            (Phase::Stopping, ImageSignal::Calculated) => self.stopped(),
            (Phase::Disabled | Phase::Ready | Phase::Imaging, ImageSignal::Calculated)
            | (Phase::Disabled | Phase::Ready, ImageSignal::Imaged { .. }) => ImageOutcome::Ignored,
            _ => ImageOutcome::Fault,
        }
    }

    fn start(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        if let Some(callback) = ports.image_client_callback() { ports.notify_image_client(callback); }
        self.area = ImageArea([0; 2]);
        let in_use = ports.statistics_in_use();
        ports.set_statistics_in_use(record, StatisticsInUse { search: true, ..in_use });
    }

    fn request(&mut self, work: ImageWork, ports: &mut impl Ports) {
        self.area = ports.prepare_image_area(self.channel, self.area);
        self.image_pending = true;
        let (area, channel) = (self.area, self.channel);
        match work {
            ImageWork::Image(arguments) => ask(ports, ImageLog::Image, ImageLaneRequest::Image { arguments, area, channel }),
            ImageWork::Snapshot(argument) => ask(ports, ImageLog::Snapshot, ImageLaneRequest::Snapshot { argument, area, channel }),
        }
    }

    fn stopped(&mut self) -> ImageOutcome {
        if self.image_pending { return ImageOutcome::Pending; }
        self.phase = Phase::Disabled;
        ImageOutcome::Complete
    }

    fn client(&mut self, kind: ImageRequest) -> &mut Option<ImageClient> {
        match kind { ImageRequest::Image => &mut self.image, ImageRequest::Snapshot => &mut self.snapshot }
    }
}

fn ask(ports: &mut impl Ports, entry: ImageLog, request: ImageLaneRequest) {
    let logging = ports.image_logging();
    let logged = if entry == ImageLog::Snapshot { logging.snapshot } else { logging.image };
    if logged { ports.log_image(entry); }
    ports.request_image(request);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImageReply {
    Interrupted { interruption: Interruption, callback: Option<ClientCallback>, answer: InterruptionAnswer },
    Finished { kind: ImageRequest, failed: bool, callback: Option<ClientCallback>, channel: ImageChannel, value: ReplyValue },
}

/// The channel word is read as signed.
const fn within(channel: ImageChannel, count: ChannelCount) -> bool {
    let channel = channel.0 as i32;
    channel >= 0 && channel < count.0 as i32
}

fn deliver(reply: ImageReply, ports: &mut impl ReplyPorts) {
    match reply {
        ImageReply::Interrupted { interruption, callback, answer } => replies::deliver(replies::Reply {
            target: ReplyTarget::Command(match interruption { Interruption::Cancel(_) => CommandKind::Command15, Interruption::Stop => CommandKind::Command16 }),
            callback,
            status: CompletionStatus(match answer { InterruptionAnswer::Interrupted => 20, InterruptionAnswer::NothingPending => 21 }),
            values: xm1_saclac_interface::stock::parameters(&[]),
        }, ports),
        ImageReply::Finished { kind, failed, callback, channel, value } => {
            let lens = ports.lens_report();
            let value = if within(channel, ports.channel_count()) { value } else { crate::stock::NO_CHANNEL_VALUE };
            let values = crate::stock::image_reply_values(channel, value, lens);
            replies::deliver(replies::Reply {
                target: ReplyTarget::Command(match kind { ImageRequest::Image => CommandKind::Command14, ImageRequest::Snapshot => CommandKind::Command2b }),
                callback,
                status: CompletionStatus(if failed { -94 } else { 0 }),
                values: xm1_saclac_interface::stock::parameters(&values),
            }, ports);
        }
    }
}
