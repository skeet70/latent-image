use crate::panel_route::*;
use std::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Effect {
    Store(RouteField, u32), Avout(ResourceId, bool), Gpio(usize, u32),
    Acquire(ResourceId), Release(ResourceToken), Delay, Enter, Read(u32), Write(u32), Leave,
}
struct Runtime { effects: Vec<Effect>, mux: u32, acquired: ResourceToken }
impl Runtime {
    fn new() -> Self { Self { effects: Vec::new(), mux: 0xabcdef12, acquired: ResourceToken(73) } }
}
impl RouteRuntime for Runtime {
    fn store(&mut self, field: RouteField, value: u32) { self.effects.push(Effect::Store(field, value)); }
    fn avout(&mut self, resource: ResourceId, enabled: bool) { self.effects.push(Effect::Avout(resource, enabled)); }
    fn gpio(&mut self, write: GpioWrite) { self.effects.push(Effect::Gpio(write.pin.address(), write.pin.word(write.high))); }
    fn acquire(&mut self, resource: ResourceId) -> ResourceToken { self.effects.push(Effect::Acquire(resource)); self.acquired }
    fn release(&mut self, token: ResourceToken) { self.effects.push(Effect::Release(token)); }
    fn start_delay(&mut self) { self.effects.push(Effect::Delay); }
    fn enter_critical(&mut self) { self.effects.push(Effect::Enter); }
    fn read_mux(&mut self) -> u32 { self.effects.push(Effect::Read(self.mux)); self.mux }
    fn write_mux(&mut self, value: u32) { self.mux = value; self.effects.push(Effect::Write(value)); }
    fn leave_critical(&mut self) { self.effects.push(Effect::Leave); }
}
fn state() -> RouteState {
    RouteState {
        resource_primary: ResourceToken(9), resource_route: ResourceToken(0),
        avout_request: RequestState(0), timing: TimingSelector(4), requested_source: SourceId(2),
        status: RequestState(1), auxiliary: RequestState(91), secondary_status: RequestState(0),
        recorded_source: SourceId(99), current_profile: ProfileSelection::None, initialized: RequestState(1),
    }
}
fn gpio(encoded: u32, high: bool) -> Option<GpioWrite> { GpioPin::parse(encoded).map(|pin| GpioWrite { pin, high }) }
fn profile() -> RouteProfile {
    RouteProfile {
        source_mux: MuxValue(2), output_mux: MuxValue(1), channel_mux: MuxValue(3),
        format_mux: MuxValue(5), clock_mux: MuxValue(6), resource: ResourceAction::Acquire(ResourceId(19)),
        gpios: [gpio(0x10002, true), gpio(0x11000f, false), gpio(0x20003, true)],
        avout: AvoutAction::Set { resource: ResourceId(23), enabled: true },
    }
}
#[test]
fn nonzero_route_orders_gpio_resource_and_five_separate_critical_updates() {
    let mut state = state();
    let mut runtime = Runtime::new();
    record_source(SourceId(0x1234), &mut state, &[profile(); 8], &mut runtime);
    assert_eq!(runtime.effects, [
        Effect::Avout(ResourceId(23), true), Effect::Store(RouteField::AvoutRequest, 1),
        Effect::Gpio(0xfff89040, 0x00040004), Effect::Gpio(0xfff89040, 0x80000000),
        Effect::Acquire(ResourceId(19)), Effect::Delay,
        Effect::Enter, Effect::Read(0xabcdef12), Effect::Write(0xabcdef12), Effect::Leave,
        Effect::Enter, Effect::Read(0xabcdef12), Effect::Write(0xabcdbf12), Effect::Leave,
        Effect::Enter, Effect::Read(0xabcdbf12), Effect::Write(0xabcdb512), Effect::Leave,
        Effect::Enter, Effect::Read(0xabcdb512), Effect::Write(0xabcdb512), Effect::Leave,
        Effect::Enter, Effect::Read(0xabcdb512), Effect::Write(0xabcdb562), Effect::Leave,
        Effect::Gpio(0xfff89080, 0x00080008), Effect::Store(RouteField::CurrentProfile, 2),
        Effect::Store(RouteField::RecordedSource, 0x1234),
    ]);
    assert_eq!(state.resource_primary, ResourceToken(9));
    assert_eq!(state.resource_route, ResourceToken(73));
    assert_eq!(state.recorded_source, SourceId(0x1234));
    assert_eq!(state.current_profile, ProfileSelection::Selected(ProfileId::parse(2).unwrap()));
}
#[test]
fn unchanged_profile_only_records_source_and_preserves_pending_resource() {
    let mut state = state();
    state.current_profile = ProfileSelection::Selected(ProfileId::parse(2).unwrap());
    state.resource_route = ResourceToken(82);
    let mut runtime = Runtime::new();
    record_source(SourceId(0xdeadbeef), &mut state, &[profile(); 8], &mut runtime);
    assert_eq!(runtime.effects, [Effect::Store(RouteField::RecordedSource, 0xdeadbeef)]);
    assert_eq!(state.resource_route, ResourceToken(82));
}
#[test]
fn failed_acquisition_still_delays_and_routes() {
    let mut state = state();
    let mut runtime = Runtime::new();
    runtime.acquired = ResourceToken(0);
    record_source(SourceId(7), &mut state, &[profile(); 8], &mut runtime);
    assert_eq!(state.resource_route, ResourceToken(0));
    assert_eq!(state.current_profile, ProfileSelection::Selected(ProfileId::parse(2).unwrap()));
    let acquire = runtime.effects.iter().position(|effect| *effect == Effect::Acquire(ResourceId(19))).unwrap();
    assert_eq!(runtime.effects[acquire + 1], Effect::Delay);
    assert_eq!(runtime.effects.last(), Some(&Effect::Store(RouteField::RecordedSource, 7)));
}
#[test]
fn release_disables_noncanonical_active_flags_without_releasing_primary() {
    let mut state = state();
    state.resource_route = ResourceToken(42);
    state.avout_request = RequestState(19);
    let mut profile = profile();
    profile.resource = ResourceAction::Release;
    profile.avout = AvoutAction::Set { resource: ResourceId(23), enabled: false };
    profile.gpios = [None; 3];
    let mut runtime = Runtime::new();
    route(ProfileId::parse(2).unwrap(), &mut state, &profile, &mut runtime);
    assert_eq!(&runtime.effects[..4], [
        Effect::Avout(ResourceId(23), false), Effect::Store(RouteField::AvoutRequest, 0),
        Effect::Release(ResourceToken(42)), Effect::Store(RouteField::ResourceRoute, 0),
    ]);
    assert_eq!(state.resource_primary, ResourceToken(9));
    assert_eq!(state.resource_route, ResourceToken(0));
}
#[test]
fn lookup_covers_request_switch_and_exact_status_comparisons() {
    for timing in 1..=10 {
        for (source, expected) in [(0,1),(1,1),(2,2),(3,6),(7,6),(8,7),(9,7),(10,1),(u32::MAX,1)] {
            let mut state = state();
            state.timing = TimingSelector(timing);
            state.requested_source = SourceId(source);
            assert_eq!(lookup(&mut state, &mut Runtime::new()).unwrap().code(), expected);
        }
    }
    for status in [0,2,u32::MAX] {
        for (secondary, expected) in [(0,3),(1,0),(2,3)] {
            let mut state = state();
            state.status = RequestState(status);
            state.secondary_status = RequestState(secondary);
            assert_eq!(lookup(&mut state, &mut Runtime::new()).unwrap().code(), expected);
        }
    }
    for timing in [12, u32::MAX] {
        let mut state = state();
        state.timing = TimingSelector(timing);
        let before = state;
        let mut runtime = Runtime::new();
        assert_eq!(lookup(&mut state, &mut runtime), None);
        assert_eq!(state, before);
        assert!(runtime.effects.is_empty());
    }
}
#[test]
fn initial_lookup_resets_once_before_recording_even_when_no_route_selected() {
    for timing in [0,11] {
        let mut state = state();
        state.timing = TimingSelector(timing);
        state.initialized = RequestState(2);
        let mut runtime = Runtime::new();
        record_source(SourceId(77), &mut state, &[profile(); 8], &mut runtime);
        assert_eq!(runtime.effects, [
            Effect::Store(RouteField::ResourcePrimary, 0), Effect::Store(RouteField::ResourceRoute, 0),
            Effect::Store(RouteField::AvoutRequest, 0), Effect::Store(RouteField::Timing, 11),
            Effect::Store(RouteField::RequestedSource, 0), Effect::Store(RouteField::Status, 0),
            Effect::Store(RouteField::Auxiliary, 0), Effect::Store(RouteField::SecondaryStatus, 0),
            Effect::Store(RouteField::RecordedSource, 0), Effect::Store(RouteField::CurrentProfile, 8),
            Effect::Store(RouteField::Initialized, 1), Effect::Store(RouteField::RecordedSource, 77),
        ]);
        assert_eq!(state.timing, TimingSelector(11));
        assert_eq!(state.current_profile, ProfileSelection::None);
        assert_eq!(state.recorded_source, SourceId(77));
        runtime.effects.clear();
        record_source(SourceId(88), &mut state, &[profile(); 8], &mut runtime);
        assert_eq!(runtime.effects, [Effect::Store(RouteField::RecordedSource, 88)]);
    }
}
#[test]
fn gpio_bounds_include_aliased_banks_sixteen_and_seventeen() {
    assert_eq!(GpioPin::parse(0x10000f).unwrap().address(), 0xfff89000);
    assert_eq!(GpioPin::parse(0x11000f).unwrap().word(true), 0x80008000);
    assert_eq!(GpioPin::parse(0x120000), None);
    assert_eq!(GpioPin::parse(0x10010), None);
    assert_eq!(GpioPin::parse(u32::MAX), None);
}
#[test]
fn mux_values_are_not_masked_before_insertion() {
    let mut state = state();
    let mut profile = profile();
    profile.output_mux = MuxValue(u32::MAX);
    profile.resource = ResourceAction::Keep;
    profile.avout = AvoutAction::Keep;
    profile.gpios = [None; 3];
    let mut runtime = Runtime::new();
    route(ProfileId::parse(2).unwrap(), &mut state, &profile, &mut runtime);
    assert_eq!(runtime.effects[2], Effect::Write(0xffffef12));
}

