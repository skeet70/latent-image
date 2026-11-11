#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceId(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceId(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceToken(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequestState(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimingSelector(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MuxValue(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileId(u8);
impl ProfileId {
    pub const fn parse(value: u32) -> Option<Self> { if value < 8 { Some(Self(value as u8)) } else { None } }
    pub const fn index(self) -> usize { self.0 as usize }
    pub const fn code(self) -> u32 { self.0 as u32 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileSelection { Selected(ProfileId), None, Other(u32) }
impl ProfileSelection {
    pub const fn from_stock(value: u32) -> Self {
        match ProfileId::parse(value) { Some(id) => Self::Selected(id), None if value == 8 => Self::None, None => Self::Other(value) }
    }
    pub const fn code(self) -> u32 { match self { Self::Selected(id) => id.code(), Self::None => 8, Self::Other(value) => value } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteState {
    pub resource_primary: ResourceToken,
    pub resource_route: ResourceToken,
    pub avout_request: RequestState,
    pub timing: TimingSelector,
    pub requested_source: SourceId,
    pub status: RequestState,
    pub auxiliary: RequestState,
    pub secondary_status: RequestState,
    pub recorded_source: SourceId,
    pub current_profile: ProfileSelection,
    pub initialized: RequestState,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteField {
    ResourcePrimary, ResourceRoute, AvoutRequest, Timing, RequestedSource, Status,
    Auxiliary, SecondaryStatus, RecordedSource, CurrentProfile, Initialized,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceAction { Keep, Acquire(ResourceId), Release }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvoutAction { Keep, Set { resource: ResourceId, enabled: bool } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpioPin { bank: u8, bit: u8 }
impl GpioPin {
    pub const fn parse(encoded: u32) -> Option<Self> {
        let bank = encoded >> 16;
        let bit = encoded & 0xffff;
        if bank < 18 && bit < 16 { Some(Self { bank: bank as u8, bit: bit as u8 }) } else { None }
    }
    pub const fn address(self) -> usize { 0xfff89000 + ((self.bank as usize & 15) * 64) }
    pub const fn word(self, high: bool) -> u32 { (1 << (self.bit + 16)) | ((high as u32) << self.bit) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpioWrite { pub pin: GpioPin, pub high: bool }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteProfile {
    pub source_mux: MuxValue,
    pub output_mux: MuxValue,
    pub channel_mux: MuxValue,
    pub format_mux: MuxValue,
    pub clock_mux: MuxValue,
    pub resource: ResourceAction,
    pub gpios: [Option<GpioWrite>; 3],
    pub avout: AvoutAction,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MuxField { Output, Channel, Format, Source, Clock }
impl MuxField {
    pub const fn mask(self) -> u32 {
        match self { Self::Output => 0x10000, Self::Channel => 0x7000, Self::Format => 0xf00, Self::Source => 0x60000, Self::Clock => 0xf0 }
    }
    pub const fn shift(self) -> u32 {
        match self { Self::Output => 16, Self::Channel => 12, Self::Format => 8, Self::Source => 17, Self::Clock => 4 }
    }
}
pub trait RouteRuntime {
    fn store(&mut self, field: RouteField, value: u32);
    fn avout(&mut self, resource: ResourceId, enabled: bool);
    fn gpio(&mut self, write: GpioWrite);
    fn acquire(&mut self, resource: ResourceId) -> ResourceToken;
    fn release(&mut self, token: ResourceToken);
    fn start_delay(&mut self);
    fn enter_critical(&mut self);
    fn read_mux(&mut self) -> u32;
    fn write_mux(&mut self, value: u32);
    fn leave_critical(&mut self);
}
pub fn lookup(state: &mut RouteState, runtime: &mut impl RouteRuntime) -> Option<ProfileId> {
    let index = match state.timing.0 {
        0 | 11 => {
            if state.initialized != RequestState(1) {
                state.resource_primary = ResourceToken(0);
                state.resource_route = ResourceToken(0);
                state.avout_request = RequestState(0);
                state.timing = TimingSelector(11);
                state.requested_source = SourceId(0);
                state.status = RequestState(0);
                state.auxiliary = RequestState(0);
                state.secondary_status = RequestState(0);
                state.recorded_source = SourceId(0);
                state.current_profile = ProfileSelection::None;
                state.initialized = RequestState(1);
                for (field, value) in [
                    (RouteField::ResourcePrimary, 0), (RouteField::ResourceRoute, 0),
                    (RouteField::AvoutRequest, 0), (RouteField::Timing, 11),
                    (RouteField::RequestedSource, 0), (RouteField::Status, 0),
                    (RouteField::Auxiliary, 0), (RouteField::SecondaryStatus, 0),
                    (RouteField::RecordedSource, 0), (RouteField::CurrentProfile, 8),
                    (RouteField::Initialized, 1),
                ] { runtime.store(field, value); }
            }
            return None;
        }
        1..=10 if state.status == RequestState(1) => match state.requested_source.0 {
            2 => 2, 3..=7 => 6, 8 | 9 => 7, _ => 1,
        },
        1..=10 => if state.secondary_status == RequestState(1) { 0 } else { 3 },
        _ => return None,
    };
    ProfileId::parse(index)
}
pub fn route(id: ProfileId, state: &mut RouteState, profile: &RouteProfile, runtime: &mut impl RouteRuntime) {
    if state.current_profile == ProfileSelection::Selected(id) { return; }
    if let AvoutAction::Set { resource, enabled } = profile.avout {
        if (state.avout_request.0 != 0) != enabled {
            runtime.avout(resource, enabled);
            state.avout_request = RequestState(enabled as u32);
            runtime.store(RouteField::AvoutRequest, state.avout_request.0);
        }
    }
    for write in profile.gpios[..2].iter().flatten() { runtime.gpio(*write); }
    match profile.resource {
        ResourceAction::Acquire(resource) if state.resource_route == ResourceToken(0) => {
            state.resource_route = runtime.acquire(resource);
            runtime.start_delay();
        }
        ResourceAction::Release if state.resource_route != ResourceToken(0) => {
            runtime.release(state.resource_route);
            state.resource_route = ResourceToken(0);
            runtime.store(RouteField::ResourceRoute, 0);
        }
        _ => {}
    }
    for (field, value) in [
        (MuxField::Output, profile.output_mux), (MuxField::Channel, profile.channel_mux),
        (MuxField::Format, profile.format_mux), (MuxField::Source, profile.source_mux),
        (MuxField::Clock, profile.clock_mux),
    ] {
        runtime.enter_critical();
        let current = runtime.read_mux();
        runtime.write_mux((current & !field.mask()) | (value.0 << field.shift()));
        runtime.leave_critical();
    }
    if let Some(write) = profile.gpios[2] { runtime.gpio(write); }
    state.current_profile = ProfileSelection::Selected(id);
    runtime.store(RouteField::CurrentProfile, id.code());
}
pub trait ProfileRead { fn profile(&self, id: ProfileId) -> RouteProfile; }
impl ProfileRead for [RouteProfile; 8] {
    fn profile(&self, id: ProfileId) -> RouteProfile { self[id.index()] }
}
pub fn record_source(source: SourceId, state: &mut RouteState, profiles: &impl ProfileRead, runtime: &mut impl RouteRuntime) {
    if let Some(id) = lookup(state, runtime) {
        if state.current_profile != ProfileSelection::Selected(id) {
            route(id, state, &profiles.profile(id), runtime);
        }
    }
    state.recorded_source = source;
    runtime.store(RouteField::RecordedSource, source.0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockToken(pub u32);
pub trait AvoutRuntime {
    fn lock_ownership(&mut self) -> LockToken;
    fn unlock_ownership(&mut self, token: LockToken);
    fn set_request(&mut self, resource: ResourceId, request: RequestState);
    fn request(&mut self, resource: ResourceId) -> RequestState;
    fn read_control(&mut self) -> u32;
    fn enter_critical(&mut self) -> LockToken;
    fn write_control(&mut self, value: u32);
    fn leave_critical(&mut self, token: LockToken);
}
pub fn set_avout_request(resource: ResourceId, enabled: bool, runtime: &mut impl AvoutRuntime) {
    set_avout_request_state(resource, RequestState(enabled as u32), runtime);
}
pub fn set_avout_request_state(resource: ResourceId, request: RequestState, runtime: &mut impl AvoutRuntime) {
    let ownership = runtime.lock_ownership();
    runtime.set_request(resource, request);
    let any = runtime.request(ResourceId(0)) == RequestState(1)
        || runtime.request(ResourceId(1)) == RequestState(1);
    let first = runtime.read_control();
    let change = if first & (1 << 28) == 0 && any {
        true
    } else {
        runtime.read_control() & (1 << 28) != 0 && !any
    };
    if change {
        let critical = runtime.enter_critical();
        let control = runtime.read_control();
        runtime.write_control(if any { control | (1 << 28) } else { control & !(1 << 28) });
        runtime.leave_critical(critical);
        if any { while runtime.read_control() & (1 << 24) == 0 {} }
    }
    runtime.unlock_ownership(ownership);
}

#[cfg(all(target_arch = "arm", feature = "panel"))]
mod abi {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};
    extern "C" {
        fn xm1_power_resource_acquire(resource: u32, output: *mut u32) -> i32;
        fn xm1_power_resource_release(token: u32) -> i32;
        fn xm1_power_software_delay_start(period: u32) -> i32;
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(token: u32);
        fn xm1_power_save_irq() -> u32;
        fn xm1_power_restore_irq(token: u32);
    }
    unsafe fn read(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
    unsafe fn write(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value); } }
    fn state_address(field: RouteField) -> usize {
        0x8e1ff8 + match field {
            RouteField::ResourcePrimary => 0, RouteField::ResourceRoute => 4,
            RouteField::AvoutRequest => 8, RouteField::Timing => 0xc,
            RouteField::RequestedSource => 0x10, RouteField::Status => 0x14,
            RouteField::Auxiliary => 0x18, RouteField::SecondaryStatus => 0x1c,
            RouteField::RecordedSource => 0x20, RouteField::CurrentProfile => 0x24,
            RouteField::Initialized => 0x28,
        }
    }
    unsafe fn load_state() -> RouteState {
        let load = |field| unsafe { read(state_address(field)) };
        RouteState {
            resource_primary: ResourceToken(load(RouteField::ResourcePrimary)),
            resource_route: ResourceToken(load(RouteField::ResourceRoute)),
            avout_request: RequestState(load(RouteField::AvoutRequest)),
            timing: TimingSelector(load(RouteField::Timing)),
            requested_source: SourceId(load(RouteField::RequestedSource)),
            status: RequestState(load(RouteField::Status)),
            auxiliary: RequestState(load(RouteField::Auxiliary)),
            secondary_status: RequestState(load(RouteField::SecondaryStatus)),
            recorded_source: SourceId(load(RouteField::RecordedSource)),
            current_profile: ProfileSelection::from_stock(load(RouteField::CurrentProfile)),
            initialized: RequestState(load(RouteField::Initialized)),
        }
    }
    struct RomProfiles;
    impl ProfileRead for RomProfiles {
        fn profile(&self, id: ProfileId) -> RouteProfile {
            let base = 0x7ceeb8 + id.index() * 0x50;
            let word = |offset| unsafe { read(base + offset) };
            let gpio = |offset| {
                if word(offset + 8) == 0 { return None; }
                GpioPin::parse(word(offset)).map(|pin| GpioWrite { pin, high: word(offset + 4) & 1 != 0 })
            };
            RouteProfile {
                source_mux: MuxValue(word(0)), output_mux: MuxValue(word(4)),
                channel_mux: MuxValue(word(8)), format_mux: MuxValue(word(0xc)),
                clock_mux: MuxValue(word(0x10)),
                resource: if word(0x1c) == 0 { ResourceAction::Keep }
                    else if word(0x18) == 0 { ResourceAction::Release }
                    else { ResourceAction::Acquire(ResourceId(word(0x14))) },
                gpios: [gpio(0x20), gpio(0x2c), gpio(0x38)],
                avout: if word(0x4c) == 0 { AvoutAction::Keep }
                    else { AvoutAction::Set { resource: ResourceId(word(0x44)), enabled: word(0x48) != 0 } },
            }
        }
    }
    struct Native { critical: LockToken }
    impl RouteRuntime for Native {
        fn store(&mut self, field: RouteField, value: u32) { unsafe { write(state_address(field), value); } }
        fn avout(&mut self, resource: ResourceId, enabled: bool) { set_avout_request(resource, enabled, self); }
        fn gpio(&mut self, gpio: GpioWrite) { unsafe { write(gpio.pin.address(), gpio.pin.word(gpio.high)); } }
        fn acquire(&mut self, resource: ResourceId) -> ResourceToken {
            unsafe {
                let output = state_address(RouteField::ResourceRoute);
                let _ = xm1_power_resource_acquire(resource.0, output as *mut u32);
                ResourceToken(read(output))
            }
        }
        fn release(&mut self, token: ResourceToken) { unsafe { let _ = xm1_power_resource_release(token.0); } }
        fn start_delay(&mut self) { unsafe { let _ = xm1_power_software_delay_start(1); } }
        fn enter_critical(&mut self) { self.critical = LockToken(unsafe { xm1_power_save_irq() }); }
        fn read_mux(&mut self) -> u32 { unsafe { read(0xfff88024) } }
        fn write_mux(&mut self, value: u32) { unsafe { write(0xfff88024, value); } }
        fn leave_critical(&mut self) { unsafe { xm1_power_restore_irq(self.critical.0); } }
    }
    impl AvoutRuntime for Native {
        fn lock_ownership(&mut self) -> LockToken { LockToken(unsafe { xm1_power_interrupt_lock() }) }
        fn unlock_ownership(&mut self, token: LockToken) { unsafe { xm1_power_interrupt_unlock(token.0); } }
        fn set_request(&mut self, resource: ResourceId, request: RequestState) {
            unsafe { write(0x409c24c4usize.wrapping_add((resource.0 as usize).wrapping_mul(4)), request.0); }
        }
        fn request(&mut self, resource: ResourceId) -> RequestState {
            RequestState(unsafe { read(0x409c24c4 + resource.0 as usize * 4) })
        }
        fn read_control(&mut self) -> u32 { unsafe { read(0xfff88048) } }
        fn enter_critical(&mut self) -> LockToken { LockToken(unsafe { xm1_power_save_irq() }) }
        fn write_control(&mut self, value: u32) { unsafe { write(0xfff88048, value); } }
        fn leave_critical(&mut self, token: LockToken) { unsafe { xm1_power_restore_irq(token.0); } }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_record_source(source: u32) {
        let mut state = unsafe { load_state() };
        record_source(SourceId(source), &mut state, &RomProfiles, &mut Native { critical: LockToken(0) });
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_avout_request(resource: u32, request: u32) -> i32 {
        set_avout_request_state(ResourceId(resource), RequestState(request), &mut Native { critical: LockToken(0) });
        0
    }
}
#[cfg(all(target_arch = "arm", feature = "panel"))]
pub use abi::{xm1_scanout_avout_request, xm1_scanout_panel_record_source};
