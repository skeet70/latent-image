#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum State {
    Stopped = 1,
    Idle,
    CommandTransmit,
    CommandDelay,
    AcknowledgmentTransmit,
    CompletionDelay,
    CommandRetryDelay,
    CommandRetryTransmit,
    AcknowledgmentRetryDelay,
    AcknowledgmentRetryTransmit,
    RecoveryDelay,
    RecoveryTransmit,
    Failed,
    PageWriteTransmit,
    PageReadTransmit,
    PageAcceptance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Event {
    Enter,
    Initialize,
    Stop,
    Request,
    WireCompleted,
    TimerExpired,
    Busy,
    Accepted,
    Invalid,
    Recover,
    Exhausted,
    WritePage,
    ReadPage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Handler {
    Noop,
    Unexpected,
    EnterStopped,
    EnterIdle,
    Register,
    SelectRequest,
    TransmitCommand,
    ValidateCommand,
    CommandDelay,
    TransmitAcknowledgment,
    ValidateAcknowledgment,
    CompletionDelay,
    RetryDelay,
    RecoveryDelay,
    TransmitRecovery,
    ValidateRecovery,
    Fail,
    TransmitWrite,
    CompleteWrite,
    TransmitRead,
    CompleteRead,
}

#[derive(Clone, Copy)]
pub(crate) struct Transition {
    pub handler: Handler,
    pub next: Option<State>,
}

impl State {
    pub(crate) const fn transition(self, event: Event) -> Transition {
        use Event::*;
        use Handler as H;
        use State as S;
        let (handler, next) = match (self, event) {
            (S::Stopped, Enter) => (H::EnterStopped, None),
            (S::Stopped, Initialize) => (H::Register, Some(S::Idle)),
            (S::Stopped, Stop | Request) => (H::Noop, None),
            (S::Failed, Enter) => (H::Fail, Some(S::Stopped)),
            (S::Failed, _) => (H::Unexpected, None),
            (_, Stop) => (H::Noop, Some(S::Stopped)),
            (S::Idle, Enter) => (H::EnterIdle, None),
            (S::Idle, Request) => (H::SelectRequest, Some(S::CommandTransmit)),
            (S::CompletionDelay, TimerExpired) => (H::SelectRequest, Some(S::CommandTransmit)),
            (S::Idle | S::CompletionDelay, WritePage) => (H::Noop, Some(S::PageWriteTransmit)),
            (S::Idle | S::CompletionDelay, ReadPage) => (H::Noop, Some(S::PageReadTransmit)),
            (S::CommandTransmit | S::CommandRetryTransmit, Enter) => (H::TransmitCommand, None),
            (S::CommandTransmit, WireCompleted) => (H::ValidateCommand, Some(S::CommandDelay)),
            (S::CommandTransmit, Invalid) => (H::Noop, Some(S::AcknowledgmentRetryDelay)),
            (S::CommandDelay, Enter) => (H::CommandDelay, None),
            (S::CommandDelay, TimerExpired) => (H::Noop, Some(S::AcknowledgmentTransmit)),
            (S::AcknowledgmentTransmit | S::AcknowledgmentRetryTransmit, Enter) => (H::TransmitAcknowledgment, None),
            (S::AcknowledgmentTransmit, WireCompleted) => (H::ValidateAcknowledgment, Some(S::CompletionDelay)),
            (S::AcknowledgmentTransmit, Busy) => (H::Noop, Some(S::CommandDelay)),
            (S::AcknowledgmentTransmit, Invalid) => (H::Noop, Some(S::CommandRetryDelay)),
            (S::CompletionDelay, Enter) => (H::CompletionDelay, None),
            (S::CompletionDelay, Accepted) => (H::Noop, Some(S::Idle)),
            (S::CommandRetryDelay | S::AcknowledgmentRetryDelay, Enter) => (H::RetryDelay, None),
            (S::CommandRetryDelay, TimerExpired) => (H::Noop, Some(S::CommandRetryTransmit)),
            (S::AcknowledgmentRetryDelay, TimerExpired) => (H::Noop, Some(S::AcknowledgmentRetryTransmit)),
            (S::CommandRetryTransmit | S::AcknowledgmentRetryTransmit, WireCompleted) => (H::Noop, Some(S::RecoveryDelay)),
            (S::CommandTransmit | S::AcknowledgmentTransmit | S::RecoveryTransmit, Recover) => (H::Noop, Some(S::RecoveryDelay)),
            (S::RecoveryDelay, Enter) => (H::RecoveryDelay, None),
            (S::RecoveryDelay, TimerExpired) => (H::Noop, Some(S::RecoveryTransmit)),
            (S::CommandTransmit | S::AcknowledgmentTransmit | S::RecoveryDelay, Exhausted) => (H::Noop, Some(S::Failed)),
            (S::RecoveryTransmit, Enter) => (H::TransmitRecovery, None),
            (S::RecoveryTransmit, WireCompleted) => (H::ValidateRecovery, Some(S::CompletionDelay)),
            (S::PageWriteTransmit, Enter) => (H::TransmitWrite, None),
            (S::PageWriteTransmit, WireCompleted) => (H::CompleteWrite, Some(S::PageAcceptance)),
            (S::PageReadTransmit, Enter) => (H::TransmitRead, None),
            (S::PageReadTransmit, WireCompleted) => (H::CompleteRead, Some(S::PageAcceptance)),
            (S::PageAcceptance, Enter) => (H::Noop, None),
            (S::PageAcceptance, Accepted) => (H::Noop, Some(S::CompletionDelay)),
            (S::CommandTransmit | S::CommandDelay | S::AcknowledgmentTransmit | S::CompletionDelay | S::CommandRetryDelay | S::CommandRetryTransmit | S::AcknowledgmentRetryDelay | S::AcknowledgmentRetryTransmit | S::RecoveryDelay | S::RecoveryTransmit, Request) => (H::Noop, None),
            _ => (H::Unexpected, None),
        };
        Transition { handler, next }
    }
}
