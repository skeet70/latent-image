use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::{bridge::write, machine::{Machine, RunOutcome}, trace::{Hit, TaskCapture, TaskContext}};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, num::NonZeroUsize, path::Path};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailureCase { SemaphoreExhaustion, DeletionRetry, DispatchCpuLock }
#[derive(Clone, Copy)]
enum Fault { None, CpuLockAtDisable }
#[derive(Clone, Copy, Serialize)]
#[serde(transparent)]
struct CodeWord(u32);
#[derive(Clone, Copy, Serialize)]
#[serde(transparent)]
struct Word(u32);
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Debug)]
#[serde(rename_all = "kebab-case")]
enum EntryKind { Delay, DeleteDelay, CreateDelay, #[serde(other)] Other }
#[derive(Deserialize)]
#[serde(try_from = "String")]
struct DataAddress(u32);
impl TryFrom<String> for DataAddress {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        let address = super::address(&value)?;
        super::require(address != 0 && address & 3 == 0, "unaligned or null guest address")?;
        Ok(Self(address))
    }
}
impl<'de> Deserialize<'de> for CodeWord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        DataAddress::deserialize(deserializer).map(|address| Self(address.0))
    }
}
#[derive(Deserialize)]
struct Contract { entries: Vec<Entry>, kernel_root_table: DataAddress }
#[derive(Deserialize)]
struct Entry { label: EntryKind, address: CodeWord }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Boundary { Return, Allocate, Poll, Wait, Disable, Enable, Diagnostic }
#[derive(Serialize)]
struct BoundaryHit { boundary: Boundary, arguments: [Word; 4], cpsr: Word, instructions: u64 }
#[derive(Serialize)]
struct Call { entry: CodeWord, arguments: [Word; 4], returned: Option<Word>, last_kernel_error: Option<Word>, hits: Vec<BoundaryHit> }
#[derive(Serialize)]
pub struct Receipt { case: FailureCase, contexts: Vec<Hit>, calls: Vec<Call>, complete: bool, error: Option<String> }

