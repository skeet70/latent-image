use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};
use super::PANEL_BYTES;

pub(crate) struct StaticBuffers<const N: usize = PANEL_BYTES> {
    taken: AtomicBool,
    pixels: UnsafeCell<[[u8; N]; 2]>,
}

// The atomic token grants the only access to the backing arrays.
unsafe impl<const N: usize> Sync for StaticBuffers<N> {}

impl<const N: usize> StaticBuffers<N> {
    pub(crate) const fn new(fill: u8) -> Self {
        Self { taken: AtomicBool::new(false), pixels: UnsafeCell::new([[fill; N]; 2]) }
    }

    pub(crate) fn take(&'static self) -> [&'static mut [u8]; 2] {
        assert!(self.taken.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok());
        let [first, second] = unsafe { &mut *self.pixels.get() };
        [first, second]
    }
}
