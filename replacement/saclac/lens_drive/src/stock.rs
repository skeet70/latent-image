#![forbid(unsafe_code)]

//! The lens children's X-M1 1.50 stock boundary: the event codes the row functions route
//! (m1 0xefd2fc, m35 0xf0194c, m2 0xefd624), the words their actions return, and the reply
//! entries and argument words each reply reaches.

use crate::ports::{DriveDetail, Reply, Status};
use crate::{bring_up, live, s1, Outcome};
use xm1_saclac_interface::lens::Setting;
use xm1_saclac_interface::{stock, ClientNotification, CommandKind, Completed, CompletionStatus, Message, Parameters, ReplyTarget};

/// An action's return word: 0 fault, 1 complete, 2 pending, 3 no entry.
pub const fn outcome_word(outcome: Outcome) -> u32 {
    match outcome {
        Outcome::Fault => 0,
        Outcome::Complete => 1,
        Outcome::Pending => 2,
        Outcome::Inactive => 3,
    }
}

pub const fn status(status: Status) -> CompletionStatus {
    CompletionStatus(match status {
        Status::Done => 0,
        Status::Progress => 18,
        Status::Accepted => 20,
        Status::Unneeded => 21,
        Status::Refused => -98,
        Status::Lens(value) => value,
    })
}

fn completed(code: u32, parameters: Parameters<'_>) -> Option<Completed<'_>> {
    let code = u8::try_from(code).ok()?;
    stock::completion(code).map(|completion| Completed { completion, parameters })
}

/// m1's event for a code `0xefd2fc` routes; `parameters` is the received payload.
pub fn live_event(code: u32, parameters: Parameters<'_>) -> live::Event<'_> {
    use live::{DriveOrigin, Event, Profile};
    match code {
        0x02 => Event::Profile(Profile::Primary),
        0x03 => Event::Profile(Profile::Secondary),
        0xa3 => Event::Setting(Setting::Request04),
        0xa4 => Event::Setting(Setting::Request06),
        0xa6 => Event::Setting(Setting::Request05),
        0xa5 => Event::PrepareDrive,
        0x0e => Event::Drive(DriveOrigin::Command),
        0xa7 => Event::Drive(DriveOrigin::Cancel),
        0x0f => Event::Cancel,
        0x10 => Event::Stop,
        0x1b => Event::Step,
        0x2c => Event::Select,
        0x2d => Event::Release,
        0x34..=0x39 | 0x3d => completed(code, parameters).map_or(Event::Other, Event::Completed),
        _ => Event::Other,
    }
}

/// m35's event for a code `0xf0194c` routes.
pub fn s1_event(code: u32, parameters: Parameters<'_>) -> s1::Event<'_> {
    use s1::Event;
    match code {
        0xa7 => Event::CancelDrive,
        0x0f => Event::Cancel,
        0x10 => Event::Stop,
        0x33 | 0x34 => completed(code, parameters).map_or(Event::Other, Event::Completed),
        _ => Event::Other,
    }
}

/// m2's event for a code `0xefd624` routes.
pub fn bring_up_event(code: u32, parameters: Parameters<'_>) -> bring_up::Event<'_> {
    use bring_up::Event;
    match code {
        0x04 => Event::BringUp,
        0x05 => Event::Stop,
        0x2f..=0x32 | 0x3f | 0x40 => completed(code, parameters).map_or(Event::Other, Event::Completed),
        _ => Event::Other,
    }
}

/// The m1 event for a queue message the live router forwards.
pub fn live_message(message: Message<'_>) -> live::Event<'_> {
    match message {
        Message::Command(command) => match command.kind {
            CommandKind::Command02 => live::Event::Profile(live::Profile::Primary),
            CommandKind::Command03 => live::Event::Profile(live::Profile::Secondary),
            CommandKind::LensDrive => live::Event::Drive(live::DriveOrigin::Command),
            CommandKind::Cancel => live::Event::Cancel,
            CommandKind::Command10 => live::Event::Stop,
            CommandKind::Command2c => live::Event::Select,
            CommandKind::Command2d => live::Event::Release,
            _ => live::Event::Other,
        },
        Message::Completed(done) => live::Event::Completed(done),
    }
}

pub const fn reply_target(reply: &Reply) -> ReplyTarget {
    match reply {
        Reply::Drive(_) => ReplyTarget::Command(CommandKind::LensDrive),
        Reply::Cancel(_) => ReplyTarget::Command(CommandKind::Cancel),
        Reply::Stop(_) => ReplyTarget::Command(CommandKind::Command10),
        Reply::Step { .. } => ReplyTarget::Notification(ClientNotification::Notification1b),
        Reply::Select { .. } => ReplyTarget::Command(CommandKind::Command2c),
        Reply::Release(_) => ReplyTarget::Command(CommandKind::Command2d),
        Reply::BringUp { .. } => ReplyTarget::Command(CommandKind::LensBringUp),
        Reply::LensStop { .. } => ReplyTarget::Command(CommandKind::LensStop),
    }
}

/// The reply entry a reply calls and its argument words, status first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyCall {
    pub entry: u32,
    pub arguments: [u32; 4],
    pub count: usize,
}

pub fn reply_call(reply: &Reply) -> ReplyCall {
    let word = |value: Status| status(value).0 as u32;
    let (entry, arguments, count) = match *reply {
        Reply::Drive(drive) => {
            let detail = match drive.detail {
                DriveDetail::Lens(lens) => u32::from(lens.0),
                DriveDetail::Reported(value) => value.0,
            };
            (0xefb104, [word(drive.status), drive.value.0, u32::from(drive.fourth_not_one), detail], 4)
        }
        Reply::Cancel(value) => (0xefb194, [word(value), 0, 0, 0], 1),
        Reply::Stop(value) => (0xefb1dc, [word(value), 0, 0, 0], 1),
        Reply::Step { status: value, lens } => (0xefb604, [word(value), u32::from(lens.0), 0, 0], 2),
        Reply::Select { status: value, lens, value: selected } => (0xefb654, [word(value), u32::from(lens.0), selected.0, 0], 3),
        Reply::Release(value) => (0xefb6ac, [word(value), 0, 0, 0], 1),
        Reply::BringUp { status: value, values } => (0xefa348, [word(value), values[0].0, values[1].0, values[2].0], 4),
        Reply::LensStop { status: value, value: stopped } => (0xefa3a8, [word(value), stopped.0, 0, 0], 2),
    };
    ReplyCall { entry, arguments, count }
}
