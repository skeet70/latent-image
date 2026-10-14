"""Guest RAM patches: code in an unused block plus aligned word swaps.

`board.write_memory` between runs invalidates native translations for both
cores and both RAM aliases and flushes pending maintenance, so written code and
swapped code words are honored on the next run. Patching while the board is
mid-run is not supported. Removal is host-side only; do not remove a patch whose
code the guest may be executing.
"""
import struct


def word(board, address):
    return int.from_bytes(bytes(board.read_memory(address, 4, cpu_id=0)), 'little')


class PatchHandle:
    def __init__(self, board, base, reserve, swaps):
        self.board, self.base, self.reserve, self.swaps, self.active = board, base, reserve, swaps, True

    def remove(self, force=False):
        """Restore swapped words (refused if the guest changed them unless force) and zero the block."""
        if not self.active:
            return
        for address, old, new in self.swaps:
            if word(self.board, address) != new and not force:
                raise RuntimeError(f'Word {address:#x} no longer holds {new:#x}; not restoring')
        for address, old, new in reversed(self.swaps):
            self.board.write_memory(address, struct.pack('<I', old))
        self.board.write_memory(self.base, bytes(self.reserve))
        self.active = False


def install_ram_patch(board, code, base, word_swaps=(), reserve=None):
    """Write `code` at `base` (block must be all zero), then swap words.

    word_swaps: (address, expected_old, new) aligned 32-bit words, verified first.
    reserve: bytes of zeroed block to claim (default len(code) rounded to 4).
    """
    reserve = max(reserve or 0, (len(code) + 3) & ~3)
    if base & 3 or base < 0 or base + reserve > len(board.ram) or not code:
        raise ValueError('Patch block must be word aligned, nonempty and inside RAM')
    if bytes(board.read_memory(base, reserve, cpu_id=0)) != bytes(reserve):
        raise RuntimeError(f'Block {base:#x}+{reserve:#x} is not unused (nonzero)')
    swaps = [(int(a), int(old), int(new)) for a, old, new in word_swaps]
    for address, old, _ in swaps:
        if address & 3:
            raise ValueError(f'Swap address {address:#x} is not word aligned')
        if base <= address < base + reserve:
            raise ValueError('Swap address lies inside the patch block')
        if word(board, address) != old:
            raise RuntimeError(f'Word {address:#x} is {word(board, address):#x}, expected {old:#x}')
    board.write_memory(base, bytes(code))
    for address, _, new in swaps:
        board.write_memory(address, struct.pack('<I', new))
    return PatchHandle(board, base, reserve, swaps)
