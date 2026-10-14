"""Read-only memory, architecture and device fingerprints of a Rust board.

This is inspection support for native checkpoint tests and diagnostics. It
does not construct or execute another backend or measure comparative speed.
"""
import ctypes
import hashlib
import json
from time import perf_counter

from .checkpoint import semantic_fingerprint


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def inspection_state(board):
    started = perf_counter()
    memory = hashlib.sha256()
    for offset in range(0, ctypes.sizeof(board.ram), 1 << 20):
        memory.update(ctypes.string_at(
            board.ram_pointer+offset, min(1 << 20, ctypes.sizeof(board.ram)-offset)))
    private = []
    for core, cpu in enumerate(board.cpus):
        ranges = list(getattr(board, 'private_mmio_ranges', ())) + [
            (0xfff01000, 0xef000),
            (0xffff0000+(0x1000 if core else 0), 0xf000 if core else 0x10000),
        ]
        private.append({hex(address): hashlib.sha256(cpu.mem_read(address, size)).hexdigest()
                        for address, size in ranges})
    semantic = semantic_fingerprint(board)
    checks = dict(
        report_sha256=digest(board.snapshot()),
        devices_sha256=digest([d.snapshot() for d in board.devices if hasattr(d, 'snapshot')]),
        dram_sha256=memory.hexdigest(), sram_sha256=hashlib.sha256(bytes(board.sram)).hexdigest(),
        vectors_sha256=hashlib.sha256(bytes(board.vector_scratch)).hexdigest(),
        private_sha256=digest(private), architecture_sha256=digest(semantic['architecture']),
        model_state_sha256=semantic['python_state_sha256'],
        sctlr_banks=[core['sctlr_banks'] for core in semantic['architecture']],
        mappings_sha256=digest([list(cpu.mem_regions()) for cpu in board.cpus]),
        mapping_contract=getattr(board, 'memory_layout_tracking_contract', None),
        constructor_mapping_identity=(board.memory_layout_token is board.memory_layout_origin_token),
        remap_in_progress=getattr(board, '_restorable_low_remap', None) is not None,
    )
    return checks, perf_counter()-started
