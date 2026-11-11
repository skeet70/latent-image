//! Configuration serial action consumers at the pinned X-M1 1.50 boundary.
//! x70view 55436fc85c0560dd252b0410af6f2427fe198cd3: action 14 = 0x00dbaeb0,
//! action 15 = 0x00dbb16c, sample consumer = 0x00dba5c0.

use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey};
use crate::configuration_i2c_actor::{Device, Error, I2cActor, RegisterIo, TransferId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SerialError { Configuration(BackendError), Driver(Error), Pending }

impl From<BackendError> for SerialError {
    fn from(value: BackendError) -> Self { Self::Configuration(value) }
}
impl From<Error> for SerialError {
    fn from(value: Error) -> Self { Self::Driver(value) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SerialStart { Complete(ActionCompletion), Pending(TransferId) }

pub struct SerialConfiguration {
    driver: I2cActor,
    pending: Option<(TransferId, [ConfigurationKey; 10], bool)>,
}

impl SerialConfiguration {
    pub const fn new() -> Self { Self { driver: I2cActor::new(), pending: None } }

    pub fn action14(&mut self, io: &mut impl RegisterIo, images: &mut ConfigImages<'_>, keys: &[ConfigurationKey; 14]) -> Result<SerialStart, SerialError> {
        if self.pending.is_some() { return Err(SerialError::Pending); }
        let direction = keys[0].read(images)?;
        let selector = keys[1].read(images)?;
        let device = match selector {
            0 => Device::Zero, 2 => Device::Two, 3 => Device::Three, 4 => Device::Five,
            _ => return Ok(SerialStart::Complete(ActionCompletion::service_rejected())),
        };
        let requested = keys[2].read(images)?;
        let length = if selector == 3 { 1 } else if (1..=10).contains(&requested) { requested } else { 0 };
        let register = keys[3].read(images)?;
        let reading = match direction { 0 => true, 1 => false, _ => return Ok(SerialStart::Complete(ActionCompletion::service_rejected())) };
        if reading && selector == 3 { return Ok(SerialStart::Complete(ActionCompletion::service_rejected())); }
        let mut packet = [0; 11];
        packet[0] = register;
        if !reading {
            for index in 0..10 { packet[index + 1] = keys[index + 4].read(images)?; }
        }
        if selector == 2 {
            if reading { for key in &keys[4..] { key.write(images, 0)?; } }
            return Ok(SerialStart::Complete(ActionCompletion::service_success()));
        }
        if reading && length == 0 { return Ok(SerialStart::Complete(ActionCompletion::service_rejected())); }
        let id = self.driver.begin(io, device, &packet[..if reading { 1 } else { length as usize + 1 }], if reading { length } else { 0 })?;
        let mut outputs = [keys[4]; 10];
        outputs.copy_from_slice(&keys[4..]);
        self.pending = Some((id, outputs, reading));
        Ok(SerialStart::Pending(id))
    }

    pub fn interrupt(&mut self, id: TransferId) -> Result<(), SerialError> { Ok(self.driver.interrupt(id)?) }

    pub fn poll(&mut self, io: &mut impl RegisterIo, images: &mut ConfigImages<'_>, id: TransferId) -> Result<Option<ActionCompletion>, SerialError> {
        let Some((pending, outputs, reading)) = self.pending else { return Err(SerialError::Driver(Error::Stale)); };
        if pending != id { return Err(SerialError::Driver(Error::Stale)); }
        match self.driver.poll(io, id) {
            Ok(None) => Ok(None),
            Ok(Some(received)) => {
                self.pending = None;
                if reading {
                    for (index, key) in outputs.iter().enumerate() { key.write(images, received.bytes[index])?; }
                }
                Ok(Some(ActionCompletion::service_success()))
            }
            Err(Error::Bus | Error::Cancelled) => {
                self.pending = None;
                Ok(Some(ActionCompletion::service_rejected()))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn cancel(&mut self, io: &mut impl RegisterIo, id: TransferId) -> Result<(), SerialError> { Ok(self.driver.cancel(io, id)?) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleGeneration(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleTotals(pub [u32; 3]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleTick(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleAction { Complete(ActionCompletion), RestartPending }

pub struct SampleAccumulator {
    generation: u32,
    armed: bool,
    totals: SampleTotals,
    restart: Option<SampleTick>,
}

impl SampleAccumulator {
    pub const fn new() -> Self { Self { generation: 0, armed: false, totals: SampleTotals([0; 3]), restart: None } }

    pub fn begin(&mut self) -> SampleGeneration {
        self.generation = self.generation.wrapping_add(1);
        self.totals = SampleTotals([0; 3]);
        self.armed = true;
        self.restart = None;
        SampleGeneration(self.generation)
    }

    pub fn sample(&mut self, generation: SampleGeneration, values: SampleTotals) -> bool {
        if !self.armed || generation.0 != self.generation { return false; }
        for (total, value) in self.totals.0.iter_mut().zip(values.0) { *total = total.wrapping_add(value); }
        true
    }

    pub fn stop(&mut self) -> [u8; 6] {
        self.armed = false;
        let [a, b, c] = self.totals.0;
        [(a >> 8) as u8, a as u8, (b >> 8) as u8, b as u8, (c >> 8) as u8, c as u8]
    }

    pub fn action15(&mut self, images: &mut ConfigImages<'_>, keys: &[ConfigurationKey; 7], now: SampleTick) -> Result<SampleAction, SerialError> {
        if self.restart.is_some() { return Err(SerialError::Pending); }
        match keys[0].read(images)? {
            0 => {
                let bytes = self.stop();
                for (key, byte) in keys[1..].iter().zip(bytes) { key.write(images, byte)?; }
                Ok(SampleAction::Complete(ActionCompletion::service_success()))
            }
            1 => {
                self.restart = Some(now);
                Ok(SampleAction::RestartPending)
            }
            _ => Ok(SampleAction::Complete(ActionCompletion::service_rejected())),
        }
    }

    pub fn poll_restart(&mut self, now: SampleTick) -> Option<(ActionCompletion, SampleGeneration)> {
        let start = self.restart?;
        if now.0.wrapping_sub(start.0) < 20 { return None; }
        let generation = self.begin();
        Some((ActionCompletion::service_success(), generation))
    }

    pub fn cancel_restart(&mut self) -> Option<ActionCompletion> {
        self.restart.take().map(|_| ActionCompletion::service_rejected())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeripheralStatusBytes(pub [u8; 9]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeripheralFlags(pub [u8; 3]);

pub struct PeripheralStatus {
    status: PeripheralStatusBytes,
    flags: PeripheralFlags,
}

impl PeripheralStatus {
    pub const fn adopt(status: PeripheralStatusBytes, flags: PeripheralFlags) -> Self { Self { status, flags } }

    pub fn accumulate(&mut self, incoming: PeripheralStatusBytes) {
        for (saved, byte) in self.status.0.iter_mut().zip(incoming.0) { *saved |= byte; }
    }

    pub fn publish(&self, images: &mut ConfigImages<'_>, keys: &[ConfigurationKey; 8], completion_saved_value: u32) -> Result<ActionCompletion, BackendError> {
        let s = self.status.0;
        let f = self.flags.0;
        let flags = (f[0] != 0) as u8 | (((f[1] != 0) as u8) << 1) | (((f[2] != 0) as u8) << 2);
        let first = (s[0] != 0) as u8 | (((s[4] != 0) as u8) << 1) | (((s[7] != 0) as u8) << 2) | (((s[8] != 0) as u8) << 3);
        let last = (s[1] != 0) as u8 | (((s[2] != 0) as u8) << 1) | (((s[3] != 0) as u8) << 2) | if s[5] != 0 { 0x28 } else { 0 } | if completion_saved_value == 1 { 0x10 } else { 0 };
        for (key, byte) in keys.iter().zip([0, flags, first, 0, 0, 0, last, 0]) { key.write(images, byte)?; }
        Ok(ActionCompletion::service_success())
    }
}

#[cfg(test)]
mod tests {
    use super::{SampleAccumulator, SampleTotals};

    #[test]
    fn accumulator_wraps_and_publishes_low_halfwords_big_endian() {
        let mut samples = SampleAccumulator::new();
        let generation = samples.begin();
        assert!(samples.sample(generation, SampleTotals([0xffff_ffff, 0x1234_ffff, 0x89ab_cdef])));
        assert!(samples.sample(generation, SampleTotals([2, 2, 0])));
        assert_eq!(samples.stop(), [0, 1, 0, 1, 0xcd, 0xef]);
        assert!(!samples.sample(generation, SampleTotals([1; 3])));
        let next = samples.begin();
        assert!(!samples.sample(generation, SampleTotals([1; 3])));
        assert!(samples.sample(next, SampleTotals([0x1234; 3])));
        assert_eq!(samples.stop(), [0x12, 0x34, 0x12, 0x34, 0x12, 0x34]);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterReadbackResidue(u16);

impl RegisterReadbackResidue {
    pub const fn from_abi(value: u16) -> Self { Self(value) }
}

#[cfg(target_arch = "arm")]
unsafe extern "C" {
    fn xm1_scanout_panel_transfer_frontend(register: u32, value: u32) -> i32;
    fn xm1_scanout_panel_auxiliary_frontend(first: u16, second: u16) -> i32;
    fn xm1_scanout_panel_byte_frontend(first: u16, second: u16) -> i32;
}

#[cfg(target_arch = "arm")]
pub fn action10(images: &mut ConfigImages<'_>, keys: &[ConfigurationKey; 7], residue: RegisterReadbackResidue) -> Result<ActionCompletion, BackendError> {
    let selector = keys[0].read(images)?;
    match selector {
        0 => {
            let value = keys[4].read(images)?;
            xm1_power_lifecycle::audio_release::write_codec(
                &mut xm1_power_lifecycle::audio_release::native::NativeAudioDevice,
                xm1_power_lifecycle::audio_release::CodecWord(u16::from(value)),
            );
        }
        1..=3 => {
            let register = keys[2].read(images)?;
            let value = u16::from_be_bytes([keys[3].read(images)?, keys[4].read(images)?]);
            unsafe {
                match selector {
                    1 => { xm1_scanout_panel_transfer_frontend(u32::from(register), u32::from(value)); }
                    2 => { xm1_scanout_panel_auxiliary_frontend(u16::from(register), value); }
                    _ => { xm1_scanout_panel_byte_frontend(u16::from(register), value); }
                }
            }
        }
        4 => {
            let _register = u16::from_be_bytes([keys[1].read(images)?, keys[2].read(images)?]);
            let _value = keys[4].read(images)?;
        }
        5 | 7 => {
            let _register = u16::from_be_bytes([keys[1].read(images)?, keys[2].read(images)?]);
            let _value = u16::from_be_bytes([keys[3].read(images)?, keys[4].read(images)?]);
        }
        6 | 8 => {
            let _register = u16::from_be_bytes([keys[1].read(images)?, keys[2].read(images)?]);
            let [high, low] = residue.0.to_be_bytes();
            keys[5].write(images, high)?;
            keys[6].write(images, low)?;
        }
        _ => return Ok(ActionCompletion::service_rejected()),
    }
    Ok(ActionCompletion::service_success())
}

#[cfg(test)]
mod caller_tests {
    use super::{PeripheralFlags, PeripheralStatus, PeripheralStatusBytes, SampleAccumulator, SampleAction, SampleTick};
    use crate::backend::ConfigImages;
    use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey};

    #[test]
    fn restart_waits_twenty_ticks_across_wrap_before_completing() {
        let mut bytes = [0; 8];
        bytes[0] = 1;
        let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let keys = core::array::from_fn(|index| ConfigurationKey::from_abi(index as u16));
        let mut samples = SampleAccumulator::new();
        assert_eq!(samples.action15(&mut images, &keys, SampleTick(u32::MAX - 9)), Ok(SampleAction::RestartPending));
        assert!(samples.poll_restart(SampleTick(9)).is_none());
        assert_eq!(samples.poll_restart(SampleTick(10)).map(|(completion, _)| completion), Some(ActionCompletion::service_success()));
        assert!(samples.poll_restart(SampleTick(11)).is_none());
    }

    #[test]
    fn cancelled_restart_cannot_complete_later() {
        let mut bytes = [0; 8];
        bytes[0] = 1;
        let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let keys = core::array::from_fn(|index| ConfigurationKey::from_abi(index as u16));
        let mut samples = SampleAccumulator::new();
        samples.action15(&mut images, &keys, SampleTick(1)).unwrap();
        assert_eq!(samples.cancel_restart(), Some(ActionCompletion::service_rejected()));
        assert!(samples.poll_restart(SampleTick(30)).is_none());
    }

    #[test]
    fn peripheral_publication_preserves_accumulated_bits_and_reserved_zeroes() {
        let mut bytes = [0xa5; 8];
        let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let keys = core::array::from_fn(|index| ConfigurationKey::from_abi(index as u16));
        let mut status = PeripheralStatus::adopt(PeripheralStatusBytes([0, 1, 0, 0, 0, 0, 0, 0, 0]), PeripheralFlags([9, 0, 1]));
        status.accumulate(PeripheralStatusBytes([1, 0, 0, 0, 1, 1, 0, 0, 1]));
        assert_eq!(status.publish(&mut images, &keys, 1), Ok(ActionCompletion::service_success()));
        assert_eq!(bytes, [0, 5, 11, 0, 0, 0, 0x39, 0]);
    }
}
