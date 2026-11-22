//! Machine 0, the playback source/mode status machine. Its 31 actions (table indices 2..=32 of
//! the action table at 0x01218ec0) read a status from one of the retained source dispatchers,
//! set the machine's state halfword and report through the retained notification, pending-event
//! and mode-work services. The value an action returns is the dispatcher's result.

use crate::DevicePacket;

/// The state halfword of machine 0 (0x014ea5ac).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    Idle = 0,
    Primary = 1,
    Posted = 2,
    Value = 3,
    Buffer = 4,
    Message = 5,
    Work = 6,
    Machine20 = 7,
}

/// The mode-work state written through 0x0121ca80.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum WorkState {
    Primary = 1,
    Second = 2,
    Third = 3,
}

/// The retained status sources an action reads its status from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// 0x0120bd54
    Parameter,
    /// 0x0120c268
    Navigation,
    /// 0x0120c7b4
    ParameterTransition,
    /// 0x0120da08
    Machine20,
    /// 0x0120dad8
    BufferMode,
    /// 0x0120e830
    Effect,
    /// 0x0120f120
    Machine34,
    /// 0x01212e48
    Message,
}

/// The retained services without arguments or result that the actions call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0120c650
    PublishRequestedMode,
    /// 0x0121cca4
    SubmitIndexedStorageBuffer,
    /// 0x0121cd84
    ApplySelectedRecordFlags,
    /// 0x0121cde4
    WriteErrorTrace,
    /// 0x0121ce80
    SourceErrorNotification,
    /// 0x0121d578
    NotifyPrimary,
    /// 0x0121d5ac
    ConsumeSecondary,
    /// 0x0121d5e8, packet 0x81f
    Notify081f,
    /// 0x0121d624, packet 0x819
    Notify0819,
    /// 0x0121d660, packet 0x84e
    Notify084e,
    /// 0x0121d694
    BindSelectedRecord,
    /// 0x0121d6f8, packet 0x850
    Notify0850,
    /// 0x0121d72c, packet 0x84f
    Notify084f,
    /// 0x0121d7b0
    CancelBufferStep,
    /// 0x0121d7f8
    CallbackReady,
    /// 0x0121e624
    PostPendingEvent,
    /// 0x0121e794
    ApplySelectedModeOptions,
    /// 0x0121e884
    ResetWorkspaces,
    /// 0x0121ea08, packet 0x821
    Notify0821,
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x0121eb94, packet 0x825
    Notify0825,
    /// 0x0121ebc8, packet 0x826
    Notify0826,
}

/// What machine 0 reaches outside itself.
pub trait Services {
    fn status(&mut self, source: Source) -> i32;
    fn call(&mut self, call: Call);
    fn set_state(&mut self, state: State);
    fn set_work_state(&mut self, state: WorkState);
    /// Through 0x72b680.
    fn notify_device(&mut self, packet: &DevicePacket);
    /// The word at 0x014ea694.
    fn mode_value(&self) -> u32;
    /// Clears the word at 0x014ea690.
    fn clear_buffer_step(&mut self);
}

/// The status codes the sources return that some action handles; any other value takes the
/// action's default arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Notify,
    Notify0825,
    Notify0826,
    StartPrimary,
    NotifyOnly,
    StartWork2,
    StartWork3,
    Error,
    Packet081e,
    Notify084e,
    Notify081f,
    Notify0819,
    Packet082d,
    NotifyPosted,
    Notify084f,
    Notify0850,
    Abort,
    Other,
}

impl Status {
    fn parse(raw: i32) -> Self {
        match raw {
            4 => Self::Notify,
            5 => Self::Notify0825,
            6 => Self::Notify0826,
            8 => Self::StartPrimary,
            9 => Self::NotifyOnly,
            10 => Self::StartWork2,
            11 => Self::StartWork3,
            12 => Self::Error,
            13 => Self::Packet081e,
            14 => Self::Notify084e,
            15 => Self::Notify081f,
            16 => Self::Notify0819,
            17 => Self::Packet082d,
            18 => Self::NotifyPosted,
            20 => Self::Notify084f,
            21 => Self::Notify0850,
            255 => Self::Abort,
            _ => Self::Other,
        }
    }
}

const ABORTED: i32 = 255;

fn read<S: Services>(services: &mut S, source: Source) -> Status {
    Status::parse(services.status(source))
}

