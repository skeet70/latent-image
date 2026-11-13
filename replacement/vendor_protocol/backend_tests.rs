use crate::{backend::*, backend_adapter::*, commands::{Backend, BackendStatus, ConfigNamespace}, debug_events::ApplicationEvent, extra_configuration::*, settings_reset};
use record_store::Bank;
use xm1_settings_storage_core::mirror::*;

#[test]
fn spans_reject_wrap_and_holes_but_preserve_valid_empty_requests() {
    assert_eq!(ConfigSpan::new(u32::MAX, 3), Err(BackendError::Range));
    assert_eq!(ConfigSpan::new(CONFIG_BYTES-1, 2), Err(BackendError::Range));
    assert_eq!(ConfigSpan::new(CONFIG_BYTES, 0), Err(BackendError::Range));
    assert!(ConfigSpan::new(CONFIG_BYTES-1, 0).is_ok());
    assert!(ConfigSpan::new(DIAGNOSTIC_BASE+DIAGNOSTIC_BYTES-1, 1).is_ok());
    assert_eq!(ConfigSpan::new(DIAGNOSTIC_BASE+DIAGNOSTIC_BYTES, 0), Err(BackendError::Range));
    assert!(PageOffset::new(0x1f800).is_ok());
    assert_eq!(PageOffset::new(0x20000), Err(BackendError::Range));
    assert_eq!(PageOffset::new(1), Err(BackendError::Range));
}

#[test]
fn configuration_reads_cross_split_without_touching_cache_or_output_tail() {
    let mut first = [0; 0x10000];
    first[0xffff] = 7;
    let second = [8, 9];
    let mut encoded = [6; 256];
    let images = ConfigImages::adopt(&first, &second, &[11, 12, 13], &mut encoded, &[21, 22, 23]);
    let mut output = [0xa5; 4];
    images.read(ConfigSpan::new(0xffff, 3).unwrap(), ConfigReadMode::Cached, &mut output).unwrap();
    assert_eq!(output, [7,8,9,0xa5]);
    images.read(ConfigSpan::new(DIAGNOSTIC_BASE+2, 1).unwrap(), ConfigReadMode::Cached, &mut output).unwrap();
    assert_eq!(output, [13,8,9,0xa5]);
    let before = output;
    assert_eq!(images.read(ConfigSpan::new(DIAGNOSTIC_BASE+2, 2).unwrap(), ConfigReadMode::Cached, &mut output), Err(BackendError::Capacity));
    assert_eq!(output, before);
    images.read(ConfigSpan::new(CONFIG_BYTES-1, 0).unwrap(), ConfigReadMode::Encoded, &mut []).unwrap();
    assert_eq!(images.byte(0), Some(6));
}

#[test]
fn bounded_debug_memory_rejects_unbound_addresses_and_disabled_writes() {
    let mut bytes = [1,2,3,4];
    let mut memory = DebugMemory::adopt(0x1000, &mut bytes).unwrap();
    assert_eq!(memory.write(false,0x1000,&[9]),Err(BackendError::Disabled));
    assert_eq!(memory.write(true,u32::MAX,&[9,8]),Err(BackendError::Range));
    memory.write(true,0x1001,&[9,8]).unwrap();
    let mut output = [0;4];
    memory.read(true,0x1000,&mut output).unwrap();
    assert_eq!(output,[1,9,8,4]);
}

#[test]
fn debug_ring_consumes_wrapped_bytes_and_reports_overflow_once() {
    let mut bytes = [0;DEBUG_RING_BYTES];
    bytes[DEBUG_RING_BYTES-2..].copy_from_slice(&[1,2]);
    bytes[..2].copy_from_slice(&[3,4]);
    let mut ring = DebugRing::adopt(&bytes,(DEBUG_RING_BYTES-2) as u32,2,true,1).unwrap();
    let mut output = [0xa5;6];
    let query = ring.query(true,&mut output);
    assert_eq!(query.actual.0,4);
    assert_eq!(query.overflow,1);
    assert_eq!(output,[1,2,3,4,0,0]);
    let empty = ring.query(true,&mut output);
    assert_eq!(empty.actual.0,0);
    assert_eq!(empty.overflow,0);
}

