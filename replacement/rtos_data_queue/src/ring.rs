use crate::env::Env;
use crate::layout::{DataQueueConfig, DataQueueControl, GuestAddress};

/// A creation record's ring capacity, in words.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Capacity(pub u32);

/// One data-queue word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Payload(pub u32);

/// A creation record's caller-supplied ring storage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct RingBase(pub GuestAddress);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct BufferedCount(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct RingIndex(pub u32);

/// A caller-supplied word the body writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct OutWord(pub GuestAddress);

/// The ring view of one queue's creation and control records.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ring {
    config: DataQueueConfig,
    control: DataQueueControl,
}

impl Ring {
    pub const fn new(config: DataQueueConfig, control: DataQueueControl) -> Self {
        Self { config, control }
    }

    pub fn capacity<M: Env>(&self, m: &M) -> Capacity {
        Capacity(m.load_word(self.config.capacity()))
    }

    pub fn count<M: Env>(&self, m: &M) -> BufferedCount {
        BufferedCount(m.load_word(self.control.count()))
    }

    /// A8 `0x48edc` / B8 `0xd8e9c`.
    #[inline(never)]
    pub fn append<M: Env>(self, m: &M, payload: Payload) -> bool {
        let capacity = m.load_word(self.config.capacity());
        let count = m.load_word(self.control.count());
        if count >= capacity {
            return false;
        }
        let tail = m.load_word(self.control.write());
        m.store_word(self.word(m, tail), payload.0);
        m.store_word(self.control.count(), count.wrapping_add(1));
        let tail = tail.wrapping_add(1);
        m.store_word(self.control.write(), if capacity <= tail { 0 } else { tail });
        true
    }

    /// A8 `0x48f44` / B8 `0xd8f04`; a full ring drops its oldest word.
    pub fn force_append<M: Env>(&self, m: &M, payload: Payload) {
        let capacity = m.load_word(self.config.capacity());
        let tail = m.load_word(self.control.write());
        m.store_word(self.word(m, tail), payload.0);
        let tail = tail.wrapping_add(1);
        let tail = if capacity <= tail { 0 } else { tail };
        m.store_word(self.control.write(), tail);
        let count = m.load_word(self.control.count());
        if count < capacity {
            m.store_word(self.control.count(), count.wrapping_add(1));
        } else {
            m.store_word(self.control.read(), tail);
        }
    }

    /// A8 `0x48fa4` / B8 `0xd8f64`; the caller has already checked the count.
    #[inline(never)]
    pub fn pop<M: Env>(self, m: &M, out: OutWord) -> Payload {
        let capacity = m.load_word(self.config.capacity());
        let head = m.load_word(self.control.read());
        let payload = Payload(m.load_word(self.word(m, head)));
        m.store_word(out.0, payload.0);
        m.store_word(self.control.count(), m.load_word(self.control.count()).wrapping_sub(1));
        let head = head.wrapping_add(1);
        m.store_word(self.control.read(), if capacity <= head { 0 } else { head });
        payload
    }

    fn word<M: Env>(&self, m: &M, index: u32) -> GuestAddress {
        GuestAddress(m.load_word(self.config.ring_base()).wrapping_add(index.wrapping_mul(4)))
    }
}
