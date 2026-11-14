#![forbid(unsafe_code)]

use xm1_saclac_config::{pipeline as c, stock::StockStageBlock};
use xm1_saclac_imaging::{auto, driver as d, mvrec, raid0};
use xm1_saclac_imaging::stock::{arguments as a, refusal_status};
use xm1_saclac_shared::stock::Machine;
use crate::stock::stages as retained;

pub struct StageDriver<I, A, M> {
    pub isp: I,
    pub arguments: A,
    pub machine: M,
}

#[derive(Clone, Copy)]
pub struct Session {
    unit: c::Unit,
    client: d::Client,
    id: Option<d::SessionId>,
    input: u32,
}

fn unit(unit: c::Unit) -> Option<d::Unit> {
    match unit {
        c::Unit::Input => None,
        c::Unit::Transform => Some(d::Unit::Mvrec),
        c::Unit::Processing => Some(d::Unit::Raid0),
        c::Unit::Filter => Some(d::Unit::Auto),
    }
}
fn bank(bank: c::Bank) -> d::Bank { match bank { c::Bank::First => d::Bank::First, c::Bank::Second => d::Bank::Second } }
fn status(result: Result<(), d::Refusal>) -> i32 { result.map_or_else(refusal_status, |()| 0) }
fn window([first, second, repeat, count]: [u16; 4]) -> mvrec::Window {
    mvrec::Window { position: [first, second], repeat: repeat as u8, count: count as u8 }
}

impl<I: d::Isp, A, M> StageDriver<I, A, M> {
    fn parameter<T>(&mut self, session: &Session, parsed: Result<T, d::ParameterFault>, apply: impl FnOnce(&mut I, T) -> Result<(), d::Refusal>) -> i32 {
        match parsed {
            Ok(value) => status(apply(&mut self.isp, value)),
            Err(fault) => refusal_status(d::refuse_parameter(&mut self.isp, session.client, session.id, fault)),
        }
    }
}

impl<I: d::Isp, A: a::GuestMemory + xm1_saclac_config::stock::GuestMemory, M: Machine> c::ImageStages for StageDriver<I, A, M> {
    type Session = Session;
    type Block = StockStageBlock<A>;