#[test]
fn registered_debug_executor_emits_power_off_and_configuration_writes_remain_visible_on_error() {
    let mut config = [0;256];
    config[0xf7] = 1;
    let images = ConfigImages::adopt(&[],&[],&[],&mut config,&[]);
    let mut memory = [0;4];
    let ring_bytes = [0;DEBUG_RING_BYTES];
    let rules = crate::configuration_rules::STOCK_RULES;
    let mut backend = CommandBackend::new(images,DebugMemory::adopt(0,&mut memory).unwrap(),DebugRing::adopt(&ring_bytes,0,0,false,0).unwrap(),&rules).unwrap();
    assert_eq!(backend.debug_execute(b"POWER OFF\0"),BackendStatus::Failed);
    backend.register_application_executor().unwrap();
    assert_eq!(backend.debug_execute(b"power off\0"),BackendStatus::Complete);
    assert_eq!(backend.take_application_event(),Some(ApplicationEvent::PowerOffRequested));
    assert_eq!(backend.debug_execute(b"POWER BAD\0"),BackendStatus::Failed);
    assert_eq!(backend.config_write(ConfigNamespace::Encoded,0xf7,&[0]),BackendStatus::Complete);
    assert_eq!(backend.debug_execute(b"POWER OFF\0"),BackendStatus::Failed);
    assert_eq!(backend.take_application_event(),None);
    assert_eq!(backend.config_write(ConfigNamespace::Cached,12,&[44]),BackendStatus::Failed);
    assert_eq!(backend.identity_byte(12),Some(44));
    assert_eq!(backend.config_write(ConfigNamespace::Encoded,0x80,&[1]),BackendStatus::Pending);
    assert_eq!(backend.take_configuration_event(),Some(ConfigurationEvent { destination:crate::configuration_queue::ConfigurationDestination::Broadcast,selector:crate::configuration_queue::ConfigurationSelector::parse(1).unwrap() }));
    assert_eq!(backend.config_write(ConfigNamespace::Encoded,u32::MAX,&[6,7]),BackendStatus::Invalid);
    assert_eq!(backend.identity_byte(0),Some(0));
}

#[test]
fn extra_configuration_commit_is_visible_through_the_medium() {
    let mut banks: std::boxed::Box<[[u8;BLOCK_BYTES];2]> = std::vec![[0xff;BLOCK_BYTES];2].into_boxed_slice().try_into().ok().unwrap();
    let mut medium = HostConfigurationMedium::adopt(&mut banks);
    let mut live = [0;RECORD_BYTES];
    let mut scratch = [0;RECORD_BYTES];
    let mut configuration = ExtraConfiguration::load(&mut live,&mut scratch,&mut medium).unwrap();
    assert_eq!(configuration.commit(&mut medium),CommitOutcome::Clean);
    configuration.edit()[33] = 19;
    assert_eq!(configuration.commit(&mut medium),CommitOutcome::Written { bank:Bank::Zero,page:PageOffset::new(0).unwrap() });
    let mut readback = [0;RECORD_BYTES];
    medium.read(Bank::Zero,PageOffset::new(0).unwrap(),&mut readback).unwrap();
    assert_eq!(readback[33],19);
    assert_eq!(&readback[..4],&[1,0,0,0]);
    let mut reboot = [0;RECORD_BYTES];
    let restored = ExtraConfiguration::load(&mut reboot,&mut scratch,&mut medium).unwrap();
    assert_eq!(restored.record()[33],19);
}

