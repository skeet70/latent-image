"""The relinked RAM-push CPU1 park job against a command CPU0 already cancelled.

RAM_PUSH_RELINK names a build-recipe-bank-ram.sh output directory (private data).
"""
import os
import struct
import unittest
from pathlib import Path

from emulator.boards.profile import resolve
from emulator.boot import make_board
from unicorn.arm_const import (UC_ARM_REG_CPSR, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_R0,
                               UC_ARM_REG_R1, UC_ARM_REG_SP)

RELINK = os.environ.get('RAM_PUSH_RELINK')
HALT = 0x30000
STACK = 0x38000
CANCEL = 5
PARK_TIMEOUT = 0xfffffff9
# Rendezvous word offsets per linked layout, keyed by its size: (command, nonce, service core).
LAYOUTS = {0x60: (0x20, 0x00, 0x08), 0x80: (0x00, 0x04, 0x0c)}


def symbols(path):
    table = {}
    for row in path.read_text().splitlines():
        fields = row.split()
        if len(fields) == 4:
            table[fields[3]] = (int(fields[0], 16), int(fields[1], 16))
    return table


@unittest.skipUnless(RELINK, 'RAM_PUSH_RELINK names a relinked RAM-push build')
class ParkAfterCancelTests(unittest.TestCase):
    def setUp(self):
        relink = Path(RELINK)
        self.symbols = symbols(relink / 'kernel.syms')
        manifest_base = self.symbols['__ram_push_start'][0]
        profile = resolve('xm1-150', profile_file='firmware/x-m1/1.50/emulator-rec.json')
        self.board = make_board(entry='loader', quantum=8, storage_profile='erased',
                                firmware_profile=profile)
        self.board.core_state[1]['runnable'] = False
        self.board.write_memory(manifest_base, (relink / 'runtime.bin').read_bytes())
        self.board.write_memory(HALT, struct.pack('<I', 0xeafffffe))

    def run_park(self, budget):
        rendezvous, size = self.symbols['rendezvous']
        command, nonce, service_core = LAYOUTS[size]
        self.board.write_memory(rendezvous, bytes(size))
        self.board.write_memory(rendezvous + nonce, struct.pack('<I', 0x123456))
        self.board.write_memory(rendezvous + service_core, struct.pack('<I', 0xffffffff))
        self.board.write_memory(rendezvous + command, struct.pack('<I', CANCEL))
        cpu = self.board.cpus[0]
        for register, value in ((UC_ARM_REG_PC, self.symbols['peer_park_action'][0]),
                                (UC_ARM_REG_CPSR, 0xd3), (UC_ARM_REG_SP, STACK),
                                (UC_ARM_REG_LR, HALT), (UC_ARM_REG_R0, rendezvous),
                                (UC_ARM_REG_R1, 0)):
            cpu.reg_write(register, value)
        end = self.board.instructions + budget
        while self.board.instructions < end and cpu.reg_read(UC_ARM_REG_PC) != HALT:
            self.board.run(4096, pause_after=min(end, self.board.instructions + 2048))
            if self.board.failure and cpu.reg_read(UC_ARM_REG_PC) != HALT:
                raise AssertionError(self.board.failure)
        word = struct.unpack('<I', self.board.read_memory(rendezvous + command, 4))[0]
        return cpu.reg_read(UC_ARM_REG_PC) == HALT, cpu.reg_read(UC_ARM_REG_R0), word

    def test_late_park_job_leaves_without_parking_and_keeps_cancel(self):
        returned, status, command = self.run_park(2_000_000)
        self.assertTrue(returned, 'park job still spinning after CPU0 cancelled')
        self.assertEqual(status, PARK_TIMEOUT)
        self.assertEqual(command, CANCEL)


if __name__ == '__main__':
    unittest.main()
