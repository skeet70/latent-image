#![cfg(target_arch = "arm")]

use core::{cell::RefCell, mem::MaybeUninit, ptr::{addr_of_mut, read_volatile, write_volatile}};
use xm1_kernel::{Context, SlotTable};
use xm1_saclac_calc::{adjust::motion::state::MotionState, scene_recognition::SceneRecognition};
use xm1_saclac_imaging::stock::StockIsp;
use xm1_saclac_interface::{calc::SharedAddress, stock::{self as wire, StockCallbackWord}, Completed, Message};
use xm1_saclac_peers::EmulatorOnly;
use xm1_saclac_profile_geometry::{controller::ProfileGeometry, profile::Profiles, stock::StockTables};
use xm1_saclac_profile_records::stock::{Stock, StockMemory};
use xm1_saclac_search_bindings::RecordBinding;
use xm1_saclac_shared::stock::{Guest, Machine};
use xm1_shot_sequence::shot_ports::ShotPorts;
use crate::{command_source::CommandSource, exposure_memory::ExposurePlacement, exposure_services::ExposureState, geometry::GeometryBindings, peer_context::Poster, peer_services::{self, Peers}, runtime::Runtime, services::Services, shot_services::{self, Libraries, PipelineCallbacks}, stages::{KernelExclusion, StageDriver}};
use super::{callbacks as cb, geometry::Memory as GeometryMemory, stages::StageMemory, statistics::NativeDelivery};

type Peer = Peers<'static, Guest, SlotTable, EmulatorOnly>;
type Isp = StockIsp<KernelExclusion<'static, SlotTable>>;
type Geometry = ProfileGeometry<GeometryMemory<Guest>, GeometryBindings<Guest, Peer, Isp>, StockTables>;
type Placement = ExposurePlacement<'static>;
type External = Libraries<'static, Guest, StockMemory, Stock, Peer, Isp, StockTables, Placement, Peer>;
type ShotMemory = super::shot::ShotMemory<'static, Guest>;
type Shot = ShotPorts<ShotMemory, External>;
type Root = Runtime<'static, Guest, StockMemory, Stock, Peer, Isp, StockTables, ShotMemory, External, Placement, NativeDelivery>;

// Every address lent to a peer comes from immovable factory storage or a declared shared ABI block.
struct Owner {
    poster: Poster<SlotTable>,
    services: RefCell<Services<Guest, StockMemory, Stock, Peer>>,
    geometry: RefCell<Geometry>,
    recognition: &'static RefCell<SceneRecognition>,
    exposure: ExposureState,
    placement: RefCell<Placement>,
    subjects: RefCell<crate::shot_subjects::Workspace>,
    capture: RefCell<crate::shot_capture_workspace::Workspace>,
    motion: RefCell<Option<MotionState>>,
    live_record: RecordBinding,
    s1_record: RecordBinding,
    results: [u32; xm1_saclac_interface::statistics::RESULTS_BYTES / 4],
    shot: RefCell<Shot>,
    root: Root,
}

static mut OWNER: MaybeUninit<Owner> = MaybeUninit::uninit();
static mut READY: bool = false;
static mut RECOGNITION: RefCell<SceneRecognition> = RefCell::new(SceneRecognition::new());

macro_rules! word {
    ($function:path) => { StockCallbackWord($function as *const () as u32) };
}

