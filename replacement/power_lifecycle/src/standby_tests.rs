use super::*;
use std::vec::Vec;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Effect { SaveIrq, WholeCache, DisableData, DisableInstruction, Read(InterruptBank), Mask(InterruptBank), WakeEnable, Wait, WakeAcknowledge, Restore(InterruptBank, BankMask), RestoreIrq(SavedIrq), EnableInstruction, EnableData }
struct Hardware { effects: Vec<Effect> }
impl StandbyHardware for Hardware {
    fn save_irq(&mut self) -> SavedIrq { self.effects.push(Effect::SaveIrq); SavedIrq(0x80) }
    fn whole_cache(&mut self) { self.effects.push(Effect::WholeCache); }
    fn disable_data_cache(&mut self) { self.effects.push(Effect::DisableData); }
    fn disable_instruction_cache(&mut self) { self.effects.push(Effect::DisableInstruction); }
    fn read_mask(&mut self, bank: InterruptBank) -> BankMask { self.effects.push(Effect::Read(bank)); BankMask(bank as u32 + 0x12340000) }
    fn mask_all(&mut self, bank: InterruptBank) { self.effects.push(Effect::Mask(bank)); }
    fn enable_standby_wake(&mut self) { self.effects.push(Effect::WakeEnable); }
    fn wait_for_interrupt(&mut self) { self.effects.push(Effect::Wait); }
    fn acknowledge_standby_wake(&mut self) { self.effects.push(Effect::WakeAcknowledge); }
    fn restore_mask(&mut self, bank: InterruptBank, mask: BankMask) { self.effects.push(Effect::Restore(bank, mask)); }
    fn restore_irq(&mut self, saved: SavedIrq) { self.effects.push(Effect::RestoreIrq(saved)); }
    fn enable_instruction_cache(&mut self) { self.effects.push(Effect::EnableInstruction); }
    fn enable_data_cache(&mut self) { self.effects.push(Effect::EnableData); }
}
#[test]
fn only_whole_event_word_one_permits_physical_standby() {
    for output in [WaitOutput::Unwritten, WaitOutput::Written(EventWord(0)), WaitOutput::Written(EventWord(2)), WaitOutput::Written(EventWord(3)), WaitOutput::Written(EventWord(u32::MAX))] {
        for status in [WaitStatus(0), WaitStatus(-50), WaitStatus(-42)] {
            let mut hardware = Hardware { effects: Vec::new() };
            let outcome = after_wait(&mut hardware, status, output);
            assert_eq!(outcome, if output == WaitOutput::Unwritten { StandbyStep::OutputIndeterminate } else { StandbyStep::AwaitingEvent });
            assert!(hardware.effects.is_empty());
        }
    }
}
#[test]
fn wake_restores_all_banks_irq_and_enables_caches_even_after_failed_wait() {
    use InterruptBank::*;
    let expected = [Effect::SaveIrq, Effect::WholeCache, Effect::DisableData, Effect::DisableInstruction,
        Effect::Read(First), Effect::Read(Second), Effect::Read(Third), Effect::Read(Fourth),
        Effect::Mask(First), Effect::Mask(Second), Effect::Mask(Third), Effect::Mask(Fourth),
        Effect::WakeEnable, Effect::Wait, Effect::WakeAcknowledge,
        Effect::Restore(First, BankMask(0x12340000)), Effect::Restore(Second, BankMask(0x12340001)),
        Effect::Restore(Third, BankMask(0x12340002)), Effect::Restore(Fourth, BankMask(0x12340003)),
        Effect::RestoreIrq(SavedIrq(0x80)), Effect::EnableInstruction, Effect::EnableData];
    for status in [WaitStatus(0), WaitStatus(-42)] {
        let mut hardware = Hardware { effects: Vec::new() };
        for _ in 0..2 {
            assert_eq!(after_wait(&mut hardware, status, WaitOutput::Written(EventWord(1))), StandbyStep::Resumed);
        }
        assert_eq!(hardware.effects, expected.iter().chain(expected.iter()).cloned().collect::<Vec<_>>());
    }
}
