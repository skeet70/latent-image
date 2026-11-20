#![forbid(unsafe_code)]

use xm1_focus_measurement_round::{lanes::Selection, pipeline::{Exposure, GainStep, HistoryStatistics, Pipeline, PipelineSnapshot, PipelineWord, Statistics, TimingWord}, planner::{Cadence, Mode, Work}, profile::{FrameInterval, FrameRate, GridCell, IntervalHistory, Profile, ProfileGeometry as RecordGeometry, ProfileRecords, PublishedResult, StatisticsInUse, WindowPosition, WindowRecord, WindowStatus}, record::{Adjustment, AdjustmentWords, AlternateSearch, Power, Request, Settings, Trace, TraceMark}, replies::{Effect, Effects}};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{round::GeometryCalibration, Calc, Calc1, Completed, ImageProcessor, ImgDc, RecordHandle};
use xm1_saclac_profile_geometry::controller::{ProfileGeometry, Tables};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use crate::{geometry::GeometryBindings, services::Services, stock::geometry::Memory as GeometryMemory};

/// One child effect's borrows; dropping the view invalidates its consumed auxiliary result range.
pub struct RoundPorts<'a, M: Machine, R, E, P, I, T> {
    pub services: &'a mut Services<M, R, E, P>,
    pub geometry: &'a mut ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>,
    pub recognition: &'a core::cell::RefCell<xm1_saclac_calc::scene_recognition::SceneRecognition>,
    pub search_record: RecordHandle,
    pub(crate) auxiliary_buffer: Option<u32>,
}

impl<'a, M: Machine, R, E, P, I, T> RoundPorts<'a, M, R, E, P, I, T> {
    pub fn new(services: &'a mut Services<M, R, E, P>, geometry: &'a mut ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>, recognition: &'a core::cell::RefCell<xm1_saclac_calc::scene_recognition::SceneRecognition>, search_record: RecordHandle) -> Self {
        Self { services, geometry, recognition, search_record, auxiliary_buffer: None }
    }
}

impl<M: Machine, R, E, P, I, T> Drop for RoundPorts<'_, M, R, E, P, I, T> {
    fn drop(&mut self) {
        if let Some(buffer) = self.auxiliary_buffer {
            self.services.machine.enter(0x74_0e74, &[buffer as usize, 12]);
        }
    }
}

macro_rules! forward {
    ($contract:path { $(fn $name:ident(&mut self $(, $argument:ident: $type:ty)*) $(-> $result:ty)?;)* }) => {
        impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + xm1_saclac_interface::Statistics, I: Isp, T: Tables>
            $contract for RoundPorts<'_, M, R, E, P, I, T> {
            $(fn $name(&mut self $(, $argument: $type)*) $(-> $result)? {
                <Services<M, R, E, P> as $contract>::$name(self.services $(, $argument)*)
            })*
        }
    };
}
pub(crate) use forward;

forward!(ProfileRecords {
    fn offered(&mut self) -> Option<Selection>;
    fn statistics_in_use(&mut self) -> StatisticsInUse;
    fn set_statistics_in_use(&mut self, record: RecordHandle, in_use: StatisticsInUse);
    fn detach_capture_buffer(&mut self, record: RecordHandle);
    fn invalidate(&mut self, record: RecordHandle, result: PublishedResult);
    fn invalidate_focus_result(&mut self);
    fn grants_measurement_history(&mut self, record: RecordHandle) -> bool;
    fn publish_history(&mut self, record: RecordHandle, entry: &xm1_focus_measurement_round::history::HistoryEntry);
    fn clear_history(&mut self, record: RecordHandle);
    fn acquisition_level(&mut self) -> i16;
    fn interval_history(&mut self) -> IntervalHistory;
    fn set_interval_history(&mut self, record: RecordHandle, history: IntervalHistory);
    fn profile(&mut self, record: RecordHandle) -> Profile;
    fn geometry(&mut self, record: RecordHandle) -> RecordGeometry;
    fn window(&mut self) -> WindowRecord;
    fn store_window_status(&mut self, record: RecordHandle, status: WindowStatus);
    fn store_window_position(&mut self, record: RecordHandle, position: WindowPosition);
});
forward!(Settings {
    fn search_permitted(&mut self) -> bool;
    fn alternate_search(&mut self) -> AlternateSearch;
    fn cadence(&mut self, work: Work, mode: Mode) -> Cadence;
    fn frame_rate_threshold(&mut self) -> FrameRate;
    fn keeps_interval_history(&mut self) -> bool;
    fn geometry_calibration(&mut self) -> GeometryCalibration;
    fn traces_measurement(&mut self) -> bool;
});
forward!(Pipeline {
    fn capture(&mut self, statistics: Statistics);
    fn snapshot(&mut self, statistics: Statistics) -> PipelineSnapshot;
    fn timing(&mut self) -> TimingWord;
    fn frame_interval(&mut self, timing: TimingWord) -> Option<FrameInterval>;
    fn request_mode_six_statistics(&mut self);
    fn take_window_refresh(&mut self) -> bool;
    fn history_statistics(&mut self, cell: GridCell) -> HistoryStatistics;
});
forward!(Exposure {
    fn gain_offset(&mut self, step: GainStep) -> u32;
    fn exposure_value(&mut self, exposure: [PipelineWord; 2]) -> i16;
    fn timing_offset(&mut self, timing: TimingWord) -> u16;
    fn relative_exposure(&mut self, exposure: PipelineWord) -> i16;
});
forward!(Power { fn request_measurement_power(&mut self, held: bool); });
forward!(Adjustment { fn adjustment_words(&mut self) -> AdjustmentWords; });
forward!(Trace { fn trace(&mut self, mark: TraceMark); });
forward!(Effects { fn emit(&mut self, effect: Effect<'_>); });
forward!(xm1_focus_measurement_round::record::Ports { fn request(&mut self, request: Request); });
forward!(xm1_focus_measurement_round::window::Ports {
    fn window_result(&mut self, completed: Completed<'_>) -> Option<xm1_focus_measurement_round::window::WindowResult>;
});
