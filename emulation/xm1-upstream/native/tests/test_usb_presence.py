"""External USB cable presence changes on a paused X-M1 board."""
import gc
import struct
import unittest

from emulator.boards.profile import resolve
from emulator.boot import make_board
from emulator.power_devices import PowerDevices
from x70_native.checkpoint import capture
from unicorn.arm_const import UC_ARM_REG_PC, UC_ARM_REG_CPSR, UC_ARM_REG_R1, UC_ARM_REG_R2

STATUS = 0xfff55104
RESULT = 0x40000
# loop: ldr r0,[r1]; str r0,[r2]; b loop
PROGRAM = struct.pack('<3I', 0xe5910000, 0xe5820000, 0xeafffffc)


def guest_status(board):
    board.write_memory(RESULT, b'\xa5' * 4)
    cpu = board.cpus[0]
    for register, value in ((UC_ARM_REG_PC, 0x30000), (UC_ARM_REG_CPSR, 0xd3),
                            (UC_ARM_REG_R1, STATUS), (UC_ARM_REG_R2, RESULT)):
        cpu.reg_write(register, value)
    board.run(64, pause_after=board.instructions + 32)
    if board.failure:
        raise AssertionError(board.failure)
    return struct.unpack('<I', board.read_memory(RESULT, 4))[0]


class USBPresenceTests(unittest.TestCase):
    def setUp(self):
        profile = resolve('xm1-150', profile_file='firmware/x-m1/1.50/emulator-rec.json')
        self.board = make_board(entry='loader', quantum=8, storage_profile='erased',
                                firmware_profile=profile)
        self.board.core_state[1]['runnable'] = False
        self.board.write_memory(0x30000, PROGRAM)
        self.power = next(d for d in self.board.devices if type(d) is PowerDevices)

    def tearDown(self):
        gc.collect()

    def test_guest_status_bit_follows_presence_changes(self):
        self.assertEqual(guest_status(self.board) >> 31, 0)
        self.power.set_usb_connected(True)
        self.assertEqual(guest_status(self.board) >> 31, 1)
        self.power.set_usb_connected(False)
        self.assertEqual(guest_status(self.board) >> 31, 0)

    def test_checkpoint_restore_returns_saved_presence(self):
        self.assertEqual(guest_status(self.board) >> 31, 0)
        saved = capture(self.board)
        self.power.set_usb_connected(True)
        self.assertEqual(guest_status(self.board) >> 31, 1)
        saved.restore()
        self.assertEqual(guest_status(self.board) >> 31, 0)

    def test_refuses_non_boolean_and_unchanged_levels(self):
        for value in (1, None, 'connected', False):
            with self.assertRaises(ValueError):
                self.power.set_usb_connected(value)
        self.assertEqual(guest_status(self.board) >> 31, 0)


if __name__ == '__main__':
    unittest.main()