fn shot_callbacks() -> shot_services::Callbacks<'static> {
    shot_services::Callbacks {
        exposure: crate::exposure_services::Callbacks {
            evaluation: word!(cb::xm1_saclac_root_exposure_evaluated),
            correction: word!(cb::xm1_saclac_root_exposure_corrected),
        },
        calculation: word!(cb::xm1_saclac_root_calculated),
        statistics: word!(cb::xm1_saclac_root_exposure_statistics),
        capture_selection_hook: word!(xm1_power_lifecycle::record_task43_selection_publish::xm1_power_record_helper_10029a8),
        capture_calculations: cb::capture_calculation_callbacks(),
        moves: cb::capture_move_callbacks(),
        capture_statistics: cb::capture_statistics_callbacks(),
        measurement: cb::measurement_callbacks(),
        capture_image: cb::capture_image_callbacks(),
        pipeline: PipelineCallbacks {
            raw_calculated: word!(cb::xm1_saclac_root_pipeline_raw_calculated),
            tone_calculated: word!(cb::xm1_saclac_root_pipeline_tone_calculated),
            gradation_calculated: word!(cb::xm1_saclac_root_pipeline_gradation_calculated),
            raw_checked: word!(cb::xm1_saclac_root_pipeline_raw_checked),
            frame_prepared: word!(cb::xm1_saclac_root_pipeline_frame_finished),
            tone_progress: word!(cb::xm1_saclac_root_pipeline_tone_progress),
            tone_finished: word!(cb::xm1_saclac_root_pipeline_tone_finished),
            output: word!(cb::xm1_saclac_root_pipeline_output_finished),
            commit: word!(cb::xm1_saclac_root_pipeline_commit_finished),
            statistics: word!(cb::xm1_saclac_root_pipeline_statistics_finished),
            statistics_commit: word!(cb::xm1_saclac_root_pipeline_statistics_committed),
            statistics_progress: word!(cb::xm1_saclac_root_pipeline_statistics_progress),
            progress_gate: &super::statistics::PIPELINE_PROGRESS,
        },
    }
}

