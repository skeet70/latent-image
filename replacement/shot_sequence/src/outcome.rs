#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Complete,
    Pending,
    /// The relative lens move of a finish completed while its absolute move is outstanding.
    Progress,
    /// The event was not for this step; nothing changed.
    Unhandled,
    /// The step refused the event and has reported the fault.
    Fault,
    /// The capture stopped on an error status it has replied with.
    Aborted,
}

pub(crate) fn fault(ports: &mut impl crate::ports::Ports) -> Outcome {
    ports.fault();
    Outcome::Fault
}
