use super::actor_request::{InitializationWord, Operation, QueueRequest, RenderRequest, RequestError, RequestId, WaiterId};
use super::context::{ContextError, GraphicsContext, OutputBatch, OutputMode};
use super::publisher::{CloseState, ContextId, FrameId, PublisherError, SubmissionError};
use super::raster::RasterResources;
use super::runtime::{render_output, RenderError};
use super::SurfaceId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorError { NotOpen, AlreadyOpen, Busy, Closing, SequenceExhausted, Request(RequestError), Context(ContextError), Render(RenderError), Publisher(PublisherError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingReason { Presentation, Release }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    Accepted { request: RequestId, operation: Operation },
    Queued { request: RequestId, batch: OutputBatch },
    Pending { request: RequestId, reason: PendingReason },
    Closed { request: RequestId },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaiterStatus { Accepted, Closed, Error(ActorError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaiterDelivery { pub request: RequestId, pub waiters: [Option<WaiterId>; 6], pub status: WaiterStatus }
#[derive(Clone, Copy)]
enum Phase {
    Idle,
    Open,
    Rendering { request: RequestId, batch: OutputBatch, waiters: [Option<WaiterId>; 6] },
    Closing { request: RequestId, waiter: Option<WaiterId> },
    Closed,
}
pub struct OpenFailure { pub error: ActorError, pub context: GraphicsContext }
pub struct TaskActor {
    context: Option<GraphicsContext>,
    phase: Phase,
    next_request: core::num::NonZeroU64,
    deliveries: [Option<WaiterDelivery>; 2],
}
impl TaskActor {
    pub const fn new() -> Self {
        Self { context: None, phase: Phase::Idle, next_request: core::num::NonZeroU64::MIN, deliveries: [None; 2] }
    }
    fn reserve_request(&mut self) -> Result<RequestId, ActorError> {
        if self.deliveries.iter().any(Option::is_some) { return Err(ActorError::Busy); }
        let next = self.next_request.checked_add(1).ok_or(ActorError::SequenceExhausted)?;
        let request = RequestId(self.next_request);
        self.next_request = next;
        Ok(request)
    }
    pub fn open(&mut self, context: GraphicsContext, request: QueueRequest) -> Result<Event, OpenFailure> {
        let fail = |error, context| Err(OpenFailure { error, context });
        let QueueRequest::Open { initialization: InitializationWord(_), waiter } = request else {
            return fail(ActorError::Request(RequestError::DescriptorMismatch), context);
        };
        if !matches!(self.phase, Phase::Idle) { return fail(ActorError::AlreadyOpen, context); }
        let modes = [OutputMode::Panel, OutputMode::PanelAlternate, OutputMode::HalfPanel, OutputMode::Wide544, OutputMode::Hd720, OutputMode::Standard480];
        if !modes.into_iter().any(|mode| context.require_output(mode).is_ok()) {
            let error = match context.require_output(OutputMode::Panel) { Err(error) => error, Ok(_) => ContextError::MissingSurface };
            return fail(ActorError::Context(error), context);
        }
        let request = match self.reserve_request() { Ok(request) => request, Err(error) => return fail(error, context) };
        self.context = Some(context);
        self.phase = Phase::Open;
        self.deliveries[0] = delivery(request, one_waiter(waiter), WaiterStatus::Accepted);
        Ok(Event::Accepted { request, operation: Operation::Open })
    }
    pub fn render(&mut self, assets: &mut RasterResources<'_, '_, '_>, input: RenderRequest<'_>) -> Result<Event, ActorError> {
        self.render_request(assets, input).map(|(request, batch)| Event::Queued { request, batch })
    }
    pub fn queue_render(&mut self, assets: &mut RasterResources<'_, '_, '_>, input: RenderRequest<'_>) -> Result<OutputBatch, ActorError> {
        self.render_request(assets, input).map(|(_, batch)| batch)
    }
    fn render_request(&mut self, assets: &mut RasterResources<'_, '_, '_>, input: RenderRequest<'_>) -> Result<(RequestId, OutputBatch), ActorError> {
        match self.phase { Phase::Open => {}, Phase::Idle => return Err(ActorError::NotOpen), Phase::Closing { .. } | Phase::Closed => return Err(ActorError::Closing), Phase::Rendering { .. } => return Err(ActorError::Busy) }
        let request = self.reserve_request()?;
        let composition = input.composition;
        let result = render_output(self.context.as_mut().ok_or(ActorError::NotOpen)?, assets, composition.mode, composition.first, composition.second);
        match result {
            Ok(batch) => {
                self.phase = Phase::Rendering { request, batch, waiters: input.waiters };
                Ok((request, batch))
            }
            Err(error) => {
                let error = ActorError::Render(error);
                self.deliveries[0] = delivery(request, input.waiters, WaiterStatus::Error(error));
                Err(error)
            }
        }
    }
    pub fn submit<E>(&mut self, surface: SurfaceId, frame: FrameId, transport: impl FnOnce(SurfaceId, FrameId, &[u8]) -> Result<(), SubmissionError<E>>) -> Result<Result<(), SubmissionError<E>>, ActorError> {
        let Phase::Rendering { batch, .. } = self.phase else { return Err(ActorError::Busy); };
        if !batch.frames().into_iter().flatten().any(|member| member.surface == surface && member.frame == frame) { return Err(ActorError::Context(ContextError::UnknownBatch)); }
        self.context.as_mut().ok_or(ActorError::NotOpen)?.surface(surface).map_err(ActorError::Context)?.submit(frame, transport).map_err(ActorError::Publisher)
    }
    // Input is the scanout driver's frame-latched event, not its commit receipt.
    pub fn acknowledge(&mut self, surface: SurfaceId, frame: FrameId) -> Result<Event, ActorError> {
        let Phase::Rendering { request, waiters, .. } = self.phase else { return Err(ActorError::Busy); };
        let context = self.context.as_mut().ok_or(ActorError::NotOpen)?;
        context.acknowledge(surface, frame).map_err(ActorError::Context)?;
        if context.take_output_presented().is_some() {
            self.phase = Phase::Open;
            self.deliveries[0] = delivery(request, waiters, WaiterStatus::Accepted);
            Ok(Event::Accepted { request, operation: Operation::Render })
        } else { Ok(Event::Pending { request, reason: PendingReason::Presentation }) }
    }
    pub fn close(&mut self, input: QueueRequest) -> Result<Event, ActorError> {
        let QueueRequest::Close { waiter } = input else { return Err(ActorError::Request(RequestError::DescriptorMismatch)); };
        match self.phase { Phase::Open | Phase::Rendering { .. } => {}, Phase::Idle => return Err(ActorError::NotOpen), _ => return Err(ActorError::Closing) }
        let request = self.reserve_request()?;
        if let Phase::Rendering { request, waiters, .. } = self.phase {
            self.deliveries[0] = delivery(request, waiters, WaiterStatus::Error(ActorError::Closing));
        }
        self.context.as_mut().ok_or(ActorError::NotOpen)?.close();
        self.phase = Phase::Closing { request, waiter };
        self.close_progress()
    }
    // Input confirms that scanout has stopped all reads and pending submissions.
    pub fn acknowledge_detached(&mut self, surface: SurfaceId, context: ContextId) -> Result<Event, ActorError> {
        if !matches!(self.phase, Phase::Closing { .. }) { return Err(ActorError::Closing); }
        self.context.as_mut().ok_or(ActorError::NotOpen)?.acknowledge_detached(surface, context).map_err(ActorError::Context)?;
        self.close_progress()
    }
    fn close_progress(&mut self) -> Result<Event, ActorError> {
        let Phase::Closing { request, waiter } = self.phase else { return Err(ActorError::Closing); };
        if self.context.as_ref().ok_or(ActorError::NotOpen)?.close_state() == CloseState::Released {
            self.phase = Phase::Closed;
            let index = if self.deliveries[0].is_none() { 0 } else { 1 };
            self.deliveries[index] = delivery(request, one_waiter(waiter), WaiterStatus::Closed);
            Ok(Event::Closed { request })
        } else { Ok(Event::Pending { request, reason: PendingReason::Release }) }
    }
    pub fn take_waiters(&mut self) -> Option<WaiterDelivery> {
        let delivery = self.deliveries[0].take();
        self.deliveries[0] = self.deliveries[1].take();
        delivery
    }
    pub fn take_released_context(&mut self) -> Result<GraphicsContext, ActorError> {
        if !matches!(self.phase, Phase::Closed) || self.deliveries.iter().any(Option::is_some) { return Err(ActorError::Busy); }
        let context = self.context.take().ok_or(ActorError::NotOpen)?;
        self.phase = Phase::Idle;
        Ok(context)
    }
}
impl Default for TaskActor { fn default() -> Self { Self::new() } }
const fn one_waiter(waiter: Option<WaiterId>) -> [Option<WaiterId>; 6] { [waiter, None, None, None, None, None] }
fn delivery(request: RequestId, waiters: [Option<WaiterId>; 6], status: WaiterStatus) -> Option<WaiterDelivery> {
    waiters.iter().any(Option::is_some).then_some(WaiterDelivery { request, waiters, status })
}