#[inline(never)]
unsafe fn initialize(marker: &'static u32) -> &'static Root {
    let owner = addr_of_mut!(OWNER).cast::<Owner>();
    let lens = EmulatorOnly::new(marker).expect("root image requires emulator lens admission");
    unsafe { addr_of_mut!((*owner).poster).write(Poster::new(SlotTable)); }
    let poster = unsafe { &*addr_of_mut!((*owner).poster) };
    let peers = || Peers::new(poster, Guest, lens, peer_services::Callbacks {
        adjustment: word!(cb::xm1_saclac_root_adjustment_synchronized),
        lens: word!(cb::xm1_saclac_root_lens_synchronized),
        subject: word!(cb::xm1_saclac_root_subject_selected),
    });
    unsafe {
        addr_of_mut!((*owner).services).write(RefCell::new(Services::new(Guest, StockMemory, Stock, peers())));
        addr_of_mut!((*owner).geometry).write(RefCell::new(ProfileGeometry::new(GeometryMemory(Guest), GeometryBindings {
            stages: StageDriver { isp: StockIsp { exclusion: KernelExclusion(poster.kernel()) }, arguments: StageMemory, machine: Guest }, peers: peers(),
        }, Profiles::new(StockTables))));
        addr_of_mut!((*owner).recognition).write(&*addr_of_mut!(RECOGNITION));
        addr_of_mut!((*owner).exposure).write(ExposureState::default());
        addr_of_mut!((*owner).subjects).write(const { RefCell::new(crate::shot_subjects::Workspace::new()) });
        addr_of_mut!((*owner).capture).write(RefCell::new(crate::shot_capture_workspace::Workspace::default()));
        addr_of_mut!((*owner).motion).write(RefCell::new(None));
        addr_of_mut!((*owner).live_record).write(RecordBinding::new(wire::record_handle_from_a2(u32::MAX)));
        addr_of_mut!((*owner).s1_record).write(RecordBinding::new(wire::record_handle_from_a2(u32::MAX)));
        addr_of_mut!((*owner).results).write([0; xm1_saclac_interface::statistics::RESULTS_BYTES / 4]);
    }
    let services = unsafe { &*addr_of_mut!((*owner).services) };
    let geometry = unsafe { &*addr_of_mut!((*owner).geometry) };
    let recognition = unsafe { *addr_of_mut!((*owner).recognition) };
    let exposure = unsafe { &*addr_of_mut!((*owner).exposure) };
    let live_record = unsafe { &*addr_of_mut!((*owner).live_record) };
    let s1_record = unsafe { &*addr_of_mut!((*owner).s1_record) };
    unsafe { addr_of_mut!((*owner).placement).write(RefCell::new(ExposurePlacement::new(&exposure.context).expect("guest exposure address"))); }
    let placement = unsafe { &*addr_of_mut!((*owner).placement) };
    let libraries = Libraries {
        services, geometry, recognition, placement, exposure, peers: peers(), record: wire::record_handle_from_a2(u32::MAX), live_search_record: live_record,
        subjects: unsafe { &*addr_of_mut!((*owner).subjects) }, capture: unsafe { &*addr_of_mut!((*owner).capture) }, motion: unsafe { &*addr_of_mut!((*owner).motion) }, callbacks: shot_callbacks(),
    };
    unsafe { addr_of_mut!((*owner).shot).write(RefCell::new(ShotPorts::new(super::shot::ShotMemory(Guest, &exposure.context), libraries))); }
    let shot = unsafe { &*addr_of_mut!((*owner).shot) };
    services.borrow_mut().bind_round_callbacks(super::round_callbacks::callbacks());
    let results = SharedAddress(unsafe { addr_of_mut!((*owner).results) } as u32);
    unsafe {
        addr_of_mut!((*owner).root).write(Runtime::new(services, geometry, recognition, placement, shot, live_record, s1_record,
            crate::runtime::Callbacks {
                root: crate::root_utilities::Callbacks { lens_status: word!(cb::xm1_saclac_root_lens_status) },
                live: crate::live_router_ports::Callbacks { lens_corrected: word!(cb::xm1_saclac_root_lens_corrected) },
            }, results, NativeDelivery));
        write_volatile(addr_of_mut!(READY), true);
    }
    unsafe { &*addr_of_mut!((*owner).root) }
}

pub(super) fn area_services() -> (&'static RefCell<Services<Guest, StockMemory, Stock, Peer>>, &'static RefCell<Geometry>) {
    match xm1_kernel::Kernel::new(SlotTable).context() {
        Context::Task(context) if context.id().get() == 43 => {}
        _ => panic!("completion areas require task43"),
    }
    assert!(unsafe { read_volatile(addr_of_mut!(READY)) }, "completion before root initialization");
    let owner = addr_of_mut!(OWNER).cast::<Owner>();
    unsafe { (&*addr_of_mut!((*owner).services), &*addr_of_mut!((*owner).geometry)) }
}

/// Only task43 owns the aggregate; callback tasks post without borrowing it.
pub fn dispatch(event: u32, parameters: &[u8], marker: &'static u32) -> i32 {
    match xm1_kernel::Kernel::new(SlotTable).context() {
        Context::Task(context) if context.id().get() == 43 => {}
        _ => panic!("root dispatch requires task43"),
    }
    let owner = addr_of_mut!(OWNER).cast::<Owner>();
    let root = if unsafe { read_volatile(addr_of_mut!(READY)) } {
        unsafe { &*addr_of_mut!((*owner).root) }
    } else {
        unsafe { initialize(marker) }
    };
    let code = u8::try_from(event).expect("saclac event code");
    let dispatch = if let Some(kind) = wire::command(code) {
        Guest.with_command(kind, |command| root.dispatch(Message::Command(command)))
    } else if let Some(completion) = wire::completion(code) {
        root.dispatch(Message::Completed(Completed { completion, parameters: wire::parameters(parameters) }))
    } else {
        Guest.enter(super::FAULT, &[]);
        return 0;
    };
    match dispatch.outcome {
        xm1_saclac_controllers::root::Outcome::Complete => 3,
        xm1_saclac_controllers::root::Outcome::Pending => 1,
        xm1_saclac_controllers::root::Outcome::Inactive => 2,
        xm1_saclac_controllers::root::Outcome::Fault => 0,
    }
}
