"""Immutable bus1 topology and explicitly chosen external register inputs."""
from dataclasses import dataclass
from .control_bus_contracts import ControlBusRegisters


DEFAULT_REGISTERS = ControlBusRegisters(base=0xfff85040, irq=0x66)


@dataclass(frozen=True)
class SecondaryBusReplies:
    usb_connected: bool = False
    register_address: int | None = 0x0a
    pwic_address: int | None = 0x30
    register_values: tuple = ((2, 0), (5, 0), (0x0c, 0), (0x11, 0), (0x13, 0))
    register_write_masks: tuple = ((0x0c, 0xff), (0x11, 0xff))
    pwic_values: tuple = ()
    pwic_write_masks: tuple = ((0x61, 0x10), (0x20, 2))
    pwic_read_prefixes: tuple = ((0x20, b'\0\0\0'),)
    register_modes: tuple = (9,)
    pwic_modes: tuple = (1,)

    def __post_init__(self):
        addresses = [a for a in (self.register_address, self.pwic_address) if a is not None]
        if (type(self.usb_connected) is not bool
                or any(type(a) is not int or not 0 <= a <= 0x7f for a in addresses)
                or len(set(addresses)) != len(addresses)):
            raise ValueError('Invalid secondary bus cable/address contract')
        for rows in (self.register_values, self.register_write_masks, self.pwic_values, self.pwic_write_masks):
            if (type(rows) is not tuple or any(type(row) is not tuple or len(row) != 2
                    or any(type(value) is not int or not 0 <= value <= 255 for value in row) for row in rows)
                    or len({row[0] for row in rows}) != len(rows)):
                raise ValueError('Invalid secondary bus register map')
        if not {row[0] for row in self.register_write_masks} <= {row[0] for row in self.register_values}:
            raise ValueError('Writable secondary bus registers require initial values')
        if (type(self.pwic_read_prefixes) is not tuple
                or any(type(row) is not tuple or len(row) != 2 or type(row[0]) is not int
                       or not 0 <= row[0] <= 255 or type(row[1]) is not bytes or len(row[1]) not in (0, 3)
                       for row in self.pwic_read_prefixes)
                or len({row[0] for row in self.pwic_read_prefixes}) != len(self.pwic_read_prefixes)):
            raise ValueError('Unsupported secondary bus receive framing')
        for modes in (self.register_modes, self.pwic_modes):
            if (type(modes) is not tuple or not modes
                    or any(type(mode) is not int or mode not in (1, 9) for mode in modes)
                    or len(set(modes)) != len(modes)):
                raise ValueError('Unsupported secondary bus endpoint modes')


DEFAULT_REPLIES = SecondaryBusReplies()
