//! The requests posted to calc and calc1, and the memory calc1 reads with them.

use xm1_saclac_interface::calc::{AcquisitionGrid, Calc1Payload, CalcPayload};
use xm1_saclac_interface::round_acquisition::AcquisitionRequest;
use xm1_saclac_interface::stock::StockCallbackWord;

/// calc and calc1 as saclac reaches them: posts in order, and the memory they read.
pub trait Peers {
    type Error;
    fn calc(&mut self, payload: &CalcPayload) -> Result<(), Self::Error>;
    fn calc1(&mut self, payload: &Calc1Payload) -> Result<(), Self::Error>;
    /// Places the grid where calc1 reads it and makes it visible to calc1.
    fn store_acquisition_grid(&mut self, grid: &AcquisitionGrid);
}

/// Calc1 0x3d00. The grid is in place before the request is posted.
pub fn request_acquisition<P: Peers>(peers: &mut P, grid: &AcquisitionGrid, request: AcquisitionRequest, callback: StockCallbackWord) -> Result<(), P::Error> {
    peers.store_acquisition_grid(grid);
    peers.calc1(&Calc1Payload::Acquisition { request, callback })
}
