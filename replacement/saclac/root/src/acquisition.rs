#![forbid(unsafe_code)]

//! The live and S1 acquisitions share one selection and use the same record, calculation and
//! peer bindings. Guest layouts and retained entry calls stay in `stock::acquisition`.

use xm1_focus_measurement_round::acquisition::ports::*;
use xm1_focus_measurement_round::acquisition::selection::SelectionMemory;
use xm1_focus_measurement_round::auxiliary::ResourceIndex;
use xm1_focus_measurement_round::pipeline::{Exposure, GainStep, HistoryStatistics, Pipeline, PipelineSnapshot, PipelineWord, Statistics, TimingWord};
use xm1_focus_measurement_round::profile::{FrameInterval, GridCell, ProfileSelection, RetainedWord};
use xm1_focus_measurement_round::record::{Adjustment, AdjustmentWords, Trace, TraceMark};
use xm1_saclac_calc::adjust::{configuration, profile_adjustment as programs, rate_policy, sample_grid};
use xm1_saclac_calc::exposure::ports::{Properties, Property};
use xm1_saclac_config::table::{self, Row, Step};
use xm1_saclac_interface::calc::{AcquisitionGrid, Calc1Payload, CalcPayload, SharedAddress};
use xm1_saclac_interface::round_acquisition::*;
use xm1_saclac_interface::stock::{self as wire, round_acquisition as codec};
use xm1_saclac_interface::{Calc, Calc1, Calc1Request, CalcRequest, Completed, ImgDc, ImgDcOperation, ImgDcRequest, RecordHandle, Statistics as StatisticsPeer, StatisticsOperation, StatisticsRequest};
use xm1_saclac_profile_records::field::Index;
use xm1_saclac_profile_records::stock::{record, shared};
use xm1_saclac_profile_records::{MeasurementSelection, Memory, SharedHeaderByte, StockEntries, WindowStatus};
use xm1_saclac_shared::lens::{self, LensControl};
use xm1_saclac_shared::settings::{AdjustmentKey, Adjustments};
use xm1_saclac_shared::stock::{Machine, Stock};

use crate::services::Services;
use crate::stock::acquisition as boundary;

/// The state retained between the two children and the controllers' selection-value setter.
pub struct AcquisitionState {
    pub(crate) selection: SelectionMemory,
    grids: [AcquisitionGrid; 2],
}

impl Default for AcquisitionState {
    fn default() -> Self { Self { selection: SelectionMemory::new(), grids: [AcquisitionGrid([0; 1236]); 2] } }
}

fn branch_index(branch: Branch) -> usize { match branch { Branch::Live => 0, Branch::S1 => 1 } }
fn profile_selection(word: u32) -> ProfileSelection { match word { 0 => ProfileSelection::Unselected, 1 => ProfileSelection::Record, other => ProfileSelection::Other(other as u8) } }
fn selection_word(selection: ProfileSelection) -> u32 { match selection { ProfileSelection::Unselected => 0, ProfileSelection::Record => 1, ProfileSelection::Other(word) => u32::from(word) } }
static EMPTY_GRID: AcquisitionGrid = AcquisitionGrid([0; 1236]);

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Services<M, R, E, P> {
    /// The controllers' 0xf251c4 report; S1 consumes it after restoring the selection.
    pub fn mark_selection_changed(&mut self) { self.acquisition.selection.mark_value_changed(); }
}



