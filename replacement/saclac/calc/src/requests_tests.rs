use crate::requests::{request_acquisition, Peers};
use std::vec::Vec;
use xm1_saclac_interface::calc::{AcquisitionGrid, Calc1Payload, CalcPayload};
use xm1_saclac_interface::round_acquisition::AcquisitionRequest;
use xm1_saclac_interface::stock::StockCallbackWord;

#[derive(Debug, PartialEq)]
enum Seen { Grid(u8), Calc(CalcPayload), Calc1(Calc1Payload) }

#[derive(Default)]
struct Recorder(Vec<Seen>);

impl Peers for Recorder {
    type Error = ();
    fn calc(&mut self, payload: &CalcPayload) -> Result<(), ()> { self.0.push(Seen::Calc(*payload)); Ok(()) }
    fn calc1(&mut self, payload: &Calc1Payload) -> Result<(), ()> { self.0.push(Seen::Calc1(*payload)); Ok(()) }
    fn store_acquisition_grid(&mut self, grid: &AcquisitionGrid) { self.0.push(Seen::Grid(grid.0[0])); }
}

#[test]
fn acquisition_stores_its_grid_before_the_calc1_request() {
    let mut peers = Recorder::default();
    let mut grid = [0; 1236];
    grid[0] = 9;
    let request = AcquisitionRequest { mode: 5, ..AcquisitionRequest::default() };
    let callback = StockCallbackWord(0x00f3_3568);
    request_acquisition(&mut peers, &AcquisitionGrid(grid), request, callback).unwrap();
    assert_eq!(peers.0, [Seen::Grid(9), Seen::Calc1(Calc1Payload::Acquisition { request, callback })]);
}
