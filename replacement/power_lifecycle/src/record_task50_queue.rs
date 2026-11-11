use crate::record_address_space::{CallbackHandler, CallbackId, GuestAddressSpace, RecordContext, RecordError};
use crate::shutdown::{KernelStatus, TaskId};
use crate::record_task50::{Event, Job, JobState, Opcode};

#[derive(Clone, Copy)]
pub struct Task50Packet {
    pub(crate) opcode: Opcode,
    pub(crate) sender: TaskId,
    pub(crate) length: u8,
    pub(crate) payload: [u8; 36],
    pub(crate) callback: Option<CallbackId>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Phase {
    Idle,
    Focus { event: Event, argument: JobState },
    Iris { event: Event, argument: JobState },
    NotifyFocus { event: Event, argument: JobState },
    NotifyPosition { event: Event, argument: JobState },
    NotifyIris { event: Event, argument: JobState },
    Dispatch { event: Event, argument: JobState },
}
pub struct Task50Runtime<'storage> {
    slots: &'storage mut [Option<Task50Packet>],
    head: usize,
    length: usize,
    pub(crate) received_callback: Option<CallbackId>,
    job_callbacks: [Option<CallbackId>; 34],
    status_callback: Option<CallbackId>,
    position_callback: Option<CallbackId>,
    pub(crate) phase: Phase,
}
impl<'storage> Task50Runtime<'storage> {
    pub fn new(slots: &'storage mut [Option<Task50Packet>]) -> Self {
        Self { slots, head: 0, length: 0, received_callback: None, job_callbacks: [None; 34], status_callback: None, position_callback: None, phase: Phase::Idle }
    }
    pub fn is_empty(&self) -> bool { self.length == 0 }
    pub fn job_callback(&self, job: Job) -> Option<CallbackId> { usize::try_from(job.0).ok().and_then(|index| self.job_callbacks.get(index)).copied().flatten() }
    pub fn set_status_callback(&mut self, callback: Option<CallbackId>) { self.status_callback = callback; }
    pub fn set_position_callback(&mut self, callback: Option<CallbackId>) { self.position_callback = callback; }
    pub fn status_callback(&self) -> Option<CallbackId> { self.status_callback }
    pub fn position_callback(&self) -> Option<CallbackId> { self.position_callback }
    pub(crate) fn accept_job_callback(&mut self, job: Job) { self.job_callbacks[job.0 as usize] = self.received_callback; }
    pub(crate) fn push(&mut self, packet: Task50Packet) -> Result<(), KernelStatus> {
        if self.length == self.slots.len() { return Err(KernelStatus(-62)); }
        let index = (self.head + self.length) % self.slots.len();
        self.slots[index] = Some(packet);
        self.length += 1;
        Ok(())
    }
    pub(crate) fn rollback_push(&mut self) {
        self.length -= 1;
        let index = (self.head + self.length) % self.slots.len();
        self.slots[index] = None;
    }
    pub(crate) fn front(&self) -> Option<Task50Packet> { if self.length == 0 { None } else { self.slots[self.head] } }
    pub(crate) fn pop(&mut self) {
        self.slots[self.head] = None;
        self.head = (self.head + 1) % self.slots.len();
        self.length -= 1;
    }
}
pub trait Task50QueueAccess { fn task50(&mut self) -> &mut Task50Runtime<'_>; }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavedInterrupts(pub u32);
pub trait Task50Kernel: Task50QueueAccess {
    fn enter_task50_queue(&mut self) -> SavedInterrupts;
    fn leave_task50_queue(&mut self, saved: SavedInterrupts);
    fn wake_task50(&mut self) -> Result<(), KernelStatus>;
    fn wait_task50(&mut self) -> Result<(), KernelStatus>;
    fn current_task(&mut self) -> TaskId;
}
pub trait Task50Callbacks<M: GuestAddressSpace>: Sized {
    fn task50_callback(&self, id: CallbackId) -> Result<CallbackHandler<RecordContext<M, Self>>, crate::record_address_space::CallbackError>;
}
pub fn post_with_callback<M: GuestAddressSpace, E: Task50Kernel>(context: &mut RecordContext<M, E>, opcode: Opcode, words: &[u32], callback: Option<CallbackId>) -> Result<u32, RecordError> {
    if words.len() > 9 { return Err(KernelStatus(-17).into()); }
    let mut payload = [0; 36];
    for (destination, value) in payload.chunks_exact_mut(4).zip(words) { destination.copy_from_slice(&value.to_le_bytes()); }
    let mut sender = context.effects.current_task();
    if sender.0 == 0 { sender.0 = 0xff; }
    let packet = Task50Packet { opcode, sender, length: (words.len() * 4) as u8, payload, callback };
    let saved = context.effects.enter_task50_queue();
    let result = context.effects.task50().push(packet);
    if let Err(error) = result { context.effects.leave_task50_queue(saved); return Err(error.into()); }
    let result = context.effects.wake_task50();
    if result.is_err() { context.effects.task50().rollback_push(); }
    context.effects.leave_task50_queue(saved);
    result.map(|()| 0).map_err(RecordError::from)
}
