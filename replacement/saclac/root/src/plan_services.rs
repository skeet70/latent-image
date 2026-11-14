#![forbid(unsafe_code)]

use xm1_saclac_calc::adjust::{lens::Step, motion::plan_ports::{PlanServices, PlanSpeedRequest, PositionRemap}};
use xm1_saclac_config::limits;
use xm1_saclac_search_bindings::calc::Derivations;
use xm1_saclac_shared::{settings::{Channel, ChannelRemap, RemapArguments}, stock::{Argument::Buffer, Machine}};
use crate::{services::Services, stock::acquisition::Aligned};

impl<M: Machine + Clone, R, E, P> PlanServices for Services<M, R, E, P> {
    fn plan_speed(&mut self, request: PlanSpeedRequest) -> Step {
        let mode = match request.selector {
            2 => limits::LimitMode::Uncapped,
            1 => limits::LimitMode::Bounded { upper: request.maximum_step, lower: request.minimum_step },
            _ => limits::LimitMode::Capped,
        };
        let query = limits::LimitQuery {
            channel: Channel(request.lens as i32), first: request.step, value: u32::from(request.depth), divisor: request.period,
            mode, kind: if request.drive_kind.0 == 0 { limits::Kind::First } else { limits::Kind::Second },
            side: limits::Side::Other, limit_mode: 8,
        };
        let (size, rate) = limits::selected_limit(&self.stock, &mut Derivations::new(&self.machine), query);
        Step { size, rate }
    }
    fn remap_position(&mut self, request: PositionRemap) -> i16 {
        self.stock.remap(RemapArguments([request.position as i32 as u32, request.lens, request.calibration, request.step, 0, 0, request.side])).0 as i16
    }
    fn minimum_speed(&mut self) -> u32 { limits::shifted_floor(&self.stock) }
    fn current_side(&mut self) -> u32 { self.machine.enter(0x76_dad4, &[]) }
    fn checks_drive_speed(&mut self) -> bool { self.machine.enter(0x76_e4b0, &[]) == 1 }
    fn load_register(&mut self, register: u8) -> u8 { self.machine.enter(0x74_2088, &[usize::from(register)]) as u8 }
    fn snapshot_mode(&mut self) -> u32 { self.machine.enter(0x76_fc4c, &[]) }
    fn cached_focus(&mut self) -> Option<[u32; 4]> {
        if self.machine.enter(0x105_38a4, &[]) != 1 { return None; }
        // 0x10538c4 writes nothing. Define its formerly unwritten output as zero.
        let mut output = Aligned([0; 16]);
        self.machine.enter_with(0x105_38c4, &[Buffer(0)], &mut output.0);
        Some(core::array::from_fn(|index| u32::from_le_bytes(output.0[4 * index..4 * index + 4].try_into().expect("cached focus word"))))
    }
    fn release_aux_text(&mut self) { self.machine.enter(0x111_0a84, &[]); }
}
