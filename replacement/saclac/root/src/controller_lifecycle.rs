#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_controllers::{lifecycle::{self, Effects, Request, Service, Startup}, root::Input};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{CommandKind, ImageProcessor, RecordHandle};
use xm1_saclac_profile_geometry::controller::{ProfileGeometry, Tables};
use xm1_saclac_profile_records::{stock::shared, Memory, StockEntries};
use xm1_saclac_shared::{lens::LensNotices, stock::{Argument::{Buffer, Word}, Machine}};
use crate::{geometry::GeometryBindings, record_contexts::{apply_trigger, Contexts}, services::Services, stock::geometry::Memory as GeometryMemory};

type Geometry<M, P, I, T> = ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>;

/// The incoming command remains borrowed until all of its service publications finish.
/// The peer actors retain their own state; calculation and geometry use the owned models.
pub struct Lifecycle<'a, M, R, E, P, I, T> {
    pub services: &'a RefCell<Services<M, R, E, P>>,
    pub geometry: &'a RefCell<Geometry<M, P, I, T>>,
    pub trigger: Input<'a>,
    pub record: RecordHandle,
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables>
    Lifecycle<'_, M, R, E, P, I, T> {
    pub fn startup(&self) -> Startup {
        if self.services.borrow().machine.read_u32(0x95_4dcc) == 2 { Startup::ResumeLive } else { Startup::Normal }
    }

    /// f14668 resets the shared registry only outside ResumeLive, but always drops its cache.
    pub fn initialize_contexts(&mut self, contexts: &mut Contexts) {
        let startup = self.startup();
        contexts.reset();
        let mut services = self.services.borrow_mut();
        if startup == Startup::Normal { services.entries.initialize(); }
        services.stock.clear_notices();
    }

    pub fn initialize_services(&mut self) {
        let startup = self.startup();
        {
            let mut services = self.services.borrow_mut();
            // f01ccc applies the staged command again before f14824 initializes services.
            apply_trigger(&mut services, self.record, self.trigger);
            // f14834..48 defines the word and halfword. Its two unused tail bytes are zero.
            let _ = services.records.write_shared(self.record, &shared::group_24::AT_01A4, [1, 0, 0, 0, 0, 0, 0, 0]);
        }
        lifecycle::start_services(startup, self);
    }

    pub fn stop_services(&mut self) { lifecycle::stop_services(self); }

    fn configuration(&self) -> u32 {
        let Input::Message(xm1_saclac_interface::Message::Command(command)) = self.trigger else { panic!("service start requires its Start command") };
        assert_eq!(command.kind, CommandKind::Start);
        u32::from_le_bytes(command.parameters.bytes()[160..164].try_into().expect("Start configuration pointer"))
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables>
    Effects for Lifecycle<'_, M, R, E, P, I, T> {
    fn emit(&mut self, request: Request) {
        let machine = self.services.borrow().machine.clone();
        match request {
            Request::ReserveAccessor => { machine.enter(0xef_9b18, &[]); }
            Request::ResetImageBuffer => { machine.enter(0x75_e6c4, &[]); }
            Request::Start(Service::Lens) => { machine.enter(0x76_d8f8, &[]); }
            Request::Stop(Service::Lens) => { machine.enter(0x76_d950, &[]); }
            Request::Start(Service::RecordingBuffers) => self.geometry.borrow_mut().initialize(),
            Request::Stop(Service::RecordingBuffers) => { let _ = self.geometry.borrow_mut().terminate(); }
            Request::Start(Service::Calculation) => xm1_saclac_calc::library_state::start(&mut *self.services.borrow_mut()),
            Request::Stop(Service::Calculation) => xm1_saclac_calc::library_state::stop(&mut *self.services.borrow_mut()),
            Request::Start(Service::Image) => {
                let source = self.configuration();
                let mut buffers = [0; 16];
                for (to, from) in buffers.chunks_exact_mut(4).zip([24, 32, 28, 36]) {
                    to.copy_from_slice(&machine.read_u32(source.wrapping_add(from)).to_le_bytes());
                }
                machine.enter_with(0xfe_a418, &[Buffer(0)], &mut buffers);
            }
            Request::Stop(Service::Image) => { machine.enter(0xfe_a428, &[]); }
            Request::Start(Service::AuxiliaryGate) => { machine.enter(0x105_46d0, &[]); }
            Request::Stop(Service::AuxiliaryGate) => { machine.enter(0x105_46e0, &[]); }
            Request::Start(Service::Statistics) => {
                let source = self.configuration();
                // f148ac..d0 supplies words 0,1,2,4,5; the unread setup padding is zero.
                let mut setup = [0; 24];
                for (to, from) in [(0, 0), (4, 16), (16, 4), (20, 20)] {
                    setup[to..to + 4].copy_from_slice(&machine.read_u32(source.wrapping_add(from)).to_le_bytes());
                }
                machine.enter_with(0x103_9580, &[Buffer(0), Word(0)], &mut setup);
            }
            Request::Stop(Service::Statistics) => { machine.enter(0x103_9610, &[0]); }
            Request::Start(Service::Tracking) => { machine.enter(0x105_0f8c, &[]); }
            Request::Stop(Service::Tracking) => { machine.enter(0x105_0fa4, &[]); }
            Request::Start(Service::MotionDetection) => { machine.enter(0x105_1fb4, &[]); }
            Request::Stop(Service::MotionDetection) => { machine.enter(0x105_1fcc, &[]); }
            Request::PublishParameters => apply_trigger(&mut *self.services.borrow_mut(), self.record, self.trigger),
            Request::ReplyStarted => crate::controller_replies::scalar_reply(machine, CommandKind::Start, xm1_saclac_interface::CompletionStatus(0)),
            Request::ReplyStopped => crate::controller_replies::scalar_reply(machine, CommandKind::Stop, xm1_saclac_interface::CompletionStatus(0)),
        }
    }
}