fn start_primary<S: Services>(services: &mut S, state: State) -> i32 {
    services.set_state(state);
    services.call(Call::NotifyPrimary);
    services.set_work_state(WorkState::Primary);
    if !cfg!(playback_control_start_unpublished) {
        services.call(Call::PublishRequestedMode);
    }
    0
}

fn start_work<S: Services>(services: &mut S, work: WorkState) -> i32 {
    services.set_state(State::Work);
    services.set_work_state(work);
    services.call(Call::PublishRequestedMode);
    0
}

fn posted<S: Services>(services: &mut S, state: Option<State>, call: Option<Call>, result: i32) -> i32 {
    if let Some(state) = state {
        services.set_state(state);
    }
    if let Some(call) = call {
        services.call(call);
    }
    services.call(Call::PostPendingEvent);
    result
}

fn abort<S: Services>(services: &mut S) -> i32 {
    let state = if cfg!(playback_control_abort_keeps_state) { None } else { Some(State::Posted) };
    posted(services, state, None, ABORTED)
}

fn packet_081e<S: Services>(services: &mut S) {
    services.notify_device(&DevicePacket::new(0x81e, services.mode_value()));
}

fn packet_082d<S: Services>(services: &mut S) {
    services.notify_device(&DevicePacket::new(0x82d, 0));
}

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

/// Statuses 8, 12 and 255 of actions 6 to 9.
fn resolve_and_start<S: Services>(services: &mut S, status: Status) -> i32 {
    match status {
        Status::StartPrimary => start_primary(services, State::Primary),
        Status::Error => {
            services.set_state(State::Primary);
            services.call(Call::SourceErrorNotification);
            if !cfg!(playback_control_error_without_restart) {
                services.set_work_state(WorkState::Primary);
                services.call(Call::PublishRequestedMode);
            }
            0
        }
        Status::Abort => abort(services),
        _ => 0,
    }
}

/// Statuses 4, 8, 12 and 255 shared by the routing actions.
fn common<S: Services>(services: &mut S, status: Status) -> Option<i32> {
    Some(match status {
        Status::Notify => posted(services, Some(State::Posted), Some(Call::NotifyPrimary), 1),
        Status::StartPrimary => start_primary(services, State::Primary),
        Status::Error => posted(services, Some(State::Posted), Some(Call::SourceErrorNotification), 1),
        Status::Abort => abort(services),
        _ => return None,
    })
}

/// Statuses 5 and 6.
fn value_notifications<S: Services>(services: &mut S, status: Status) -> Option<i32> {
    Some(match status {
        Status::Notify0825 => posted(services, Some(State::Posted), Some(Call::Notify0825), 1),
        Status::Notify0826 => posted(services, Some(State::Posted), Some(Call::Notify0826), 1),
        _ => return None,
    })
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    services.call(Call::ResetWorkspaces);
    match read(services, Source::Parameter) {
        Status::StartPrimary => start_primary(services, State::Primary),
        Status::Notify => posted(services, Some(State::Posted), Some(Call::NotifyPrimary), 2),
        Status::Error => posted(services, Some(State::Posted), Some(Call::SourceErrorNotification), 1),
        Status::Abort => abort(services),
        _ => {
            services.call(Call::CallbackReady);
            0
        }
    }
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    match read(services, Source::Parameter) {
        Status::StartPrimary => start_primary(services, State::Primary),
        Status::Error => posted(services, Some(State::Primary), Some(Call::SourceErrorNotification), 1),
        Status::Abort => abort(services),
        _ => 0,
    }
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    services.status(Source::Effect);
    if read(services, Source::Navigation) != Status::StartPrimary {
        return 0;
    }
    if read(services, Source::ParameterTransition) == Status::Notify {
        posted(services, None, Some(Call::ConsumeSecondary), 3)
    } else {
        set(services, State::Primary, 0)
    }
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    let status = read(services, Source::Navigation);
    services.status(Source::Effect);
    resolve_and_start(services, status)
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    let status = read(services, Source::BufferMode);
    resolve_and_start(services, status)
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    let status = read(services, Source::Message);
    resolve_and_start(services, status)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    let status = read(services, Source::Machine34);
    resolve_and_start(services, status)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    match read(services, Source::Machine20) {
        Status::StartPrimary => start_primary(services, State::Primary),
        Status::Notify => posted(services, Some(State::Posted), Some(Call::NotifyPrimary), 1),
        _ => 0,
    }
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    services.status(Source::Parameter);
    services.call(Call::NotifyPrimary);
    if !cfg!(playback_control_buffer_step_kept) {
        services.clear_buffer_step();
    }
    services.call(Call::PostPendingEvent);
    2
}

