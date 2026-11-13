#![forbid(unsafe_code)]

//! The lens children behind the controllers' child contract.

use crate::{bring_up, live, s1, Outcome};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, LensControl};
use xm1_saclac_interface::{CommandKind, Message, RecordHandle};

#[derive(Default)]
pub struct LensChildren {
    bring_up: bring_up::BringUp,
    live: live::LiveLens,
    s1: s1::S1Lens,
}

impl LensChildren {
    pub const fn new() -> Self {
        Self { bring_up: bring_up::BringUp::new(), live: live::LiveLens::new(), s1: s1::S1Lens::new() }
    }

    /// `ChildRequest::LensBringUp`.
    pub fn bring_up(&mut self, message: Message<'_>, record: RecordHandle, ports: &mut impl bring_up::Ports) -> ChildOutcome {
        let event = match message {
            Message::Command(command) => match command.kind {
                CommandKind::LensBringUp => bring_up::Event::BringUp,
                CommandKind::LensStop => bring_up::Event::Stop,
                _ => bring_up::Event::Other,
            },
            Message::Completed(done) => bring_up::Event::Completed(done),
        };
        child_outcome(self.bring_up.handle(event, record, ports))
    }

    /// `ChildRequest::LensLive`.
    pub fn live(&mut self, event: ChildEvent<'_, LensControl>, ports: &mut impl live::Ports) -> ChildOutcome {
        let (event, record) = match event {
            ChildEvent::Control { control, record, .. } => (live_control(control), record),
            ChildEvent::Command { command, record } => (live_command(command.kind), record),
            ChildEvent::Completed { completed, record } => (live::Event::Completed(completed), record),
        };
        child_outcome(self.live.handle(event, record, ports))
    }

    /// `ChildRequest::LensS1`.
    pub fn s1(&mut self, event: ChildEvent<'_, LensControl>, ports: &mut impl s1::Ports) -> ChildOutcome {
        let (event, record) = match event {
            ChildEvent::Control { control: LensControl::CancelDrive, record, .. } => (s1::Event::CancelDrive, record),
            ChildEvent::Control { record, .. } => (s1::Event::Other, record),
            ChildEvent::Command { command, record } => (
                match command.kind {
                    CommandKind::Cancel => s1::Event::Cancel,
                    CommandKind::Command10 => s1::Event::Stop,
                    _ => s1::Event::Other,
                },
                record,
            ),
            ChildEvent::Completed { completed, record } => (s1::Event::Completed(completed), record),
        };
        child_outcome(self.s1.handle(event, record, ports))
    }
}

const fn live_control(control: LensControl) -> live::Event<'static> {
    match control {
        LensControl::PrepareDrive => live::Event::PrepareDrive,
        LensControl::CancelDrive => live::Event::Drive(live::DriveOrigin::Cancel),
        LensControl::Setting(setting) => live::Event::Setting(setting),
        LensControl::Step => live::Event::Step,
    }
}

const fn live_command(kind: CommandKind) -> live::Event<'static> {
    match kind {
        CommandKind::Command02 => live::Event::Profile(live::Profile::Primary),
        CommandKind::Command03 => live::Event::Profile(live::Profile::Secondary),
        CommandKind::LensDrive => live::Event::Drive(live::DriveOrigin::Command),
        CommandKind::Cancel => live::Event::Cancel,
        CommandKind::Command10 => live::Event::Stop,
        CommandKind::Command2c => live::Event::Select,
        CommandKind::Command2d => live::Event::Release,
        _ => live::Event::Other,
    }
}

const fn child_outcome(outcome: Outcome) -> ChildOutcome {
    match outcome {
        Outcome::Complete => ChildOutcome::Complete,
        Outcome::Pending => ChildOutcome::Pending,
        Outcome::Inactive => ChildOutcome::Inactive,
        Outcome::Fault => ChildOutcome::Fault,
    }
}
