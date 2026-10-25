//! Paused X-M1 1.50 scheduler records.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Address(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TaskId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Ready,
    QueueWait,
    Other(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Task {
    pub core: usize,
    pub id: TaskId,
    pub entry: Address,
    pub status: Status,
    pub wait_object: Address,
    pub saved_sp: Address,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub table_counts: [u32; 2],
    pub timer_now: u32,
    pub timer_next: u32,
    pub tasks: Vec<Task>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Truncated,
    InvalidCount,
    InvalidAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StartupPhase(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MachineState(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MessageId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoadCommand(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Completion {
    NotCompleted,
    Complete,
    Other(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Startup {
    pub phase: StartupPhase,
    pub message: MessageId,
    pub machine_states: [MachineState; 6],
    pub dispatch_modes: [MachineState; 6],
    pub load_commands: [(LoadCommand, Completion); 9],
}

fn physical(address: Address) -> Result<usize, Error> {
    let address = address.0;
    let address = if (0x4000_0000..0x6000_0000).contains(&address) { address - 0x4000_0000 } else { address };
    if address >= 0x2000_0000 || address & 3 != 0 {
        return Err(Error::InvalidAddress);
    }
    Ok(address as usize)
}

fn words<const N: usize>(ram: &[u8], address: Address) -> Result<[u32; N], Error> {
    let start = physical(address)?;
    let bytes = ram.get(start..start + N * 4).ok_or(Error::Truncated)?;
    Ok(std::array::from_fn(|index| {
        let offset = index * 4;
        u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
    }))
}

pub(crate) fn collect(ram: &[u8]) -> Result<Snapshot, Error> {
    let [timer_now, timer_next] = words(ram, Address(0x4005_476c))?;
    let mut snapshot = Snapshot { table_counts: [0; 2], timer_now, timer_next, tasks: Vec::new() };
    for (core, binding) in [0x4005_4610, 0x4005_4694].into_iter().enumerate() {
        let [table, count] = words(ram, Address(binding))?;
        if count == 0 && table == 0 {
            continue;
        }
        if !(1..=128).contains(&count) {
            return Err(Error::InvalidCount);
        }
        let start = physical(Address(table))?;
        let table_bytes = ram.get(start..start + count as usize * 0x38).ok_or(Error::Truncated)?;
        snapshot.table_counts[core] = count;
        for row in table_bytes.chunks_exact(0x38) {
            let field = |offset| u32::from_le_bytes([row[offset], row[offset + 1], row[offset + 2], row[offset + 3]]);
            let descriptor = field(8);
            if descriptor == 0 {
                continue;
            }
            let description = words::<10>(ram, Address(descriptor))?;
            if description[2] == 0 {
                continue;
            }
            snapshot.tasks.push(Task {
                core,
                id: TaskId(description[8]),
                entry: Address(description[2]),
                status: match row[12] { 1 => Status::Ready, 0x12 => Status::QueueWait, value => Status::Other(value) },
                wait_object: Address(field(20)),
                saved_sp: Address(field(24)),
            });
        }
    }
    Ok(snapshot)
}

pub(crate) fn collect_startup(ram: &[u8]) -> Result<Startup, Error> {
    let [phase] = words(ram, Address(0x0095_709c))?;
    let [message] = words(ram, Address(0x0095_7084))?;
    let states = words::<3>(ram, Address(0x0097_2e88))?;
    let modes = words::<3>(ram, Address(0x0091_9a28))?;
    let loads = words::<9>(ram, Address(0x0094_4440))?;
    let half = |words: &[u32; 3], index: usize| MachineState((words[index / 2] >> ((index % 2) * 16)) as u16);
    Ok(Startup {
        phase: StartupPhase(phase),
        message: MessageId(message as u16),
        machine_states: std::array::from_fn(|index| half(&states, index)),
        dispatch_modes: std::array::from_fn(|index| half(&modes, index)),
        load_commands: std::array::from_fn(|index| {
            let completion = match loads[index] { 0 => Completion::NotCompleted, 1 => Completion::Complete, value => Completion::Other(value) };
            (LoadCommand(0x4c00 + index as u16), completion)
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::{collect, collect_startup, Address, Completion, Error, LoadCommand, MachineState, MessageId, StartupPhase, Status, TaskId};

    fn fixture() -> Vec<u8> {
        let mut ram = vec![0; 0x60000];
        for (address, value) in [
            (0x54610, 0x40058000u32), (0x54614, 1), (0x54694, 0x59000), (0x54698, 1),
            (0x5476c, 120), (0x54770, 130),
            (0x58008, 0x4005a000), (0x5800c, 0x12), (0x58014, 0x4005b000), (0x58018, 0x4005cf00),
            (0x59008, 0x5a028), (0x5900c, 1), (0x59018, 0x5df00),
            (0x5a008, 0xc22f60), (0x5a020, 1), (0x5a030, 0x6d2700), (0x5a048, 40),
        ] {
            ram[address..address + 4].copy_from_slice(&value.to_le_bytes());
        }
        ram
    }

    #[test]
    fn reports_both_core_tasks_and_their_guest_wait_states() {
        let snapshot = collect(&fixture()).unwrap();
        assert_eq!(snapshot.table_counts, [1, 1]);
        assert_eq!((snapshot.timer_now, snapshot.timer_next), (120, 130));
        assert_eq!(snapshot.tasks.len(), 2);
        assert_eq!((snapshot.tasks[0].core, snapshot.tasks[0].id, snapshot.tasks[0].entry), (0, TaskId(1), Address(0xc22f60)));
        assert_eq!(snapshot.tasks[0].status, Status::QueueWait);
        assert_eq!(snapshot.tasks[0].wait_object, Address(0x4005b000));
        assert_eq!(snapshot.tasks[1].status, Status::Ready);
        assert_eq!(snapshot.tasks[1].id, TaskId(40));
    }

    #[test]
    fn reports_unloaded_tables_without_claiming_task_progress() {
        let snapshot = collect(&vec![0; 0x60000]).unwrap();
        assert!(snapshot.tasks.is_empty());
        assert_eq!(snapshot.table_counts, [0, 0]);
    }

    #[test]
    fn refuses_corrupt_counts_and_device_pointers() {
        let mut ram = fixture();
        ram[0x54614..0x54618].copy_from_slice(&129u32.to_le_bytes());
        assert_eq!(collect(&ram), Err(Error::InvalidCount));
        ram[0x54614..0x54618].copy_from_slice(&1u32.to_le_bytes());
        ram[0x58008..0x5800c].copy_from_slice(&0xfff50000u32.to_le_bytes());
        assert_eq!(collect(&ram), Err(Error::InvalidAddress));
    }

    #[test]
    fn preserves_unclassified_state_and_rejects_truncation() {
        let mut ram = fixture();
        ram[0x5800c] = 0x33;
        assert_eq!(collect(&ram).unwrap().tasks[0].status, Status::Other(0x33));
        assert_eq!(collect(&ram[..0x5a040]), Err(Error::Truncated));
    }

    #[test]
    fn reports_guest_startup_states_and_completed_load_commands() {
        let mut ram = vec![0; 0x973000];
        for (address, value) in [
            (0x95709c, 1u32), (0x957084, 0x003b4b04),
            (0x972e88, 0x00020001), (0x972e8c, 0x00040003), (0x972e90, 0x00060005),
            (0x919a28, 0x00080007), (0x919a2c, 0x000a0009), (0x919a30, 0x000c000b),
            (0x944440, 1), (0x944448, 7),
        ] {
            ram[address..address + 4].copy_from_slice(&value.to_le_bytes());
        }
        let startup = collect_startup(&ram).unwrap();
        assert_eq!(startup.phase, StartupPhase(1));
        assert_eq!(startup.message, MessageId(0x4b04));
        assert_eq!(startup.machine_states, [1, 2, 3, 4, 5, 6].map(MachineState));
        assert_eq!(startup.dispatch_modes, [7, 8, 9, 10, 11, 12].map(MachineState));
        assert_eq!(startup.load_commands[0], (LoadCommand(0x4c00), Completion::Complete));
        assert_eq!(startup.load_commands[1], (LoadCommand(0x4c01), Completion::NotCompleted));
        assert_eq!(startup.load_commands[2], (LoadCommand(0x4c02), Completion::Other(7)));
        ram[0x944444..0x944448].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(collect_startup(&ram).unwrap().load_commands[1].1, Completion::Complete);
        assert_eq!(collect_startup(&ram[..0x950000]), Err(Error::Truncated));
    }
}
