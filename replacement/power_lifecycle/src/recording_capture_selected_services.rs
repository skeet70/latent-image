use crate::record_address_space::{CallbackError, CallbackId, GuestAddressSpace, MemoryError, RecordContext, RecordError};
use crate::recording_capture_resources::CaptureResourceError;

#[derive(Debug)]
pub enum SelectedError { Record(RecordError), Resource(CaptureResourceError) }
impl From<MemoryError> for SelectedError { fn from(error: MemoryError) -> Self { Self::Record(error.into()) } }
impl From<CallbackError> for SelectedError { fn from(error: CallbackError) -> Self { Self::Record(error.into()) } }
impl From<RecordError> for SelectedError { fn from(error: RecordError) -> Self { Self::Record(error) } }
impl From<CaptureResourceError> for SelectedError { fn from(error: CaptureResourceError) -> Self { Self::Resource(error) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureCommand { Output, Drive, Pair, Selection, Rectangle, Channel, Window, Auxiliary }
impl CaptureCommand {
    pub const fn operation(self) -> u16 { match self { Self::Output => 0x0d, Self::Drive => 0x1a, Self::Pair => 0x1d, Self::Selection => 0x1e, Self::Rectangle => 0x1f, Self::Channel => 0x20, Self::Window => 0x21, Self::Auxiliary => 0x26 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMessage { Output, DriveReady, DriveChanged, Pair, Selection, Rectangle, Channel, Window, Auxiliary }
impl CaptureMessage {
    pub const fn operation(self) -> u16 { match self { Self::Output => 0x13a5, Self::DriveReady => 0x13b0, Self::DriveChanged => 0x13b1, Self::Pair => 0x13b5, Self::Selection => 0x13b6, Self::Rectangle => 0x13b9, Self::Channel => 0x13b7, Self::Window => 0x13b8, Self::Auxiliary => 0x13bd } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingControl { Defer, Repeat, MissingState, Notify }
#[derive(Clone, Copy)]
pub struct SelectedCallbacks {
    pub output: CallbackId,
    pub drive: CallbackId,
    pub pair: CallbackId,
    pub selection: CallbackId,
    pub rectangle: CallbackId,
    pub channel: CallbackId,
    pub window: CallbackId,
    pub auxiliary: CallbackId,
}
pub trait CaptureSelectedEffects<M: GuestAddressSpace>: Sized {
    fn command(context: &mut RecordContext<M, Self>, command: CaptureCommand, callback: CallbackId, payload: &[u32]) -> Result<(), RecordError>;
    fn message(context: &mut RecordContext<M, Self>, message: CaptureMessage, payload: &[u32]) -> Result<(), RecordError>;
    fn control(context: &mut RecordContext<M, Self>, control: RecordingControl) -> Result<(), RecordError>;
}