#[derive(Debug, PartialEq, Eq)]
enum AvEffect {
    Lock, Unlock(LockToken), Request(ResourceId, RequestState), ReadRequest(ResourceId),
    Read(u32), Enter, Write(u32), Leave(LockToken),
}
#[derive(Debug)]
struct ObservationLimit;
struct AvRuntime {
    requests: [RequestState; 2],
    reads: std::collections::VecDeque<u32>,
    effects: Vec<AvEffect>,
}
impl AvRuntime {
    fn new(requests: [RequestState; 2], reads: &[u32]) -> Self {
        Self { requests, reads: reads.iter().copied().collect(), effects: Vec::new() }
    }
}
impl AvoutRuntime for AvRuntime {
    fn lock_ownership(&mut self) -> LockToken { self.effects.push(AvEffect::Lock); LockToken(11) }
    fn unlock_ownership(&mut self, token: LockToken) { self.effects.push(AvEffect::Unlock(token)); }
    fn set_request(&mut self, resource: ResourceId, request: RequestState) {
        self.effects.push(AvEffect::Request(resource, request));
        if let Some(slot) = self.requests.get_mut(resource.0 as usize) { *slot = request; }
    }
    fn request(&mut self, resource: ResourceId) -> RequestState {
        self.effects.push(AvEffect::ReadRequest(resource));
        self.requests[resource.0 as usize]
    }
    fn read_control(&mut self) -> u32 {
        let value = self.reads.pop_front().unwrap_or_else(|| std::panic::panic_any(ObservationLimit));
        self.effects.push(AvEffect::Read(value));
        value
    }
    fn enter_critical(&mut self) -> LockToken { self.effects.push(AvEffect::Enter); LockToken(37) }
    fn write_control(&mut self, value: u32) { self.effects.push(AvEffect::Write(value)); }
    fn leave_critical(&mut self, token: LockToken) { self.effects.push(AvEffect::Leave(token)); }
}
#[test]
fn avout_enable_keeps_ownership_lock_through_ready_poll() {
    let mut runtime = AvRuntime::new([RequestState(0); 2], &[5, 10, 0, 0x01000000]);
    set_avout_request(ResourceId(1), true, &mut runtime);
    assert_eq!(runtime.effects, [
        AvEffect::Lock, AvEffect::Request(ResourceId(1), RequestState(1)),
        AvEffect::ReadRequest(ResourceId(0)), AvEffect::ReadRequest(ResourceId(1)),
        AvEffect::Read(5), AvEffect::Enter, AvEffect::Read(10), AvEffect::Write(0x1000000a),
        AvEffect::Leave(LockToken(37)), AvEffect::Read(0), AvEffect::Read(0x01000000),
        AvEffect::Unlock(LockToken(11)),
    ]);
}
#[test]
fn avout_disable_rechecks_hardware_and_clears_only_request_bit() {
    let mut runtime = AvRuntime::new([RequestState(0), RequestState(1)], &[0x10000000,0x10000004,0x100000ff]);
    set_avout_request(ResourceId(1), false, &mut runtime);
    assert_eq!(runtime.effects, [
        AvEffect::Lock, AvEffect::Request(ResourceId(1), RequestState(0)),
        AvEffect::ReadRequest(ResourceId(0)), AvEffect::ReadRequest(ResourceId(1)),
        AvEffect::Read(0x10000000), AvEffect::Read(0x10000004),
        AvEffect::Enter, AvEffect::Read(0x100000ff), AvEffect::Write(0xff),
        AvEffect::Leave(LockToken(37)), AvEffect::Unlock(LockToken(11)),
    ]);
}
#[test]
fn avout_exact_one_scan_short_circuits_and_unchanged_hardware_is_not_written() {
    let mut active = AvRuntime::new([RequestState(1), RequestState(2)], &[0x10000000,0x10000000]);
    set_avout_request(ResourceId(2), true, &mut active);
    assert_eq!(active.effects, [
        AvEffect::Lock, AvEffect::Request(ResourceId(2), RequestState(1)), AvEffect::ReadRequest(ResourceId(0)),
        AvEffect::Read(0x10000000), AvEffect::Read(0x10000000), AvEffect::Unlock(LockToken(11)),
    ]);
    let mut inactive = AvRuntime::new([RequestState(2), RequestState(u32::MAX)], &[0,0]);
    set_avout_request(ResourceId(2), false, &mut inactive);
    assert_eq!(inactive.effects, [
        AvEffect::Lock, AvEffect::Request(ResourceId(2), RequestState(0)),
        AvEffect::ReadRequest(ResourceId(0)), AvEffect::ReadRequest(ResourceId(1)),
        AvEffect::Read(0), AvEffect::Read(0), AvEffect::Unlock(LockToken(11)),
    ]);
}

