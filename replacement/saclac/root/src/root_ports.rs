#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_controllers::{lifecycle::{Effects, Request, Startup}, root::{Allocation, Branch, Context, Generation, Input, Outcome, Port, Utility}};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{stock::record_handle_from_a2, ImageProcessor, Parameters, RecordHandle};
use xm1_saclac_profile_geometry::controller::{ProfileGeometry, Tables};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use crate::{controller_lifecycle::Lifecycle, geometry::GeometryBindings, record_contexts::Contexts, root_utilities::{Callbacks, Utilities}, services::Services, stock::geometry::Memory as GeometryMemory};

type Geometry<M, P, I, T> = ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>;

/// A root entry borrows the current input, its context cache and the persistent child dispatcher.
pub struct RootPorts<'a, 'entry, M, R, E, P, I, T, B> {
    services: &'a RefCell<Services<M, R, E, P>>,
    geometry: &'a RefCell<Geometry<M, P, I, T>>,
    contexts: &'entry mut Contexts,
    trigger: Input<'a>,
    callbacks: Callbacks,
    branches: B,
    record: RecordHandle,
}
impl<'a, 'entry, M, R, E, P, I, T, B> RootPorts<'a, 'entry, M, R, E, P, I, T, B> {
    pub fn new(services: &'a RefCell<Services<M, R, E, P>>, geometry: &'a RefCell<Geometry<M, P, I, T>>, contexts: &'entry mut Contexts, trigger: Input<'a>, callbacks: Callbacks, branches: B) -> Self
    where B: for<'input> FnMut(Branch, Input<'input>, RecordHandle) -> Outcome {
        Self { services, geometry, contexts, trigger, callbacks, branches, record: record_handle_from_a2(u32::MAX) }
    }
    fn lifecycle(&self) -> Lifecycle<'_, M, R, E, P, I, T> {
        Lifecycle { services: self.services, geometry: self.geometry, trigger: self.trigger, record: self.record }
    }
    fn utilities(&self) -> Utilities<'_, M, R, E, P> {
        Utilities { services: self.services, trigger: self.trigger, callbacks: self.callbacks }
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, B>
    Effects for RootPorts<'_, '_, M, R, E, P, I, T, B> {
    fn emit(&mut self, request: Request) { self.lifecycle().emit(request); }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables,
    B: for<'input> FnMut(Branch, Input<'input>, RecordHandle) -> Outcome>
    Port for RootPorts<'_, '_, M, R, E, P, I, T, B> {
    fn startup(&mut self) -> Startup { self.lifecycle().startup() }
    fn initialize_contexts(&mut self) {
        Lifecycle { services: self.services, geometry: self.geometry, trigger: self.trigger, record: self.record }.initialize_contexts(self.contexts);
    }
    fn context(&mut self, context: Context, allocation: Allocation, trigger: Input<'_>) -> RecordHandle {
        self.record = self.contexts.acquire(context, allocation, trigger, &mut *self.services.borrow_mut());
        self.record
    }
    fn release(&mut self, context: Context, generation: Generation) {
        self.contexts.release(context, generation, &mut self.services.borrow_mut().entries);
    }
    fn initialize_services(&mut self, record: RecordHandle, _parameters: Parameters<'_>) {
        self.record = record;
        self.lifecycle().initialize_services();
    }
    fn stop_services(&mut self) { self.lifecycle().stop_services(); }
    fn child(&mut self, branch: Branch, input: Input<'_>, record: RecordHandle) -> Outcome { (self.branches)(branch, input, record) }
    fn utility(&mut self, utility: Utility, record: RecordHandle) { self.utilities().apply(utility, record); }
    fn autofocus_admitted(&mut self) -> bool { self.utilities().autofocus_admitted() }
    fn s1_lens_route(&mut self, record: RecordHandle) -> bool { self.utilities().s1_lens_route(record) }
    fn still_record(&mut self, record: RecordHandle) -> bool { self.utilities().still_record(record) }
    fn live_option_allowed(&mut self) -> bool { self.utilities().live_option_allowed() }
    fn fault(&mut self) { self.services.borrow().machine.enter(crate::stock::FAULT, &[]); }
}
