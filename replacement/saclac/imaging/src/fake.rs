use std::vec::Vec;

use crate::driver::{Client, Event, Isp, Operation, Record, Register, SessionId, Unit};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    Read(Register),
    Write(Register, u32),
    Note(Unit, Operation),
    Log(Event),
    Enter,
    Leave,
}

pub struct FakeIsp {
    pub effects: Vec<Effect>,
    registers: Vec<(Register, u32)>,
    records: Vec<(Unit, Record)>,
}

pub const IDLE: Record = Record { holder: Client::NONE, operation: Operation::RELEASED, active: None, next: SessionId(1) };

impl FakeIsp {
    pub fn new() -> Self {
        let records = [Unit::Auto, Unit::Raid0, Unit::Mvrec].into_iter().map(|unit| (unit, IDLE)).collect();
        Self { effects: Vec::new(), registers: Vec::new(), records }
    }

    /// Sets a register's value without recording an effect.
    pub fn preset(&mut self, register: Register, value: u32) {
        match self.registers.iter_mut().find(|(at, _)| *at == register) {
            Some(slot) => slot.1 = value,
            None => self.registers.push((register, value)),
        }
    }

    pub fn value(&self, register: Register) -> Option<u32> {
        self.registers.iter().find(|(at, _)| *at == register).map(|(_, value)| *value)
    }

    pub fn stored_record(&self, unit: Unit) -> Record {
        self.records.iter().find(|(at, _)| *at == unit).map(|(_, record)| *record).unwrap()
    }

    pub fn put_record(&mut self, unit: Unit, record: Record) {
        self.records.iter_mut().find(|(at, _)| *at == unit).unwrap().1 = record;
    }

    pub fn writes(&self) -> Vec<(Register, u32)> {
        self.effects.iter().filter_map(|effect| match effect {
            Effect::Write(register, value) => Some((*register, *value)),
            _ => None,
        }).collect()
    }

    pub fn logs(&self) -> Vec<Event> {
        self.effects.iter().filter_map(|effect| match effect {
            Effect::Log(event) => Some(*event),
            _ => None,
        }).collect()
    }

    pub fn take(&mut self) -> Vec<Effect> { core::mem::take(&mut self.effects) }
}

impl Isp for FakeIsp {
    fn read(&mut self, register: Register) -> u32 {
        self.effects.push(Effect::Read(register));
        self.value(register).unwrap_or(0)
    }

    fn write(&mut self, register: Register, value: u32) {
        self.effects.push(Effect::Write(register, value));
        self.preset(register, value);
    }

    fn record(&mut self, unit: Unit) -> Record { self.stored_record(unit) }

    fn set_record(&mut self, unit: Unit, record: Record) { self.put_record(unit, record) }

    fn note(&mut self, unit: Unit, operation: Operation) {
        self.effects.push(Effect::Note(unit, operation));
        self.records.iter_mut().find(|(at, _)| *at == unit).unwrap().1.operation = operation;
    }

    fn log(&mut self, event: Event) { self.effects.push(Effect::Log(event)) }

    fn exclusive<R>(&mut self, body: impl FnOnce(&mut Self) -> R) -> R {
        self.effects.push(Effect::Enter);
        let result = body(self);
        self.effects.push(Effect::Leave);
        result
    }
}
