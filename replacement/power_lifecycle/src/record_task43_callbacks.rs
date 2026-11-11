use crate::record_address_space::{CallbackError, CallbackKey, CallbackRegistry, CallbackSignature, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RecordError};
use crate::record_task43::CompletionStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionError { Memory(MemoryError), Callback(CallbackError), Runtime(RecordError) }

impl From<MemoryError> for CompletionError {
    fn from(error: MemoryError) -> Self { Self::Memory(error) }
}
impl From<CallbackError> for CompletionError {
    fn from(error: CallbackError) -> Self { Self::Callback(error) }
}
impl From<RecordError> for CompletionError {
    fn from(error: RecordError) -> Self { Self::Runtime(error) }
}

fn release<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, command: u32, status: CompletionStatus) -> Result<(), MemoryError> {
    if status.releases_command() {
        context.write_u8(GuestAddress::new(0x169cf24).checked_add(command)?, 0)?;
    }
    Ok(())
}

pub fn complete_status<M: GuestAddressSpace, E, const N: usize>(context: &mut RecordContext<M, E>, callbacks: &CallbackRegistry<RecordContext<M, E>, N>, command: u32, storage: GuestAddress, status: CompletionStatus) -> Result<(), CompletionError> {
    let callback = CallbackKey::new(context.read_u32(storage)?);
    release(context, command, status)?;
    if let Some(callback) = callback {
        let id = callbacks.resolve(callback, CallbackSignature::Status)?;
        callbacks.invoke_status(id, context, status.0 as u32)?;
    }
    Ok(())
}

pub fn complete_status_word<M: GuestAddressSpace, E, const N: usize>(context: &mut RecordContext<M, E>, callbacks: &CallbackRegistry<RecordContext<M, E>, N>, command: u32, storage: GuestAddress, status: CompletionStatus, value: u32) -> Result<(), CompletionError> {
    let callback = CallbackKey::new(context.read_u32(storage)?);
    release(context, command, status)?;
    if let Some(callback) = callback {
        let id = callbacks.resolve(callback, CallbackSignature::StatusWord)?;
        callbacks.invoke_status_word(id, context, status.0 as u32, value)?;
    }
    Ok(())
}

pub fn complete_status_three_words<M: GuestAddressSpace, E, const N: usize>(context: &mut RecordContext<M, E>, callbacks: &CallbackRegistry<RecordContext<M, E>, N>, command: u32, storage: GuestAddress, status: CompletionStatus, words: [u32; 3]) -> Result<(), CompletionError> {
    let callback = CallbackKey::new(context.read_u32(storage)?);
    release(context, command, status)?;
    if let Some(callback) = callback {
        let id = callbacks.resolve(callback, CallbackSignature::StatusThreeWords)?;
        callbacks.invoke_status_three_words(id, context, status.0 as u32, words)?;
    }
    Ok(())
}

pub fn complete_history<M: GuestAddressSpace, E, const N: usize>(context: &mut RecordContext<M, E>, callbacks: &CallbackRegistry<RecordContext<M, E>, N>, command: u32, storage: GuestAddress, inserted_offset: u32, status: CompletionStatus, words: [u32; 4]) -> Result<(), CompletionError> {
    let callback = CallbackKey::new(context.read_u32(storage)?);
    let inserted = context.read_u32(storage.checked_add(inserted_offset)?)?;
    release(context, command, status)?;
    if let Some(callback) = callback {
        let id = callbacks.resolve(callback, CallbackSignature::StatusFiveWords)?;
        callbacks.invoke_status_five_words(id, context, status.0 as u32, [words[0], words[1], inserted, words[2], words[3]])?;
    }
    Ok(())
}
