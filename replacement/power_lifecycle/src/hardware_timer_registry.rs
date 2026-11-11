#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Channel(u8);
impl Channel {
    pub const fn parse(value: u32) -> Option<Self> { if value < 4 { Some(Self(value as u8)) } else { None } }
    pub const fn index(self) -> usize { self.0 as usize }
    pub const fn source(self) -> u32 { self.0 as u32 + 5 }
    pub const fn bank(self) -> usize { 0xfff9_1000 + self.index() * 16 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Callback(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrqToken(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Generation(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Units { Milliseconds, Microseconds, Nanoseconds }
impl Units { pub const fn parse(value: u32) -> Option<Self> { match value { 0 => Some(Self::Milliseconds), 1 => Some(Self::Microseconds), 2 => Some(Self::Nanoseconds), _ => None } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Period(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mode(pub u32);
#[repr(C)]
pub struct HardwareTimerRequest {
    pub period: u32,
    pub units: u32,
    pub mode: u32,
    pub callback: Option<unsafe extern "C" fn()>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Configuration { pub period: Period, pub units: Units, pub mode: Mode, pub callback: Option<Callback> }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum Error { InvalidHandle = -20, AlreadyStarted = -12, AlreadyStopped = -11, InvalidConfiguration = -30 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register { Counter, Reload, Control }
impl Register { pub const fn offset(self) -> usize { match self { Self::Counter => 0, Self::Reload => 4, Self::Control => 8 } } }
pub trait Hardware {
    fn write(&mut self, channel: Channel, register: Register, value: u32);
    fn attach(&mut self, channel: Channel) -> Result<IrqToken, i32>;
    fn detach(&mut self, token: IrqToken) -> i32;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Diagnostic { Acquire = 17, Release = 18 }
pub trait Diagnostics { fn command_error(&mut self, diagnostic: Diagnostic); }
pub fn report_acquire(result: Result<Handle, Error>, diagnostics: &mut impl Diagnostics) -> Result<Handle, Error> {
    if result.is_err() { diagnostics.command_error(Diagnostic::Acquire); }
    result
}
pub fn release(registry: &mut Registry, handle: Handle, diagnostics: &mut impl Diagnostics) -> Result<(), Error> {
    let result = registry.release(handle);
    if result.is_err() { diagnostics.command_error(Diagnostic::Release); }
    result
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timing { pub reload: u32, pub prescaler: u8 }
impl Configuration {
    pub fn timing(self) -> Result<Timing, Error> {
        let value = self.period.0;
        let (prescaler, reload) = match self.units {
            Units::Milliseconds => {
                let bounds = [0x57619, 0xaec33, 0x15d867, 0x2bb0cf, 0x57619e, 0xaec33d, 0x15d867b, 0x2bb0cf7];
                let shift = bounds.iter().position(|bound| value <= *bound).ok_or(Error::InvalidConfiguration)? as u8;
                (shift, ((u64::from(value) * 12000) >> shift) as u32)
            }
            Units::Microseconds => {
                let bounds = [0x15555554, 0x2aaaaaaa, 0x55555554, 0xaaaaaaa9];
                let shift = bounds.iter().position(|bound| value <= *bound).unwrap_or(4) as u8;
                (shift, ((u64::from(value) * 12) >> shift) as u32)
            }
            Units::Nanoseconds => (0, if value <= 0x15555555 { value.wrapping_mul(12) / 1000 } else { 0 }),
        };
        if reload == 0 { Err(Error::InvalidConfiguration) } else { Ok(Timing { reload, prescaler }) }
    }
}
#[repr(C)]
pub struct Registry {
    phases: [u32; 4], selectors: [u32; 4], handles: [u32; 4], irq_tokens: [u32; 4], callbacks: [u32; 4], modes: [u32; 4],
}
impl Registry {
    pub const fn new() -> Self { Self { phases: [0; 4], selectors: [u32::MAX; 4], handles: [0; 4], irq_tokens: [0; 4], callbacks: [0; 4], modes: [0; 4] } }
    fn resolve(&self, handle: Handle) -> Result<Channel, Error> {
        self.handles.iter().position(|value| *value == handle.0 && handle.0 != 0).map(|index| Channel(index as u8)).ok_or(Error::InvalidHandle)
    }
    pub fn acquire(&mut self, channel: Channel, generation: &mut Generation) -> Result<Handle, Error> {
        let index = channel.index();
        if self.handles[index] != 0 { return Err(Error::InvalidHandle); }
        loop { generation.0 = generation.0.wrapping_add(1); if generation.0 != 0 && !self.handles.contains(&generation.0) { break; } }
        self.handles[index] = generation.0;
        self.phases[index] = 1;
        self.selectors[index] = index as u32;
        Ok(Handle(generation.0))
    }
    pub fn start(&mut self, hardware: &mut impl Hardware, handle: Handle, configuration: Configuration) -> i32 {
        let channel = match self.resolve(handle) { Ok(value) => value, Err(error) => return error as i32 };
        let index = channel.index();
        if self.phases[index] != 1 { return Error::AlreadyStarted as i32; }
        let timing = match configuration.timing() { Ok(value) => value, Err(error) => return error as i32 };
        self.phases[index] = 2;
        self.modes[index] = configuration.mode.0;
        hardware.write(channel, Register::Counter, 0);
        hardware.write(channel, Register::Reload, timing.reload);
        let mut status = 0;
        if let Some(callback) = configuration.callback {
            self.callbacks[index] = callback.0;
            match hardware.attach(channel) { Ok(token) => self.irq_tokens[index] = token.0, Err(error) => status = error }
        }
        hardware.write(channel, Register::Control, 3 | (u32::from(timing.prescaler) << 4));
        status
    }
    pub fn stop(&mut self, hardware: &mut impl Hardware, handle: Handle) -> i32 {
        let channel = match self.resolve(handle) { Ok(value) => value, Err(error) => return error as i32 };
        let index = channel.index();
        if self.phases[index] == 1 { return Error::AlreadyStopped as i32; }
        hardware.write(channel, Register::Control, 0);
        hardware.write(channel, Register::Reload, 0);
        let status = if self.irq_tokens[index] == 0 { 0 } else { hardware.detach(IrqToken(self.irq_tokens[index])) };
        self.irq_tokens[index] = 0;
        self.phases[index] = 1;
        status
    }
    pub fn release(&mut self, handle: Handle) -> Result<(), Error> {
        let index = self.resolve(handle)?.index();
        self.selectors[index] = u32::MAX;
        self.phases[index] = 0;
        self.handles[index] = 0;
        Ok(())
    }
    pub fn interrupt(&mut self, hardware: &mut impl Hardware, channel: Channel) -> Callback {
        let index = channel.index();
        if self.modes[index] == 0 { let _ = self.stop(hardware, Handle(self.handles[index])); }
        Callback(self.callbacks[index])
    }
}
#[cfg(all(target_arch = "arm", not(test)))]
#[path = "hardware_timer_registry_native.rs"]
mod native;
#[cfg(test)]
#[path = "hardware_timer_registry_tests.rs"]
mod tests;
