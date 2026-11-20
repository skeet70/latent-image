//! Qualification view of the acquisition state (issue 117, X-M1 1.50).
//!
//! `composition::qualification` writes the live branch's grid to the stock 1236-byte grid at
//! `0x16bf370`, which `store_selection_window` already places from
//! `AcquisitionState::grids[Branch::Live]` (`acquisition_ports.rs:609`,
//! `stock/acquisition.rs::grid_address`). The view exposes the same array by value.

use super::AcquisitionState;
use xm1_saclac_interface::calc::AcquisitionGrid;

/// The live branch's grid, the one at `0x16bf370`.
pub fn live_grid(state: &AcquisitionState) -> AcquisitionGrid { state.grids[0] }
