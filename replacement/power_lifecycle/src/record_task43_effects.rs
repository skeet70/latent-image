use crate::record_address_space::{ByteCount, CallbackError, CallbackHandler, CallbackId, CallbackKey, CallbackSignature, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::shutdown::{KernelStatus, QueueId, RequestId, TaskId};
pub use crate::record_address_space::RecordError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordMessage {
    pub operation: RequestId,
    pub sender: TaskId,
    pub destination: QueueId,
    pub length: ByteCount,
    pub payload: GuestAddress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageCallback {
    pub offset: ByteCount,
    pub callback: CallbackId,
}

pub trait RecordingEffects<M: GuestAddressSpace>: Sized {
    fn current_task(&mut self, memory: &mut M) -> Result<TaskId, MemoryError>;
    /// Queue publication copies payload bytes and their captured callback generations before returning.
    fn send_message(&mut self, memory: &mut M, message: RecordMessage) -> Result<KernelStatus, RecordError>;
    fn send_bytes(&mut self, memory: &mut M, operation: RequestId, sender: TaskId, destination: QueueId, payload: &[u8], callbacks: &[MessageCallback]) -> Result<KernelStatus, RecordError>;
    /// Receive writes the source header, payload and captured callback bindings into the caller's descriptor.
    fn receive_message(&mut self, memory: &mut M, queue: QueueId, descriptor: GuestAddress, timeout: i32) -> Result<KernelStatus, RecordError>;
    fn register_callback(&mut self, key: CallbackKey, handler: CallbackHandler<RecordContext<M, Self>>) -> Result<CallbackId, CallbackError>;
    fn resolve_callback_id(&self, key: CallbackKey, signature: CallbackSignature) -> Result<CallbackId, CallbackError>;
    fn callback_handler(&self, id: CallbackId) -> Result<CallbackHandler<RecordContext<M, Self>>, CallbackError>;
    fn callback_at(&self, slot: GuestAddress, signature: CallbackSignature) -> Result<CallbackId, CallbackError>;
    fn bind_callback(&mut self, slot: GuestAddress, callback: CallbackId) -> Result<(), CallbackError>;
    fn clear_callback_binding(&mut self, slot: GuestAddress);
    fn copy_callback_bindings(&mut self, destination: GuestAddress, source: GuestAddress, length: ByteCount) -> Result<(), CallbackError>;
    fn interrupt_lock(&mut self, memory: &mut M) -> Result<u32, MemoryError>;
    fn interrupt_unlock(&mut self, memory: &mut M, saved: u32) -> Result<(), MemoryError>;
    fn append_stream_text(&mut self, memory: &mut M, task: TaskId, text: GuestAddress) -> Result<(), MemoryError>;
    fn syslog_thread_name(&mut self, memory: &mut M, text: GuestAddress) -> Result<(), MemoryError>;
}
