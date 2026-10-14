"""Bounded stock output-format register commits and bit28 latch handshake.

The unchanged driver964ae8/964d28 copies separate shadow fields into each
bank. 966478 requests bit28 and polls it clear. Pixel interpretation, physical
pins, color transforms and frame production are deliberately unmodeled.
"""
from dataclasses import dataclass, field

from .hardware import UnsupportedDevice


@dataclass
class FormatterBank:
    registers: dict = field(default_factory=dict)
    latched: dict = field(default_factory=dict)
    pending: dict | None = None
    due: int | None = None
    requests: int = 0
    completions: int = 0


class DisplayFormatter:
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    BASES = (0xfff69000, 0xfff6c000)
    STROBE = 1 << 28
    # Masks from actual shadow producer964fc8 and setters965c4c/964e5c..
    # 964ea0. Byte-array calibration fields are retained opaquely.
    MASKS = {
        0x00: 0xfffff7f7, 0x04: 0x1fff, 0x20: 0x7fff0fff,
        0x24: 0xfff, 0x28: 0x1fff1fff, 0x2c: 0x1fff1fff,
        0x34: 0xffffffff,
        **{offset: 0x03ff07ff for offset in range(0x38, 0x4c, 4)},
        **{offset: 0x1fff1fff for offset in (0x4c, 0x54, 0x5c)},
        **{offset: 0x1fff for offset in (0x50, 0x58, 0x60)},
        **{offset: 0xffffffff for offset in range(0x64, 0x78, 4)},
        0x78: 0xffffff,
        **{offset: 0xffffffff for offset in range(0x7c, 0xc4, 4)},
        0xc4: 0xf1ffdfff, 0xc8: 0,
        **{offset: 0x1fff1fff for offset in (0x100, 0x108, 0x110)},
        **{offset: 0x1fff for offset in (0x104, 0x10c, 0x114)},
    }

    def __init__(self, board, *, latch_delay_ticks=2):
        if (isinstance(latch_delay_ticks, bool) or
                not isinstance(latch_delay_ticks, int) or latch_delay_ticks < 1):
            raise ValueError('Display formatter latch delay must be positive')
        self.board = board
        self.latch_delay_ticks = latch_delay_ticks
        self.banks = [FormatterBank(), FormatterBank()]
        self.trace = []
        self.last_tick = board.virtual_ticks

    def contains(self, address, size):
        return size > 0 and any(base <= address and address+size <= base+0x1000
                               for base in self.BASES)

    def _address(self, address, size):
        for instance, base in enumerate(self.BASES):
            offset = address-base
            if 0 <= offset < 0x1000:
                if size != 4 or offset & 3 or offset not in self.MASKS:
                    raise UnsupportedDevice(f'Unknown display formatter register/access {address:#x}/{size}')
                return instance, offset
        raise UnsupportedDevice(f'Unknown display formatter bank {address:#x}')

    def _record(self, name, **data):
        self.trace.append({'tick': self.board.virtual_ticks, 'name': name, **data})
        if len(self.trace) > 1000:
            del self.trace[:500]
        self.board.record(name, **data)

    def read(self, cpu_id, address, size):
        instance, offset = self._address(address, size)
        self.tick(self.board)
        return self.banks[instance].registers.get(offset, 0)

    def write(self, cpu_id, address, size, value):
        instance, offset = self._address(address, size)
        if not 0 <= value <= 0xffffffff or value & ~self.MASKS[offset]:
            raise UnsupportedDevice(f'Unrecovered display formatter bits {address:#x}={value:#x}')
        self.tick(self.board)
        bank = self.banks[instance]
        if bank.pending is not None:
            raise UnsupportedDevice('Display formatter mutation/abort during pending latch is unproved')
        if offset == 0 and value & self.STROBE:
            previous = bank.registers.get(0)
            if previous is None or value & ~self.STROBE != previous:
                raise UnsupportedDevice('Display formatter latch requires exact stock control read-modify-write')
            bank.pending = dict(bank.registers)
            bank.due = self.board.virtual_ticks + self.latch_delay_ticks
            bank.requests += 1
            self._record('Display formatter latch requested', cpu=cpu_id, instance=instance,
                         due_tick=bank.due, registers=len(bank.pending),
                         timing='explicit inferred virtual latch delay')
        bank.registers[offset] = value
        self._record('Display formatter register retained', cpu=cpu_id, instance=instance,
                     offset=hex(offset), value=hex(value))

    def tick(self, board):
        now = board.virtual_ticks
        if now < self.last_tick:
            raise UnsupportedDevice('Display formatter virtual time moved backwards')
        for instance, bank in enumerate(self.banks):
            if bank.due is not None and bank.due <= now:
                due = bank.due
                bank.latched = bank.pending
                bank.pending, bank.due = None, None
                bank.registers[0] &= ~self.STROBE
                bank.completions += 1
                self._record('Display formatter latch completed', instance=instance,
                             source_tick=due, registers=len(bank.latched),
                             effect='register-state latch only; no pixels/IRQ/callback')
        self.last_tick = now

    def next_event_tick(self, now):
        due = [bank.due for bank in self.banks if bank.due is not None]
        return min(due) if due else None

    def snapshot(self):
        return {'device': type(self).__name__, 'latch_delay_ticks': self.latch_delay_ticks,
                'instances': [{'instance': instance, 'base': hex(self.BASES[instance]),
                    'registers': {hex(k): hex(v) for k, v in sorted(bank.registers.items())},
                    'latched_registers': {hex(k): hex(v) for k, v in sorted(bank.latched.items())},
                    'latch_due': bank.due, 'latch_pending': bank.pending is not None,
                    'requests': bank.requests, 'completions': bank.completions}
                    for instance, bank in enumerate(self.banks)],
                'limitations': [
                    'Separate output-format banks and copied offset sets come from964ae8/964d28; masked values are retained opaquely',
                    'Control bit28 request/poll-clear comes from966478; zero reset and positive latch delay are explicit unmeasured fixtures',
                    'Latched register state is bookkeeping, not hardware-proven sample timing or a synthesized frame',
                    'No RAM pixel writes, scanout selection, matrix/gamma/color effects, physical signal, IRQ or firmware callback is modeled',
                    'Unknown offsets/access widths/bits and pending mutation/abort stop explicitly']}
