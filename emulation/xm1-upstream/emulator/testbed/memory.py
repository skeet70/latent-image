"""Passive guest-write watches on RAM, chained onto `board._mmio`.

One observer is installed per board. It keeps every watched range and every
range ever hooked (native hooks cannot be removed), returns None for those and
forwards all other addresses to the handler it replaced. Only guest CPU
accesses are seen; host `write_memory` and device DMA are not. Hooked ranges
take a Python callback per access, so keep them small. Other RAM observers
(`tools/drive_afs_shot.py`) must use disjoint ranges. An installed observer
makes the board non-portable for checkpoint saving.
"""
from unicorn import UC_MEM_READ

from . import events

MASK = 0x3fffffff


class _Watcher:
    def __init__(self, board):
        self.board, self.watches, self.hooked = board, [], set()
        self.previous = board._mmio

    def install(self, board):
        board._mmio = self.observe
        board.testbed_watcher = self

    def hook(self, low, high):
        if (low, high) in self.hooked:
            return
        self.hooked.add((low, high))
        for core in (0, 1):
            for alias in (0, 0x40000000):
                self.board._native.add_mmio_hook(core, low | alias, high | alias)

    def native_passthrough(self, address):
        """True when this observer forwards `address` unchanged to `previous`."""
        physical = address & MASK
        return not any(low <= physical <= high for low, high in self.hooked)

    def observe(self, cpu, kind, address, size, value, core):
        physical = address & MASK
        if not any(low <= physical <= high for low, high in self.hooked):
            return self.previous(cpu, kind, address, size, value, core)
        if kind == UC_MEM_READ:
            return None
        hits = [w for w in self.watches if w.address < physical + size and physical < w.address + w.size]
        if not hits:
            return None
        from unicorn.arm_const import UC_ARM_REG_PC
        pc = cpu.reg_read(UC_ARM_REG_PC)
        written = (value & ((1 << (size * 8)) - 1)).to_bytes(size, 'little')
        for w in hits:
            old = bytes(cpu.mem_read(w.address | (address & ~MASK), w.size))
            new = bytearray(old)
            for index in range(size):
                offset = physical + index - w.address
                if 0 <= offset < w.size:
                    new[offset] = written[index]
            if bytes(new) != old:
                events.emit(self.board, 'mem.write', address=w.address, size=w.size, label=w.name,
                            old=int.from_bytes(old, 'little'), new=int.from_bytes(new, 'little'),
                            pc=pc, core=core)
        return None


class Watch:
    def __init__(self, watcher, address, size, name):
        self.watcher, self.address, self.size, self.name = watcher, address, size, name

    def remove(self):
        if self in self.watcher.watches:
            self.watcher.watches.remove(self)


def watch(board, address, size, name=None):
    """Emit `mem.write` (address, size, label, old, new, pc, core) when a guest store changes the bytes."""
    if address < 0 or size <= 0 or address + size > len(board.ram):
        raise ValueError('Watch must lie in RAM')
    watcher = getattr(board, 'testbed_watcher', None)
    if watcher is None:
        watcher = _Watcher(board)
        watcher.install(board)
    events.bus_for(board)
    handle = Watch(watcher, address, size, name)
    watcher.watches.append(handle)
    watcher.hook(address, address + size - 1)
    return handle
