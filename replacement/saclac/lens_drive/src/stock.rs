#![forbid(unsafe_code)]

//! The lens children's X-M1 1.50 stock boundary: the event codes the row functions route
//! (m1 0xefd2fc, m35 0xf0194c, m2 0xefd624), the words their actions return, and the reply
//! entries and argument words each reply reaches.

use crate::ports::{DriveDetail, DriveFourth, DrivePolicy, Probe, Probes, Reply, Status};
use crate::{bring_up, live, s1, Outcome};
use xm1_saclac_interface::lens::Setting;
use xm1_saclac_interface::{stock, ClientNotification, CommandKind, Completed, CompletionStatus, Parameters, ReplyTarget};

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

/// Configuration byte 0x88ef's drive-correction bits: 0xf0310c tests bit 3, 0xf0311c bit 1 and
/// 0xf0313c bit 2.
pub const fn drive_policy(byte: u8) -> DrivePolicy {
    DrivePolicy { always: byte & 8 != 0, resource_one: byte & 2 != 0, resource_zero: byte & 4 != 0 }
}

/// The reply flag word. 0xf14ecc, 0xf14f2c, 0xf15000 and 0xf28f4c OR bit 1, bit 0 and bit 2
/// for the forced probes 0x76d9a8, 0x76da38 and 0x76dba4.
pub const fn reply_flags(probes: Probes) -> u8 {
    let mut bits = 0;
    if matches!(probes.bit1, Probe::Forced) { bits |= 2; }
    if matches!(probes.bit0, Probe::Forced) { bits |= 1; }
    if matches!(probes.bit2, Probe::Forced) { bits |= 4; }
    bits
}

/// The step reply's flag word: 0xf14fa0 assigns bit 0 where the other replies OR it, so a forced
/// bit-0 probe clears bit 1.
pub const fn step_reply_flags(probes: Probes) -> u8 {
    let mut bits = if matches!(probes.bit1, Probe::Forced) { 2 } else { 0 };
    if matches!(probes.bit0, Probe::Forced) { bits = 1; }
    if matches!(probes.bit2, Probe::Forced) { bits |= 4; }
    bits
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
                DriveDetail::Lens(probes) => u32::from(reply_flags(probes)),
                DriveDetail::Reported(value) => value.0,
            };
            let fourth = match drive.fourth {
                DriveFourth::One => 0,
                DriveFourth::Carried(_) => 1,
            };
            (0xefb104, [word(drive.status), drive.value.0, fourth, detail], 4)
        }
        Reply::Cancel(value) => (0xefb194, [word(value), 0, 0, 0], 1),
        Reply::Stop(value) => (0xefb1dc, [word(value), 0, 0, 0], 1),
        Reply::Step { status: value, probes } => (0xefb604, [word(value), u32::from(step_reply_flags(probes)), 0, 0], 2),
        Reply::Select { status: value, probes, value: selected } => (0xefb654, [word(value), u32::from(reply_flags(probes)), selected.0, 0], 3),
        Reply::Release(value) => (0xefb6ac, [word(value), 0, 0, 0], 1),
        Reply::BringUp { status: value, values } => (0xefa348, [word(value), values[0].0, values[1].0, values[2].0], 4),
        Reply::LensStop { status: value, value: stopped } => (0xefa3a8, [word(value), stopped.0, 0, 0], 2),
    };
    ReplyCall { entry, arguments, count }
}
