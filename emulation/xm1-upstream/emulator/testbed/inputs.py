"""Guest-context input and call injection through the stock key-scanner callback.

One stub in unused heap wraps the scanner callback word (0xca7d00 -> 0x941948).
Each scanner invocation runs at most one queued request (function pointer plus
four arguments) from the scanner's own context, stores r0 and marks it done,
then jumps to the stock scanner. A debug-shell key is request 0x11eb51c(event,0),
as in tools/verify_debug_key_s1_emulator.py. The USB-task context of the real
debug shell is not modelled; the stub is declared emulator machinery.
"""
from functools import lru_cache
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile

from . import events
from .memory import watch
from .patch import install_ram_patch, word

BASE, BLOCK = 0xf50000, 0x200       # unused heap tail in stock REC boots; checked zero
DATA = BASE + 0x100                 # +0 head index; entries at +0x10, 32 bytes each
SLOTS, ENTRY = 8, 32
SCANNER_WORD, SCANNER = 0xca7d00, 0x941948
EMIT_KEY = 0x11eb51c
from tools.x70_buttons import KEY_EVENTS  # sole firmware-1.10 parser table
EMPTY, PENDING, DONE = 0, 1, 2

SOURCE = f"""
    .syntax unified
    .arm
    .text
stub:
    push {{r0-r6, lr}}
    ldr r4, data_addr
    ldr r5, [r4]
    add r6, r4, #0x10
    add r6, r6, r5, lsl #5
    ldr r0, [r6]
    cmp r0, #{PENDING}
    bne 1f
    ldr r12, [r6, #4]
    ldr r0, [r6, #8]
    ldr r1, [r6, #12]
    ldr r2, [r6, #16]
    ldr r3, [r6, #20]
    blx r12
    str r0, [r6, #24]
    mov r0, #{DONE}
    str r0, [r6]
    ldr r5, [r4]
    add r5, r5, #1
    and r5, r5, #{SLOTS - 1}
    str r5, [r4]
1:
    pop {{r0-r6, lr}}
    ldr pc, scanner_addr
data_addr: .word {DATA:#x}
scanner_addr: .word {SCANNER:#x}
"""


@lru_cache(maxsize=1)
def build_stub():
    from elftools.elf.elffile import ELFFile
    compiler = shutil.which('clang')
    if not compiler:
        raise RuntimeError('clang is required to assemble the scanner stub')
    with tempfile.TemporaryDirectory(prefix='x70-testbed-stub-') as temporary:
        source, obj = Path(temporary) / 'stub.s', Path(temporary) / 'stub.o'
        source.write_text(SOURCE)
        subprocess.run([compiler, '--target=armv7r-none-eabi', '-mcpu=cortex-r4', '-marm', '-c',
                        str(source), '-o', str(obj)], check=True, capture_output=True, text=True)
        with obj.open('rb') as stream:
            elf = ELFFile(stream)
            if any(s['sh_type'] in ('SHT_REL', 'SHT_RELA') and s.num_relocations()
                   for s in elf.iter_sections()):
                raise RuntimeError('Unresolved relocation in stub')
            code = elf.get_section_by_name('.text').data()
    if len(code) > DATA - BASE:
        raise RuntimeError('Stub overlaps its data block')
    return code


class Ticket:
    """One queued request; `done`/`result` read guest memory."""
    def __init__(self, stub, slot, function, args, auto_free):
        self.stub, self.slot, self.function, self.args, self.auto_free = stub, slot, function, args, auto_free
        self.freed = False

    @property
    def done(self):
        return not self.freed and word(self.stub.board, self.stub.entry(self.slot)) == DONE

    def result(self):
        if not self.done:
            raise RuntimeError('Request has not completed')
        return self.stub.free(self)


class ScannerStub:
    def __init__(self, board):
        self.board, self.tail, self.tickets, self.handle, self.watches = board, 0, {}, None, []

    @staticmethod
    def for_board(board):
        stub = getattr(board, 'testbed_scanner_stub', None)
        if stub is None:
            stub = board.testbed_scanner_stub = ScannerStub(board)
        return stub

    def entry(self, slot):
        return DATA + 0x10 + slot * ENTRY

    def ensure_installed(self):
        if self.handle is not None:
            return
        board, code = self.board, build_stub()
        current = word(board, SCANNER_WORD)
        if current == SCANNER:
            self.handle = install_ram_patch(board, code, BASE, [(SCANNER_WORD, SCANNER, BASE)], reserve=BLOCK)
        elif current == BASE and bytes(board.read_memory(BASE, len(code), cpu_id=0)) == code:
            raise RuntimeError('Scanner stub already installed by another owner; reuse its Testbed')
        else:
            raise RuntimeError(f'Scanner callback word holds {current:#x}; neither stock nor our stub')
        self.tail = 0
        for slot in range(SLOTS):
            self.watches.append(watch(board, self.entry(slot), 4, name='testbed.queue'))
        events.bus_for(board).on('mem.write', self._on_write)

    def _on_write(self, record):
        if record['label'] != 'testbed.queue' or record['new'] != DONE:
            return
        slot = (record['address'] - self.entry(0)) // ENTRY
        ticket = self.tickets.get(slot)
        if ticket is not None:
            events.emit(self.board, 'input.done', slot=slot, function=ticket.function,
                        args=ticket.args, result=word(self.board, self.entry(slot) + 24))

    def submit(self, function, args=(), auto_free=False):
        if len(args) > 4:
            raise ValueError('At most four register arguments')
        self.ensure_installed()
        for ticket in [t for t in self.tickets.values() if t.auto_free and t.done]:
            self.free(ticket)
        slot = self.tail
        if slot in self.tickets:
            raise RuntimeError('Request queue full')
        args = tuple(a & 0xffffffff for a in args)
        base = self.entry(slot)
        body = struct.pack('<7I', 0, function & 0xffffffff, *args, *(0,) * (4 - len(args)), 0)
        self.board.write_memory(base, body)
        self.board.write_memory(base, struct.pack('<I', PENDING))     # state last
        ticket = self.tickets[slot] = Ticket(self, slot, function, args, auto_free)
        self.tail = (slot + 1) % SLOTS
        return ticket

    def free(self, ticket):
        base = self.entry(ticket.slot)
        result = word(self.board, base + 24)
        self.board.write_memory(base, bytes(ENTRY))
        ticket.freed = True
        del self.tickets[ticket.slot]
        return result