pub fn action12<S: Services>(_services: &mut S) -> i32 {
    1
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    let result = navigation(services);
    if cfg!(playback_control_navigation_result_shifted) { result + 1 } else { result }
}

fn navigation<S: Services>(services: &mut S) -> i32 {
    let status = read(services, Source::Navigation);
    if let Some(result) = value_notifications(services, status) {
        return result;
    }
    match status {
        Status::StartWork2 => start_work(services, WorkState::Second),
        Status::StartWork3 => start_work(services, WorkState::Third),
        Status::NotifyOnly => {
            services.call(Call::NotifyPrimary);
            0
        }
        Status::Notify084f => value_call(services, Call::Notify084f),
        Status::Notify0850 => value_call(services, Call::Notify0850),
        status => common(services, status).unwrap_or(0),
    }
}

fn value_call<S: Services>(services: &mut S, call: Call) -> i32 {
    services.set_state(State::Value);
    services.call(call);
    0
}

/// Statuses 13, 9, 17 and 15 of actions 14 and 19, which leave the state unchanged except on 15.
fn buffer_tail<S: Services>(services: &mut S, status: Status) -> i32 {
    match status {
        Status::Packet081e => {
            packet_081e(services);
            0
        }
        Status::NotifyOnly => {
            services.call(Call::NotifyPrimary);
            0
        }
        Status::Packet082d => {
            packet_082d(services);
            0
        }
        Status::Notify081f => posted(services, Some(State::Posted), Some(Call::Notify081f), 15),
        _ => 0,
    }
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    let status = read(services, Source::BufferMode);
    common(services, status).unwrap_or_else(|| buffer_tail(services, status))
}

pub fn action15<S: Services>(services: &mut S) -> i32 {
    let status = read(services, Source::Message);
    if let Some(result) = common(services, status) {
        return result;
    }
    services.set_state(State::Message);
    if status == Status::NotifyOnly {
        services.call(Call::NotifyPrimary);
    }
    0
}

/// Statuses 9, 13, 17, 15 and 16 and the default of the machine-34 actions 16, 21, 22 and 32.
/// `packet_082d_state` is whether status 17 enters `Work`.
fn machine34_tail<S: Services>(services: &mut S, status: Status, packet_082d_state: bool) -> i32 {
    match status {
        Status::NotifyOnly => {
            services.set_state(State::Work);
            services.call(Call::NotifyPrimary);
            0
        }
        Status::Packet081e => {
            services.set_state(State::Work);
            packet_081e(services);
            0
        }
        Status::Packet082d => {
            if packet_082d_state {
                services.set_state(State::Work);
            }
            packet_082d(services);
            if cfg!(playback_control_packet_082d_result_dropped) { 0 } else { 17 }
        }
        Status::Notify081f => posted(services, Some(State::Posted), Some(Call::Notify081f), 15),
        Status::Notify0819 => posted(services, Some(State::Posted), Some(Call::Notify0819), 16),
        _ => set(services, State::Work, 0),
    }
}

pub fn action16<S: Services>(services: &mut S) -> i32 {
    let status = read(services, Source::Machine34);
    common(services, status).unwrap_or_else(|| machine34_tail(services, status, true))
}

pub fn action17<S: Services>(services: &mut S) -> i32 {
    services.call(Call::BindSelectedRecord);
    services.call(Call::ApplySelectedModeOptions);
    let status = read(services, Source::Navigation);
    if let Some(result) = value_notifications(services, status) {
        return result;
    }
    match status {
        Status::Notify => posted(services, None, Some(Call::NotifyPrimary), 1),
        Status::StartWork2 => start_work(services, WorkState::Second),
        Status::StartWork3 => start_work(services, WorkState::Third),
        Status::Error => posted(services, None, Some(Call::SourceErrorNotification), 1),
        Status::Abort => abort(services),
        Status::Notify084f => value_call(services, Call::Notify084f),
        Status::Notify0850 => value_call(services, Call::Notify0850),
        _ => set(services, State::Value, 0),
    }
}

