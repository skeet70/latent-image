"""Immutable firmware bindings and the board's CPU-visible memory service."""
import ctypes
from dataclasses import dataclass
import hashlib

from .profile import number


@dataclass(frozen=True)
class LoaderValidation:
    entry: int
    sections: tuple


@dataclass(frozen=True)
class ResetRelease:
    register: int
    mask: int
    source: int
    target: int
    checks: tuple


@dataclass(frozen=True)
class FirmwareBindings:
    loader: LoaderValidation
    reset: ResetRelease


def firmware_bindings(profile):
    loader, reset = profile.loader_validation, profile.reset_release
    return FirmwareBindings(
        LoaderValidation(number(loader['entry']), tuple(loader['sections'])),
        ResetRelease(*(number(reset[k]) for k in ('register', 'mask', 'source', 'target')),
                     tuple((number(s['address']), bytes.fromhex(s['expected_hex'])) for s in reset['checks'])))




@dataclass(frozen=True)
class MemoryBus:
    """Bus reads see allocated DRAM even while low CPU fetches alias ROM.

    The alias is fixed by the supported SoC ABI, not inferred from a body id.
    The service retains the board's native allocation owner and has no state.
    """
    board: object

    @property
    def ram_size(self):
        return len(self.board.ram)

    @property
    def rom_hash(self):
        return hashlib.sha256(self.board.rom).hexdigest()

    @property
    def dram_ranges(self):
        return ((0, self.ram_size), (0x40000000, 0x40000000 + self.ram_size))

    def ram_offset(self, address, size):
        if size < 0:
            raise ValueError('Memory transfer size must be nonnegative')
        physical = address & 0x3fffffff if 0x40000000 <= address < 0x40000000 + self.ram_size else address
        return physical if 0 <= physical and physical + size <= self.ram_size else None

    def read(self, address, size, cpu_id=0):
        offset = self.ram_offset(address, size)
        if offset is not None:
            native = getattr(self.board, '_native', None)
            observe = getattr(native, 'access_probe_bus_read', None)
            if observe is not None:
                observe(cpu_id, address, size, self.board.instructions, self.board.virtual_ticks)
            return ctypes.string_at(self.board.ram_pointer + offset, size)
        return bytes(self.board.cpus[cpu_id].mem_read(address, size))

    def write(self, address, data, cpu_id=0):
        return self.board.write_memory(address, data, cpu_id=cpu_id)