struct RetainedRegisters { bytes: [u8;16] }
impl HeaderBackupDriver for RetainedRegisters {
    type Task = ();
    fn task(&mut self)->Option<()> { Some(()) }
    fn acquire(&mut self,_:&(),_:BackupSemaphore) {}
    fn release(&mut self,_:&(),_:BackupSemaphore) {}
    fn diagnostic(&mut self,_:BackupDiagnostic) {}
    fn admit(&mut self,_:RequestDirection)->Result<(),DeviceStatus> { Ok(()) }
    fn write_backup(&mut self,key:BackupKey,value:BackupValue,_:BackupWriteMode)->DeviceStatus { self.bytes[key.0 as usize] = value.0; DeviceStatus(0) }
    fn store_request_status(&mut self,_:RequestStatus) {}
    fn report_failure(&mut self,_:RequestStatus) {}
}

#[test]
fn counter_reset_composes_real_mirror_and_area2_record() {
    let mut live = [0xa5;BACKING_BYTES];
    let mut shadow = [0;16];
    let keys = core::array::from_fn(|index|BackupKey(index as u16));
    let mut backing = SharedBacking::without_export_residue(&mut live,&mut shadow,&keys,RetainedRegisters { bytes:[0;16] });
    let mut area2 = [0x55;0x200];
    settings_reset::reset_counters(&mut backing,&mut area2);
    assert_eq!(&backing.live()[..4],&[0,0,100,0]);
    assert_eq!(&backing.live()[0x128..0x140],&[0;24]);
    assert_eq!(&area2[0x24..0x28],&[100,0,0,0]);
    assert_eq!(&area2[4..10],&[0;6]);
    assert_eq!(area2[10],0x55);
    assert_eq!(backing.driver().bytes, *backing.shadow());
    assert_eq!(backing.live()[..16].iter().fold(0u8,|sum,byte|sum.wrapping_add(*byte)),0);
}

impl BackupDriver for RetainedRegisters {
    fn refresh_custom(&mut self, _: &[u8;CUSTOM_BYTES]) {}
}
struct CustomMedium(std::boxed::Box<[[u8;record_store::BLOCK_BYTES];2]>);
impl record_store::RecordMedium for CustomMedium {
    fn read(&mut self, kind:record_store::RecordKind, bank:Bank, offset:record_store::BytePosition, output:&mut[u8])->Result<(),record_store::MediumError> {
        if kind != record_store::RecordKind::Kind3 { return Err(record_store::MediumError(-1)); }
        let start = offset.0 as usize;
        let end = start.checked_add(output.len()).ok_or(record_store::MediumError(-1))?;
        output.copy_from_slice(self.0[bank as usize].get(start..end).ok_or(record_store::MediumError(-1))?);
        Ok(())
    }
    fn program(&mut self, kind:record_store::RecordKind, bank:Bank, offset:record_store::BytePosition, input:&[u8])->Result<(),record_store::MediumError> {
        if kind != record_store::RecordKind::Kind3 { return Err(record_store::MediumError(-1)); }
        let start = offset.0 as usize;
        let end = start.checked_add(input.len()).ok_or(record_store::MediumError(-1))?;
        let output = self.0[bank as usize].get_mut(start..end).ok_or(record_store::MediumError(-1))?;
        if output.iter().zip(input).any(|(old,new)| old & new != *new) { return Err(record_store::MediumError(-1)); }
        for (old,new) in output.iter_mut().zip(input) { *old &= *new; }
        Ok(())
    }
    fn erase(&mut self,kind:record_store::RecordKind,bank:Bank)->Result<(),record_store::MediumError> {
        if kind != record_store::RecordKind::Kind3 { return Err(record_store::MediumError(-1)); }
        self.0[bank as usize].fill(0xff);
        Ok(())
    }
}
struct CustomStaging([u8;0x600]);
impl record_store::Staging for CustomStaging {
    fn bytes(&mut self,_:record_store::RecordKind)->&mut[u8] { &mut self.0 }
}

