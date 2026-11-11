use crate::shutdown::{CompletionSource, KernelStatus, ShutdownRtos};
use crate::shutdown_requests::{self, OwnedCompletionAddress, Task8Readiness};

pub struct CompletionRequests<'a> {
    pub task8_readiness: Task8Readiness,
    pub task8_completion: OwnedCompletionAddress,
    pub calendar_completion: OwnedCompletionAddress,
    pub task8_payload: &'a mut [u8; 16],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FollowOnObservation {
    pub optional: Option<Result<(), KernelStatus>>,
    pub task8: Option<Result<(), KernelStatus>>,
    pub calendar: Option<Result<(), KernelStatus>>,
}
impl FollowOnObservation {
    pub const NONE: Self = Self { optional: None, task8: None, calendar: None };
}
impl CompletionRequests<'_> {
    pub fn submit(&mut self, source: CompletionSource, rtos: &mut impl ShutdownRtos) -> FollowOnObservation {
        match source {
            CompletionSource::Settings => FollowOnObservation {
                optional: Some(shutdown_requests::optional_shutdown(rtos)),
                task8: Some(shutdown_requests::task8_shutdown(rtos, self.task8_readiness, self.task8_completion, self.task8_payload)),
                calendar: None,
            },
            CompletionSource::Optional => FollowOnObservation {
                optional: None,
                task8: None,
                calendar: Some(shutdown_requests::calendar_shutdown(rtos, self.calendar_completion)),
            },
            _ => FollowOnObservation::NONE,
        }
    }
}
