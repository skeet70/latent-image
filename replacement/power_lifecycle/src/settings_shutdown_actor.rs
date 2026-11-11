use crate::record_store::{self as store, BlockBuffer, Callbacks, CheckVerdict, Entry, Handle, RecordMedium, Service, Staging};
use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos};

pub const RECORD_BYTES: usize = 0x600;
pub const RELEASE_COMMAND: RequestId = RequestId(0x6301);
pub const SETTINGS_QUEUE: QueueId = QueueId(83);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackToken(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TransferState { Initial = 0, Active = 1, Released = 2, Fourth = 3 }
impl TransferState {
    pub const fn from_abi(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Initial), 1 => Some(Self::Active), 2 => Some(Self::Released), 3 => Some(Self::Fourth), _ => None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchError { InvalidPayload, Lock(KernelStatus), Unlock(KernelStatus) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppendObservation { Unchanged, Closed, Attempted(Result<store::Appended, store::NotAppended>) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReleaseObservation { pub append: AppendObservation }

pub trait ReleaseRuntime {
    fn acquire(&mut self) -> Result<(), KernelStatus>;
    fn release(&mut self) -> Result<(), KernelStatus>;
    fn cancel_timer(&mut self, timer: TimerHandle);
    fn acknowledge_release(&mut self, callback: CallbackToken);
}

pub fn post_release(rtos: &mut impl ShutdownRtos, callback: CallbackToken) -> Result<(), KernelStatus> {
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RELEASE_COMMAND, sender, destination: SETTINGS_QUEUE, payload: &callback.0.to_le_bytes() })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferAction { Start, Update, Restart, Callback, Release, Refresh, Poll, NoAction }
pub const fn dispatch_route(operation: RequestId, state: TransferState) -> Option<TransferAction> {
    match operation.0 {
        0x6300 => Some(match state {
            TransferState::Initial => TransferAction::Start, TransferState::Active => TransferAction::Update,
            TransferState::Released => TransferAction::Restart, TransferState::Fourth => TransferAction::Callback,
        }),
        0x6301 => Some(TransferAction::Release),
        0x6303 if matches!(state, TransferState::Active) => Some(TransferAction::Refresh),
        0x6304 if matches!(state, TransferState::Active) => Some(TransferAction::Poll),
        0x6302..=0x6309 => Some(TransferAction::NoAction),
        _ => None,
    }
}

#[repr(transparent)]
pub struct TransferRequest(pub [u8; 28]);
pub fn post_transfer(rtos: &mut impl ShutdownRtos, request: &TransferRequest) -> Result<(), KernelStatus> {
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RequestId(0x6300), sender, destination: SETTINGS_QUEUE, payload: &request.0 })
}

pub fn register(service: &mut Service<SettingsCodec>) -> Result<Handle, store::RegistrationError> {
    service.register(store::RecordKind::Kind4, store::RecordBytes(RECORD_BYTES as u16), store::RecordCount(20), store::SequenceOffset(0), SettingsCodec)
}

pub struct SettingsRecord(pub [u8; RECORD_BYTES]);
pub struct SettingsCodec;
impl Callbacks for SettingsCodec {
    fn seal(&mut self, record: &mut [u8]) { seal(record.try_into().expect("settings record geometry")); }
    fn check(&mut self, record: &mut [u8]) -> CheckVerdict {
        let Ok(record) = <&[u8; RECORD_BYTES]>::try_from(&*record) else { return CheckVerdict::Reject; };
        if valid(record) { CheckVerdict::Accept } else { CheckVerdict::Reject }
    }
}
fn checksum(record: &[u8; RECORD_BYTES]) -> u8 { record[4..0x5fc].iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) }
pub fn seal(record: &mut [u8; RECORD_BYTES]) { record[0x5fc] = checksum(record); }
pub fn valid(record: &[u8; RECORD_BYTES]) -> bool { checksum(record) == record[0x5fc] }

pub struct SettingsShutdownActor<'a> {
    state: TransferState,
    timer: Option<TimerHandle>,
    current: &'a mut SettingsRecord,
    snapshot: &'a SettingsRecord,
}
impl<'a> SettingsShutdownActor<'a> {
    pub fn adopt(state: TransferState, timer: Option<TimerHandle>, current: &'a mut SettingsRecord, snapshot: &'a SettingsRecord) -> Self {
        Self { state, timer, current, snapshot }
    }
    pub const fn state(&self) -> TransferState { self.state }
    pub fn release<C: Callbacks, M: RecordMedium, S: Staging, B: BlockBuffer<M>>(
        &mut self, payload: &[u8], runtime: &mut impl ReleaseRuntime,
        service: &mut Service<C>, handle: Handle, entry: &mut Entry<'_, M, S, B>,
    ) -> Result<ReleaseObservation, DispatchError> {
        let payload: &[u8; 4] = payload.try_into().map_err(|_| DispatchError::InvalidPayload)?;
        let callback = CallbackToken(u32::from_le_bytes(*payload));
        runtime.acquire().map_err(DispatchError::Lock)?;
        if let Some(timer) = self.timer.take() { runtime.cancel_timer(timer); }
        if callback.0 != 0 { runtime.acknowledge_release(callback); }
        let append = if self.current.0[2..0x5ff] == self.snapshot.0[2..0x5ff] { AppendObservation::Unchanged }
        else if let Some(registration) = service.get_mut(handle) {
            AppendObservation::Attempted(entry.append(&mut registration.area, &mut registration.callbacks, store::Seed(0), &self.current.0))
        } else { AppendObservation::Closed };
        service.close(handle);
        self.state = TransferState::Released;
        runtime.release().map_err(DispatchError::Unlock)?;
        Ok(ReleaseObservation { append })
    }
}
