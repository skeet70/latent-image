#![forbid(unsafe_code)]

use xm1_saclac_controllers::replies;
use xm1_saclac_interface::{ClientCallback, CommandKind, CompletionStatus, Parameters, ReplyTarget};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageRequest { Image, Snapshot }

/// The retained channel an image reply names.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImageChannel(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyValue(pub u32);

/// What an image or snapshot command leaves for its eventual reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageClient {
    pub callback: Option<ClientCallback>,
    pub channel: ImageChannel,
    pub value: ReplyValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Interruption { Cancel, Stop }

/// Code the client of an interruption receives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterruptionAnswer { Interrupted, NothingPending }

/// What the image lane asks of the image peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImagePeer<'a> {
    Initialize,
    PrepareGeometry,
    Request { kind: ImageRequest, parameters: Parameters<'a> },
    Cancel(Parameters<'a>),
    Stop,
}

/// What the image lane answers its clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageReply {
    Interrupted { interruption: Interruption, callback: Option<ClientCallback>, answer: InterruptionAnswer },
    Finished { kind: ImageRequest, failed: bool, callback: Option<ClientCallback>, channel: ImageChannel, value: ReplyValue },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageEffect<'a> { Peer(ImagePeer<'a>), Reply(ImageReply) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageSignal<'a> {
    Initialize,
    Enable,
    Stop,
    Run,
    Request { kind: ImageRequest, parameters: Parameters<'a>, client: ImageClient },
    Interrupt { interruption: Interruption, parameters: Parameters<'a>, callback: Option<ClientCallback> },
    Calculated,
    Imaged { kind: ImageRequest, failed: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageOutcome { Ignored, Complete, Pending, Idle, Fault }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Disabled, Ready, Imaging, Stopping }

/// The image lane: client image and snapshot requests, their interruption and
/// their replies. Its calculation request never becomes ready.
pub struct ImageJob {
    phase: Phase,
    image_pending: bool,
    channel: ImageChannel,
    image: Option<ImageClient>,
    snapshot: Option<ImageClient>,
}

impl Default for ImageJob { fn default() -> Self { Self::new() } }

impl ImageJob {
    pub const fn new() -> Self {
        Self { phase: Phase::Disabled, image_pending: false, channel: ImageChannel(0), image: None, snapshot: None }
    }

    pub fn signal<'a>(&mut self, signal: ImageSignal<'a>, effect: &mut impl FnMut(ImageEffect<'a>)) -> ImageOutcome {
        if let ImageSignal::Request { kind, client, .. } = signal {
            self.channel = client.channel;
            *self.client(kind) = Some(client);
        }
        match (self.phase, signal) {
            (Phase::Disabled, ImageSignal::Initialize) => { effect(ImageEffect::Peer(ImagePeer::Initialize)); ImageOutcome::Complete }
            (Phase::Disabled, ImageSignal::Enable) => {
                effect(ImageEffect::Peer(ImagePeer::Initialize));
                self.phase = Phase::Ready;
                ImageOutcome::Complete
            }
            (Phase::Disabled, ImageSignal::Stop) => ImageOutcome::Complete,
            (Phase::Ready, ImageSignal::Stop) => { self.phase = Phase::Disabled; ImageOutcome::Complete }
            (Phase::Imaging, ImageSignal::Stop) => {
                effect(ImageEffect::Peer(ImagePeer::Stop));
                self.phase = Phase::Stopping;
                ImageOutcome::Pending
            }
            (Phase::Disabled | Phase::Ready | Phase::Imaging, ImageSignal::Run) => ImageOutcome::Idle,
            (Phase::Ready, ImageSignal::Request { kind, parameters, .. }) => {
                effect(ImageEffect::Peer(ImagePeer::PrepareGeometry));
                self.image_pending = true;
                effect(ImageEffect::Peer(ImagePeer::Request { kind, parameters }));
                self.phase = Phase::Imaging;
                ImageOutcome::Pending
            }
            (Phase::Ready, ImageSignal::Interrupt { interruption, callback, .. }) => {
                effect(ImageEffect::Reply(ImageReply::Interrupted { interruption, callback, answer: InterruptionAnswer::NothingPending }));
                ImageOutcome::Complete
            }
            (Phase::Imaging, ImageSignal::Interrupt { interruption, parameters, callback }) => {
                effect(ImageEffect::Peer(match interruption { Interruption::Cancel => ImagePeer::Cancel(parameters), Interruption::Stop => ImagePeer::Stop }));
                effect(ImageEffect::Reply(ImageReply::Interrupted { interruption, callback, answer: InterruptionAnswer::Interrupted }));
                ImageOutcome::Pending
            }
            (Phase::Imaging, ImageSignal::Imaged { kind, failed }) => {
                self.image_pending = false;
                self.phase = Phase::Ready;
                let client = *self.client(kind);
                effect(ImageEffect::Reply(ImageReply::Finished {
                    kind,
                    failed,
                    callback: client.and_then(|client| client.callback),
                    channel: self.channel,
                    value: client.map_or(ReplyValue(0), |client| client.value),
                }));
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

    fn stopped(&mut self) -> ImageOutcome {
        if self.image_pending { return ImageOutcome::Pending; }
        self.phase = Phase::Disabled;
        ImageOutcome::Complete
    }

    fn client(&mut self, kind: ImageRequest) -> &mut Option<ImageClient> {
        match kind { ImageRequest::Image => &mut self.image, ImageRequest::Snapshot => &mut self.snapshot }
    }
}

/// Lens-control queries an image reply reports; each is set when its query answers 2.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensReport { pub first: bool, pub second: bool, pub third: bool }

/// How many image channels a reply may name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelCount(pub u8);

impl ImageChannel {
    /// The channel word is read as signed.
    pub const fn within(self, count: ChannelCount) -> bool {
        let channel = self.0 as i32;
        channel >= 0 && channel < count.0 as i32
    }
}

pub trait ReplyPorts: replies::Effects {
    fn channel_count(&mut self) -> ChannelCount;
    fn lens_report(&mut self) -> LensReport;
}

pub fn deliver(reply: ImageReply, ports: &mut impl ReplyPorts) {
    match reply {
        ImageReply::Interrupted { interruption, callback, answer } => replies::deliver(replies::Reply {
            target: ReplyTarget::Command(match interruption { Interruption::Cancel => CommandKind::Command15, Interruption::Stop => CommandKind::Command16 }),
            callback,
            status: CompletionStatus(match answer { InterruptionAnswer::Interrupted => 20, InterruptionAnswer::NothingPending => 21 }),
            values: xm1_saclac_interface::stock::parameters(&[]),
        }, ports),
        ImageReply::Finished { kind, failed, callback, channel, value } => {
            let lens = ports.lens_report();
            let value = if channel.within(ports.channel_count()) { value } else { crate::stock::NO_CHANNEL_VALUE };
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
