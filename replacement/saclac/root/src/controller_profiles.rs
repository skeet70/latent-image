#![forbid(unsafe_code)]

use core::cell::{Cell, RefCell};
use xm1_saclac_calc::exposure::ports::Placement;
use xm1_saclac_controllers::{children::Children, live_profile::LiveProfile, live_statistics::LiveStatistics, live_view::{Controller, PreparationOverride, Signal}, root::Outcome, s1::StatisticsEvent, s1_statistics::S1Statistics, s1_statistics_leaf::{Leaf, S1Profile}};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{stock::record_handle_from_a2, Calc, Calc1, ImageProcessor, ImgDc, LensDc, RecordHandle, Statistics};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::state::StateMemory;
use crate::{controller_layers::Layers, live_view_ports::View, statistics_results::Delivery};

/// Both live and S1 entries share these profile and preparation owners across queue messages.
pub struct Profiles {
    pub live: RefCell<LiveProfile>,
    pub live_record: Cell<RecordHandle>,
    pub statistics: RefCell<LiveStatistics>,
    pub view: RefCell<Controller>,
    pub s1_statistics: RefCell<S1Statistics>,
    pub s1_profile: RefCell<S1Profile>,
    pub preparation: RefCell<PreparationOverride>,
}
impl Default for Profiles { fn default() -> Self { Self::new() } }
impl Profiles {
    pub fn new() -> Self {
        Self {
            live: RefCell::new(LiveProfile::new()),
            live_record: Cell::new(record_handle_from_a2(u32::MAX)),
            statistics: RefCell::new(LiveStatistics::new()),
            view: RefCell::new(Controller::new()),
            s1_statistics: RefCell::new(S1Statistics::new()),
            s1_profile: RefCell::new(S1Profile::new()),
            preparation: RefCell::new(PreparationOverride::None),
        }
    }

    pub fn view<M: Machine + Clone, R: Memory, E: StockEntries,
        P: ImageProcessor + ImgDc + Statistics + LensDc + Calc + Calc1,
        I: Isp, T: Tables, S: StateMemory, X, A: Placement, C: Children, D: Delivery>(
        &self, signal: Signal<'_>, record: RecordHandle,
        layers: &mut Layers<'_, M, R, E, P, I, T, S, X, A, C, D>,
    ) -> Outcome {
        assert!(core::ptr::eq(&self.live, layers.live_profile));
        assert!(core::ptr::eq(&self.live_record, layers.live_record));
        self.live_record.set(record);
        self.view.borrow_mut().handle(signal, record, &mut self.preparation.borrow_mut(),
            &mut View { layers, statistics: &self.statistics })
    }

    pub fn s1_statistics<M: Machine + Clone, R: Memory, E: StockEntries,
        P: ImageProcessor + ImgDc + Statistics + LensDc + Calc + Calc1,
        I: Isp, T: Tables, S: StateMemory, X, A: Placement, C: Children, D: Delivery>(
        &self, event: StatisticsEvent<'_>, record: RecordHandle,
        layers: &mut Layers<'_, M, R, E, P, I, T, S, X, A, C, D>,
    ) -> Outcome {
        assert!(core::ptr::eq(&self.live, layers.live_profile));
        self.s1_statistics.borrow_mut().send(event, record, &mut Leaf {
            layers,
            profile: &mut self.s1_profile.borrow_mut(),
            live: &mut self.live.borrow_mut(),
            preparation: &mut self.preparation.borrow_mut(),
        })
    }
}
