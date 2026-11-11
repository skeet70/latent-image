#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Opcode(pub u16);
impl Opcode { pub const fn event(self) -> Event { Event(u32::from_le_bytes([self.0 as u8, (self.0 >> 8) as u8, 0, 0]).wrapping_sub(0x4200)) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Event(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Job(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobState(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobPolicy { ReleaseAfterMessage, RetainUntilCompletion }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiveStatus(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Synchronization { pub first: bool, pub position: i16, pub mode: u32 }

pub trait Task50Io {
    fn opcode(&self) -> Opcode;
    fn job_policy(&self, job: Job) -> JobPolicy;
    fn job_state(&self, job: Job) -> JobState;
    fn job_active(&self, job: Job) -> bool;
    fn set_job_active(&mut self, job: Job, active: bool);
    fn job_pending(&self, job: Job) -> bool;
    fn set_job_pending(&mut self, job: Job, pending: bool);
    fn active_job(&self) -> Job;
    fn set_active_job(&mut self, job: Job);
    fn copy_received_to_job(&mut self, job: Job);
    fn receive_failure(&mut self, status: ReceiveStatus);
    fn synchronization(&self, event: Event) -> Option<Synchronization>;
    fn synchronize_focus(&mut self, synchronization: Option<Synchronization>);
    fn iris_in_progress(&self) -> bool;
    fn synchronize_iris(&mut self);
    fn notify_focus_status(&mut self);
    fn notify_position(&mut self);
    fn notify_iris_status(&mut self);
    fn dispatch(&mut self, event: Event, argument: JobState);
}
pub fn release_job(io: &mut impl Task50Io, job: Job) {
    if io.job_pending(job) { io.set_job_pending(job, false); }
    else {
        io.set_job_active(job, false);
        if io.active_job() == job { io.set_active_job(Job(0x21)); }
    }
}
pub fn before_receive(io: &mut impl Task50Io) {
    let previous = Job(i32::from(io.opcode().0) - 0x4200);
    if previous.0 <= 0x20 && io.job_policy(previous) == JobPolicy::ReleaseAfterMessage {
        release_job(io, previous);
        if io.active_job() == previous { io.set_active_job(Job(0x50)); }
    }
}
pub fn consume(io: &mut impl Task50Io, received: Result<(), ReceiveStatus>) {
    if let Err(status) = received { io.receive_failure(status); }
    let event = io.opcode().event();
    let argument = if event.0 <= 0x20 {
        let job = Job(event.0 as i32);
        if !io.job_active(job) {
            io.copy_received_to_job(job);
            io.set_job_active(job, true);
            let argument = io.job_state(job);
            if argument != JobState(-1) && io.active_job() == Job(0x21) { io.set_active_job(job); }
            argument
        } else {
            if io.job_policy(job) == JobPolicy::RetainUntilCompletion { io.set_job_pending(job, true); }
            JobState(-1)
        }
    } else { io.job_state(io.active_job()) };
    let focus_event = event.0.wrapping_sub(0x31) <= 0xb;
    let iris_event = event.0.wrapping_sub(0x3f) <= 2;
    if focus_event { io.synchronize_focus(io.synchronization(event)); }
    if iris_event && io.iris_in_progress() { io.synchronize_iris(); }
    if event == Event(0x4c) || focus_event { io.notify_focus_status(); io.notify_position(); }
    if iris_event || event == Event(0x4c) { io.notify_iris_status(); }
    io.dispatch(event, argument);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Adjustment { pub position: u32, pub fraction: u16 }
pub fn adjusted_position(position: u32, flags: u8, count: i32) -> Adjustment {
    let mut value = position;
    let mut adjustment = u32::from(flags & 0x3f);
    if flags & 0x80 == 0 {
        if flags & 0x40 != 0 { value = value.wrapping_add(1); }
        if (value as i32) < count.wrapping_sub(1) { adjustment <<= 10; }
        else { value = count.wrapping_sub(1) as u32; adjustment = 0; }
    } else {
        let mut limit = u32::from(flags & 0x40);
        if flags & 0x40 != 0 { limit = 1; }
        if flags & 0x3f != 0 { limit = limit.wrapping_add(1); }
        if (value as i32) < limit as i32 { value = 0; adjustment = 0; }
        else { value = value.wrapping_sub(limit); adjustment = if flags & 0x3f == 0 { 0 } else { adjustment.wrapping_mul(0xfffffc00) & 0xffff }; }
    }
    Adjustment { position: value, fraction: adjustment as u16 }
}
