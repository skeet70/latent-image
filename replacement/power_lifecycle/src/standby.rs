#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventWord(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitOutput { Unwritten, Written(EventWord) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaitStatus(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavedIrq(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterruptBank { First, Second, Third, Fourth }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BankMask(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StandbyStep { AwaitingEvent, OutputIndeterminate, Resumed }

pub trait StandbyHardware {
    fn save_irq(&mut self) -> SavedIrq;
    fn whole_cache(&mut self);
    fn disable_data_cache(&mut self);
    fn disable_instruction_cache(&mut self);
    fn read_mask(&mut self, bank: InterruptBank) -> BankMask;
    fn mask_all(&mut self, bank: InterruptBank);
    fn enable_standby_wake(&mut self);
    fn wait_for_interrupt(&mut self);
    fn acknowledge_standby_wake(&mut self);
    fn restore_mask(&mut self, bank: InterruptBank, mask: BankMask);
    fn restore_irq(&mut self, saved: SavedIrq);
    fn enable_instruction_cache(&mut self);
    fn enable_data_cache(&mut self);
}

pub fn after_wait(hardware: &mut impl StandbyHardware, _status: WaitStatus, output: WaitOutput) -> StandbyStep {
    match output {
        WaitOutput::Unwritten => return StandbyStep::OutputIndeterminate,
        WaitOutput::Written(EventWord(1)) => (),
        WaitOutput::Written(_) => return StandbyStep::AwaitingEvent,
    }
    let irq = hardware.save_irq();
    hardware.whole_cache();
    hardware.disable_data_cache();
    hardware.disable_instruction_cache();
    let banks = [InterruptBank::First, InterruptBank::Second, InterruptBank::Third, InterruptBank::Fourth];
    let saved = banks.map(|bank| hardware.read_mask(bank));
    for bank in banks { hardware.mask_all(bank); }
    hardware.enable_standby_wake();
    hardware.wait_for_interrupt();
    hardware.acknowledge_standby_wake();
    for (bank, mask) in banks.into_iter().zip(saved) { hardware.restore_mask(bank, mask); }
    hardware.restore_irq(irq);
    hardware.enable_instruction_cache();
    hardware.enable_data_cache();
    StandbyStep::Resumed
}

#[cfg(test)]
#[path = "standby_tests.rs"]
mod tests;