pub fn action18<S: Services>(services: &mut S) -> i32 {
    services.call(Call::BindSelectedRecord);
    match read(services, Source::BufferMode) {
        Status::Notify => posted(services, None, Some(Call::NotifyPrimary), 1),
        Status::Notify081f => posted(services, None, Some(Call::Notify081f), 15),
        Status::Error => posted(services, None, Some(Call::SourceErrorNotification), 1),
        Status::Abort => abort(services),
        Status::Packet081e => {
            services.set_state(State::Buffer);
            packet_081e(services);
            0
        }
        _ => set(services, State::Buffer, 0),
    }
}

pub fn action19<S: Services>(services: &mut S) -> i32 {
    services.call(Call::BindSelectedRecord);
    let status = read(services, Source::BufferMode);
    common(services, status).unwrap_or_else(|| buffer_tail(services, status))
}

pub fn action20<S: Services>(services: &mut S) -> i32 {
    services.call(Call::BindSelectedRecord);
    services.call(Call::ApplySelectedModeOptions);
    match read(services, Source::Message) {
        Status::Notify => posted(services, None, Some(Call::NotifyPrimary), 1),
        Status::Error => posted(services, None, Some(Call::SourceErrorNotification), 1),
        Status::Abort => abort(services),
        _ => set(services, State::Message, 0),
    }
}

fn machine34_routing<S: Services>(services: &mut S, packet_082d_state: bool) -> i32 {
    services.call(Call::BindSelectedRecord);
    services.call(Call::CancelBufferStep);
    let status = read(services, Source::Machine34);
    common(services, status)
        .or_else(|| value_notifications(services, status))
        .unwrap_or_else(|| machine34_tail(services, status, packet_082d_state))
}

pub fn action21<S: Services>(services: &mut S) -> i32 {
    machine34_routing(services, false)
}

pub fn action22<S: Services>(services: &mut S) -> i32 {
    machine34_routing(services, true)
}

pub fn action23<S: Services>(services: &mut S) -> i32 {
    match read(services, Source::Machine20) {
        Status::Notify => posted(services, Some(State::Posted), Some(Call::NotifyPrimary), 1),
        Status::StartPrimary => start_primary(services, State::Primary),
        _ => set(services, State::Machine20, 0),
    }
}

fn single<S: Services>(services: &mut S, call: Call, post: bool) -> i32 {
    services.call(call);
    if post {
        services.call(Call::PostPendingEvent);
    }
    0
}

pub fn action24<S: Services>(services: &mut S) -> i32 {
    single(services, Call::SubmitIndexedStorageBuffer, false)
}

pub fn action25<S: Services>(services: &mut S) -> i32 {
    single(services, Call::ApplySelectedRecordFlags, false)
}

pub fn action26<S: Services>(services: &mut S) -> i32 {
    single(services, Call::ApplySelectedRecordFlags, true)
}

pub fn action27<S: Services>(services: &mut S) -> i32 {
    single(services, Call::WriteErrorTrace, false)
}

pub fn action28<S: Services>(services: &mut S) -> i32 {
    single(services, Call::WriteErrorTrace, true)
}

pub fn action29<S: Services>(services: &mut S) -> i32 {
    single(services, Call::Notify0821, false)
}

pub fn action30<S: Services>(services: &mut S) -> i32 {
    match read(services, Source::Parameter) {
        Status::Notify => posted(services, Some(State::Posted), Some(Call::NotifyPrimary), 2),
        status => common(services, status).unwrap_or(0),
    }
}

pub fn action31<S: Services>(services: &mut S) -> i32 {
    match read(services, Source::ParameterTransition) {
        Status::Notify => posted(services, Some(State::Idle), Some(Call::ConsumeSecondary), 3),
        Status::Error => posted(services, Some(State::Posted), Some(Call::SourceErrorNotification), 1),
        Status::Abort => abort(services),
        _ => 0,
    }
}

pub fn action32<S: Services>(services: &mut S) -> i32 {
    let status = read(services, Source::Machine34);
    if let Some(result) = common(services, status).or_else(|| value_notifications(services, status)) {
        return result;
    }
    match status {
        Status::Notify084e => {
            services.set_state(State::Work);
            services.call(Call::Notify084e);
            0
        }
        Status::NotifyPosted => posted(services, Some(State::Posted), Some(Call::NotifyPrimary), 1),
        status => machine34_tail(services, status, true),
    }
}
