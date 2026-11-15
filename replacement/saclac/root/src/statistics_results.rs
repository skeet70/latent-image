#![forbid(unsafe_code)]

use core::sync::atomic::{AtomicBool, Ordering};

/// The callback task reads a publication, never the task-43 RefCell or its borrowed controller.
pub struct ReportingGate(AtomicBool);
impl ReportingGate {
    pub const fn new(open: bool) -> Self { Self(AtomicBool::new(open)) }
    pub fn publish(&self, open: bool) { self.0.store(open, Ordering::Release); }
    pub fn is_open(&self) -> bool { self.0.load(Ordering::Acquire) }
}

use xm1_saclac_controllers::layers::Results;
use xm1_saclac_interface::{calc::SharedAddress, statistics::RESULTS_BYTES, ClientCallback};
use xm1_saclac_shared::stock::Machine;

/// The callback receives the complete result by value (f3c010..f3c024).
#[repr(C)]
pub struct ResultWords(pub [u32; RESULTS_BYTES / 4]);

impl ResultWords {
    pub(crate) fn read(machine: &impl Machine, address: SharedAddress) -> Self {
        Self(core::array::from_fn(|index| machine.read_u32(address.0.wrapping_add(4 * index as u32))))
    }
}

pub trait Delivery {
    fn resolve_callback(&self, word: xm1_saclac_interface::stock::StockCallbackWord) -> xm1_saclac_interface::stock::StockCallbackWord { word }
    fn report_results(&mut self, open: bool);
    fn enter(&mut self, callback: ClientCallback, result: ResultWords);
}

/// A view of the peer-written result buffer; there is no second active result store.
pub struct ResultBuffer<'a, M, D> {
    pub machine: &'a M,
    pub address: SharedAddress,
    pub delivery: &'a mut D,
}

impl<M: Machine, D: Delivery> Results for ResultBuffer<'_, M, D> {
    fn address(&mut self) -> SharedAddress { self.address }
    fn read(&mut self, out: &mut [u8; RESULTS_BYTES]) {
        for (index, word) in out.chunks_exact_mut(4).enumerate() {
            word.copy_from_slice(&self.machine.read_u32(self.address.0.wrapping_add(4 * index as u32)).to_le_bytes());
        }
    }
    fn deliver(&mut self, callback: ClientCallback) {
        self.delivery.enter(callback, ResultWords::read(self.machine, self.address));
    }
}
