"""Narrow stock HDMI startup with an explicit disconnected-sink fixture.

Only the startup transmitter selector, DDC enable registers and polled HPD
bit are implemented. No EDID, lock/ready result, interrupt or pixel path is
invented. Connected-sink configuration stops at its first unknown register.
"""
from .hardware import UnsupportedDevice


class HDMIStartup:
    TX = 0xfff76000
    DDC = 0xfff77000
    HPD = TX+0x37c

    def __init__(self, board, *, hpd=False):
        if not isinstance(hpd, bool):
            raise ValueError('HDMI HPD fixture must be boolean')
        self.board = board
        self.hpd = hpd
        # Reset values are explicit unmeasured simulation fixtures.
        self.registers = {self.TX: 0, self.DDC+0x40: 0, self.DDC+0x50: 0}
        self.writes = []
        self.board.record('HDMI startup fixture', hpd=hpd,
                          profile='disconnected sink' if not hpd else 'HPD high predicate probe',
                          provenance='Stock 0x971600 reads TX+0x37c bit6; other status bits are unmeasured zero fixtures',
                          limits='No EDID, transmitter readiness, HDMI interrupts or transmitted pixels modeled')

    def contains(self, address, size):
        return size == 4 and address in (*self.registers, self.HPD)

    def read(self, core, address, size):
        if not self.contains(address, size):
            raise UnsupportedDevice(f'Unknown HDMI startup register/access {address:#x}/{size}')
        if address == self.HPD:
            return int(self.hpd) << 6
        return self.registers[address]

    def write(self, core, address, size, value):
        if not self.contains(address, size):
            raise UnsupportedDevice(f'Unknown HDMI startup register/access {address:#x}/{size}')
        if address == self.HPD:
            raise UnsupportedDevice('HDMI HPD status is read-only')
        value &= 0xffffffff
        if address == self.TX:
            # Actual 0x9710f8 preserves all bits except its four-bit selector.
            # This startup fixture knows no semantics for other control bits.
            if value not in (0x10, 0x20, 0x40, 0x80):
                raise UnsupportedDevice(f'Unknown HDMI transmitter startup control {value:#x}')
        elif address == self.DDC+0x40:
            if value not in (0, 1):
                raise UnsupportedDevice(f'Unknown HDMI DDC startup enable {value:#x}')
        elif address == self.DDC+0x50:
            if value not in (0, 8):
                raise UnsupportedDevice(f'Unknown HDMI DDC startup control {value:#x}')
        self.registers[address] = value
        self.writes.append({'cpu': core, 'address': hex(address), 'value': hex(value)})

    def snapshot(self):
        return {'name': 'HDMIStartup', 'hpd': self.hpd,
                'registers': {hex(k): hex(v) for k, v in self.registers.items()},
                'writes': self.writes,
                'limits': ['Startup reset values and HPD are explicit synthetic fixtures.',
                           'GPIO0 bit1 remains a separate BootControls input; 0x971600 combines both signals.',
                           'No EDID, lock/ready bits, connected-sink initialization, IRQs or HDMI pixel transmission modeled.']}
