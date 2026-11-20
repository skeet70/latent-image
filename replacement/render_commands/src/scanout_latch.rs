//! The scanout latch: the derived event that acknowledges an owned OSD frame to its publisher. It names the commit
//! pass that wrote the frame's buffer to the engine-1 OSD address register `0xfff6e0b4` (`D(1)+0xb4`) and fires
//! when the scheduler delivers the one-shot registered from that pass. It is not panel visibility.
//!
//! Assumptions:
//! - L4 scanout latch: `0xfff6e0b4` holds the value the last engine-1 commit pass wrote to it, and only commit passes
//!   write it (reports/display-render-contracts/scanout.md, L4).
//! - L1 interrupt ordering: stock commits engine 1 on interrupt source 14 (line `0x38`); scheduler one-shots are
//!   delivered on sources 9/10 (line `0x37`). Their relative order is not established; the latch fires at the
//!   one-shot's delivery, when stock `0x6d7f30` would post `0xfefe`.
//! - The engine-1 commit drains the secondary callback list unconditionally after its register writes, so the pass
//!   that drains the bridge queued after `arm` is the first engine-1 pass after `arm`.
//! - The stock interrupt lock `0x7235d8`/`0x7236b0` held around `arm` and the queue excludes commit passes from that
//!   interval, and held around every other call here serializes task 73, the commit drain and the delivery.
//!
//! Under these, a register value at the bridge that names the armed buffer and differs from the value read at `arm`
//! was written by the pass running the bridge.

use super::publisher::FrameId;

/// Low 30 bits of an OSD buffer address, as the engine-1 shadow and `0xfff6e0b4` carry it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanoutAddress(u32);
impl ScanoutAddress {
    pub const fn new(address: u32) -> Self { Self(address & 0x3fff_ffff) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Armed { frame: FrameId, buffer: ScanoutAddress, before: ScanoutAddress }

pub struct ScanoutLatch {
    armed: Option<Armed>,
    /// The frame of the one-shot registered while none was outstanding.
    associated: Option<FrameId>,
    outstanding: u32,
    latched: Option<FrameId>,
}

impl ScanoutLatch {
    pub const fn new() -> Self { Self { armed: None, associated: None, outstanding: 0, latched: None } }

    /// Task 73, immediately before queueing the bridge: `frame` was handed off in `buffer` and the register reads
    /// `register`. Arms nothing when the register already names `buffer`.
    pub fn arm(&mut self, frame: FrameId, buffer: ScanoutAddress, register: ScanoutAddress) {
        self.armed = (buffer != register).then_some(Armed { frame, buffer, before: register });
    }

    /// The bridge, run by the commit pass that drains it, with the register value after that pass's writes and
    /// whether its delivery one-shot was registered. The armed frame is consumed either way.
    #[inline]
    pub fn commit_pass(&mut self, register: ScanoutAddress, registered: bool) {
        let armed = self.armed.take();
        if !registered { return; }
        let first = self.outstanding == 0;
        self.outstanding = self.outstanding.saturating_add(1);
        if let Some(armed) = armed.filter(|armed| first && register == armed.buffer && register != armed.before) {
            self.associated = Some(armed.frame);
        }
    }

    /// The scheduler delivered one of the bridge's one-shots. The associated frame latches once every one-shot
    /// registered up to and including its own has been delivered, whatever their delivery order.
    #[inline]
    pub fn delivered(&mut self) {
        if self.outstanding == 0 { return; }
        self.outstanding -= 1;
        if self.outstanding == 0 {
            if let Some(frame) = self.associated.take() { self.latched = Some(frame); }
        }
    }

    pub fn take(&mut self) -> Option<FrameId> { self.latched.take() }
}

impl Default for ScanoutLatch {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
#[path = "scanout_latch_tests.rs"]
mod tests;
