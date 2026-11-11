use super::*;
use crate::controller_registration::TimerProducers;

#[test]
fn connection_reports_once_and_requires_ten_absent_ticks() {
    let producers = TimerProducers::new();
    let mut registrations = ControllerRegistrations::new(&producers);
    let mut connection = ConnectionState::new();
    assert_eq!(connection.update_connection(false, &mut registrations), None);
    assert_eq!(connection.update_connection(true, &mut registrations), Some(ConnectionNotification::Connected));
    assert_eq!(ConnectionNotification::Connected.status(), -12);
    for _ in 0..9 { assert_eq!(connection.update_connection(false, &mut registrations), None); }
    assert_eq!(connection.update_connection(false, &mut registrations), Some(ConnectionNotification::Disconnected));
    assert_eq!(ConnectionNotification::Disconnected.status(), -11);
    assert_eq!(connection.update_connection(false, &mut registrations), None);
}

#[test]
fn reconnecting_before_threshold_resets_absence_interval() {
    let producers = TimerProducers::new();
    let mut registrations = ControllerRegistrations::new(&producers);
    let mut connection = ConnectionState::new();
    connection.update_connection(true, &mut registrations);
    for _ in 0..9 { connection.update_connection(false, &mut registrations); }
    assert_eq!(connection.update_connection(true, &mut registrations), None);
    for _ in 0..9 { assert_eq!(connection.update_connection(false, &mut registrations), None); }
    assert_eq!(connection.update_connection(false, &mut registrations), Some(ConnectionNotification::Disconnected));
}

#[test]
fn selected_buffer_follows_mode_and_remains_published() {
    let protocol = PublishedRegion::new(0x1000, 32).unwrap();
    let service = PublishedRegion::new(0x2000, 32).unwrap();
    let unselected = PublishedRegion::new(0x3000, 4).unwrap();
    let mut connection = ConnectionState::new();
    for (kind, region) in [(RequestKind::Protocol, protocol), (RequestKind::Service, service),
        (RequestKind::Unselected, unselected), (RequestKind::FileOnly, unselected)] {
        assert_eq!(connection.select_request_buffer(kind, protocol, service, unselected), region);
        assert_eq!(connection.selected_buffer(), Some(region));
    }
}

#[test]
fn battery_change_is_stored_before_its_notification() {
    let mut connection = ConnectionState::new();
    let mut ptp = PtpRuntime::new();
    ptp.set_session(Some(crate::ptp_types::PtpSessionId(1)));
    ptp.set_battery(BatteryLevel::Full);
    connection.battery_changed(&mut ptp, BatteryLevel::Full);
    assert_eq!(ptp.take_event(), None);
    connection.battery_changed(&mut ptp, BatteryLevel::Low);
    assert_eq!(ptp.battery(), BatteryLevel::Low);
    assert!(ptp.take_event().is_some());
}

#[test]
fn battery_flags_preserve_source_priority() {
    for (flags, expected) in [(0, 3), (1, 3), (4, 1), (8, 1), (16, 2), (20, 1), (17, 3), (u32::MAX, 3)] {
        assert_eq!(BatteryLevel::from_status_flags(flags) as u8, expected);
    }
}

struct Timers(std::vec::Vec<u32>);
impl xm1_power_services::shutdown_task7_8_transport::RegistrationRegistry for Timers {
    fn release(&mut self, _: xm1_power_services::shutdown_task7_8::RegistryHandle) -> ServiceStatus { ServiceStatus(0) }
}
impl TimerRegistry for Timers {
    fn start_periodic(&mut self, period: xm1_power_services::device_timer_release::TimerPeriod, _: crate::controller_registration::TimerEvent)
        -> Result<xm1_power_services::shutdown_task7_8::RegistryHandle, ServiceStatus> {
        self.0.push(period.0);
        Ok(xm1_power_services::shutdown_task7_8::RegistryHandle(self.0.len() as u32))
    }
}

#[test]
fn connection_begin_leaves_the_transfer_state_initialized_and_the_session_closed() {
    let producers = TimerProducers::new();
    let mut registrations = ControllerRegistrations::new(&producers);
    let mut connection = ConnectionState::new();
    let mut ptp = PtpRuntime::new();
    ptp.set_session(Some(crate::ptp_types::PtpSessionId(9)));
    let mut bridge = PictBridge::new();
    let mut timers = Timers(std::vec::Vec::new());
    connection.begin_connection_protocol(&mut ptp, &mut bridge, &mut registrations, &mut timers, BatteryLevel::Medium).unwrap();
    assert_eq!(bridge.phase(), crate::pictbridge_runtime::Phase::Configure);
    assert_eq!(ptp.session(), None);
    assert_eq!(ptp.battery(), BatteryLevel::Medium);
    assert_eq!(timers.0, [250, 60_000]);
    connection.begin_connection_protocol(&mut ptp, &mut bridge, &mut registrations, &mut timers, BatteryLevel::Medium).unwrap();
    assert_eq!(bridge.phase(), crate::pictbridge_runtime::Phase::Configure);
}

#[test]
fn active_file_ownership_moves_out_once() {
    let mut connection = ConnectionState::new();
    let handle = FileHandle::from_raw(7).unwrap();
    connection.set_active(handle);
    assert_eq!(connection.active(), Some(handle));
    assert_eq!(connection.take_close(), Some(handle));
    assert_eq!(connection.take_close(), None);
}

#[derive(Default)]
struct Notifications { sent: std::vec::Vec<(RequestId, QueueId, std::vec::Vec<u8>)> }
impl ShutdownRtos for Notifications {
    fn current_task(&mut self) -> xm1_power_services::shutdown::TaskId { xm1_power_services::shutdown::TaskId(36) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        self.sent.push((message.operation, message.destination, message.payload.to_vec()));
        Ok(())
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { unreachable!() }
}

#[test]
fn close_completion_publishes_controller_event_eleven_and_boolean_failure() {
    let mut notifications = Notifications::default();
    publish_close_started(&mut notifications).unwrap();
    publish_close_status(0, &mut notifications).unwrap();
    publish_close_status(22, &mut notifications).unwrap();
    assert_eq!(notifications.sent[0], (RequestId(0x3407), QueueId(36), [0; 24].to_vec()));
    let mut success = [0; 24];
    success[0] = 11;
    assert_eq!(notifications.sent[1], (RequestId(0x3408), QueueId(36), success.to_vec()));
    let mut failure = success;
    failure[4] = 1;
    assert_eq!(notifications.sent[2], (RequestId(0x3408), QueueId(36), failure.to_vec()));
}

#[test]
fn secondary_shutdown_clears_the_connection_owner() {
    let producers = TimerProducers::new();
    let mut registrations = ControllerRegistrations::new(&producers);
    let mut connection = ConnectionState::new();
    connection.update_connection(true, &mut registrations);
    assert!(connection.secondary_active());
    connection.clear_secondary_activity();
    assert!(!connection.secondary_active());
    assert_eq!(connection.update_connection(true, &mut registrations), Some(ConnectionNotification::Connected));
}
