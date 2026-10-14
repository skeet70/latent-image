"""Immutable EXR II register contracts and bounded external reply wiring."""
from dataclasses import dataclass


@dataclass(frozen=True)
class ControlBusRegisters:
    base: int = 0xfff85000
    irq: int = 0x65
    offsets: frozenset = frozenset((0, 4, 8, 0x10, 0x18))
    modes: tuple = (9,)

    def __post_init__(self):
        if (type(self.base) is not int or not 0 <= self.base <= 0xffffffc0
                or self.base & 0x3f or type(self.irq) is not int or not 0 <= self.irq < 256
                or self.offsets != frozenset((0, 4, 8, 0x10, 0x18))
                or type(self.modes) is not tuple or not self.modes
                or len(set(self.modes)) != len(self.modes)
                or any(type(mode) is not int or mode not in (1, 9) for mode in self.modes)):
            raise ValueError('Unsupported control-bus register contract')


@dataclass(frozen=True)
class ControlBusReplies:
    """Two existing protocol laws; None leaves that external part absent.

    Addresses/registers are board parameters, independent of a firmware
    descriptor index. The selected scenario profile still controls whether
    these parts ACK, provide stationary XYZ, or provide a heartbeat input.
    This does not infer an external chip from a matching descriptor table.
    """
    sensor_address: int | None = 0x1c
    prefix_address: int | None = 0x55
    sensor_registers: frozenset = frozenset((0x0e, 0x0f, 0x11, 0x15, 0x17, 0x18,
        0x1d, 0x1f, 0x20, 0x21, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28,
        0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31))
    prefix_high: int = 0x1c
    heartbeat_register: int = 0x1c06

    def __post_init__(self):
        addresses = [a for a in (self.sensor_address, self.prefix_address) if a is not None]
        if (any(type(a) is not int or not 0 <= a <= 0x7f for a in addresses)
                or len(set(addresses)) != len(addresses)
                or type(self.sensor_registers) is not frozenset
                or any(type(r) is not int or not 0 <= r <= 255 for r in self.sensor_registers)
                or type(self.prefix_high) is not int or not 0 <= self.prefix_high <= 255
                or type(self.heartbeat_register) is not int or not 0 <= self.heartbeat_register <= 0xffff
                or self.heartbeat_register >> 8 != self.prefix_high):
            raise ValueError('Unsupported control-bus reply contract')


DEFAULT_REGISTERS = ControlBusRegisters()
DEFAULT_REPLIES = ControlBusReplies()


@dataclass(frozen=True)
class RegisterStatusReply:
    """Register-byte writes and a separate two-byte external status input.

    Status is independent of a register pointer or firmware task. No chip
    identity or autonomous measurement behavior is inferred.
    """
    address: int
    write_masks: tuple
    status: bytes
    write_replies: tuple = ()

    def __post_init__(self):
        if (type(self.address) is not int or not 0 <= self.address <= 0x7f
                or type(self.write_masks) is not tuple or not self.write_masks
                or any(type(row) is not tuple or len(row) != 2 or
                       any(type(v) is not int or not 0 <= v <= 255 for v in row)
                       for row in self.write_masks)
                or len({row[0] for row in self.write_masks}) != len(self.write_masks)
                or type(self.status) is not bytes or len(self.status) != 2):
            raise ValueError('Invalid register/status reply contract')
        masks = dict(self.write_masks)
        if (type(self.write_replies) is not tuple or
                any(type(row) is not tuple or len(row) != 3 or
                    type(row[0]) is not int or row[0] not in masks or
                    type(row[1]) is not int or not 0 <= row[1] <= 255 or
                    row[1] & ~masks[row[0]] or type(row[2]) is not bytes or len(row[2]) != 2
                    for row in self.write_replies) or
                len({row[:2] for row in self.write_replies}) != len(self.write_replies)):
            raise ValueError('Invalid completed-write/status transition contract')
