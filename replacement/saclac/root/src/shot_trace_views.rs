#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::shot_trace::{CalculationBlock, SelectedCells}, stock::calculation::{BlockOffset, Cell, Pass}};
use xm1_saclac_interface::{calculation::{AreaIndex, GridPass, ResultPass}, selection::{CellCode, SelectionSlot}};
use xm1_saclac_profile_geometry::services::LevelPair;
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::stock::state;

pub(crate) struct Block<'a, M>(pub(crate) &'a M);
impl<M: Machine> CalculationBlock for Block<'_, M> {
    fn read_u8(&mut self, at: BlockOffset) -> u8 { self.0.read_u8(state::BLOCK.get().wrapping_add(at.get())) }
    fn read_u16(&mut self, at: BlockOffset) -> u16 { self.0.read_u16(state::BLOCK.get().wrapping_add(at.get())) }
    fn read_u32(&mut self, at: BlockOffset) -> u32 { self.0.read_u32(state::BLOCK.get().wrapping_add(at.get())) }
}

pub(crate) struct Cells<'a, M>(pub(crate) &'a M);
impl<M: Machine> SelectedCells for Cells<'_, M> {
    fn cell(&mut self, pass: Pass, index: u8) -> Cell {
        let pass = match pass {
            Pass::Plain => ResultPass::Plain,
            Pass::Grid(pass) => ResultPass::Grid(GridPass::new(u32::from(pass)).expect("trace grid pass")),
        };
        let at = state::cell(SelectionSlot { kind: None, pass, index: AreaIndex::new(index).expect("trace area index") }).get();
        let cell = CellCode { column: self.0.read_u8(at), row: self.0.read_u8(at.wrapping_add(1)) }.cell();
        Cell { column: cell.column, row: cell.row }
    }
    fn cell_level(&mut self, cell: Cell) -> u32 { LevelPair([cell.column, cell.row]).encoded_class() }
}