impl<M: Machine + Clone, R: Memory, E: StockEntries, P: StatisticsPeer> Pipeline for Services<M, R, E, P> {
    fn capture(&mut self, statistics: Statistics) { boundary::capture(&self.machine, statistics) }
    fn snapshot(&mut self, statistics: Statistics) -> PipelineSnapshot {
        xm1_focus_measurement_round::stock::pipeline_snapshot(&boundary::snapshot(&self.machine, statistics))
    }
    fn timing(&mut self) -> TimingWord { boundary::timing(&self.machine) }
    fn frame_interval(&mut self, timing: TimingWord) -> Option<FrameInterval> { boundary::frame_interval(&self.machine, timing).map(FrameInterval) }
    fn request_mode_six_statistics(&mut self) {
        if self.machine.read_u32(0x0170_3afc) == 0 {
            assert!(self.peers.request(StatisticsRequest {
                operation: StatisticsOperation::Request43,
                parameters: wire::parameters(&[]),
                completions: &[],
            }).is_ok(), "mode-six statistics enqueue failed");
        }
    }
    fn take_window_refresh(&mut self) -> bool { boundary::take_window_refresh(&self.machine) }
    fn history_statistics(&mut self, cell: GridCell) -> HistoryStatistics { boundary::history_statistics(&self.machine, cell) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Exposure for Services<M, R, E, P> {
    fn gain_offset(&mut self, step: GainStep) -> u32 { self.stock.zone_limit(step.0 as u8) as i32 as u32 }
    fn exposure_value(&mut self, exposure: [PipelineWord; 2]) -> i16 { table::combined_word(&self.stock, Step(exposure[0].0), Row(exposure[1].0)) }
    fn timing_offset(&mut self, timing: TimingWord) -> u16 { boundary::timing_offset(&self.machine, timing) }
    fn relative_exposure(&mut self, exposure: PipelineWord) -> i16 { table::relative_word(&self.stock, Row(exposure.0)) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Adjustment for Services<M, R, E, P> {
    fn adjustment_words(&mut self) -> AdjustmentWords {
        AdjustmentWords {
            mode: RetainedWord(u32::from(configuration::shared::mode_is_one(self))),
            recording: RetainedWord(configuration::shared::word(self)),
            controllers: RetainedWord(u32::from(self.search.configuration.controller_flag(configuration::ControllerFlag::First))),
            search: RetainedWord(u32::from(self.search.configuration.refresh())),
        }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Trace for Services<M, R, E, P> {
    fn trace(&mut self, mark: TraceMark) { boundary::trace(&self.machine, boundary::trace_point(mark)) }
}

fn program(program: Program) -> programs::AdjustmentProfile {
    programs::AdjustmentProfile::new(program.0).expect("an acquisition program is a profile table index")
}

fn selection(code: u32) -> MeasurementSelection {
    match code { 0 => MeasurementSelection::Unselected, 1 => MeasurementSelection::Record, other => MeasurementSelection::Other(other as u8) }
}

fn treatments(inputs: &ModeInputs) -> xm1_saclac_calc::adjust::scene_policy::TreatmentInputs {
    xm1_saclac_calc::adjust::scene_policy::TreatmentInputs {
        active: inputs.grid_kind, subjects: u32::from(inputs.grid_byte),
        measurement_selection: selection(selection_word(inputs.selection)), scene: inputs.updated_position,
        class: inputs.category, callback_armed: inputs.mode_flag,
        held: inputs.change_category, single_subject: inputs.grid_flag,
    }
}

fn slot_index(slot: PositionSlot) -> Index<5> {
    Index::new(match slot { PositionSlot::Reported => 1, PositionSlot::Lens => 2, PositionSlot::Current => 3, PositionSlot::Selected => 4 }).unwrap()
}

#[path = "acquisition_ports.rs"]
mod binding;
pub use binding::Acquisition;


pub(crate) struct CalculationPeers<'a, M, P> { pub(crate) machine: &'a mut M, pub(crate) peers: &'a mut P }

impl<M: Machine, P: Calc + Calc1> xm1_saclac_calc::requests::Peers for CalculationPeers<'_, M, P> {
    type Error = ();
    fn calc(&mut self, payload: &CalcPayload) -> Result<(), ()> {
        let (operation, bytes) = wire::calc_payload(payload);
        Calc::request(self.peers, CalcRequest { operation, parameters: wire::parameters(bytes.bytes()), completions: &[] }).map_err(|_| ())
    }
    fn calc1(&mut self, payload: &Calc1Payload) -> Result<(), ()> {
        let (operation, bytes) = wire::calc1_payload(payload);
        Calc1::request(self.peers, Calc1Request { operation, parameters: wire::parameters(bytes.bytes()), completions: &[] }).map_err(|_| ())
    }
    fn store_acquisition_grid(&mut self, grid: &AcquisitionGrid) {
        boundary::place(self.machine, boundary::CALC1_GRID.0, &grid.0);
        boundary::clean(self.machine, boundary::CALC1_GRID.0, wire::ACQUISITION_GRID.2);
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: Calc + Calc1> xm1_saclac_calc::requests::Peers for Services<M, R, E, P> {
    type Error = ();
    fn calc(&mut self, payload: &CalcPayload) -> Result<(), ()> {
        xm1_saclac_calc::requests::Peers::calc(&mut CalculationPeers { machine: &mut self.machine, peers: &mut self.peers }, payload)
    }
    fn calc1(&mut self, payload: &Calc1Payload) -> Result<(), ()> {
        xm1_saclac_calc::requests::Peers::calc1(&mut CalculationPeers { machine: &mut self.machine, peers: &mut self.peers }, payload)
    }
    fn store_acquisition_grid(&mut self, grid: &AcquisitionGrid) {
        xm1_saclac_calc::requests::Peers::store_acquisition_grid(&mut CalculationPeers { machine: &mut self.machine, peers: &mut self.peers }, grid)
    }
}

#[cfg(saclac_qualification)]
pub mod qualification;

