use core::{marker::PhantomPinned, pin::Pin};
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ImageAddress(pub GuestAddress);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct NotificationCode(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct StackResidue(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct TaskQueue(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum Operation { Open = 0x0200, Close = 0x0201, Begin = 0x0204, Cancel = 0x0205, Output = 0x020b, Configure = 0x020c, Exposure = 0x1338 }

pub enum Request<'a> {
    Open { input: ImageAddress, configuration: ImageAddress, resources: ImageAddress, option: u32, notification: NotificationCode },
    Close { operation: u32, option: u32, context: u32, notification: NotificationCode, reserved: u32 },
    Configure { configuration: &'a mut [u32; 250], notification: NotificationCode },
    Begin { operation: u32, option: u32, notification: NotificationCode, context: u32 },
    Cancel { operation: u32, option: u32, residue: StackResidue },
    Output { operation: u32, option: u32, notification: NotificationCode, context: u32 },
    Exposure { value: u32, mode: u16, context: u32, residue: StackResidue },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { Memory(MemoryError), Transport(i32) }
impl From<MemoryError> for Error { fn from(error: MemoryError) -> Self { Self::Memory(error) } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion { Continue, Success, Rejected }

pub const fn classify_notification(operation: u16) -> Completion {
    match operation {
        0x0816 | 0x0901 | 0x090a => Completion::Success,
        0x091c | 0x091d | 0x0932 | 0x0933 | 0x0934 => Completion::Continue,
        value if value & 0xff00 == 0x0900 => Completion::Rejected,
        _ => Completion::Continue,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Notification { pub receive_status: i32, pub operation: u32, pub value: u32 }

pub trait Task3Queues {
    fn post_words(&mut self, queue: TaskQueue, operation: Operation, payload: &[u32]) -> Result<(), i32>;
    fn receive_notification(&mut self, queue: TaskQueue) -> Notification;
}

pub fn helper_e86d30<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<TaskQueue, MemoryError> {
    context.read_u32(GuestAddress::new(0x0157_bdf0)).map(TaskQueue)
}

pub struct Task3Runtime {
    configuration: GuestAddress,
    _pinned: PhantomPinned,
}

impl Task3Runtime {
    pub const fn new(configuration: GuestAddress) -> Self { Self { configuration, _pinned: PhantomPinned } }

    /// The admitted snapshot region remains reserved until recording releases its configuration reference.
    pub fn submit<M: GuestAddressSpace, E: Task3Queues>(self: Pin<&mut Self>, context: &mut RecordContext<M, E>, request: Request<'_>) -> Result<(), Error> {
        let snapshot = self.as_ref().get_ref().configuration;
        let mut destination = TaskQueue(3);
        let (operation, words, count) = match request {
            Request::Open { input, configuration, resources, option, notification } => {
                let value = context.read_u32(configuration.0.checked_add(0x148)?)?;
                context.write_u32(configuration.0.checked_add(0x144)?, value)?;
                (Operation::Open, [input.0.get(), configuration.0.get(), resources.0.get(), notification.0, option], 5)
            }
            Request::Close { operation, option, context, notification, reserved } =>
                (Operation::Close, [operation, option, context, notification.0, reserved], 5),
            Request::Configure { configuration, notification } => {
                configuration[0x144 / 4] = configuration[0x148 / 4];
                for (index, value) in configuration.iter().enumerate() {
                    context.write_u32(snapshot.checked_add(index as u32 * 4)?, *value)?;
                }
                (Operation::Configure, [snapshot.get(), notification.0, 0, 0, 0], 2)
            }
            Request::Begin { operation, option, notification, context } =>
                (Operation::Begin, [operation, option, notification.0, context, 0], 4),
            Request::Cancel { operation, option, residue } =>
                (Operation::Cancel, [operation, option, residue.0, 0, 0], 3),
            Request::Output { operation, option, notification, context } =>
                (Operation::Output, [operation, option, notification.0, context, 0], 4),
            Request::Exposure { value, mode, context: request_context, residue } => {
                context.write_u8(GuestAddress::new(0x0157_dd34), 0)?;
                context.write_u8(GuestAddress::new(0x0157_dd35), 0)?;
                destination = helper_e86d30(context)?;
                (Operation::Exposure, [value, (residue.0 & 0xffff0000) | u32::from(mode), request_context, 0, 0], 3)
            }
        };
        context.effects.post_words(destination, operation, &words[..count]).map_err(Error::Transport)
    }

    pub fn poll<M: GuestAddressSpace, E: Task3Queues>(self: Pin<&mut Self>, context: &mut RecordContext<M, E>) -> Result<Option<i32>, Error> {
        let _ = self;
        let notification = context.effects.receive_notification(TaskQueue(7));
        if notification.receive_status != 0 { return Ok(Some(-255)); }
        Ok(match classify_notification(notification.operation as u16) {
            Completion::Continue => None,
            Completion::Success => Some(0),
            Completion::Rejected => Some(-255),
        })
    }
}
