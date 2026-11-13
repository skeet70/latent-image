#![forbid(unsafe_code)]

use xm1_saclac_interface::{ClientCallback, Parameters};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageEffect<'a> {
    Initialize,
    PrepareGeometry,
    Request { kind: ImageRequest, parameters: Parameters<'a> },
    Cancel(Parameters<'a>),
    Stop,
    Interrupted { interruption: Interruption, callback: Option<ClientCallback>, answer: InterruptionAnswer },
    Finished { kind: ImageRequest, failed: bool, callback: Option<ClientCallback>, channel: ImageChannel, value: ReplyValue },
}

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
            (Phase::Disabled, ImageSignal::Initialize) => { effect(ImageEffect::Initialize); ImageOutcome::Complete }
            (Phase::Disabled, ImageSignal::Enable) => {
                effect(ImageEffect::Initialize);
                self.phase = Phase::Ready;
                ImageOutcome::Complete
            }
            (Phase::Disabled, ImageSignal::Stop) => ImageOutcome::Complete,
            (Phase::Ready, ImageSignal::Stop) => { self.phase = Phase::Disabled; ImageOutcome::Complete }
            (Phase::Imaging, ImageSignal::Stop) => {
                effect(ImageEffect::Stop);
                self.phase = Phase::Stopping;
                ImageOutcome::Pending
            }
            (Phase::Disabled | Phase::Ready | Phase::Imaging, ImageSignal::Run) => ImageOutcome::Idle,
            (Phase::Ready, ImageSignal::Request { kind, parameters, .. }) => {
                effect(ImageEffect::PrepareGeometry);
                self.image_pending = true;
                effect(ImageEffect::Request { kind, parameters });
                self.phase = Phase::Imaging;
                ImageOutcome::Pending
            }
            (Phase::Ready, ImageSignal::Interrupt { interruption, callback, .. }) => {
                effect(ImageEffect::Interrupted { interruption, callback, answer: InterruptionAnswer::NothingPending });
                ImageOutcome::Complete
            }
            (Phase::Imaging, ImageSignal::Interrupt { interruption, parameters, callback }) => {
                effect(match interruption { Interruption::Cancel => ImageEffect::Cancel(parameters), Interruption::Stop => ImageEffect::Stop });
                effect(ImageEffect::Interrupted { interruption, callback, answer: InterruptionAnswer::Interrupted });
                ImageOutcome::Pending
            }
            (Phase::Imaging, ImageSignal::Imaged { kind, failed }) => {
                self.image_pending = false;
                self.phase = Phase::Ready;
                let client = *self.client(kind);
                effect(ImageEffect::Finished {
                    kind,
                    failed,
                    callback: client.and_then(|client| client.callback),
                    channel: self.channel,
                    value: client.map_or(ReplyValue(0), |client| client.value),
                });
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