    fn acquire(&mut self, unit_id: c::Unit, client: c::Client) -> (i32, Session) {
        let mut session = self.unheld(unit_id, client);
        let result = match unit(unit_id) {
            Some(unit) => match d::acquire(&mut self.isp, unit, session.client) {
                Ok(id) => { session.id = Some(id); 0 }
                Err(refusal) => refusal_status(refusal),
            },
            None => retained::acquire(&self.machine, client.0, &mut session.input),
        };
        (result, session)
    }
    fn release(&mut self, session: Session) -> i32 {
        match unit(session.unit) {
            Some(unit) => status(d::release(&mut self.isp, unit, session.client, session.id)),
            None => retained::release(&self.machine, session.client.0, session.input),
        }
    }
    fn unheld(&self, unit: c::Unit, client: c::Client) -> Session {
        Session { unit, client: d::Client(client.0), id: None, input: 0xffff }
    }
    fn program(&mut self, session: &Session, block: &Self::Block, part: c::Part) -> i32 {
        use c::Part::*;
        let at = block.part(part);
        let memory = &self.arguments;
        let client = session.client;
        let caller = auto::Caller { client, session: session.id };
        match part {
            InputMode | InputControl | InputRegister3c | InputRegister14 | InputRegister34 | InputRegister34Alternate | InputRegister28 => {
                let value = if part == InputMode { a::GuestMemory::word(memory, at) } else { 0 };
                retained::program(&self.machine, session.client.0, session.input, part, at, value)
            }
            ProcessingOrigin => status(raid0::set_origin(&mut self.isp, client, a::raid0::origin(memory, at))),
            ProcessingFormat => status(raid0::set_format(&mut self.isp, client, a::raid0::format(memory, at))),
            Extent => status(raid0::set_extent(&mut self.isp, client, a::raid0::extent(memory, at))),
            Division => { let value = a::raid0::division(memory, at); self.parameter(session, value, |isp, value| raid0::set_division(isp, client, value)) }
            Points => { let value = a::raid0::points(memory, at); match value {
                Ok((points, shift)) => status(raid0::set_points(&mut self.isp, client, points, shift)),
                Err(fault) => refusal_status(d::refuse_parameter(&mut self.isp, client, session.id, fault)),
            } }
            Pairs => { let value = a::raid0::pairs(memory, at); match value {
                Ok((pairs, shift)) => status(raid0::set_pairs(&mut self.isp, client, pairs, shift)),
                Err(fault) => refusal_status(d::refuse_parameter(&mut self.isp, client, session.id, fault)),
            } }
            SecondWindow => status(raid0::set_second_window(&mut self.isp, client, a::raid0::second_window(memory, at))),
            SecondExtent => status(raid0::set_second_extent(&mut self.isp, client, a::raid0::second_extent(memory, at))),
            RotatedTables => { raid0::load_rotated(&mut self.isp, a::raid0::rotated(memory, at)); 0 }
            FirstTables => { raid0::load_first_tables(&mut self.isp, a::raid0::table_set(memory, at)); 0 }
            SecondTables => { raid0::load_second_tables(&mut self.isp, a::raid0::table_set(memory, at)); 0 }
            Area => status(raid0::set_area(&mut self.isp, client, a::raid0::area(memory, at))),
            BlockLayout => status(raid0::set_block(&mut self.isp, client, a::raid0::block(memory, at))),
            Window => status(raid0::set_window(&mut self.isp, client, a::raid0::window(memory, at))),
            Curves => { raid0::load_curves(&mut self.isp, a::raid0::curves(memory, at)); 0 }
            TransformOffset(which) | TransformSecondOffset(which) | TransformThirdOffset(which) => {
                let offset = match part { TransformOffset(_) => mvrec::Offset::First, TransformSecondOffset(_) => mvrec::Offset::Second, _ => mvrec::Offset::Third };
                status(mvrec::set_offset(&mut self.isp, client, bank(which), offset, a::mvrec::offset(memory, at, offset)))
            }
            TransformWindow(which) => status(mvrec::set_window(&mut self.isp, client, bank(which), a::mvrec::window(memory, at))),
            TransformTable(which) => { mvrec::load_table(&mut self.isp, bank(which), a::mvrec::table(memory, at)); 0 }
            TransformDimensions(which) => status(mvrec::set_dimensions(&mut self.isp, client, bank(which), a::mvrec::dimensions(memory, at))),
            TransformGeometry(which) => { let value = a::mvrec::geometry(memory, at); self.parameter(session, value, |isp, value| { mvrec::set_geometry(isp, bank(which), value); Ok(()) }) }
            TransformSecondaryGeometry(which) => status(mvrec::set_secondary_geometry(&mut self.isp, client, bank(which), a::mvrec::secondary_geometry(memory, at))),
            TransformReadback(which) => { let value = a::mvrec::readback(memory, at); self.parameter(session, value, |isp, value| mvrec::set_readback(isp, client, bank(which), value)) }
            TransformAddress(which) => status(mvrec::set_address(&mut self.isp, client, bank(which), a::mvrec::address(memory, at))),
            TransformNibble(which) => { let [low, checked] = a::mvrec::nibbles(memory, at); status(mvrec::set_nibble(&mut self.isp, client, bank(which), low, checked)) }
            FilterSelectors => { let value = a::auto::selectors(memory, at); self.parameter(session, value, |isp, value| { auto::set_selectors(isp, caller, value); Ok(()) }) }
            FilterGrid(which) => {
                let grid = match which { c::Grid::First => auto::GridId::First, c::Grid::Second => auto::GridId::Second, c::Grid::Third => auto::GridId::Third };
                status(auto::set_grid(&mut self.isp, caller, grid, a::auto::grid(memory, at)))
            }
            FilterFirstBank => status(auto::set_first_bank(&mut self.isp, caller, &a::auto::bank(memory, at))),
            FilterSecondBank => status(auto::set_second_bank(&mut self.isp, caller, &a::auto::bank(memory, at))),
            FilterWindow(which) => {
                let window = match which { c::FilterWindow::First => a::auto::first_window(memory, at), c::FilterWindow::Second => a::auto::second_window(memory, at), c::FilterWindow::Third => a::auto::third_window(memory, at), c::FilterWindow::Fourth => a::auto::fourth_window(memory, at) };
                status(auto::set_window(&mut self.isp, caller, window))
            }
        }
    }
    fn enable_input(&mut self, session: &Session, enabled: bool) -> i32 { retained::enable(&self.machine, session.client.0, session.input, enabled) }
    fn reset_input_unit(&mut self) -> i32 { retained::reset(&self.machine) }
    fn set_stage(&mut self, _: &Session, stage: c::Stage, mode: c::StageMode) -> i32 {
        let stage = match stage { c::Stage::First => raid0::Stage::First, c::Stage::Second => raid0::Stage::Second, c::Stage::Third => raid0::Stage::Third, c::Stage::Fourth => raid0::Stage::Fourth };
        let mode = match mode { c::StageMode::Off => raid0::StageMode::Off, c::StageMode::On => raid0::StageMode::On, c::StageMode::Alternate => raid0::StageMode::Alternate };
        raid0::set_stage(&mut self.isp, stage, mode); 0
    }
    fn gates(&mut self, _: &Session) -> (i32, [u32; 2]) { let value = raid0::gates(&mut self.isp); (0, [u32::from(value.first), u32::from(value.second)]) }
    fn set_gates(&mut self, session: &Session, words: [u32; 2]) -> i32 { self.parameter(session, a::raid0::gates(words), |isp, value| { raid0::set_gates(isp, value); Ok(()) }) }
    fn path(&mut self, _: &Session) -> (i32, [u32; 2]) {
        let value = raid0::path(&mut self.isp);
        let source = match value.source { raid0::PathSource::First => 0, raid0::PathSource::Second => 1, raid0::PathSource::Third => 2, raid0::PathSource::Fourth => 3 };
        (0, [source, u32::from(value.enabled)])
    }
    fn set_path(&mut self, session: &Session, words: [u32; 2]) -> i32 { self.parameter(session, a::raid0::path(words), |isp, value| { raid0::set_path(isp, value); Ok(()) }) }
    fn switch(&mut self, _: &Session, switch: c::Switch) -> (i32, u32) { (0, u32::from(match switch { c::Switch::Gate => raid0::gate(&mut self.isp), c::Switch::Output => raid0::output(&mut self.isp) })) }
    fn set_switch(&mut self, session: &Session, switch: c::Switch, word: u32) -> i32 {
        let value = match switch { c::Switch::Gate => a::raid0::gate(word), c::Switch::Output => a::raid0::output(word) };
        self.parameter(session, value, |isp, value| { match switch { c::Switch::Gate => raid0::set_gate(isp, value), c::Switch::Output => raid0::set_output(isp, value) }; Ok(()) })
    }
    fn mode(&mut self, _: &Session) -> (i32, c::ModeWords) { let mode = raid0::mode(&mut self.isp); (0, c::ModeWords { enabled: u32::from(mode.enabled), level: mode.level }) }
    fn set_mode(&mut self, session: &Session, mode: c::ModeWords) -> i32 { self.parameter(session, a::raid0::mode(mode.enabled, mode.level), |isp, value| { raid0::set_mode(isp, value); Ok(()) }) }
    fn set_transform_offset(&mut self, session: &Session, offset: [u16; 2], which: c::Bank) -> i32 { status(mvrec::set_offset(&mut self.isp, session.client, bank(which), mvrec::Offset::First, offset.map(u32::from))) }
    fn transform_window(&mut self, _: &Session, which: c::Bank) -> (i32, [u16; 4]) { let value = mvrec::window(&mut self.isp, bank(which)); (0, [value.position[0], value.position[1], u16::from(value.repeat), u16::from(value.count)]) }
    fn set_transform_window(&mut self, session: &Session, value: [u16; 4], which: c::Bank) -> i32 { status(mvrec::set_window(&mut self.isp, session.client, bank(which), window(value))) }
    fn load_transform_table(&mut self, _: &Session, table: &[u32; 31], which: c::Bank) -> i32 { mvrec::load_table(&mut self.isp, bank(which), table); 0 }
}

pub struct KernelExclusion<'a, B: xm1_kernel::Backend>(pub &'a xm1_kernel::Kernel<B>);
impl<B: xm1_kernel::Backend> Copy for KernelExclusion<'_, B> {}
impl<B: xm1_kernel::Backend> Clone for KernelExclusion<'_, B> { fn clone(&self) -> Self { *self } }
impl<B: xm1_kernel::Backend> d::Exclusion for KernelExclusion<'_, B> {
    fn run<R>(&self, body: impl FnOnce() -> R) -> R {
        use xm1_kernel::{Context, Lockable};
        fn locked<B: xm1_kernel::Backend, R>(cx: &mut impl Lockable<B>, body: impl FnOnce() -> R) -> R {
            let mut irq = cx.mask_irq();
            let _cores = irq.lock_cores();
            body()
        }
        match self.0.context() {
            Context::Task(mut cx) => locked(&mut cx, body),
            Context::Interrupt(mut cx) => locked(&mut cx, body),
            Context::Masked(mut cx) => locked(&mut cx, body),
        }
    }
}
