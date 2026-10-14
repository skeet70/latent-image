"""Read-only X-M1 1.50 UI/task evidence.

0063975C applies application modes at 009500EC; 0073CCB8 fills startup
state at 009570DC. 00043AC0 installs the two kernel task-table bindings.
Mode values are reported without importing another body's interpretation.
"""
import copy
from dataclasses import dataclass
from enum import Enum
import hashlib
import struct

RAM_BYTES = 0x20000000


class TaskRole(str, Enum):
    MAIN_UI = 'main_ui'
    GRAPHIC = 'graphic'
    VGLIB = 'vglib'
    UILIB = 'uilib'


@dataclass(frozen=True)
class MemoryField:
    address: int
    size: int = 4


# Entry bodies: 00D0A238, 006D2230, 006D7F88 and 0110F960.
TASK_ENTRIES = {
    0xc22f60: TaskRole.MAIN_UI,
    0x6d2700: TaskRole.GRAPHIC,
    0x6d7540: TaskRole.VGLIB,
    0x1109130: TaskRole.UILIB,
}
# 00043AC0 copies table/count into two 0x84-byte kernel records.
TASK_TABLES = ((0, 0x40054610), (1, 0x40054694))
UI_FIELDS = {
    'mode': MemoryField(0x9500ec), 'previous_mode': MemoryField(0x9500f0),
    'pending': MemoryField(0x9500f8), 'busy': MemoryField(0x950110, 1),
    'startup_mode': MemoryField(0x9570dc),
}
REGIONS = (
    ('application_modes', 0x9500ec, 0x28), ('startup_state', 0x9570dc, 0x10),
    ('main_ui_packet', 0x944580, 0x10), ('menu_state', 0x9571d0, 0x14),
    ('uilib_packet', 0x15e46c4, 0x18), ('uilib_batch', 0x95a698, 0x18),
)


def ram_range(address, count):
    physical = address - 0x40000000 if 0x40000000 <= address < 0x60000000 else address
    return count > 0 and 0 < physical and physical + count <= RAM_BYTES


class Reader:
    def __init__(self, board):
        self.board = board
        self.reads = []

    def raw(self, address, count, cpu=0):
        if not ram_range(address, count):
            raise ValueError(f'Non-RAM diagnostic read refused: {address:#x}+{count:#x}')
        data = bytes(self.board.read_memory(address, count, cpu_id=cpu))
        if len(data) != count:
            raise ValueError('Truncated host diagnostic read')
        self.reads.append(dict(cpu=cpu, address=hex(address), bytes=count,
                               sha256=hashlib.sha256(data).hexdigest()))
        return data

    def record(self, label, address, count, cpu=0):
        result = dict(label=label, address=hex(address), bytes=count, cpu=cpu)
        try:
            data = self.raw(address, count, cpu)
            result.update(hex=data.hex(), sha256=hashlib.sha256(data).hexdigest())
            if count % 4 == 0:
                result['words'] = [hex(v[0]) for v in struct.iter_unpack('<I', data)]
        except Exception as error:
            result['error'] = str(error)
        return result

    def value(self, address, count=4, cpu=0):
        return int.from_bytes(self.raw(address, count, cpu), 'little')


def ui_state(reader):
    return {name: reader.value(field.address, field.size) for name, field in UI_FIELDS.items()}


def task_state(reader):
    tables, tasks = [], []
    for cpu, base in TASK_TABLES:
        table_record = reader.record('task_table_pointer_count', base, 8, cpu)
        tables.append(table_record)
        if 'hex' not in table_record:
            continue
        table, count = struct.unpack('<II', bytes.fromhex(table_record['hex']))
        if not 1 <= count <= 128 or not ram_range(table, count * 0x38):
            table_record['error'] = 'Task table is unloaded or outside the bounded RAM range'
            continue
        try:
            rows = reader.raw(table, count * 0x38, cpu)
        except Exception as error:
            table_record['error'] = str(error)
            continue
        table_record.update(table=hex(table), count=count,
                            table_sha256=hashlib.sha256(rows).hexdigest())
        for index in range(count):
            words = struct.unpack_from('<14I', rows, index * 0x38)
            item = dict(cpu=cpu, index=index, tcb_address=hex(table + index * 0x38),
                        status_byte=words[3] & 0xff, wait_object=hex(words[5]),
                        saved_sp=hex(words[6]))
            item['status_interpretation'] = {1: 'ready', 0x12: 'queue wait'}.get(
                words[3] & 0xff, 'unclassified')
            if ram_range(words[2], 0x28):
                descriptor = reader.record('task_descriptor', words[2], 0x28, cpu)
                item['descriptor'] = descriptor
                if 'hex' in descriptor:
                    desc = struct.unpack('<10I', bytes.fromhex(descriptor['hex']))
                    role = TASK_ENTRIES.get(desc[2])
                    item.update(entry=hex(desc[2]), label=None if role is None else role.value,
                                task_id=desc[8], active=bool(desc[9]), priority=desc[3],
                                stack_base=hex(desc[5]), stack_bytes=desc[4])
            tasks.append(item)
    return tables, tasks


def collect(board):
    """Return bounded paused-board evidence, without a boot-completion claim."""
    reader = Reader(board)
    regions = {label: reader.record(label, address, count) for label, address, count in REGIONS}
    tables, tasks = task_state(reader)
    snapshot = copy.deepcopy(board.snapshot())
    devices = [copy.deepcopy(device.snapshot()) for device in board.devices if hasattr(device, 'snapshot')]
    return dict(schema='xm1-150-ui-state-v1', instructions=board.instructions,
                virtual_ticks=board.virtual_ticks, firmware_sha256=snapshot.get('firmware_sha256'),
                rom_sha256=snapshot.get('rom_sha256'), failure=copy.deepcopy(board.failure),
                cores=snapshot.get('cores'), milestones=snapshot.get('milestones'),
                ui=ui_state(reader), regions=regions, task_tables=tables, tasks=tasks,
                devices=devices, memory_reads=reader.reads,
                limits=[
                    'Paused RAM reads and passive snapshots only; no guest writes, callbacks or MMIO transactions.',
                    'Task descriptor, TCB stride and queue-wait status are decoded from the X-M1 1.50 kernel.',
                    'Unloaded task tables are boot progress, not rendering readiness.',
                    'Mode values, queue waits and frame samples do not establish shooting readiness or settings persistence.',
                ])
