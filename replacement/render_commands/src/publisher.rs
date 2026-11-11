use super::SurfaceId;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContextId(core::num::NonZeroU64);
impl ContextId {
    pub const fn new(value: u64) -> Option<Self> {
        match core::num::NonZeroU64::new(value) { Some(value) => Some(Self(value)), None => None }
    }
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FrameId { context: ContextId, sequence: u64 }
impl FrameId {
    pub const fn sequence(self) -> u64 { self.sequence }
    pub const fn context(self) -> ContextId { self.context }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferState { Free, Drawing(FrameId), Queued(FrameId), Scanning(FrameId) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublisherError { BufferLength, Busy, Closed, UnknownFrame, WrongState, SequenceExhausted, StaleAcknowledgment, NotDetached }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainState { Pending, Drained }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseState { Pending, Released }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramePresented { pub surface: SurfaceId, pub frame: FrameId }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmissionError<E> { Rejected(E), Uncertain(E) }

pub struct BufferAdmissionError { buffers: [&'static mut [u8]; 2] }
impl BufferAdmissionError {
    pub const fn error(&self) -> PublisherError { PublisherError::BufferLength }
    pub fn into_buffers(self) -> [&'static mut [u8]; 2] { self.buffers }
}
impl core::fmt::Debug for BufferAdmissionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("BufferAdmissionError").field("error", &self.error()).finish()
    }
}

struct Buffer { pixels: &'static mut [u8], state: BufferState, submitted: bool }

pub struct Publisher {
    surface: SurfaceId,
    context: ContextId,
    buffers: [Buffer; 2],
    next_sequence: u64,
    last_presented: Option<FrameId>,
    closed: bool,
}

impl Publisher {
    // Context identities must not be reused while a scanout event can remain in flight.
    pub fn new(context: ContextId, surface: SurfaceId, first: &'static mut [u8], second: &'static mut [u8]) -> Result<Self, BufferAdmissionError> {
        if first.len() != surface.byte_len() || second.len() != surface.byte_len() { return Err(BufferAdmissionError { buffers: [first, second] }); }
        Ok(Self { surface, context, buffers: [Buffer { pixels: first, state: BufferState::Free, submitted: false }, Buffer { pixels: second, state: BufferState::Free, submitted: false }], next_sequence: 1, last_presented: None, closed: false })
    }

    pub fn begin(&mut self) -> Result<FrameId, PublisherError> {
        if self.closed { return Err(PublisherError::Closed); }
        let index = self.buffers.iter().position(|buffer| buffer.state == BufferState::Free).ok_or(PublisherError::Busy)?;
        let next = self.next_sequence.checked_add(1).ok_or(PublisherError::SequenceExhausted)?;
        let frame = FrameId { context: self.context, sequence: self.next_sequence };
        self.next_sequence = next;
        self.buffers[index].state = BufferState::Drawing(frame);
        Ok(frame)
    }

    pub fn drawing_pixels(&mut self, frame: FrameId) -> Result<&mut [u8], PublisherError> {
        let buffer = self.buffers.iter_mut().find(|buffer| buffer.state == BufferState::Drawing(frame)).ok_or(PublisherError::WrongState)?;
        Ok(buffer.pixels)
    }

    pub fn publish(&mut self, frame: FrameId) -> Result<FrameId, PublisherError> {
        self.validate_publish(frame)?;
        let buffer = self.buffers.iter_mut().find(|buffer| buffer.state == BufferState::Drawing(frame)).ok_or(PublisherError::WrongState)?;
        buffer.state = BufferState::Queued(frame);
        Ok(frame)
    }

    pub(crate) fn validate_publish(&self, frame: FrameId) -> Result<(), PublisherError> {
        if self.closed { return Err(PublisherError::Closed); }
        if self.buffers.iter().any(|buffer| matches!(buffer.state, BufferState::Drawing(earlier) if earlier < frame)) { return Err(PublisherError::WrongState); }
        if !self.buffers.iter().any(|buffer| buffer.state == BufferState::Drawing(frame)) { return Err(PublisherError::WrongState); }
        Ok(())
    }

    pub fn cancel(&mut self, frame: FrameId) -> Result<(), PublisherError> {
        self.validate_cancel(frame)?;
        let buffer = self.buffers.iter_mut().find(|buffer| matches!(buffer.state, BufferState::Drawing(id) | BufferState::Queued(id) if id == frame)).ok_or(PublisherError::WrongState)?;
        buffer.state = BufferState::Free;
        Ok(())
    }

    pub(crate) fn validate_cancel(&self, frame: FrameId) -> Result<(), PublisherError> {
        let buffer = self.buffers.iter().find(|buffer| matches!(buffer.state, BufferState::Drawing(id) | BufferState::Queued(id) if id == frame)).ok_or(PublisherError::WrongState)?;
        if buffer.submitted { return Err(PublisherError::Busy); }
        Ok(())
    }

    pub fn queued(&self) -> Option<FrameId> {
        self.buffers.iter().filter_map(|buffer| match buffer.state { BufferState::Queued(frame) if !buffer.submitted => Some(frame), _ => None }).min()
    }

    pub fn submit<E>(&mut self, frame: FrameId, submit: impl FnOnce(SurfaceId, FrameId, &[u8]) -> Result<(), SubmissionError<E>>) -> Result<Result<(), SubmissionError<E>>, PublisherError> {
        if self.buffers.iter().any(|buffer| matches!(buffer.state, BufferState::Queued(earlier) if earlier < frame) && !buffer.submitted) { return Err(PublisherError::WrongState); }
        let buffer = self.buffers.iter_mut().find(|buffer| buffer.state == BufferState::Queued(frame)).ok_or(PublisherError::WrongState)?;
        if buffer.submitted || self.closed { return Err(if self.closed { PublisherError::Closed } else { PublisherError::Busy }); }
        buffer.submitted = true;
        let result = submit(self.surface, frame, buffer.pixels);
        if matches!(&result, Err(SubmissionError::Rejected(_))) { buffer.submitted = false; }
        Ok(result)
    }

    // Call only for the scanout driver's corresponding frame-latched event.
    pub fn acknowledge(&mut self, frame: FrameId) -> Result<FramePresented, PublisherError> {
        if frame.context != self.context { return Err(PublisherError::UnknownFrame); }
        if self.last_presented.is_some_and(|last| frame <= last) { return Err(PublisherError::StaleAcknowledgment); }
        let index = self.buffers.iter().position(|buffer| buffer.state == BufferState::Queued(frame) && buffer.submitted).ok_or(PublisherError::UnknownFrame)?;
        for buffer in &mut self.buffers {
            if matches!(buffer.state, BufferState::Scanning(_))
                || (buffer.submitted && matches!(buffer.state, BufferState::Queued(earlier) if earlier < frame)) {
                buffer.state = BufferState::Free;
                buffer.submitted = false;
            }
        }
        self.buffers[index].state = BufferState::Scanning(frame);
        self.last_presented = Some(frame);
        Ok(FramePresented { surface: self.surface, frame })
    }

    pub fn drain_state(&self) -> DrainState {
        if self.buffers.iter().any(|buffer| matches!(buffer.state, BufferState::Drawing(_) | BufferState::Queued(_))) { DrainState::Pending } else { DrainState::Drained }
    }

    pub fn close(&mut self) -> CloseState {
        self.closed = true;
        for buffer in &mut self.buffers {
            if matches!(buffer.state, BufferState::Drawing(_) | BufferState::Queued(_)) && !buffer.submitted { buffer.state = BufferState::Free; }
        }
        self.close_state()
    }

    pub fn close_state(&self) -> CloseState {
        if self.closed && self.buffers.iter().all(|buffer| buffer.state == BufferState::Free) { CloseState::Released } else { CloseState::Pending }
    }

    // Call after scanout has stopped reading this surface, including any pending submission.
    pub fn acknowledge_detached(&mut self, context: ContextId) -> Result<CloseState, PublisherError> {
        if !self.closed || context != self.context { return Err(PublisherError::NotDetached); }
        for buffer in &mut self.buffers { buffer.state = BufferState::Free; buffer.submitted = false; }
        Ok(self.close_state())
    }

    pub fn release(self) -> Result<[&'static mut [u8]; 2], Self> {
        if self.close_state() != CloseState::Released { return Err(self); }
        let [first, second] = self.buffers;
        Ok([first.pixels, second.pixels])
    }

    pub fn states(&self) -> [BufferState; 2] { [self.buffers[0].state, self.buffers[1].state] }
    pub const fn surface(&self) -> SurfaceId { self.surface }
    pub const fn context(&self) -> ContextId { self.context }
}
