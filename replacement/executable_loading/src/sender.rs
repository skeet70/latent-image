use crate::{Clock, Completion, CompletionOutcome, SenderKernel, QUEUE_CAPACITY_BYTES, QUEUE_DEPTH, QUEUE_ID};
use crate::protocol::{Callback, CallbackArgument, CommandKind, ProtocolError, Request, RequestId};
use xm1_application_nand::dma_read::Deadline;

pub struct Sender {
    next_id: Option<RequestId>,
    pending: [Option<Request>; QUEUE_DEPTH as usize],
    retained: bool,
}

impl Sender {
    pub fn new<K: SenderKernel>(kernel: &mut K) -> Result<Self, ProtocolError> {
        kernel.initialize_queue(QUEUE_ID, QUEUE_CAPACITY_BYTES, QUEUE_DEPTH)?;
        let sender = Self { next_id: RequestId::new(1), pending: [None; QUEUE_DEPTH as usize], retained: false };
        kernel.start_receiver()?;
        Ok(sender)
    }

    pub fn send<C: Clock, K: SenderKernel>(&mut self, kind: CommandKind, deadline: Deadline, callback: Callback, argument: CallbackArgument, clock: &mut C, kernel: &mut K) -> Result<Request, ProtocolError> {
        if self.retained { return Err(ProtocolError::RetainedOperation); }
        let slot = self.pending.iter().position(Option::is_none).ok_or(ProtocolError::Outstanding)?;
        let now = clock.now();
        if self.pending.iter().flatten().any(|request| !request.live(now)) {
            return Err(ProtocolError::RetainedOperation);
        }
        if now >= deadline.instant() { return Err(ProtocolError::Expired); }
        let id = self.next_id.ok_or(ProtocolError::InvalidRequest)?;
        let request = Request { id, kind, deadline, callback, argument };
        self.next_id = id.next();
        kernel.post(QUEUE_ID, kind.operation(), &request.encode(), deadline)?;
        self.pending[slot] = Some(request);
        Ok(request)
    }

    pub fn wait<C: Clock, K: SenderKernel>(&mut self, request: Request, clock: &mut C, kernel: &mut K) -> Result<Completion, ProtocolError> {
        let slot = self.pending.iter().position(|pending| *pending == Some(request)).ok_or(ProtocolError::InvalidRequest)?;
        loop {
            if !request.live(clock.now()) { return Err(ProtocolError::Expired); }
            if let Some(completion) = kernel.take_completion(request.id, request.kind) {
                if completion.id != request.id || completion.kind != request.kind {
                    return Err(ProtocolError::MissingCompletion);
                }
                self.retained |= matches!(completion.outcome,
                    CompletionOutcome::Failed { retention: crate::Retention::HeldQuiescent | crate::Retention::Unproved, .. });
                if !request.live(clock.now()) { return Err(ProtocolError::Expired); }
                self.pending[slot] = None;
                return Ok(completion);
            }
            kernel.wait(request.id, request.kind, request.deadline)?;
        }
    }

    pub fn pending_for_kind(&self, kind: CommandKind) -> Option<Request> {
        self.pending.iter().flatten().filter(|request| request.kind == kind).min_by_key(|request| request.id.word()).copied()
    }

    pub fn request_sync<C: Clock, K: SenderKernel>(&mut self, kind: CommandKind, deadline: Deadline, clock: &mut C, kernel: &mut K) -> Result<Completion, ProtocolError> {
        let request = self.send(kind, deadline, Callback::None, CallbackArgument(0), clock, kernel)?;
        self.wait(request, clock, kernel)
    }
}
