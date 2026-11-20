use crate::{Clock, Completion, CompletionOutcome, SenderKernel, QUEUE_ID, QUEUE_MESSAGE_BYTES, QUEUE_MESSAGES};
use crate::protocol::COMMAND_COUNT;
use crate::protocol::{Callback, CallbackArgument, CommandKind, ProtocolError, Request, RequestId};
use crate::nand::dma_read::Deadline;

pub struct Sender {
    next_id: Option<RequestId>,
    /// At most one outstanding request per command kind.
    pending: [Option<Request>; COMMAND_COUNT],
    retained: bool,
}

impl Sender {
    pub fn new<K: SenderKernel>(kernel: &mut K) -> Result<Self, ProtocolError> {
        kernel.initialize_queue(QUEUE_ID, QUEUE_MESSAGES, QUEUE_MESSAGE_BYTES)?;
        let sender = Self { next_id: RequestId::new(1), pending: [None; COMMAND_COUNT], retained: false };
        kernel.start_receiver()?;
        Ok(sender)
    }

    pub fn send<C: Clock, K: SenderKernel>(&mut self, kind: CommandKind, deadline: Deadline, callback: Callback, argument: CallbackArgument, clock: &mut C, kernel: &mut K) -> Result<Request, ProtocolError> {
        self.reap_completed(kernel);
        if self.retained { return Err(ProtocolError::RetainedOperation); }
        if self.pending.iter().flatten().any(|request| request.kind == kind) {
            return Err(ProtocolError::Outstanding);
        }
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

    fn reap_completed<K: SenderKernel>(&mut self, kernel: &mut K) {
        for slot in &mut self.pending {
            let Some(request) = *slot else { continue; };
            let Some(completion) = kernel.peek_completion(request.id, request.kind) else { continue; };
            self.retained |= matches!(completion.outcome,
                CompletionOutcome::Failed { retention: crate::Retention::HeldQuiescent | crate::Retention::Unproved, .. });
            *slot = None;
        }
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
