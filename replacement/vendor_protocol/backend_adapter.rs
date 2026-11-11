use crate::{backend::{BackendError, ConfigImages, ConfigReadMode, ConfigSpan, DebugMemory, DebugRequest, CONFIG_BYTES}, commands::{Backend, BackendStatus, ByteCount, ConfigNamespace, DebugAddress, DebugQuery}};

pub const DEBUG_RING_BYTES: usize = 0x2000;
pub struct DebugRing<'a> { bytes: &'a [u8; DEBUG_RING_BYTES], read: usize, write: usize, wrapped: bool, overflow: u32 }
impl<'a> DebugRing<'a> {
    pub fn adopt(bytes: &'a [u8; DEBUG_RING_BYTES], read: u32, write: u32, wrapped: bool, overflow: u32) -> Result<Self, BackendError> {
        if read > DEBUG_RING_BYTES as u32 || write > DEBUG_RING_BYTES as u32 { return Err(BackendError::Range); }
        if (!wrapped && read != DEBUG_RING_BYTES as u32 && write < read) || (wrapped && write > read) { return Err(BackendError::Range); }
        Ok(Self { bytes, read: read as usize, write: write as usize, wrapped, overflow })
    }
    pub fn query(&mut self, enabled: bool, output: &mut [u8]) -> DebugQuery {
        if !enabled { return DebugQuery { status: BackendStatus::Unavailable, actual: ByteCount(0), overflow: 0 }; }
        if self.write == self.read { self.wrapped = false; self.overflow = 0; return DebugQuery { status: BackendStatus::Complete, actual: ByteCount(0), overflow: 0 }; }
        if self.read == DEBUG_RING_BYTES { self.read = 0; }
        let available = self.write + if self.wrapped { DEBUG_RING_BYTES } else { 0 } - self.read;
        let count = available.min(output.len());
        let first = count.min(DEBUG_RING_BYTES-self.read);
        output[..first].copy_from_slice(&self.bytes[self.read..self.read+first]);
        if count > first { output[first..count].copy_from_slice(&self.bytes[..count-first]); }
        self.read += count;
        if self.read > DEBUG_RING_BYTES { self.read -= DEBUG_RING_BYTES; self.wrapped = false; }
        else if self.read == DEBUG_RING_BYTES { self.wrapped = false; }
        output[count..].fill(0);
        let overflow = self.overflow;
        self.overflow = 0;
        DebugQuery { status: BackendStatus::Complete, actual: ByteCount(count as u32), overflow }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RulePermission { Reject, Dispatch }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationRule { pub selector: u16, pub permission: RulePermission }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationEvent { pub destination: crate::configuration_queue::ConfigurationDestination, pub selector: crate::configuration_queue::ConfigurationSelector }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugExecutor { Absent, ApplicationEvents }
pub struct CommandBackend<'a> {
    pub configuration: ConfigImages<'a>,
    pub memory: DebugMemory<'a>,
    pub ring: DebugRing<'a>,
    pub rules: &'a [ConfigurationRule],
    pending: Option<ConfigurationEvent>,
    executor: DebugExecutor,
    application_event: Option<crate::debug_events::ApplicationEvent>,
}
impl<'a> CommandBackend<'a> {
    pub fn new(configuration: ConfigImages<'a>, memory: DebugMemory<'a>, ring: DebugRing<'a>, rules: &'a [ConfigurationRule]) -> Result<Self, BackendError> {
        if rules.len() != 144 { return Err(BackendError::Capacity); }
        Ok(Self { configuration, memory, ring, rules, pending: None, executor: DebugExecutor::Absent, application_event: None })
    }
    pub fn take_configuration_event(&mut self) -> Option<ConfigurationEvent> { self.pending.take() }
    pub fn register_application_executor(&mut self) -> Result<(), BackendError> {
        if !self.configuration.debug_enabled()? { return Err(BackendError::Disabled); }
        self.executor = DebugExecutor::ApplicationEvents;
        Ok(())
    }
    pub fn take_application_event(&mut self) -> Option<crate::debug_events::ApplicationEvent> { self.application_event.take() }
}
fn status(result: Result<(), BackendError>) -> BackendStatus {
    match result { Ok(()) => BackendStatus::Complete, Err(BackendError::Disabled | BackendError::Unavailable) => BackendStatus::Unavailable, Err(BackendError::Range | BackendError::Capacity | BackendError::InvalidCommand) => BackendStatus::Invalid, Err(BackendError::Medium(_)) => BackendStatus::Failed }
}
impl Backend for CommandBackend<'_> {
    fn configuration_capacity(&self) -> u32 { CONFIG_BYTES }
    fn identity_byte(&self, index: u32) -> Option<u8> { self.configuration.byte(index) }
    fn config_read(&mut self, namespace: ConfigNamespace, offset: u32, output: &mut [u8]) -> BackendStatus {
        let Ok(length) = u32::try_from(output.len()) else { return BackendStatus::Invalid; };
        let mode = match namespace { ConfigNamespace::Cached => ConfigReadMode::Cached, ConfigNamespace::Encoded => ConfigReadMode::Encoded };
        status(ConfigSpan::new(offset, length).and_then(|span| self.configuration.read(span, mode, output)))
    }
    fn config_write(&mut self, namespace: ConfigNamespace, offset: u32, input: &[u8]) -> BackendStatus {
        let Ok(length) = u32::try_from(input.len()) else { return BackendStatus::Invalid; };
        let result = ConfigSpan::new(offset, length).and_then(|span| self.configuration.write(span, input));
        if let Err(error) = result { return status(Err(error)); }
        if namespace == ConfigNamespace::Cached { return BackendStatus::Failed; }
        if offset != 0x80 { return BackendStatus::Complete; }
        let Some(selector) = self.configuration.byte(0x80) else { return BackendStatus::Invalid; };
        let Some(rule) = self.rules.iter().find(|rule| rule.selector == u16::from(selector)) else { return BackendStatus::Failed; };
        if rule.permission != RulePermission::Dispatch { return BackendStatus::Failed; }
        if self.pending.is_some() { return BackendStatus::Failed; }
        let selector = match crate::configuration_queue::ConfigurationSelector::parse(selector) { Ok(selector) => selector, Err(_) => return BackendStatus::Failed };
        self.pending = Some(ConfigurationEvent { destination: crate::configuration_queue::ConfigurationDestination::Broadcast, selector });
        BackendStatus::Pending
    }
    fn debug_query(&mut self, output: &mut [u8]) -> DebugQuery {
        match self.configuration.debug_enabled() {
            Ok(enabled) => self.ring.query(enabled, output),
            Err(error) => DebugQuery { status: status(Err(error)), actual: ByteCount(0), overflow: 0 },
        }
    }
    fn debug_read(&mut self, address: DebugAddress, output: &mut [u8]) -> BackendStatus {
        match self.configuration.debug_enabled() { Ok(enabled) => status(self.memory.read(enabled, address.0, output)), Err(error) => status(Err(error)) }
    }
    fn debug_write(&mut self, address: DebugAddress, input: &[u8]) -> BackendStatus {
        match self.configuration.debug_enabled() { Ok(enabled) => status(self.memory.write(enabled, address.0, input)), Err(error) => status(Err(error)) }
    }
    fn debug_execute(&mut self, input: &[u8]) -> BackendStatus {
        match self.configuration.debug_enabled() { Ok(true) => {}, Ok(false) => return BackendStatus::Failed, Err(error) => return status(Err(error)) }
        let request = match DebugRequest::parse(input) { Ok(request) => request, Err(_) => return BackendStatus::Failed };
        if self.executor == DebugExecutor::Absent { return BackendStatus::Failed; }
        let event = match crate::debug_events::interpret(request) { Ok(event) => event, Err(_) => return BackendStatus::Failed };
        if self.application_event.is_some() { return BackendStatus::Failed; }
        self.application_event = Some(event);
        BackendStatus::Complete
    }
}
