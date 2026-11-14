#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_interface::{lens::{LensMode, PositionStep, ProfileSlotReference}, LensDc, ParameterValue, RecordHandle};
use xm1_saclac_lens_drive::ports::*;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use crate::services::Services;

pub struct LensPorts<'a, M, R, E, P>(pub &'a RefCell<Services<M, R, E, P>>);

macro_rules! delegate {
    ($contract:path { $(fn $name:ident(&mut self $(, $argument:ident: $type:ty)*) $(-> $result:ty)?;)* }) => {
        impl<M: Machine + Clone, R: Memory, E: StockEntries, P: LensDc> $contract for LensPorts<'_, M, R, E, P> {
            $(fn $name(&mut self $(, $argument: $type)*) $(-> $result)? {
                <Services<M, R, E, P> as $contract>::$name(&mut *self.0.borrow_mut() $(, $argument)*)
            })*
        }
    };
}
delegate!(LensControl {
    fn send(&mut self, request: LensRequest) -> Result<(), Refused>;
    fn probes(&mut self) -> Probes;
    fn resource_index(&mut self) -> ResourceIndex;
});
delegate!(CommandBlocks {
    fn drive(&mut self) -> DriveWords;
    fn set_drive_third(&mut self, value: ParameterValue);
    fn cancel(&mut self) -> ParameterValue;
    fn mode(&mut self, source: ModeSource) -> LensMode;
    fn step(&mut self) -> PositionStep;
    fn selection(&mut self) -> ParameterValue;
    fn bring_up(&mut self) -> [ParameterValue; 5];
    fn lens_stop(&mut self) -> [ParameterValue; 2];
});
delegate!(Records {
    fn profile_slot(&mut self, record: RecordHandle) -> ProfileSlotReference;
    fn resource_selection(&mut self, record: RecordHandle) -> ResourceSelection;
    fn shared_drive_bit(&mut self) -> bool;
    fn prepare_selection(&mut self, record: RecordHandle);
});
delegate!(Configuration {
    fn drive_policy(&mut self) -> DrivePolicy;
    fn adjustment_configured(&mut self) -> bool;
    fn select_lookup(&mut self, index: ResourceIndex);
});
delegate!(DriveResults {
    fn live_result(&mut self) -> ParameterValue;
    fn s1_report(&mut self) -> ParameterValue;
});
delegate!(Faults { fn fault(&mut self); });

impl<M: Machine + Clone, R, E, P> Replies for LensPorts<'_, M, R, E, P> {
    fn reply(&mut self, reply: Reply) {
        let machine = self.0.borrow().machine.clone();
        crate::lens::reply(machine, reply);
    }
}
