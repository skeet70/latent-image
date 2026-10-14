"""Stock one-unit 10-bit output control (PWM-like, exact pin assignment unknown).

0x919458 and 0x9193c0 configure/write this three-word block. It has no
observed receive/status polling, bus transaction, completion or IRQ contract.
The modeled registers retain firmware writes; no electrical output is claimed.
"""
from .hardware import UnsupportedDevice


class OutputControl:
    BASE = 0xfff86000

    def __init__(self, board):
        self.board = board
        self.registers = {self.BASE: 0, self.BASE+4: 0, self.BASE+8: 0}
        self.writes = []
        self.board.record('Ten-bit output control fixture', base=hex(self.BASE),
                          provenance='Stock descriptor0xaeaaf8 and actual0x919458/0x9193c0; initial values are unmeasured zeros',
                          limits='PWM-like LED-call-site interpretation; exact connected pin/electrical effects unknown')

    def contains(self, address, size):
        return size == 4 and address in self.registers

    def read(self, core, address, size):
        if not self.contains(address, size):
            raise UnsupportedDevice(f'Unknown ten-bit output register/access {address:#x}/{size}')
        if address != self.BASE:
            raise UnsupportedDevice(f'Unproven ten-bit output register read {address:#x}')
        return self.registers[address]

    def write(self, core, address, size, value):
        if not self.contains(address, size):
            raise UnsupportedDevice(f'Unknown ten-bit output register/access {address:#x}/{size}')
        value &= 0xffffffff
        if address == self.BASE:
            if value not in (0, 0x300):
                raise UnsupportedDevice(f'Unknown ten-bit output control {value:#x}')
        elif address == self.BASE+4:
            if value not in (0, 1) or (value == 1 and self.registers[self.BASE] != 0x300):
                raise UnsupportedDevice(f'Unknown ten-bit output enable/state {value:#x}')
        elif value > 0x3ff:
            raise UnsupportedDevice(f'Ten-bit output value out of range {value:#x}')
        self.registers[address] = value
        self.writes.append({'cpu': core, 'address': hex(address), 'value': hex(value)})

    def snapshot(self):
        return {'name': 'OutputControl', 'registers': {hex(k): hex(v) for k, v in self.registers.items()},
                'writes': self.writes,
                'limits': ['Initial values are explicit unmeasured zero fixtures.',
                           'Register-only output semantics, no completion/ready/IRQ or external bus replies invented.',
                           'PWM-like interpretation and exact physical LED/pin assignment are unverified.',
                           'Only stock descriptor unit0 is modeled; other units/fields reject.']}