#[test]
fn persistent_queue_reset_and_commit_reach_owned_media() {
    use xm1_settings_storage_core::custom::{CustomPayload,CustomRecordBackend,CustomSettingsLifecycle,CustomSettingsRecord,RecordStoreBackend,ReloadOutcome};
    use crate::persistent_queue::{ConfigurationQueueOutcome,PersistentQueue,PersistentQueueEvent,PersistentQueueOutcome};
    let mut live = [0xa5;BACKING_BYTES];
    let mut shadow = [0;16];
    let keys = core::array::from_fn(|index|BackupKey(index as u16));
    let mut backing = SharedBacking::without_export_residue(&mut live,&mut shadow,&keys,RetainedRegisters { bytes:[0;16] });
    let mut config = [0;0x9764];
    config[0x81] = 5;
    config[0x20b] = 35;
    config[0x215] = 2;
    config[0x216] = 1;
    config[0x9742..0x9762].fill(0x42);
    let mut configuration = ConfigImages::adopt(&[],&[],&[],&mut config,&[]);
    let mut custom_medium = CustomMedium(std::vec![[0xff;record_store::BLOCK_BYTES];2].into_boxed_slice().try_into().ok().unwrap());
    let mut staging = CustomStaging([0xff;0x600]);
    let mut blocks: std::boxed::Box<[[u8;record_store::BLOCK_BYTES];2]> = std::vec![[0;record_store::BLOCK_BYTES];2].into_boxed_slice().try_into().ok().unwrap();
    let mut cache = record_store::CacheState::new();
    let mut custom_backend = RecordStoreBackend::new(&mut custom_medium,&mut staging,&mut blocks,&mut cache);
    let mut previous = CustomSettingsRecord::from_working_wire([0xff;52]);
    let mut working = CustomSettingsRecord::from_working_wire([0;52]);
    let mut custom_live = CustomPayload([0;40]);
    let mut custom = CustomSettingsLifecycle::adopt(&mut previous,&mut working,&mut custom_live);
    let mut extra_banks: std::boxed::Box<[[u8;BLOCK_BYTES];2]> = std::vec![[0xff;BLOCK_BYTES];2].into_boxed_slice().try_into().ok().unwrap();
    let mut extra_medium = HostConfigurationMedium::adopt(&mut extra_banks);
    let mut extra_live = [0;RECORD_BYTES];
    let mut extra_scratch = [0;RECORD_BYTES];
    let mut extra = ExtraConfiguration::load(&mut extra_live,&mut extra_scratch,&mut extra_medium).unwrap();
    extra.edit()[0x600] = 17;
    let mut scratch = [0;0x128];
    let header = [0x55;16];
    let body = [0xaa;0x118];
    {
        let mut queue = PersistentQueue { backing:&mut backing, configuration:&mut configuration, defaults:settings_reset::OrdinaryDefaults { header:&header,body:&body }, counters:settings_reset::SavedCounters { image:0x1234,folder:0x5678 }, custom:&mut custom,custom_backend:&mut custom_backend,extra:&mut extra,medium:&mut extra_medium,scratch:&mut scratch };
        let mut state = crate::configuration_queue::ProcessingState::State0;
        let event = ConfigurationEvent { destination:crate::configuration_queue::ConfigurationDestination::Broadcast,selector:crate::configuration_queue::ConfigurationSelector::parse(6).unwrap() };
        let result = queue.apply_configuration(event,&mut state).unwrap();
        assert!(matches!(result,ConfigurationQueueOutcome::Applied(PersistentQueueOutcome::Reset(_))));
        assert_eq!(state,crate::configuration_queue::ProcessingState::State0);
        assert!(matches!(queue.apply(PersistentQueueEvent::CommitExtraConfiguration).unwrap(),PersistentQueueOutcome::Commit(CommitOutcome::Written { .. })));
    }
    assert_eq!(&backing.live()[..4],&[0x34,0x12,0x78,0x56]);
    assert_eq!(backing.live()[0x21]&63,1);
    assert_eq!(&backing.live()[0xa0..0xc0],&[0x42;32]);
    assert_eq!(&backing.live()[0xc0..0xe8],&[0;40]);
    assert_eq!(backing.driver().bytes,*backing.shadow());
    let saved = match custom_backend.read_latest() { ReloadOutcome::Latest { record,.. } => record, _ => panic!("reset record was not persisted") };
    assert_eq!(saved.payload().0[25]&31,7);
    let mut readback = [0xff;RECORD_BYTES];
    extra_medium.read(Bank::Zero,PageOffset::new(0).unwrap(),&mut readback).unwrap();
    assert_eq!(&readback[4..0x544],&[0;0x540]);
    assert_eq!(readback[0x600],17);
}