fn register(machine: &Machine, index: u8) -> Result<u32, String> {
    u32::try_from(machine.session.board.read_register(0, Register::Arm32(arm::Register::R(index)))?).map_err(|_| "register exceeds word".into())
}
fn word(machine: &Machine, address: u32) -> Result<u32, String> {
    let mut bytes = [0; 4];
    machine.session.board.read_memory(0, u64::from(address), &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn entry(contract: &Contract, label: EntryKind) -> Result<CodeWord, String> {
    contract.entries.iter().find(|row| row.label == label).map(|row| row.address).ok_or_else(|| format!("missing {label:?} entry"))
}
fn pause_task(machine: &mut Machine, contract: &Contract, task: u16) -> Result<Hit, String> {
    let root = word(machine, contract.kernel_root_table.0)?;
    let receive = word(machine, root.checked_add(0xc8).ok_or("slot address overflow")?)?;
    machine.traces.bound_full_registers(NonZeroUsize::new(2).ok_or("zero hit bound")?);
    machine.traces.stop_labels = ["time-failure-context".into()].into_iter().collect();
    machine.traces.arm_with_task_capture(&machine.session, [(receive, "time-failure-context".into())].into_iter().collect(), TaskCapture::Logical)?;
    let limit = machine.instructions().checked_add(20_000_000).ok_or("instruction bound overflow")?;
    while machine.instructions() < limit {
        machine.traces.hits.clear();
        let outcome = machine.run(100_000);
        machine.require_running()?;
        super::require(matches!(outcome, RunOutcome::Stopped | RunOutcome::Completed), "unexpected failure-context outcome")?;
        if let Some(hit) = machine.traces.hits.iter().find(|hit| hit.core == 0 && matches!(hit.task,
            Some(TaskContext::Available { task_id, context_depth, .. }) if task_id.get() == task && context_depth.get() == 0)) {
            let hit = hit.clone();
            machine.disarm()?;
            return Ok(hit);
        }
    }
    Err(format!("no task-{task} receive context within bound"))
}
fn resume_current(machine: &mut Machine) -> Result<(), String> {
    let outcome = machine.run(1);
    machine.require_running()?;
    super::require(matches!(outcome, RunOutcome::Completed), "failure-context resume did not execute")
}
fn invoke(machine: &mut Machine, receipt: &mut Receipt, target: CodeWord, arguments: [Word; 4], fault: Fault) -> Result<u32, String> {
    let mut saved = [0; 16];
    for (index, value) in saved.iter_mut().enumerate() { *value = register(machine, index as u8)?; }
    let saved_cpsr = machine.session.board.read_register(0, Register::Arm32(arm::Register::Cpsr))?;
    let exit = saved[14];
    super::require(exit != target.0 && exit & 3 == 0, "invalid probe return site")?;
    let diagnostic = word(machine, 0x4089ea8c)?;
    let sites: BTreeMap<_, _> = [(exit, Boundary::Return), (0x7247ac, Boundary::Allocate), (0x724ad8, Boundary::Poll),
        (0x724b68, Boundary::Wait), (0x723efc, Boundary::Disable), (0x723f7c, Boundary::Enable), (diagnostic, Boundary::Diagnostic)].into_iter().collect();
    machine.traces.stop_labels = ["time-failure-call".into()].into_iter().collect();
    machine.traces.arm_with_task_capture(&machine.session, sites.keys().map(|address| (*address, "time-failure-call".into())).collect(), TaskCapture::Logical)?;
    for (index, value) in arguments.iter().enumerate() {
        machine.session.board.write_register(0, Register::Arm32(arm::Register::R(index as u8)), u64::from(value.0))?;
    }
    machine.session.board.write_register(0, Register::Arm32(arm::Register::R(15)), u64::from(target.0))?;
    receipt.calls.push(Call { entry: target, arguments, returned: None, last_kernel_error: None, hits: Vec::new() });
    let call = receipt.calls.last_mut().ok_or("missing active call")?;
    let limit = machine.instructions().checked_add(20_000_000).ok_or("call bound overflow")?;
    let mut masked = false;
    loop {
        super::require(machine.instructions() < limit, "guest call exceeded bound")?;
        machine.traces.hits.clear();
        let outcome = machine.run(100_000);
        machine.require_running()?;
        super::require(matches!(outcome, RunOutcome::Stopped | RunOutcome::Completed), "unexpected guest call outcome")?;
        for hit in &machine.traces.hits {
            if hit.core != 0 { continue; }
            let address = super::address(&hit.address)?;
            let boundary = *sites.get(&address).ok_or("unregistered probe boundary")?;
            super::require(register(machine, 15)? == address, "probe hit is not the stopped CPU context")?;
            super::require(call.hits.len() < 10000, "probe boundary event limit")?;
            let mut args = [Word(0); 4];
            for (index, value) in args.iter_mut().enumerate() { *value = Word(register(machine, index as u8)?); }
            let cpsr = u32::try_from(machine.session.board.read_register(0, Register::Arm32(arm::Register::Cpsr))?).map_err(|_| "CPSR exceeds word")?;
            call.hits.push(BoundaryHit { boundary, arguments: args, cpsr: Word(cpsr), instructions: hit.instructions });
            if matches!(boundary, Boundary::Disable) && matches!(fault, Fault::CpuLockAtDisable) && !masked {
                machine.session.board.write_register(0, Register::Arm32(arm::Register::Cpsr), u64::from(cpsr | 0x80))?;
                masked = true;
            }
            if matches!(boundary, Boundary::Return) {
                call.returned = Some(Word(register(machine, 0)?));
                call.last_kernel_error = Some(Word(word(machine, 0x40a0f82c)?));
            }
        }
        if let Some(result) = call.returned {
            machine.disarm()?;
            machine.session.board.write_register(0, Register::Arm32(arm::Register::Cpsr), saved_cpsr)?;
            for (index, value) in saved.iter().enumerate() {
                machine.session.board.write_register(0, Register::Arm32(arm::Register::R(index as u8)), u64::from(*value))?;
            }
            return Ok(result.0);
        }
    }
}
fn exercise(machine: &mut Machine, contract: &Contract, receipt: &mut Receipt) -> Result<(), String> {
    receipt.contexts.push(pause_task(machine, contract, 9)?);
    let delay = entry(contract, EntryKind::Delay)?;
    let zero = Word(0);
    match receipt.case {
        FailureCase::SemaphoreExhaustion => {
            let mut available = None;
            for _ in 0..251 {
                let result = invoke(machine, receipt, CodeWord(0x7247ac), [zero, Word(1), zero, zero], Fault::None)?;
                if (result as i32) <= 0 { break; }
                super::require(result <= 250, "allocator returned an invalid semaphore id")?;
                available = Some(result);
            }
            let first = invoke(machine, receipt, delay, [Word(2500), zero, zero, zero], Fault::None)?;
            super::require(first == (-255i32) as u32, "exhausted first delay did not fail")?;
            let failed = receipt.calls.last().and_then(|call| call.hits.iter().find(|hit| matches!(hit.boundary, Boundary::Poll))).ok_or("failed delay omitted retained poll")?.arguments[0].0;
            super::require((failed as i32) <= 0, "failure probe did not reach an uncached waiter")?;
            let available = available.ok_or("no allocated semaphore to release")?;
            super::require(invoke(machine, receipt, CodeWord(0x724974), [Word(available), zero, zero, zero], Fault::None)? == 0, "could not release exhausted semaphore")?;
            super::require(invoke(machine, receipt, delay, [Word(2500), zero, zero, zero], Fault::None)? == (-255i32) as u32, "retry did not retain failed semaphore word")?;
            super::require(invoke(machine, receipt, CodeWord(0x7247ac), [zero, Word(1), zero, zero], Fault::None)? == available, "retry consumed the freed semaphore")
        }
        FailureCase::DeletionRetry => {
            let delete = entry(contract, EntryKind::DeleteDelay)?;
            let create = entry(contract, EntryKind::CreateDelay)?;
            super::require(invoke(machine, receipt, delete, [zero; 4], Fault::None)? == 0, "ownerless teardown differs")?;
            super::require(invoke(machine, receipt, create, [zero; 4], Fault::None)? == (-255i32) as u32, "immediate partial recreation differs")?;
            super::require(invoke(machine, receipt, delete, [zero; 4], Fault::None)? == 0, "partial-resource teardown retry differs")?;
            resume_current(machine)?;
            receipt.contexts.push(pause_task(machine, contract, 9)?);
            super::require(invoke(machine, receipt, create, [zero; 4], Fault::None)? == (-255i32) as u32, "post-teardown stock recreation differs")?;
            Ok(())
        }
        FailureCase::DispatchCpuLock => {
            let result = invoke(machine, receipt, delay, [Word(2500), zero, zero, zero], Fault::CpuLockAtDisable)?;
            super::require(result == (-255i32) as u32, "CPU-locked delay did not fail")?;
            let call = receipt.calls.last().ok_or("missing delay call")?;
            super::require(call.hits.iter().any(|hit| matches!(hit.boundary, Boundary::Enable)), "failed disable omitted enable attempt")?;
            super::require(call.hits.iter().filter(|hit| matches!(hit.boundary, Boundary::Diagnostic) && (hit.arguments[0].0 as i32) < 0).count() >= 2,
                "CPU lock did not produce both retained dispatch diagnostics")
        }
    }
}
pub fn run(machine: &mut Machine, contract: &[u8], case: FailureCase, output: &Path) -> Result<(), String> {
    let contract: Contract = serde_json::from_slice(contract).map_err(|error| error.to_string())?;
    let mut receipt = Receipt { case, contexts: Vec::new(), calls: Vec::new(), complete: false, error: None };
    let result = exercise(machine, &contract, &mut receipt);
    receipt.complete = result.is_ok();
    receipt.error = result.as_ref().err().cloned();
    write(&output.join("failure-receipt.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    result
}
