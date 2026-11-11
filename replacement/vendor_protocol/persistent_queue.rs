use crate::{backend::{BackendError, ConfigImages, ConfigSpan}, extra_configuration::{CommitOutcome, ConfigurationMedium, ExtraConfiguration}, settings_reset::{self, OrdinaryDefaults, ResetMode, ResetOutcome, SavedCounters}};
use xm1_settings_storage_core::{custom::{CustomRecordBackend, CustomSettingsLifecycle}, mirror::{BackingRange, BackingStatus, BackingWriteOutcome, BackupDriver, SharedBacking}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistentQueueEvent { ProfileBytes, CommitExtraConfiguration }
pub enum PersistentQueueOutcome { Read(BackingStatus), Write(BackingWriteOutcome), Reset(ResetOutcome), Commit(CommitOutcome) }
pub enum ConfigurationQueueOutcome { Applied(PersistentQueueOutcome), NoAction, Rejected, UnownedCpu(crate::configuration_queue::CpuRoot) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationCompletion { Success, Failure }
impl ConfigurationCompletion {
    pub const fn status_word(self) -> u32 { match self { Self::Success => 0, Self::Failure => 0x80000000 } }
    pub const fn backend_status(self) -> crate::commands::BackendStatus { match self { Self::Success => crate::commands::BackendStatus::Complete, Self::Failure => crate::commands::BackendStatus::Failed } }
}
impl ConfigurationQueueOutcome {
    pub fn completion(&self) -> Option<ConfigurationCompletion> {
        match self { Self::Applied(_) => Some(ConfigurationCompletion::Success), Self::Rejected => Some(ConfigurationCompletion::Failure), Self::NoAction | Self::UnownedCpu(_) => None }
    }
}

pub struct PersistentQueue<'a, 'b, D: BackupDriver, C: CustomRecordBackend, M: ConfigurationMedium> {
    pub backing: &'a mut SharedBacking<'b, D>,
    pub configuration: &'a mut ConfigImages<'b>,
    pub defaults: OrdinaryDefaults<'a>,
    pub counters: SavedCounters,
    pub custom: &'a mut CustomSettingsLifecycle<'b>,
    pub custom_backend: &'a mut C,
    pub extra: &'a mut ExtraConfiguration<'b>,
    pub medium: &'a mut M,
    pub scratch: &'a mut [u8; 0x128],
}
impl<D: BackupDriver, C: CustomRecordBackend, M: ConfigurationMedium> PersistentQueue<'_, '_, D, C, M> {
    pub fn apply_configuration(&mut self, event: crate::backend_adapter::ConfigurationEvent, state: &mut crate::configuration_queue::ProcessingState) -> Result<ConfigurationQueueOutcome, BackendError> {
        use crate::configuration_queue::{resolve, QueueDisposition};
        let resolution = resolve(event.selector, *state, self.configuration)?;
        let outcome = match resolution.disposition {
            QueueDisposition::Persistent(event) => match self.apply(event) {
                Ok(outcome) => Ok(ConfigurationQueueOutcome::Applied(outcome)),
                Err(BackendError::InvalidCommand | BackendError::Range) => Ok(ConfigurationQueueOutcome::Rejected),
                Err(error) => Err(error),
            },
            QueueDisposition::NoAction => Ok(ConfigurationQueueOutcome::NoAction),
            QueueDisposition::Rejected => Ok(ConfigurationQueueOutcome::Rejected),
            QueueDisposition::UnownedCpu(root) => return Ok(ConfigurationQueueOutcome::UnownedCpu(root)),
        };
        *state = resolution.next_state;
        outcome
    }
    pub fn apply(&mut self, event: PersistentQueueEvent) -> Result<PersistentQueueOutcome, BackendError> {
        if event == PersistentQueueEvent::CommitExtraConfiguration { return Ok(PersistentQueueOutcome::Commit(self.extra.commit(self.medium))); }
        let selector = self.configuration.byte(0x81).ok_or(BackendError::Capacity)?;
        if selector == 5 {
            return settings_reset::reset(ResetMode::OrdinaryAndCustom, self.backing, OrdinaryDefaults { header: self.defaults.header, body: self.defaults.body }, self.counters, self.configuration, self.custom, self.custom_backend, self.extra, self.scratch).map(PersistentQueueOutcome::Reset);
        }
        let index = self.configuration.byte(0x82).ok_or(BackendError::Capacity)?;
        let value = if selector == 3 || selector == 4 { self.configuration.byte(0x83).ok_or(BackendError::Capacity)? } else { 0 };
        let action = crate::transport::profile_selector(selector,index,value).map_err(|_| BackendError::InvalidCommand)?;
        match action {
            crate::transport::ProfileAction::Read { bank,index } => {
                let mut value = [0];
                let status = self.backing.export(BackingRange::new(u32::from(index.offset(bank)),1), &mut value);
                if status == BackingStatus::Success { self.configuration.write(ConfigSpan::new(0x83,1)?, &value)?; }
                Ok(PersistentQueueOutcome::Read(status))
            }
            crate::transport::ProfileAction::Write { bank,index,value } => {
                Ok(PersistentQueueOutcome::Write(self.backing.import(BackingRange::new(u32::from(index.offset(bank)),1), &[value])))
            }
            crate::transport::ProfileAction::ResetModes => unreachable!(),
        }
    }
}
