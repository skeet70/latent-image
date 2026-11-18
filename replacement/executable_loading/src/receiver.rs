use crate::{Clock, Completion, CompletionOutcome, Producer, ProducerCompletion, Publication, ReceiverKernel, Retention};
use crate::protocol::{ProtocolError, Request, COMMAND_COUNT};
use crate::nand::dma_read::{Failure, Phase, PrimaryError};

pub struct Receiver { busy: bool, last_request: u32, completed: [Option<CompletionOutcome>; COMMAND_COUNT] }
impl Receiver {
    pub const fn new() -> Self { Self { busy: false, last_request: 0, completed: [None; COMMAND_COUNT] } }
    pub const fn busy(&self) -> bool { self.busy }

    pub fn receive<C: Clock, P: Producer, K: ReceiverKernel>(&mut self, operation: u16, envelope: &[u8], clock: &mut C, producer: &mut P, kernel: &mut K) -> Result<Completion, ProtocolError> {
        let request = Request::decode(operation, envelope)?;
        if self.busy { return Err(ProtocolError::RetainedOperation); }
        if request.id.word() <= self.last_request { return Err(ProtocolError::InvalidRequest); }
        self.last_request = request.id.word();
        self.busy = true;
        let mut outcome = if !request.live(clock.now()) {
            CompletionOutcome::Rejected(ProtocolError::Expired)
        } else if let Some(completed) = self.completed[request.kind.index()] {
            completed
        } else {
            match producer.execute(request) {
                ProducerCompletion::Loaded(proof) if request.kind.owns_executable() => CompletionOutcome::Loaded(proof.report()),
                ProducerCompletion::Skipped(report) if request.kind.owns_executable() => CompletionOutcome::Skipped(report),
                ProducerCompletion::Delegated if !request.kind.owns_executable() => CompletionOutcome::Delegated,
                ProducerCompletion::Failed { failure, report, retention } => CompletionOutcome::Failed { failure, report, retention },
                _ => CompletionOutcome::Rejected(ProtocolError::Producer(-1)),
            }
        };
        if !request.live(clock.now()) { outcome = expire(outcome); }
        let mut published = publication(request, outcome);
        match kernel.publish(request, published) {
            Err(ProtocolError::Expired) if matches!(outcome, CompletionOutcome::Loaded(_) | CompletionOutcome::Skipped(_) | CompletionOutcome::Delegated) => {
                outcome = expire(outcome);
                published = publication(request, outcome);
                kernel.publish(request, published)?;
            }
            result => result?,
        }
        self.busy = published.busy;
        if matches!(published.completion.outcome, CompletionOutcome::Loaded(_) | CompletionOutcome::Skipped(_) | CompletionOutcome::Delegated) {
            self.completed[request.kind.index()] = Some(published.completion.outcome);
        }
        Ok(published.completion)
    }
}

fn expire(outcome: CompletionOutcome) -> CompletionOutcome {
    match outcome {
        CompletionOutcome::Loaded(report) | CompletionOutcome::Skipped(report) => CompletionOutcome::Failed {
            failure: Failure::Primary { primary: PrimaryError::Deadline(Phase::Publication), cleanup: None },
            report, retention: Retention::Released,
        },
        CompletionOutcome::Delegated => CompletionOutcome::Rejected(ProtocolError::Expired),
        failure => failure,
    }
}

fn publication(request: Request, outcome: CompletionOutcome) -> Publication {
    Publication {
        completion: Completion { id: request.id, kind: request.kind, outcome },
        completed_latch: matches!(outcome, CompletionOutcome::Loaded(_) | CompletionOutcome::Skipped(_) | CompletionOutcome::Delegated),
        busy: matches!(outcome, CompletionOutcome::Failed { retention: Retention::HeldQuiescent | Retention::Unproved, .. }),
        callback_result: if matches!(outcome, CompletionOutcome::Loaded(_) | CompletionOutcome::Skipped(_) | CompletionOutcome::Delegated) {
            crate::CallbackResult::Succeeded
        } else { crate::CallbackResult::Failed },
    }
}

