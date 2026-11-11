use super::{edit_value, AudioAction, BankNotificationContext, CacheBits, CacheKeep, CodecRegister, ConfigurationKey};
use crate::audio_release::{write_codec, AudioDevice, CodecWord};
use crate::audio_release::stream_construction::{Address, ConstructionIo, ConstructionMemory};
use crate::primitives::ServiceStatus;
use crate::shutdown::{RequestId, TaskId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockPort { Primary, Secondary }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanelOperation { Hold, Release }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BankPublication { Explicit(BankNotificationContext), Unchanged }

pub trait PhysicalIo: AudioDevice + ConstructionIo {
    fn post(&mut self, memory: &mut impl ConstructionMemory, command: RequestId);
    fn panel(&mut self, memory: &mut impl ConstructionMemory, operation: PanelOperation) -> Result<(), CallbackAddress>;
    fn acquire_resource(&mut self, memory: &mut impl ConstructionMemory, port: ClockPort, output: Address) -> ServiceStatus;
    fn release_resource(&mut self, memory: &mut impl ConstructionMemory, handle: ResourceHandle) -> ServiceStatus;
    fn callback(&mut self, memory: &mut impl ConstructionMemory, callback: CallbackAddress) -> Result<(), CallbackAddress>;
    fn signal(&mut self, memory: &mut impl ConstructionMemory, task: TaskId, bits: u32);
    fn publish_bank(&mut self, memory: &mut impl ConstructionMemory, publication: BankPublication);
}

struct Cpu<'a, M, I> { memory: &'a mut M, io: &'a mut I }
impl<M: ConstructionMemory, I: PhysicalIo> Cpu<'_, M, I> {
    fn word(&self, at: u32) -> u32 { self.memory.word(Address(at)) }
    fn put(&mut self, at: u32, value: u32) { self.memory.put(Address(at), value); }
    fn half(&self, at: u32) -> u16 { self.memory.half(Address(at)) }
    fn cache(&self, register: CodecRegister) -> u16 { self.half(0x82916c + u32::from(register.0) * 2) }
    fn store_cache(&mut self, register: CodecRegister, value: u16) {
        let at = Address(0x82916c + u32::from(register.0) * 2);
        self.memory.put_half(at, value);
    }
    fn edit(&mut self, register: CodecRegister, keep: CacheKeep, bits: CacheBits) {
        let value = edit_value(self.cache(register), keep, bits);
        self.store_cache(register, value);
        write_codec(self.io, CodecWord(value));
    }
    fn setting(&self, key: ConfigurationKey) -> u16 {
        u16::from(self.memory.byte(Address(self.word(0x96ca50)).offset(u32::from(key.0))))
    }
    fn load(&mut self, register: CodecRegister, key: ConfigurationKey, keep: CacheKeep, preclear: bool) {
        let value = if preclear {
            let preserved = self.cache(register) & keep.0;
            self.store_cache(register, preserved);
            preserved | self.setting(key)
        } else {
            let value = self.setting(key);
            value | self.cache(register) & keep.0
        };
        self.store_cache(register, value);
        write_codec(self.io, CodecWord(value));
    }
    fn locked_edit(&mut self, register: CodecRegister, keep: CacheKeep, bits: CacheBits) {
        let _ = self.io.lock_codec();
        self.edit(register, keep, bits);
        let _ = self.io.unlock_codec();
    }
    fn wait_milliseconds(&mut self, milliseconds: u32) {
        let reload = self.word(0xfff91004);
        let frequency = 10_000_000 / reload;
        let target = milliseconds.wrapping_mul(1000) / frequency;
        let mut previous = self.word(0xfff91004).wrapping_sub(self.word(0xfff91000));
        let mut elapsed = 0u32;
        loop {
            let next = self.word(0xfff91004).wrapping_sub(self.word(0xfff91000));
            elapsed = elapsed.wrapping_add(if previous >= next { previous.wrapping_sub(next) } else { reload.wrapping_add(previous).wrapping_add(1).wrapping_sub(next) });
            if elapsed >= target { break; }
            previous = next;
        }
    }
    fn bank(&mut self, first: ConfigurationKey, preclear: bool) {
        if preclear {
            for index in 0..9u16 {
                let register = CodecRegister(if index == 8 { 0x2c } else { 0x18 + index as u8 });
                self.load(register, ConfigurationKey(first.0 + index), CacheKeep(0xff00), true);
            }
        } else {
            for index in 0..19u16 {
                let register = CodecRegister(match index { 0..=15 => 0x18 + index as u8, 16 => 0x2c, 17 => 0x2d, _ => 0x2f });
                self.load(register, ConfigurationKey(first.0 + index), CacheKeep(0xff00), false);
                if index < 7 { self.wait_milliseconds(10); }
            }
        }
    }
    fn gain(&mut self) {
        let mode = self.word(0x8fc758);
        let index = self.memory.byte(Address(0x8fc750));
        let table = match mode {
            0 | 1 | 2 | 5 if index <= 10 => 0x7d1df8,
            3 if index.wrapping_sub(1) < 3 => 0x7d168a,
            4 if index.wrapping_sub(1) < 3 => 0x7d13f2,
            _ => return,
        };
        let key = ConfigurationKey(self.half(table + u32::from(index) * 2));
        self.load(CodecRegister(0x0f), key, CacheKeep(0xffc0), false);
    }
    fn update_clock(&mut self, at: u32, clear: u32, set: u32) {
        let saved = self.io.enter();
        self.put(at, self.word(at) & !clear | set);
        self.io.leave(saved);
    }
    fn release_resource(&mut self, port: ClockPort) {
        let slot = if port == ClockPort::Secondary { 0x8faf00 } else { 0x8faefc };
        let handle = ResourceHandle(self.word(slot));
        if handle.0 != 0 { let _ = self.io.release_resource(self.memory, handle); self.put(slot, 0); }
    }
    fn acquire_resource(&mut self, port: ClockPort) {
        let slot = Address(if port == ClockPort::Secondary { 0x8faf00 } else { 0x8faefc });
        let _ = self.io.acquire_resource(self.memory, port, slot);
    }
    fn clock_configuration(&mut self, profile: u32) {
        let row = 0x7d16c4 + profile * 20;
        self.update_clock(0xfff88024, 0x700000, self.word(row) << 20);
        self.update_clock(0xfff88040, 3, self.word(row + 8));
        self.update_clock(0xfff88040, 0xf00, self.word(row + 4) << 8);
    }
    fn clock_timing(&mut self, profile: u32, port: ClockPort) {
        let row = 0x7d16c4 + profile * 20;
        let at = if port == ClockPort::Secondary { 0xfff88050 } else { 0xfff88058 };
        self.put(at, (u32::from(self.half(row + 12)) & 0x7ff) << 16 | u32::from(self.half(row + 14)) & 0x3ff);
        self.put(at + 4, (u32::from(self.half(row + 16)) & 0x7ff) << 16 | u32::from(self.half(row + 18)) & 0x3ff);
    }
    fn finish_clock(&mut self) {
        self.update_clock(0xfff88040, 0, 0x10000000);
        while self.word(0xfff88040) & 0x1000000 == 0 { core::hint::spin_loop(); }
    }
    fn clock(&mut self, action: ClockOperation) {
        match action {
            ClockOperation::Input => {
                let mode = self.word(0x8fc758);
                let channels = self.word(0x8fc764);
                let (profile, port) = match (mode, channels) {
                    (0 | 1, 0) => (52, 0), (0 | 1 | 2, 2) => (0, 1),
                    (4 | 5, _) => (1, 0), _ => (0, 0),
                };
                self.update_clock(0xfff88040, 0x10000000, 0);
                self.clock_configuration(profile);
                let format = self.word(0x8fc760);
                let at = 0xfffa0004 + port * 0x1000;
                let bits = match format { 0 => 0x20000, 4 => 0, _ => 0x10000 };
                self.put(at, self.word(at) & !0x30000 | bits);
                let port = if channels == 2 { ClockPort::Secondary } else { ClockPort::Primary };
                let actual = if port == ClockPort::Secondary { 0xfffa1004 } else { 0xfffa0004 };
                self.put(actual, self.word(actual) | 0x1000000);
                self.release_resource(port);
                self.clock_timing(profile, port);
                self.acquire_resource(port);
            }
            ClockOperation::Output => {
                let mode = self.word(0x8fc75c);
                let profile = if mode <= 8 && 0x1df & (1 << mode) != 0 { if mode == 3 { 54 } else { 52 } } else { 0 };
                self.update_clock(0xfff88040, 0x10000000, 0);
                self.clock_configuration(profile);
                self.put(0xfffa0004, self.word(0xfffa0004) & !0x30000 | if self.word(0x8fc760) != 4 { 0x20000 } else { 0 });
                self.release_resource(ClockPort::Primary);
                self.clock_timing(profile, ClockPort::Primary);
                self.acquire_resource(ClockPort::Primary);
            }
            ClockOperation::Fixed => {
                self.update_clock(0xfff88040, 0x10000000, 0);
                let port = if self.word(0x8fc764) == 2 { ClockPort::Secondary } else { ClockPort::Primary };
                self.update_clock(0xfff88024, 0x700000, 0);
                self.update_clock(0xfff88040, 3, 2);
                self.update_clock(0xfff88040, 0xf00, 0x300);
                let at = if port == ClockPort::Secondary { 0xfffa1004 } else { 0xfffa0004 };
                self.put(at, self.word(at) & !1);
                self.put(at, self.word(at) | 0x1000000);
                self.put(at, self.word(at) & !0x30000);
                self.release_resource(port);
                let timing = if port == ClockPort::Secondary { 0xfff88050 } else { 0xfff88058 };
                self.put(timing, 0x9000f); self.put(timing + 4, 0xa0031);
                self.acquire_resource(port);
            }
        }
        self.finish_clock();
    }
    fn invoke(&mut self, action: AudioAction) -> Result<(), u32> {
        use AudioAction::*;
        match action {
            Noop => (),
            Notify13 => self.io.post(self.memory, RequestId(0x3913)), Notify0e => self.io.post(self.memory, RequestId(0x390e)), Notify0c => self.io.post(self.memory, RequestId(0x390c)),
            Notify08 => self.io.post(self.memory, RequestId(0x3908)), Notify07 => self.io.post(self.memory, RequestId(0x3907)), Notify15 => self.io.post(self.memory, RequestId(0x3915)),
            Notify0a => self.io.post(self.memory, RequestId(0x390a)), Notify09 => self.io.post(self.memory, RequestId(0x3909)), Notify05 => self.io.post(self.memory, RequestId(0x3905)),
            PanelHold | PanelRelease => self.io.panel(self.memory, if action == PanelHold { PanelOperation::Hold } else { PanelOperation::Release }).map_err(|address| address.0)?,
            ClockInput => self.clock(ClockOperation::Input), ClockOutput => self.clock(ClockOperation::Output), ClockFixed => self.clock(ClockOperation::Fixed),
            Disable => self.locked_edit(CodecRegister(0), CacheKeep(0xff87), CacheBits(0)),
            Output0309 => self.load(CodecRegister(0x17), ConfigurationKey(0x309), CacheKeep(0xff00), false),
            Output030a => self.load(CodecRegister(0x17), ConfigurationKey(0x30a), CacheKeep(0xff00), false),
            Enable13 => self.edit(CodecRegister(4), CacheKeep(0xffff), CacheBits(0x13)),
            Input0303 => self.load(CodecRegister(0xe), ConfigurationKey(0x303), CacheKeep(0xff00), false),
            Enable1b => self.edit(CodecRegister(4), CacheKeep(0xffff), CacheBits(0x1b)),
            SelectChannels => self.edit(CodecRegister(4), CacheKeep(if self.word(0x8fc764).wrapping_sub(1) <= 1 { 0xffec } else { 0xfff6 }), CacheBits(0)),
            Gain => self.gain(), Format8 => self.edit(CodecRegister(6), CacheKeep(0xfff0), CacheBits(8)),
            SelectFormat => {
                let bits = match self.word(0x8fc760) { 0 => 14, 1 => 13, 2 | 3 => 2, 4 => 8, _ => return Ok(()) };
                self.edit(CodecRegister(6), CacheKeep(0xfff0), CacheBits(bits));
            }
            Pattern5e => self.edit(CodecRegister(4), CacheKeep(0xff00), CacheBits(0x5e)),
            Settings0345 | Settings031f => {
                let first = if action == Settings0345 { 0x345 } else { 0x31f };
                let value = self.setting(ConfigurationKey(first)) & 1;
                self.edit(CodecRegister(8), CacheKeep(if action == Settings0345 { 0xfffe } else { 0xffff }), CacheBits(value));
                self.wait_milliseconds(10);
                self.load(CodecRegister(0x12), ConfigurationKey(first + 1), CacheKeep(0xff00), false);
            }
            Bank0332 => self.bank(ConfigurationKey(0x332), false), Bank030c => self.bank(ConfigurationKey(0x30c), false),
            Output032f => self.load(CodecRegister(0x17), ConfigurationKey(0x32f), CacheKeep(0xff00), false),
            Input032d => self.load(CodecRegister(0xe), ConfigurationKey(0x32d), CacheKeep(0xff00), false),
            Register0332c => self.load(CodecRegister(3), ConfigurationKey(0x32c), CacheKeep(0xfffc), false),
            Gain032d | Gain0307 => {
                let table = if action == Gain032d { 0x7d1c56 } else { 0x7d1b0e };
                let key = ConfigurationKey(self.half(table + self.word(0x8fc754).wrapping_mul(2)));
                self.load(CodecRegister(0xc), key, CacheKeep(0xff80), false);
            }
            Pattern5b => self.edit(CodecRegister(4), CacheKeep(0xff00), CacheBits(0x5b)),
            ClearLatch => {
                let _ = self.io.lock_codec(); self.put(0x8fc770, 0);
                if self.word(0x8fc76c) == 0 { self.edit(CodecRegister(0), CacheKeep(0xffef), CacheBits(0)); }
                let _ = self.io.unlock_codec();
            }
            ClearEnable2 => self.edit(CodecRegister(1), CacheKeep(0xfffd), CacheBits(0)),
            Pattern1b => self.edit(CodecRegister(4), CacheKeep(0xff00), CacheBits(0x1b)),
            Pattern1e => self.edit(CodecRegister(4), CacheKeep(0xff00), CacheBits(0x1e)),
            Enable2 => self.edit(CodecRegister(1), CacheKeep(0xffff), CacheBits(2)),
            SetLatch => {
                let _ = self.io.lock_codec(); self.put(0x8fc770, 1);
                self.edit(CodecRegister(0), CacheKeep(0xffff), CacheBits(0x10));
                let _ = self.io.unlock_codec();
            }
            ClearEnable40 => self.edit(CodecRegister(1), CacheKeep(0xffbf), CacheBits(0)),
            Input0307 => self.load(CodecRegister(0xe), ConfigurationKey(0x307), CacheKeep(0xff00), false),
            Register0306 => self.load(CodecRegister(3), ConfigurationKey(0x306), CacheKeep(0xfffc), false),
            Enable40 => self.edit(CodecRegister(1), CacheKeep(0xffff), CacheBits(0x40)),
            SelectMode => self.edit(CodecRegister(5), CacheKeep(0xff3f), CacheBits(match self.word(0x8fc760) { 0 | 2 | 3 => 0x80, 4 => 0, _ => 0x40 })),
            ClearEnable20 => self.edit(CodecRegister(1), CacheKeep(0xffdf), CacheBits(0)),
            Enable0c => self.edit(CodecRegister(1), CacheKeep(0xffff), CacheBits(0xc)),
            Bank0321 => self.bank(ConfigurationKey(0x321), true),
            Output030b => self.load(CodecRegister(0x17), ConfigurationKey(0x30b), CacheKeep(0xff00), true),
            Input0302 => self.load(CodecRegister(0xe), ConfigurationKey(0x302), CacheKeep(0xff00), true),
            Power20 => self.locked_edit(CodecRegister(0), CacheKeep(0xffff), CacheBits(0x20)),
            Mode26 => self.edit(CodecRegister(5), CacheKeep(0xff08), CacheBits(0x26)),
            Mask10 => self.edit(CodecRegister(0x10), CacheKeep(0xffc8), CacheBits(0)),
            Register0b => self.load(CodecRegister(0xb), ConfigurationKey(0x34b), CacheKeep(0xff07), true),
            Register0a => self.load(CodecRegister(0xa), ConfigurationKey(0x34a), CacheKeep(0xff04), true),
            Register09 => self.load(CodecRegister(9), ConfigurationKey(0x349), CacheKeep(0xff00), true),
            Register08 => self.load(CodecRegister(8), ConfigurationKey(0x348), CacheKeep(0xff8f), true),
            Enable20 => self.edit(CodecRegister(1), CacheKeep(0xffff), CacheBits(0x20)),
            Enable0b => self.edit(CodecRegister(4), CacheKeep(0xffff), CacheBits(0xb)),
            ClearRegister07 => self.edit(CodecRegister(7), CacheKeep(0xffbf), CacheBits(0)),
            TransferReady => {
                self.put(0x93b610, 0);
                let callback = CallbackAddress(self.word(self.word(0x93b5f8).wrapping_add(0x18)));
                if callback.0 != 0 { self.io.callback(self.memory, callback).map_err(|address| address.0)?; }
            }
            DelaySignal => {
                let task = self.word(0x957054); self.put(0x957054, u32::MAX);
                if task != u32::MAX { self.io.signal(self.memory, TaskId(task as u8), 0x800); }
            }
            BankComplete => {
                let context = BankNotificationContext(self.word(0x8fc774));
                self.io.publish_bank(self.memory, BankPublication::Explicit(context));
                self.put(0x8faef0, self.word(0x8fc778));
            }
            BankNotify => self.io.publish_bank(self.memory, BankPublication::Explicit(BankNotificationContext(0))),
            BankNotifyUnchanged => self.io.publish_bank(self.memory, BankPublication::Unchanged),
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
enum ClockOperation { Input, Output, Fixed }

pub fn invoke(memory: &mut impl ConstructionMemory, io: &mut impl PhysicalIo, action: AudioAction) -> Result<(), u32> {
    Cpu { memory, io }.invoke(action)
}
pub fn panel_control(memory: &mut impl ConstructionMemory, io: &mut impl PhysicalIo, value: u32) {
    Cpu { memory, io }.locked_edit(CodecRegister(0), CacheKeep(if value == 0 { 0xffff } else { 0xfff7 }), CacheBits(if value == 0 { 8 } else { 0 }));
}
