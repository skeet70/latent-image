//! Which stock service, if any, the pender uses to wake an executor's host task.

use crate::ids::Domain;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakePath {
    /// The host task is the caller and checks `pending` before it sleeps.
    None,
    /// Root `+0x2c`, which needs task context with CPSR.I clear.
    Task,
    /// Root `+0x30`, which needs a nonzero context word with CPSR.I clear.
    Handler,
    /// No service accepts this caller; the wake waits for the host task's next timeout.
    Unavailable,
}

#[derive(Clone, Copy, Debug)]
pub struct Caller {
    pub domain: Domain,
    pub handler_context: bool,
    pub irq_masked: bool,
    pub in_host_poll_of: Option<Domain>,
}

pub fn wake_path(target: Domain, caller: Caller) -> WakePath {
    if caller.handler_context {
        return if caller.irq_masked { WakePath::Unavailable } else { WakePath::Handler };
    }
    if caller.domain == target && caller.in_host_poll_of == Some(target) {
        return WakePath::None;
    }
    if caller.irq_masked {
        WakePath::Unavailable
    } else {
        WakePath::Task
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caller(domain: Domain, handler_context: bool, irq_masked: bool, in_host_poll_of: Option<Domain>) -> Caller {
        Caller { domain, handler_context, irq_masked, in_host_poll_of }
    }

    #[test]
    fn the_host_task_wakes_itself_without_a_service() {
        assert_eq!(wake_path(Domain::A, caller(Domain::A, false, true, Some(Domain::A))), WakePath::None);
        assert_eq!(wake_path(Domain::A, caller(Domain::A, false, false, Some(Domain::A))), WakePath::None);
    }

    #[test]
    fn other_tasks_use_the_task_wakeup() {
        assert_eq!(wake_path(Domain::B, caller(Domain::A, false, false, Some(Domain::A))), WakePath::Task);
        assert_eq!(wake_path(Domain::A, caller(Domain::A, false, false, None)), WakePath::Task);
    }

    #[test]
    fn handlers_use_the_handler_wakeup() {
        assert_eq!(wake_path(Domain::A, caller(Domain::A, true, false, Some(Domain::A))), WakePath::Handler);
    }

    #[test]
    fn masked_callers_have_no_service() {
        assert_eq!(wake_path(Domain::B, caller(Domain::A, false, true, None)), WakePath::Unavailable);
        assert_eq!(wake_path(Domain::A, caller(Domain::A, true, true, None)), WakePath::Unavailable);
    }
}
