use crate::{fake::Fake, stages::StageDriver, stock::stages::StageMemory};
use xm1_saclac_config::pipeline::{Client, ImageStages, Unit};
use xm1_saclac_imaging::driver as d;

struct Isp {
    registers: Vec<(d::Register, u32)>,
    records: [d::Record; 3],
    events: Vec<d::Event>,
}
fn index(unit: d::Unit) -> usize { match unit { d::Unit::Auto => 0, d::Unit::Raid0 => 1, d::Unit::Mvrec => 2 } }
impl Isp {
    fn new() -> Self {
        Self { registers: Vec::new(), records: [d::Record { holder: d::Client::NONE, operation: d::Operation::RELEASED, active: None, next: d::SessionId(1) }; 3], events: Vec::new() }
    }
}
impl d::Isp for Isp {
    fn read(&mut self, register: d::Register) -> u32 { self.registers.iter().find(|(at, _)| *at == register).map_or(0, |(_, value)| *value) }
    fn write(&mut self, register: d::Register, value: u32) {
        if let Some((_, old)) = self.registers.iter_mut().find(|(at, _)| *at == register) { *old = value; }
        else { self.registers.push((register, value)); }
    }
    fn record(&mut self, unit: d::Unit) -> d::Record { self.records[index(unit)] }
    fn set_record(&mut self, unit: d::Unit, record: d::Record) { self.records[index(unit)] = record; }
    fn note(&mut self, unit: d::Unit, operation: d::Operation) { self.records[index(unit)].operation = operation; }
    fn log(&mut self, event: d::Event) { self.events.push(event); }
    fn exclusive<R>(&mut self, body: impl FnOnce(&mut Self) -> R) -> R { body(self) }
}
fn driver() -> StageDriver<Isp, StageMemory, Fake> { StageDriver { isp: Isp::new(), arguments: StageMemory, machine: Fake::default() } }

#[test]
fn refused_acquire_cannot_release_the_current_holder() {
    let mut stages = driver();
    let (status, holder) = stages.acquire(Unit::Processing, Client(2));
    assert_eq!(status, 0);
    let (status, refused) = stages.acquire(Unit::Processing, Client(4));
    assert_eq!(status, -20);
    assert_ne!(stages.release(refused), 0);
    assert_eq!(stages.isp.records[1].holder, d::Client(2));
    assert_eq!(stages.release(holder), 0);
    assert_eq!(stages.isp.records[1].active, None);
    let (status, next) = stages.acquire(Unit::Processing, Client(4));
    assert_eq!(status, 0);
    assert_eq!(stages.isp.records[1].active, Some(d::SessionId(2)));
    assert_eq!(stages.release(next), 0);
}

#[test]
fn invalid_path_preserves_the_programmed_source_and_enable() {
    let mut stages = driver();
    let (_, session) = stages.acquire(Unit::Processing, Client(2));
    assert_eq!(stages.set_path(&session, [3, 1]), 0);
    assert_eq!(stages.path(&session), (0, [3, 1]));
    assert_eq!(stages.set_path(&session, [4, 1]), -30);
    assert_eq!(stages.path(&session), (0, [3, 1]));
    assert!(matches!(stages.isp.events.last(), Some(d::Event::Refused { fault: d::Fault::Parameter, .. })));
    assert_eq!(stages.set_path(&session, [1, 0]), 0);
    assert_eq!(stages.path(&session), (0, [1, 0]));
}

#[test]
fn transform_windows_keep_the_two_banks_independent() {
    use xm1_saclac_config::pipeline::Bank;
    let mut stages = driver();
    let (_, session) = stages.acquire(Unit::Transform, Client(2));
    assert_eq!(stages.set_transform_window(&session, [11, 22, 2, 3], Bank::First), 0);
    assert_eq!(stages.set_transform_window(&session, [33, 44, 4, 5], Bank::Second), 0);
    assert_eq!(stages.transform_window(&session, Bank::First), (0, [11, 22, 2, 3]));
    assert_eq!(stages.transform_window(&session, Bank::Second), (0, [33, 44, 4, 5]));
}
