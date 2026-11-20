#![forbid(unsafe_code)]

use core::cell::{Cell, RefCell};
use xm1_saclac_calc::{exposure::ports::Placement, scene_recognition::SceneRecognition};
use xm1_saclac_controllers::{children::{ChildOutcome, ChildRequest, Children}, layers::*, live_profile::{self, ConfigurationSource, ConfigurationValues, LiveProfile}, live_statistics_profile, replies::{Replies, ReplyCommand, Values}, s1_statistics_profile};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::SharedAddress, statistics::{Source, RESULTS_BYTES}, Calc, Calc1, ClientCallback, CommandKind, Completion, CompletionStatus, ImageProcessor, ImgDc, ImgDcRequest, LensDc, RecordHandle, Statistics, StatisticsRequest};
use xm1_saclac_profile_geometry::{controller::{ProfileGeometry, Tables}, stock::records::{Parameters, StatusSnapshot}};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::{lens::{self, LensBounds}, settings::{ChannelRemap, RemapArguments}, stock::{Machine, Stock}};
use xm1_shot_sequence::{shot_ports::ShotPorts, state::StateMemory};
use crate::{calculation_services::Calculation, controller_geometry::GeometryServices, controller_helpers::Helpers, controller_services::{SharedRecords, WindowExtent}, geometry::GeometryBindings, services::Services, statistics_results::{Delivery, ResultBuffer, ResultWords}, stock::geometry::Memory as GeometryMemory};

type GeometryLayer<M, P, I, T> = ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>;

/// A dispatch-scoped view; every stateful child and model stays with its persistent owner.
pub struct Layers<'a, M, R, E, P, I, T, S, X, A, C, D> {
    pub services: &'a RefCell<Services<M, R, E, P>>,
    pub geometry: &'a RefCell<GeometryLayer<M, P, I, T>>,
    pub recognition: &'a RefCell<SceneRecognition>,
    pub placement: &'a RefCell<A>,
    pub shot: &'a RefCell<ShotPorts<S, X>>,
    pub live_profile: &'a RefCell<LiveProfile>,
    pub live_record: &'a Cell<RecordHandle>,
    pub result_address: SharedAddress,
    pub extent: WindowExtent,
    pub children: &'a mut C,
    pub delivery: &'a mut D,
}