#[test]
fn configuration_selector_six_reaches_profile_consumer_without_assuming_action_indexes() {
    use crate::configuration_queue::{resolve,ConfigurationSelector,ProcessingState,QueueDisposition};
    let mut encoded = [0;0x239];
    let configuration = ConfigImages::adopt(&[],&[],&[],&mut encoded,&[]);
    let profile = resolve(ConfigurationSelector::parse(6).unwrap(),ProcessingState::State0,&configuration).unwrap();
    assert_eq!(profile.disposition,QueueDisposition::Persistent(crate::persistent_queue::PersistentQueueEvent::ProfileBytes));
    assert_eq!(profile.next_state,ProcessingState::State0);
    assert_eq!(resolve(ConfigurationSelector::parse(6).unwrap(),ProcessingState::State4,&configuration).unwrap().disposition,QueueDisposition::Rejected);
    let other = resolve(ConfigurationSelector::parse(8).unwrap(),ProcessingState::State0,&configuration).unwrap();
    match other.disposition { QueueDisposition::UnownedCpu(root) => assert_eq!(root.address(),0xecc4bc), _=>panic!("selector eight selected a different handler") }
    assert_eq!(resolve(ConfigurationSelector::parse(141).unwrap(),ProcessingState::State0,&configuration).unwrap().disposition,QueueDisposition::NoAction);
}

#[test]
fn missing_debug_gate_is_a_capacity_error_not_a_closed_gate() {
    let mut encoded = [];
    let configuration = ConfigImages::adopt(&[],&[],&[],&mut encoded,&[]);
    assert_eq!(configuration.debug_enabled(),Err(BackendError::Capacity));
}

#[test]
fn service_start_prepares_calibration_before_publishing_vendor_mode() {
    use crate::startup::{ServiceStartup,WORKSPACE_BYTES};
    use crate::startup_projection::StartupProjection;
    let mut workspace = std::vec![0xa5;WORKSPACE_BYTES+8];
    let mut startup = ServiceStartup::adopt(&mut workspace).unwrap();
    let mut config = std::vec![0;0x420000];
    config[0x238] = 1;
    config[0x3e0009] = 1;
    config[0x3e0f80] = 1;
    config[0x3e0f82] = 1;
    let mut configuration = ConfigImages::adopt(&[],&[],&[],&mut config,&[]);
    let mut live = [0;BACKING_BYTES];
    live[0x5e] = 0x80;
    let mut shadow = [0;16];
    let keys = core::array::from_fn(|index|BackupKey(index as u16));
    let mut backing = SharedBacking::without_export_residue(&mut live,&mut shadow,&keys,RetainedRegisters { bytes:[0;16] });
    let mut export = [0xa5;BACKING_BYTES];
    let mut projection = StartupProjection::new();
    let request = startup.start_1705(&mut configuration,&mut projection,&mut backing,&mut export).unwrap();
    assert_eq!(configuration.byte(0x31d80),Some(1));
    assert_eq!(configuration.byte(0x404e01),Some(8));
    assert_eq!(startup.profile_flag(),Some(true));
    assert!(startup.workspace().iter().all(|byte|*byte==0xff));
    assert_eq!(request.mode.get(),3);
    assert_eq!(request.event_code(),0x3403);
    assert_eq!(request.providers.capacity.get(),0x420000);
    assert_eq!(startup.take_usb_request(),Some(request));
    drop(startup);
    assert_eq!(&workspace[WORKSPACE_BYTES..],&[0xa5;8]);
}
