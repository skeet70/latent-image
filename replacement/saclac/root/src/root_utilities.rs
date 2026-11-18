#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_controllers::root::{Input, Utility};
use xm1_saclac_interface::{stock::{record_handle_from_a2, StockCallbackWord}, CommandKind, Completion, CompletionStatus, Message, RecordHandle};
use xm1_saclac_profile_records::{stock::record, ChangeSet, Header, Memory, MeasurementSelection, SharedHeaderByte, StockEntries};
use xm1_saclac_shared::{lens::{self, LensBounds}, settings, stock::{Machine, Stock}};
use crate::{controller_helpers::{bounds, current_bounds}, controller_replies::{autofocus_reply, parameter_word, record_reply, scalar_reply}, record_contexts::apply_trigger, services::Services};

#[derive(Clone, Copy)]
pub struct Callbacks { pub lens_status: StockCallbackWord }

pub struct Utilities<'a, M, R, E, P> {
    pub services: &'a RefCell<Services<M, R, E, P>>,
    pub trigger: Input<'a>,
    pub callbacks: Callbacks,
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Utilities<'_, M, R, E, P> {
    fn machine(&self) -> M { self.services.borrow().machine.clone() }

    fn notify_current(&mut self, record: RecordHandle) {
        let result = current_bounds(&mut *self.services.borrow_mut(), record);
        if let Some(result) = result { lens::notify_bounds(0, result, &mut Stock::new(self.machine())); }
    }

    pub fn apply(&mut self, utility: Utility, handle: RecordHandle) {
        let mut machine = self.machine();
        match utility {
            Utility::RefreshLens => {
                // f01d98 supplies zero, not its new Lens context, to f14990.
                let selected = record_handle_from_a2(0);
                machine.enter(0x74_0a10, &[]);
                machine.enter(0x74_05dc, &[]);
                let value = self.services.borrow().field(selected, &record::group_00::AT_0041);
                machine.enter(0x74_0cf8, &[usize::from(value)]);
                let value = self.services.borrow_mut().entries.header_byte(selected, SharedHeaderByte::At31);
                machine.enter(0x74_0cc4, &[usize::from(value)]);
                let value = self.services.borrow().field(selected, &record::group_00::AT_0022);
                machine.enter(0x74_0b80, &[value as i32 as usize]);
                let value = self.services.borrow().field(selected, &record::group_00::AT_0044);
                machine.enter(0x74_0bd4, &[usize::from(value)]);
                let value = machine.enter(0x76_d9c8, &[]);
                machine.enter(0x74_0b18, &[value as usize]);
                machine.enter(0x74_0a34, &[]);
                machine.enter(0x74_0a58, &[]);
            }
            Utility::PrepareAutofocus => {
                let notify = {
                    let services = self.services.borrow();
                    machine.read_u32(0x95_4ddc) != 0 && [record::group_00::AT_0044.change(), record::group_00::AT_0047.change(), record::group_00::AT_0031.change()]
                        .into_iter().any(|bit| services.records.has_changes(handle, ChangeSet::of(bit.expect("selection flag change"))).unwrap_or(false))
                };
                if notify { self.notify_current(handle); }
            }
            Utility::CopyAutofocusParameters => apply_trigger(&mut *self.services.borrow_mut(), handle, self.trigger),
            Utility::ReplyAutofocus => autofocus_reply(machine, CompletionStatus(0)),
            Utility::PublishLensStatus => {
                machine.write_u32(0x95_4dd8, parameter_word(&machine, CommandKind::Command25, 1));
                let enabled = parameter_word(&machine, CommandKind::Command25, 2);
                machine.write_u32(0x95_4ddc, enabled);
                machine.enter(0x76_f5dc, &[self.callbacks.lens_status.0 as usize, enabled as usize]);
                let position = crate::calibration_inputs::lens_position(&machine) as i16;
                let mode = if machine.enter(0x76_da38, &[]) != 0 { 1 } else { crate::calibration_inputs::lens_direction(&machine) };
                // f02840 supplies zero to this query, independently of the Root cache owner.
                let LensBounds(values) = bounds(&mut *self.services.borrow_mut(), record_handle_from_a2(0), position, mode);
                // 0xf46fc8 enters 0xefb984(0, bounds, 0): the client reads [0, bounds] through r1.
                record_reply(machine, CommandKind::Command25, CompletionStatus(0), 0, &values);
            }
            Utility::RestoreLensStatus => {
                let Input::Message(Message::Completed(done)) = self.trigger else { panic!("lens status requires its completion") };
                assert_eq!(done.completion, Completion::Lens3c);
                let bytes = done.parameters.bytes();
                let position = i16::from_le_bytes(bytes[..2].try_into().expect("lens position"));
                let mode = u32::from_le_bytes(bytes[4..8].try_into().expect("lens mode"));
                if machine.read_u32(0x95_4ddc) != 0 {
                    let result = bounds(&mut *self.services.borrow_mut(), handle, position, mode);
                    lens::notify_bounds(0, result, &mut Stock::new(machine));
                }
            }
            Utility::SetLensOption | Utility::SetLensOptionWithoutWait => {
                let enabled = parameter_word(&machine, CommandKind::Command27, 1);
                {
                    let mut services = self.services.borrow_mut();
                    if utility == Utility::SetLensOption { services.set_stopped_lens_session(handle, enabled); }
                    else { services.set_lens_session(handle, enabled, false); }
                }
                scalar_reply(machine, CommandKind::Command27, CompletionStatus(0));
            }
        }
    }

    pub fn autofocus_admitted(&self) -> bool {
        let Input::Message(Message::Command(command)) = self.trigger else { panic!("autofocus admission requires its command") };
        assert_eq!(command.kind, CommandKind::Autofocus);
        let header = Header::new(command.parameters.bytes()[8..152].try_into().expect("autofocus header"));
        // f02294..f02314: the explicit all-clear test is already covered by changes_are.
        header.changes_are(ChangeSet::of(record::group_00::AT_0044.change().expect("selection range change")))
    }

    pub fn s1_lens_route(&mut self, record: RecordHandle) -> bool {
        let stock = Stock::new(self.machine());
        settings::unselected_mode(record, &stock, &mut *self.services.borrow_mut())
    }
    pub fn still_record(&mut self, record: RecordHandle) -> bool {
        self.services.borrow_mut().entries.measurement_selection(record) == MeasurementSelection::Record
    }
    pub fn live_option_allowed(&self) -> bool { self.machine().read_u32(0x95_4dd0) == 1 }
}