#[test]
fn avout_generic_request_preserves_full_word_without_treating_nonzero_as_one() {
    let mut runtime = AvRuntime::new([RequestState(1), RequestState(0)], &[0,0]);
    set_avout_request_state(ResourceId(0), RequestState(0xdeadbeef), &mut runtime);
    assert_eq!(runtime.effects, [
        AvEffect::Lock, AvEffect::Request(ResourceId(0), RequestState(0xdeadbeef)),
        AvEffect::ReadRequest(ResourceId(0)), AvEffect::ReadRequest(ResourceId(1)),
        AvEffect::Read(0), AvEffect::Read(0), AvEffect::Unlock(LockToken(11)),
    ]);
}

#[test]
fn avout_held_ready_bit_never_completes_or_releases_ownership() {
    for polls in [1, 4, 16] {
        let mut reads = std::vec![5, 10];
        reads.extend(std::iter::repeat(0).take(polls));
        let mut runtime = AvRuntime::new([RequestState(0); 2], &reads);
        let mut completed = false;
        let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            set_avout_request(ResourceId(1), true, &mut runtime);
            completed = true;
        }));
        assert!(stopped.unwrap_err().is::<ObservationLimit>());
        assert!(!completed);
        let mut expected = std::vec![
            AvEffect::Lock, AvEffect::Request(ResourceId(1), RequestState(1)),
            AvEffect::ReadRequest(ResourceId(0)), AvEffect::ReadRequest(ResourceId(1)),
            AvEffect::Read(5), AvEffect::Enter, AvEffect::Read(10),
            AvEffect::Write(0x1000000a), AvEffect::Leave(LockToken(37)),
        ];
        expected.extend((0..polls).map(|_| AvEffect::Read(0)));
        assert_eq!(runtime.effects, expected);
    }
}
