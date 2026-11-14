#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_calc::{exposure::ports::Placement, scene_recognition::SceneRecognition};
use xm1_saclac_controllers::{children::{ChildOutcome, ChildRequest}, live, root::{Branch, Input, Outcome, Root}, s1, search::SearchTerminal};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::SharedAddress, Calc, Calc1, ImageProcessor, ImgDc, LensDc, Message, RecordHandle, Statistics};
use xm1_saclac_profile_geometry::controller::{ProfileGeometry, Tables};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_search_bindings::RecordBinding;
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{shot_ports::{ExternalPorts, ShotPorts}, state::StateMemory};
use crate::{child_runtime::ChildRuntime, command_source::CommandSource, controller_layers::Layers, controller_profiles::Profiles, controller_services::WindowExtent, geometry::GeometryBindings, live_router_ports::LivePorts, record_contexts::Contexts, root_ports::RootPorts, s1_router_ports::S1Ports, services::Services, statistics_results::Delivery, stock::geometry::Memory as GeometryMemory};

type Geometry<M, P, I, T> = ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>;
#[derive(Clone, Copy)]
pub struct Callbacks {
    pub root: crate::root_utilities::Callbacks,
    pub live: crate::live_router_ports::Callbacks,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dispatch { pub outcome: Outcome, pub terminal: Option<SearchTerminal> }

/// Storage and external-library views are placed first; this owner never reconstructs an engine.
pub struct Runtime<'s, M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X, A, D> {
    services: &'s RefCell<Services<M, R, E, P>>,
    geometry: &'s RefCell<Geometry<M, P, I, T>>,
    recognition: &'s RefCell<SceneRecognition>,
    placement: &'s RefCell<A>,
    shot_ports: &'s RefCell<ShotPorts<S, X>>,
    root: RefCell<Root>,
    live: RefCell<live::Router>,
    s1: RefCell<s1::Router>,
    contexts: RefCell<Contexts>,
    profiles: Profiles,
    children: ChildRuntime<'s, M, R, E, P, I, T, S, X>,
    delivery: RefCell<D>,
    callbacks: Callbacks,
    results: SharedAddress,
}
impl<'s, M: Machine + Clone, R: Memory, E: StockEntries,
    P: ImageProcessor + Calc + Calc1 + ImgDc + LensDc, I: Isp, T: Tables, S, X, A, D>
    Runtime<'s, M, R, E, P, I, T, S, X, A, D> {
    pub fn new(services: &'s RefCell<Services<M, R, E, P>>, geometry: &'s RefCell<Geometry<M, P, I, T>>, recognition: &'s RefCell<SceneRecognition>, placement: &'s RefCell<A>, shot_ports: &'s RefCell<ShotPorts<S, X>>, live_record: &'s RecordBinding, s1_record: &'s RecordBinding, callbacks: Callbacks, results: SharedAddress, delivery: D) -> Self {
        Self {
            services, geometry, recognition, placement, shot_ports,
            root: RefCell::new(Root::new()), live: RefCell::new(live::Router::new()), s1: RefCell::new(s1::Router::new()),
            contexts: RefCell::new(Contexts::new()), profiles: Profiles::new(),
            children: ChildRuntime::new(services, geometry, recognition, live_record, s1_record, shot_ports),
            delivery: RefCell::new(delivery), callbacks, results,
        }
    }
}
impl<M: CommandSource + Clone, R: Memory, E: StockEntries,
    P: ImageProcessor + Calc + Calc1 + ImgDc + LensDc + Statistics,
    I: Isp, T: Tables, S: StateMemory, X: ExternalPorts + crate::shot_services::BindRecord, A: Placement, D: Delivery>
    Runtime<'_, M, R, E, P, I, T, S, X, A, D> {
    pub fn dispatch(&self, message: Message<'_>) -> Dispatch {
        let mut contexts = self.contexts.borrow_mut();
        let mut ports = RootPorts::new(self.services, self.geometry, &mut contexts, Input::Message(message), self.callbacks.root,
            |branch, input, record| self.branch(branch, input, record));
        let outcome = self.root.borrow_mut().handle(message, &mut ports);
        Dispatch { outcome, terminal: self.children.take_terminal() }
    }

    fn branch(&self, branch: Branch, input: Input<'_>, record: RecordHandle) -> Outcome {
        if branch == Branch::Lens {
            let Input::Message(message) = input else { self.services.borrow().machine.enter(crate::stock::FAULT, &[]); return Outcome::Fault; };
            return match self.children.dispatch(ChildRequest::LensBringUp { message, record }) {
                ChildOutcome::Complete => Outcome::Complete,
                ChildOutcome::Pending => Outcome::Pending,
                ChildOutcome::Inactive => Outcome::Inactive,
                ChildOutcome::Fault | ChildOutcome::Measurement(_) | ChildOutcome::Search(_) => Outcome::Fault,
            };
        }
        let mut children = &self.children;
        let mut delivery = self.delivery.borrow_mut();
        let mut layers = Layers {
            services: self.services, geometry: self.geometry, recognition: self.recognition, placement: self.placement, shot: self.shot_ports,
            live_profile: &self.profiles.live, live_record: &self.profiles.live_record, result_address: self.results,
            extent: if branch == Branch::S1 { WindowExtent::S1 } else { WindowExtent::Live },
            children: &mut children, delivery: &mut *delivery,
        };
        match branch {
            Branch::Live => self.live.borrow_mut().handle(input, record, &mut LivePorts { profiles: &self.profiles, layers: &mut layers, callbacks: self.callbacks.live }),
            Branch::S1 => self.s1.borrow_mut().handle(input, record, &mut S1Ports { profiles: &self.profiles, layers: &mut layers }),
            Branch::Lens => unreachable!("lens dispatched before profile views"),
        }
    }
}
