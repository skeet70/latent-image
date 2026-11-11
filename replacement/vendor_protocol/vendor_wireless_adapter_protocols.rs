use super::super::vendor_wireless_network::{MacAddress, Ssid};
use super::super::wireless_association::{StationOrderKind, StationOrderQueue, StationQueueError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerDeadline(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerIdentity { timer: ProtocolTimer, generation: u64 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum ProtocolTimer { Background, Discovery, Authentication, Association, Scan, StationZero, StationOne, StationTwo, Security }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerCompletion { Current, Late }
#[derive(Clone, Copy)]
struct Timer { deadline: Option<TimerDeadline>, generation: u64 }
impl Timer {
    const EMPTY: Self = Self { deadline: None, generation: 0 };
    fn cancel(&mut self) { self.deadline = None; }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanRecord { pub address: MacAddress, pub ssid: Ssid }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanIdentity { index: usize, generation: u64 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError { ScanCapacity, StaleScan, ScanStopped, TimerIdentityExhausted }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationState {
    pub enabled: bool,
    pub role: StationRole,
    pub address: MacAddress,
    pub beacon_interval: u16,
    pub packet_sequence: u16,
    pub received_sequence: u16,
    pub aggregation: [u8; 8],
    pub retry_count: u32,
    pub pending_count: u32,
    pub timer_interval: u16,
    pub protection: bool,
    pub aggregation_enabled: [bool; 2],
    pub aggregation_packets: u32,
    pub transmit_statistics: [u32; 3],
    pub receive_statistics: [u32; 2],
    pub sequence_pending: bool,
    pub protocol_flags: [u8; 3],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationRole { Primary, Secondary, Auxiliary }
impl StationState {
    const fn empty(role: StationRole) -> Self {
        Self { enabled: false, role, address: MacAddress([0; 6]), beacon_interval: 0,
            packet_sequence: 0, received_sequence: 0, aggregation: [0; 8], retry_count: 0,
            pending_count: 0, timer_interval: 0, protection: false, aggregation_enabled: [false; 2],
            aggregation_packets: 0, transmit_statistics: [0; 3], receive_statistics: [0; 2],
            sequence_pending: false, protocol_flags: [0; 3] }
    }
    fn stop(&mut self) {
        self.beacon_interval = 0;
        self.packet_sequence = 0;
        self.received_sequence = 0;
        self.aggregation = [0; 8];
        self.retry_count = 0;
        self.pending_count = 0;
        self.timer_interval = 0;
        self.protection = false;
        self.aggregation_enabled = [false; 2];
        self.aggregation_packets = 0;
        self.transmit_statistics = [0; 3];
        self.receive_statistics = [0; 2];
        self.sequence_pending = false;
        self.protocol_flags = [0; 3];
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationDefaults {
    pub retry_limit: u8,
    pub beacon_interval: u16,
    pub rts_threshold: u16,
    pub fragmentation_threshold: u16,
    pub listen_interval: u32,
    pub transmit_enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerCapabilities(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerRateCode(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerRate { Automatic, Fixed(OwnerRateCode) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerNotificationState { pub capabilities: OwnerCapabilities, pub rate: OwnerRate }

pub struct Protocols {
    timers: [Timer; 9],
    stations: [StationState; 3],
    orders: StationOrderQueue<64>,
    scans: [Option<ScanRecord>; 64],
    scan_generation: u64,
    scanning: bool,
    scan_results_visible: bool,
    scan_pool_ready: bool,
    station_template: Option<StationTemplate>,
    background_enabled: bool,
    power_transition_pending: bool,
    defaults: StationDefaults,
    owner_notification: OwnerNotificationState,
}
#[derive(Clone, Copy)]
struct StationTemplate { address: MacAddress, rate_indices: [u8; 3] }

impl Default for Protocols {
    fn default() -> Self { Self::new() }
}
impl Protocols {
    pub const fn new() -> Self {
        Self { timers: [Timer::EMPTY; 9],
            stations: [StationState::empty(StationRole::Primary), StationState::empty(StationRole::Secondary), StationState::empty(StationRole::Auxiliary)],
            orders: StationOrderQueue::new(), scans: [None; 64], scan_generation: 0,
            scanning: false, scan_results_visible: false, station_template: None,
            scan_pool_ready: true,
            background_enabled: false, power_transition_pending: false,
            owner_notification: OwnerNotificationState { capabilities: OwnerCapabilities(0), rate: OwnerRate::Automatic },
            defaults: StationDefaults { retry_limit: 10, beacon_interval: 100, rts_threshold: 2347,
                fragmentation_threshold: 16335, listen_interval: 5, transmit_enabled: true } }
    }
    pub fn arm(&mut self, timer: ProtocolTimer, deadline: TimerDeadline) -> Result<TimerIdentity, ProtocolError> {
        let state = &mut self.timers[timer as usize];
        state.generation = state.generation.checked_add(1).ok_or(ProtocolError::TimerIdentityExhausted)?;
        state.deadline = Some(deadline);
        Ok(TimerIdentity { timer, generation: state.generation })
    }
    pub fn deadline(&self, timer: ProtocolTimer) -> Option<TimerDeadline> { self.timers[timer as usize].deadline }
    pub fn complete_timer(&mut self, identity: TimerIdentity) -> TimerCompletion {
        let state = &mut self.timers[identity.timer as usize];
        if state.generation != identity.generation || state.deadline.is_none() { return TimerCompletion::Late; }
        state.deadline = None;
        TimerCompletion::Current
    }
    pub fn append_order(&mut self, kind: StationOrderKind) -> Result<(), StationQueueError> { self.orders.append(kind) }
    pub fn pop_order(&mut self) -> Option<StationOrderKind> { self.orders.pop() }
    pub const fn stations(&self) -> &[StationState; 3] { &self.stations }
    pub const fn defaults(&self) -> StationDefaults { self.defaults }
    pub fn reset_owner_notification(&mut self) {
        self.owner_notification = OwnerNotificationState { capabilities: OwnerCapabilities(0x13), rate: OwnerRate::Automatic };
    }
    pub const fn owner_notification(&self) -> OwnerNotificationState { self.owner_notification }
    pub fn template(&self) -> Option<(MacAddress, [u8; 3])> { self.station_template.map(|template| (template.address, template.rate_indices)) }
    pub fn remember_scan(&mut self, record: ScanRecord) -> Result<ScanIdentity, ProtocolError> {
        if !self.scan_pool_ready { return Err(ProtocolError::ScanStopped); }
        let index = self.scans.iter().position(Option::is_none).ok_or(ProtocolError::ScanCapacity)?;
        self.scans[index] = Some(record);
        self.scan_results_visible = true;
        Ok(ScanIdentity { index, generation: self.scan_generation })
    }
    pub fn scan(&self, identity: ScanIdentity) -> Result<&ScanRecord, ProtocolError> {
        if identity.generation != self.scan_generation || !self.scan_results_visible { return Err(ProtocolError::StaleScan); }
        self.scans.get(identity.index).and_then(Option::as_ref).ok_or(ProtocolError::StaleScan)
    }
    pub fn stop(&mut self) {
        self.timers[ProtocolTimer::Background as usize].cancel();
        self.background_enabled = false;
        self.power_transition_pending = false;
        for timer in [ProtocolTimer::Discovery, ProtocolTimer::Authentication, ProtocolTimer::Association, ProtocolTimer::Scan] {
            self.timers[timer as usize].cancel();
        }
        self.orders.clear();
        self.station_template = None;
        self.scanning = false;
        self.scan_results_visible = false;
        self.scan_pool_ready = false;
        for (index, station) in self.stations.iter_mut().enumerate() {
            station.stop();
            self.timers[ProtocolTimer::StationZero as usize + index].cancel();
        }
    }
    pub fn reset(&mut self, address: MacAddress) {
        for timer in &mut self.timers { timer.cancel(); }
        self.scans = [None; 64];
        self.scan_generation = self.scan_generation.wrapping_add(1);
        self.scanning = false;
        self.scan_results_visible = false;
        self.scan_pool_ready = true;
        self.stations[0] = StationState::empty(StationRole::Primary);
        self.stations[0].enabled = true;
        self.stations[0].address = address;
        self.station_template = Some(StationTemplate { address, rate_indices: [0; 3] });
        self.power_transition_pending = false;
        self.timers[ProtocolTimer::Security as usize].cancel();
        self.background_enabled = false;
        self.timers[ProtocolTimer::Background as usize].cancel();
    }
}
