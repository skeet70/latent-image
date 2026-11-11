//! X-M1 1.50 capture request ingress at feac00, feae18 and feae4c.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackAddress(u32);

impl CallbackAddress {
    pub const fn from_snapshot(value: u32) -> Self { Self(value) }
    pub const fn get(self) -> u32 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CaptureDescriptor([u8; 0x98]);

impl CaptureDescriptor {
    pub const fn from_snapshot(bytes: [u8; 0x98]) -> Self { Self(bytes) }
    pub const fn bytes(&self) -> &[u8; 0x98] { &self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct SmallCaptureDescriptor([u8; 0x40]);

impl SmallCaptureDescriptor {
    pub const fn from_snapshot(bytes: [u8; 0x40]) -> Self { Self(bytes) }
    pub const fn bytes(&self) -> &[u8; 0x40] { &self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureArgument(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionArgument(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureCallbacks {
    pub first: CallbackAddress,
    pub second: CallbackAddress,
    pub third: CallbackAddress,
    pub fourth: CallbackAddress,
    pub fifth: CallbackAddress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request<'a> {
    Capture {
        callbacks: CaptureCallbacks,
        argument: CaptureArgument,
        descriptor: &'a CaptureDescriptor,
    },
    Small {
        callback: CallbackAddress,
        descriptor: &'a SmallCaptureDescriptor,
    },
    Position {
        callback: CallbackAddress,
        first: CaptureArgument,
        second: CaptureArgument,
        position: PositionArgument,
        final_argument: CaptureArgument,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event { Capture, Small, Position }

impl Event {
    pub const fn opcode(self) -> u16 {
        match self { Self::Capture => 0x3f49, Self::Small => 0x3f56, Self::Position => 0x3f57 }
    }
    pub const fn payload_size(self) -> usize {
        match self { Self::Capture => 0xb0, Self::Small => 0x44, Self::Position => 0x14 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputTooSmall { pub required: usize }

impl Request<'_> {
    pub const fn event(&self) -> Event {
        match self { Self::Capture { .. } => Event::Capture, Self::Small { .. } => Event::Small, Self::Position { .. } => Event::Position }
    }

    pub fn encode<'a>(&self, output: &'a mut [u8]) -> Result<&'a [u8], OutputTooSmall> {
        let required = self.event().payload_size();
        if output.len() < required { return Err(OutputTooSmall { required }); }
        let payload = &mut output[..required];
        match self {
            Self::Capture { callbacks, argument, descriptor } => {
                word(payload, 0, callbacks.first.get());
                word(payload, 4, callbacks.third.get());
                word(payload, 8, callbacks.second.get());
                word(payload, 12, argument.0);
                word(payload, 16, callbacks.fourth.get());
                word(payload, 20, callbacks.fifth.get());
                payload[24..].copy_from_slice(descriptor.bytes());
            }
            Self::Small { callback, descriptor } => {
                payload[..0x40].copy_from_slice(descriptor.bytes());
                word(payload, 0x40, callback.get());
            }
            Self::Position { callback, first, second, position, final_argument } => {
                word(payload, 0, first.0);
                word(payload, 4, second.0);
                payload[8..10].copy_from_slice(&position.0.to_le_bytes());
                // The stock wrapper leaves the halfword at offset 10 uninitialized.
                payload[10..12].fill(0);
                word(payload, 12, final_argument.0);
                word(payload, 16, callback.get());
            }
        }
        Ok(payload)
    }

}

fn word(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

use xm1_power_lifecycle::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub fn helper_ffc1ec<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(0x016f_814c), 0)?;
    for (index, kind) in [3, 4, 5, 6, 13, 14, 15, 16].into_iter().enumerate() {
        xm1_power_lifecycle::record_hardware_deep_banks::helper_edae88(context, kind, GuestAddress::new(0x016f_812c + index as u32 * 4))?;
    }
    Ok(())
}


#[cfg(test)]
#[path = "configuration_capture_runtime_tests.rs"]
mod tests;