macro_rules! implement {
    ($contract:path { $($body:tt)* }) => {
        impl<M: Machine + Clone, R: Memory, E: StockEntries,
            P: ImageProcessor + ImgDc + Statistics + LensDc + Calc + Calc1,
            I: Isp, T: Tables, S: StateMemory, X, A: Placement, C: Children, D: Delivery>
            $contract for Layers<'_, M, R, E, P, I, T, S, X, A, C, D> { $($body)* }
    };
}
macro_rules! services {
    ($contract:path { $(fn $name:ident(&mut self $(, $argument:ident: $type:ty)*) $(-> $result:ty)?;)* }) => {
        implement!($contract { $(fn $name(&mut self $(, $argument: $type)*) $(-> $result)? {
            <Services<M, R, E, P> as $contract>::$name(&mut *self.services.borrow_mut() $(, $argument)*)
        })* });
    };
}
macro_rules! geometry {
    ($contract:path { $(fn $name:ident(&mut self $(, $argument:ident: $type:ty)*) $(-> $result:ty)?;)* }) => {
        implement!($contract { $(fn $name(&mut self $(, $argument: $type)*) $(-> $result)? {
            <GeometryServices<'_, M, P, I, T> as $contract>::$name(&mut GeometryServices { geometry: self.geometry } $(, $argument)*)
        })* });
    };
}
macro_rules! helpers {
    ($contract:path { $(fn $name:ident(&mut self $(, $argument:ident: $type:ty)*) $(-> $result:ty)?;)* }) => {
        implement!($contract { $(fn $name(&mut self $(, $argument: $type)*) $(-> $result)? {
            self.with_helpers(|view| <Helpers<'_, M, R, E, P, I, T, A> as $contract>::$name(view $(, $argument)*))
        })* });
    };
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + ImgDc + Statistics + LensDc + Calc + Calc1, I: Isp, T: Tables, S: StateMemory, X, A: Placement, C: Children, D: Delivery>
    Layers<'_, M, R, E, P, I, T, S, X, A, C, D> {
    fn with_helpers<V>(&mut self, operation: impl FnOnce(&mut Helpers<'_, M, R, E, P, I, T, A>) -> V) -> V {
        let mut services = self.services.borrow_mut();
        let mut geometry = self.geometry.borrow_mut();
        let mut placement = self.placement.borrow_mut();
        operation(&mut Helpers { calculation: Calculation { services: &mut *services, geometry: &mut *geometry, placement: &mut *placement }, recognition: self.recognition })
    }
    fn fault(&self) { self.services.borrow().machine.enter(crate::stock::FAULT, &[]); }
}

services!(ProfileRecord {
    fn read(&mut self, record: RecordHandle, field: Field) -> u32;
    fn write(&mut self, record: RecordHandle, field: Field, value: u32);
    fn changed(&mut self, record: RecordHandle, change: Change) -> bool;
    fn read_block(&mut self, record: RecordHandle, block: Block, out: &mut [u32]);
    fn write_block(&mut self, record: RecordHandle, block: Block, words: &[u32]);
    fn clear_selection(&mut self, record: RecordHandle);
    fn select_descriptor(&mut self, record: RecordHandle, descriptor: u32);
    fn set_descriptor_pair(&mut self, record: RecordHandle, first: u32, second: u32);
    fn notify_selection_owner(&mut self, record: RecordHandle);
});
services!(Geometry {
    fn value(&mut self, record: RecordHandle, value: GeometryValue) -> u32;
    fn motion(&mut self) -> u32;
    fn outgrown(&mut self, record: RecordHandle, check: GeometryCheck) -> bool;
    fn admits(&mut self, requested: u32, offered: u32) -> bool;
    fn set(&mut self, flag: GeometryFlag, on: bool);
    fn alternate(&mut self) -> bool;
    fn lens_follows(&mut self) -> bool;
    fn reset_selection(&mut self);
});
services!(Configuration {
    fn hold(&mut self, held: bool);
    fn claim_selection(&mut self);
    fn publish(&mut self, record: RecordHandle, publication: Publication);
    fn search_property(&mut self) -> u32;
    fn combine_lens_pair(&mut self, first: u32, second: u32, third: u32) -> u32;
});
services!(MirrorSources {
    fn capture_window(&mut self, mode: u32, ratio: u32) -> Option<[u32; 2]>;
    fn collapse_index(&mut self, index: u32) -> u32;
    fn linearize(&mut self, value: u32) -> u32;
});
services!(Pipeline { fn stop_queried(&mut self) -> bool; });
services!(Adjustment {
    fn mode_position(&mut self, record: RecordHandle) -> u32;
    fn refresh_dispatch(&mut self, record: RecordHandle) -> u32;
    fn first_dispatch(&mut self, record: RecordHandle) -> u32;
});
services!(LensControl {
    fn overridden(&mut self, probe: LensProbe) -> bool;
    fn value(&mut self) -> u32;
    fn position(&mut self) -> u32;
    fn mode_index(&mut self) -> u32;
    fn reference(&mut self) -> u32;
    fn step(&mut self) -> u32;
    fn channel(&mut self) -> (u32, u16);
    fn third_notice(&mut self) -> u32;
    fn configured_channel(&mut self) -> u32;
});
services!(LensCurve {
    fn level(&mut self, steps: u16, base: u32, reference: u32, mode_index: u32) -> i16;
    fn interpolates(&mut self) -> bool;
    fn positions(&mut self, level: i16, channel: u32, query: CurveQuery) -> [u32; 3];
    fn interpolated_positions(&mut self, level: i16, channel: u32, fraction: u16, query: CurveQuery) -> Option<[u32; 3]>;
    fn mapped_value(&mut self, second: u32, first: u32) -> u32;
    fn combine_lens_pair(&mut self, first: u32, second: u32, third: u32) -> u32;
});
services!(CommandParameters { fn word(&mut self, command: CommandKind, index: usize) -> u32; });
services!(StatisticsHelpers {
    fn scale_capture_window(&mut self, mode: u32, ratio: u32) -> Option<[u32; 2]>;
    fn selection_ratio(&mut self, record: RecordHandle) -> [u32; 2];
    fn lens_offset(&mut self, index: u32) -> u32;
});
geometry!(DisplayRecords {
    fn reset(&mut self);
    fn parameters(&mut self) -> Parameters;
    fn status(&mut self) -> StatusSnapshot;
    fn apply_parameters(&mut self, parameters: &Parameters);
    fn apply_status(&mut self, status: &StatusSnapshot);
});
geometry!(RecordMode {
    fn initialize(&mut self) -> bool;
    fn available(&mut self, channel: RecordChannel) -> bool;
    fn begin(&mut self, channel: RecordChannel);
    fn finish(&mut self);
});
geometry!(StatisticsGeometry {
    fn correct_window(&mut self, window: [u16; 3], first: u32, second: u32, class: u32) -> [u16; 3];
    fn linearize(&mut self, value: u32) -> u32;
    fn dump_pipeline(&mut self, buffer: u32);
});
implement!(SharedHelpers {
    fn update_selection_range(&mut self, record: RecordHandle) {
        let bounds = crate::controller_helpers::current_bounds(&mut *self.services.borrow_mut(), record);
        if let Some(bounds) = bounds {
            let machine = self.services.borrow().machine.clone();
            lens::notify_bounds(0, bounds, &mut Stock::new(machine));
        }
    }
    fn clear_calculations(&mut self) { self.with_helpers(|view| view.clear_calculations()); }
    fn searched_lens_pair(&mut self, record: RecordHandle) -> u32 { self.with_helpers(|view| SharedHelpers::searched_lens_pair(view, record)) }
    fn convert_resource(&mut self, record: RecordHandle) { self.with_helpers(|view| view.convert_resource(record)); }
});
helpers!(StatisticsSettings {
    fn parameter_feature(&mut self) -> bool;
    fn set_parameter_feature(&mut self, on: bool);
    fn diagnostic_exports(&mut self) -> DiagnosticExports;
    fn export_row_data(&mut self, buffer: u32);
    fn export_debug(&mut self, buffer: u32);
});
helpers!(S1Values {
    fn descriptor(&mut self, record: RecordHandle) -> u32;
    fn lens_value(&mut self) -> u32;
    fn refresh(&mut self, record: RecordHandle) -> u32;
    fn curve(&mut self, inputs: [u32; 11]) -> u32;
    fn stop_curve(&mut self, stop_mode: u32) -> u32;
    fn unselected_view(&mut self, record: RecordHandle) -> u32;
    fn span(&mut self, record: RecordHandle) -> u32;
    fn unselected(&mut self, record: RecordHandle) -> u32;
    fn corrected_window(&mut self, inputs: &WindowInputs) -> CorrectedWindow;
    fn release_steps(&mut self, position: i16, first: u32, second: u32) -> u16;
    fn trace_snapshot(&mut self);
    fn release_channel(&mut self);
    fn searched_lens_pair(&mut self, record: RecordHandle) -> u32;
    fn extended_results(&mut self) -> bool;
    fn query_records(&mut self, query: &RecordQuery, out: &mut [u8; RECORD_QUERY_BYTES]);
    fn ticks(&mut self) -> u32;
    fn admits_selection(&mut self, requested: u32, offered: u32) -> bool;
});
implement!(ProfileShared {
    fn measured_window(&mut self) -> ([u16; 3], u32) { SharedRecords { services: self.services, extent: self.extent }.measured_window() }
    fn source(&mut self) -> Source { SharedRecords { services: self.services, extent: self.extent }.source() }
});
implement!(ConfigurationSource {
    fn values(&mut self, record: RecordHandle) -> ConfigurationValues { crate::controller_source::LiveSource { services: self.services, shot: self.shot }.values(record) }
    fn take_offset(&mut self, record: RecordHandle) -> u16 { crate::controller_source::LiveSource { services: self.services, shot: self.shot }.take_offset(record) }
});
implement!(Replies {
    fn reply(&mut self, command: ReplyCommand, status: CompletionStatus, values: Values<'_>) {
        let machine = self.services.borrow().machine.clone();
        crate::controller_replies::reply_with(machine, self, command, status, values);
    }
});
implement!(Results {
    fn address(&mut self) -> SharedAddress { self.result_address }
    fn read(&mut self, out: &mut [u8; RESULTS_BYTES]) {
        let services = self.services.borrow();
        ResultBuffer { machine: &services.machine, address: self.result_address, delivery: &mut *self.delivery }.read(out);
    }
    fn deliver(&mut self, callback: ClientCallback) {
        let result = ResultWords::read(&self.services.borrow().machine, self.result_address);
        self.delivery.enter(callback, result);
    }
});
implement!(Children { fn handle(&mut self, request: ChildRequest<'_>) -> ChildOutcome { self.children.handle(request) } });
implement!(Statistics {
    type Error = <P as Statistics>::Error;
    fn resolve_callback(&self, word: xm1_saclac_interface::stock::StockCallbackWord) -> xm1_saclac_interface::stock::StockCallbackWord { self.delivery.resolve_callback(word) }
    fn request(&mut self, request: StatisticsRequest<'_>) -> Result<(), Self::Error> { Statistics::request(&mut self.services.borrow_mut().peers, request) }
});
implement!(ImgDc {
    type Error = <P as ImgDc>::Error;
    fn request(&mut self, request: ImgDcRequest<'_>) -> Result<(), Self::Error> { ImgDc::request(&mut self.services.borrow_mut().peers, request) }
});

implement!(live_profile::Layers {
    type Profile = Self; type Geometry = Self; type Configuration = Self; type Display = Self;
    type RecordMode = Self; type Helpers = Self; type Pipeline = Self; type Source = Self;
    type ImgDc = Self; type Replies = Self; type Lens = Self; type Sources = Self;
    fn profile(&mut self) -> &mut Self { self }
    fn geometry(&mut self) -> &mut Self { self }
    fn configuration(&mut self) -> &mut Self { self }
    fn display(&mut self) -> &mut Self { self }
    fn record_mode(&mut self) -> &mut Self { self }
    fn helpers(&mut self) -> &mut Self { self }
    fn pipeline(&mut self) -> &mut Self { self }
    fn source(&mut self) -> &mut Self { self }
    fn imgdc(&mut self) -> &mut Self { self }
    fn replies(&mut self) -> &mut Self { self }
    fn lens(&mut self) -> &mut Self { self }
    fn mirror_sources(&mut self) -> &mut Self { self }
    fn adjustment_pending(&mut self) -> live_profile::AdjustmentPending { self.services.borrow().adjustment_pending() }
    fn set_adjustment_pending(&mut self, pending: live_profile::AdjustmentPending) {
        self.services.borrow_mut().set_adjustment_pending(pending);
    }
});
implement!(live_statistics_profile::Layers {
    type Profile = Self; type Shared = Self; type Geometry = Self; type Adjustment = Self;
    type Helpers = Self; type Settings = Self; type Parameters = Self; type Results = Self;
    type Replies = Self; type Statistics = Self;
    fn profile(&mut self) -> &mut Self { self }
    fn shared(&mut self) -> &mut Self { self }
    fn geometry(&mut self) -> &mut Self { self }
    fn adjustment(&mut self) -> &mut Self { self }
    fn helpers(&mut self) -> &mut Self { self }
    fn settings(&mut self) -> &mut Self { self }
    fn parameters(&mut self) -> &mut Self { self }
    fn results(&mut self) -> &mut Self { self }
    fn replies(&mut self) -> &mut Self { self }
    fn statistics(&mut self) -> &mut Self { self }
    fn primary_applied(&mut self) -> bool { self.live_profile.borrow().primary_applied() }
    fn fault(&mut self) { Layers::fault(self); }
});
implement!(s1_statistics_profile::Layers {
    type Profile = Self; type Shared = Self; type Helpers = Self; type Geometry = Self;
    type Values = Self; type Results = Self; type Statistics = Self; type Imaging = Self;
    type Children = Self; type RecordMode = Self; type Replies = Self; type Parameters = Self;
    type Lens = Self; type Curve = Self;
    fn profile(&mut self) -> &mut Self { self }
    fn shared(&mut self) -> &mut Self { self }
    fn helpers(&mut self) -> &mut Self { self }
    fn geometry(&mut self) -> &mut Self { self }
    fn values(&mut self) -> &mut Self { self }
    fn results(&mut self) -> &mut Self { self }
    fn statistics(&mut self) -> &mut Self { self }
    fn imaging(&mut self) -> &mut Self { self }
    fn children(&mut self) -> &mut Self { self }
    fn record_mode(&mut self) -> &mut Self { self }
    fn replies(&mut self) -> &mut Self { self }
    fn parameters(&mut self) -> &mut Self { self }
    fn lens(&mut self) -> &mut Self { self }
    fn curve(&mut self) -> &mut Self { self }
    fn live_statistics_record(&mut self) -> RecordHandle { self.live_record.get() }
    fn report_results(&mut self, open: bool) { self.delivery.report_results(open); }
    fn lens_option(&mut self, record: RecordHandle) {
        let machine = {
            let mut services = self.services.borrow_mut();
            let value = crate::controller_replies::parameter_word(&services.machine, CommandKind::Command27, 1);
            services.set_lens_session(record, value, true);
            services.machine.clone()
        };
        crate::controller_replies::scalar_reply(machine, CommandKind::Command27, CompletionStatus(0));
    }
    fn reply_autofocus_lens(&mut self) {
        let machine = self.services.borrow().machine.clone();
        let mut commands = xm1_saclac_shared::commands::Commands::new(Stock::new(machine.clone()));
        let mut lens = Stock::new(machine.clone());
        let mut notices = Stock::new(machine.clone());
        let mut clients = Stock::new(machine);
        commands.reply_autofocus_lens(CompletionStatus(0), &mut lens, &mut notices, &mut clients);
    }
    fn notify_bounds(&mut self, detail: u32, bounds: [u32; 3]) {
        let machine = self.services.borrow().machine.clone();
        lens::notify_bounds(detail, LensBounds(bounds), &mut Stock::new(machine));
    }
    fn mark_selection_changed(&mut self) {
        self.services.borrow_mut().mark_selection_changed();
    }
    fn remap_lens_target(&mut self, request: RemapRequest) -> u16 {
        self.services.borrow_mut().stock.remap(RemapArguments([request.position as i32 as u32, request.reference, 0, request.target, 0, u32::from(request.convert), request.mode_index])).0
    }
    fn move_lens(&mut self, request: LensMove) {
        use xm1_saclac_interface::shared::{MoveMode, RelativeMove};
        let mode = match request.mode_index { 0 => MoveMode::First, 1 => MoveMode::Second, 2 => MoveMode::Third, _ => { self.fault(); return; } };
        let request = RelativeMove { callback: request.callback, step: request.steps, mode, words: request.words };
        let refused = lens::move_relative(&request, &[Completion::Lens35], &mut self.services.borrow_mut().peers).is_err();
        if refused { self.fault(); }
    }
    fn fault(&mut self) { Layers::fault(self); }
});

implement!(LensDriveReplies {
    fn report_progress(&mut self, progress: DriveProgress) {
        let machine = self.services.borrow().machine.clone();
        let adjustments = Stock::new(machine.clone());
        let reply = xm1_saclac_shared::commands::LensDriveReply {
            status: CompletionStatus(18),
            channel: xm1_saclac_shared::settings::Channel(progress.channel as i32),
            words: [u32::from(progress.fourth_carried), progress.reported.0],
        };
        xm1_saclac_shared::commands::Commands::new(Stock::new(machine.clone())).reply_lens_drive(
            reply, &xm1_saclac_config::channel::ConfiguredChannels(&adjustments), &mut Stock::new(machine));
    }
});

implement!(xm1_saclac_controllers::live_replies::Layers {
    type Profile = Self;
    type Lens = Self;
    type Parameters = Self;
    type Replies = Self;
    type LensDrive = Self;
    fn profile(&mut self) -> &mut Self { self }
    fn lens(&mut self) -> &mut Self { self }
    fn parameters(&mut self) -> &mut Self { self }
    fn replies(&mut self) -> &mut Self { self }
    fn lens_drive(&mut self) -> &mut Self { self }
});
