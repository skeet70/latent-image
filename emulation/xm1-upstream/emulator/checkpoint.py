"""In-memory rollback for one paused Board and its original Unicorn engines.

Opaque CPU contexts remain attached to the same engines. Physical DRAM is
captured once as nonzero 64 KiB pages; aliases are not copied separately.
Pending Python event closures are refused, rather than serialized or replayed
against replaced mutable device objects. Caller must hold the session's pause
lock throughout capture/restore; these functions never run the board.
"""
from __future__ import annotations

from collections import deque
from collections.abc import Mapping
import copy
import ctypes
from dataclasses import fields, is_dataclass
import hashlib
import json
import sys
from pathlib import PurePath
from time import perf_counter
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .board import Board

from unicorn.arm_const import (
    UC_ARM_REG_R0, UC_ARM_REG_R8, UC_ARM_REG_SP, UC_ARM_REG_LR,
    UC_ARM_REG_PC, UC_ARM_REG_CPSR, UC_ARM_REG_SPSR,
    UC_ARM_REG_FPSCR, UC_ARM_REG_FPEXC, UC_ARM_REG_Q0,
)


class CheckpointError(RuntimeError):
    """The requested capture/restore cannot preserve the modeled state."""


PAGE_SIZE = 0x10000
BOARD_IDENTITIES = {"cpus", "devices", "ram", "ram_pointer", "sram",
                    "rom_storage", "vector_scratch", "memory_layout_token", "memory_layout_origin_token", "_native"}
DERIVED_FIELDS = {"_instruction_cache", "_instruction_pages", "_execution_invalidations"}
BUSY_FIELDS = {"busy", "spi_busy", "adc_busy", "dma_busy", "phase_busy"}


def _paused(board, *, capturing):
    if board.executing_cpu is not None or getattr(board, "running", False):
        raise CheckpointError("Checkpoint requires a paused board outside CPU execution")
    if board.remap_requested:
        raise CheckpointError("Checkpoint cannot capture an unfinished ROM remap")
    if getattr(board,'_restorable_low_remap',None) is not None:
        raise CheckpointError('Checkpoint requires a completed coordinated low alias remap')
    if (hasattr(board,'memory_layout_token') and
            getattr(board,'memory_layout_tracking_contract',None)!='coordinated-low-alias-v2'):
        raise CheckpointError('Checkpoint requires the current native memory mapping tracker')
    if (capturing and hasattr(board,'memory_layout_token') and
            getattr(board,'memory_layout_origin_token',None) is not board.memory_layout_token):
        raise CheckpointError('Capture refuses native memory mapping changes outside the constructor layout')
    if board.ram_pointer != ctypes.addressof(board.ram):
        raise CheckpointError("Physical DRAM pointer no longer matches its native backing")
    if capturing:
        if not getattr(board, "_checkpoint_boundary", False) or not getattr(board, "last_quantum_complete", False):
            raise CheckpointError("Capture requires Board.run(..., pause_after=...) at a completed scheduler boundary")
        if board.failure:
            raise CheckpointError("Capture requires a successful boundary before a failed MMIO/instruction")
        if board.scheduled:
            raise CheckpointError("Pending scheduled closures are not supported; choose an event-drained boundary")
        for device in board.devices:
            if any(getattr(device, name, False) for name in BUSY_FIELDS):
                raise CheckpointError(f"{type(device).__name__} still has an in-flight operation")
        # Board.run() restarts its for-loop at CPU0. Remaining cores in the
        # current cycle must therefore be unready, or already completed.
        for core in range(board.active_cpu + 1, len(board.cpus)):
            state = board.core_state[core]
            if not state.get("runnable", True):
                continue
            bank = board.interrupts.banks[core] if hasattr(board, "interrupts") else None
            if state.get("waiting", False) and (bank is None or not bank.pending & bank.enabled):
                continue
            raise CheckpointError("Pause would change CPU scheduling order; capture after the final ready core")


def capture_blocker(board: Board) -> CheckpointError | None:
    try:
        _paused(board, capturing=True)
    except CheckpointError as error:
        return error
    return None


def _memo(board):
    # deepcopy must retain the original board/device/callback owner objects.
    objects = [board, *board.cpus, *board.devices]
    objects.extend(getattr(board, name) for name in BOARD_IDENTITIES if hasattr(board, name))
    return {id(value): value for value in objects}


def _board_state(board):
    return {name: value for name, value in board.__dict__.items()
            if name not in BOARD_IDENTITIES | DERIVED_FIELDS}


def _sparse_ram(board):
    size = ctypes.sizeof(board.ram)
    pages, digest = {}, hashlib.sha256()
    zero = bytes(PAGE_SIZE)
    for offset in range(0, size, PAGE_SIZE):
        count = min(PAGE_SIZE, size - offset)
        data = ctypes.string_at(board.ram_pointer + offset, count)
        digest.update(data)
        if data != zero[:count]:
            pages[offset] = data
    return size, pages, digest.hexdigest()


def _private_ranges(board,cpu_id):
    # FFF00000 SRAM and CPU1's FFFF0000 page alias shared native backings.
    # MMIO backing is private to each Uc even though device state is shared.
    ranges=(tuple(getattr(board,'private_mmio_ranges',()))+
            ((0xfff01000, 0xef000),
             (0xffff0000 + (0x1000 if cpu_id else 0), 0xf000 if cpu_id else 0x10000)))
    native=getattr(board,'_native',None)
    getter=getattr(native,'tcm_regions',None)
    if getter is None:return ranges
    mapped=list(board.cpus[cpu_id].mem_regions())
    result=[]
    for address,size in ranges:
        parts=[]
        for lo,hi,_ in mapped:
            begin=max(address,lo);end=min(address+size,hi+1)
            if begin<end:
                if parts and parts[-1][0]+parts[-1][1]==begin:
                    a,n=parts[-1];parts[-1]=(a,n+end-begin)
                else:parts.append((begin,end-begin))
        result.extend(parts)
    for address,size in getter(cpu_id):
        if not any(a<=address and address+size<=a+n for a,n in result):result.append((address,size))
    return tuple(result)


def _private_memory(board):
    return [tuple((address, bytes(cpu.mem_read(address, size)))
                  for address, size in _private_ranges(board,cpu_id))
            for cpu_id, cpu in enumerate(board.cpus)]


def _mapping_layout(board):
    # The low ROM/RAM alias is intentionally restorable. Everything outside
    # the first16KiB must keep its exact native mapping/permissions.
    native=getattr(board,'_native',None)
    getter=getattr(native,'tcm_base_layout',None)
    return tuple(tuple((max(begin,0x4000),end,permissions)
                       for begin,end,permissions in (getter(i) if getter else cpu.mem_regions()) if end>=0x4000)
                 for i,cpu in enumerate(board.cpus))


def _tcm_state(board):
    native=getattr(board,'_native',None)
    getter=getattr(native,'tcm_export',None)
    return None if getter is None else [getter(i) for i in range(len(board.cpus))]


class BoardCheckpoint:
    """Reusable checkpoint bound to one Board and its original CPU objects."""
    def __init__(self, board):
        _paused(board, capturing=True)
        started = perf_counter()
        self._board = board
        self._cpus = tuple(board.cpus)
        self._devices = tuple(board.devices)
        self._mapping_layout=_mapping_layout(board)
        self._layout_token=getattr(board,'memory_layout_token',None)
        self._layout_contract=getattr(board,'memory_layout_tracking_contract',None)
        self._private_region_layout=tuple(_private_ranges(board,i) for i in range(len(board.cpus)))
        self._ram_pointer = board.ram_pointer
        self._backings = tuple(getattr(board, name) for name in ("ram", "sram", "vector_scratch", "rom_storage"))
        memo = _memo(board)
        self._python = copy.deepcopy(_board_state(board), memo)
        self._device_states = [copy.deepcopy(device.__dict__, memo) for device in board.devices]
        self._contexts = [cpu.context_save() for cpu in board.cpus]
        self._tcm=_tcm_state(board)
        self.ram_size, self._ram_pages, self.ram_sha256 = _sparse_ram(board)
        self._sram = bytes(board.sram)
        self._vectors = bytes(board.vector_scratch)
        self._private = _private_memory(board)
        self.capture_seconds = perf_counter() - started
        self.last_restore = None
        self.metadata = {
            "kind": "same-engine in-memory checkpoint", "format": 2,
            "instructions": board.instructions, "virtual_ticks": board.virtual_ticks,
            "firmware_sha256": board.manifest["source_sha256"],
            "rom_sha256": hashlib.sha256(board.rom).hexdigest(),
            "dram_bytes": self.ram_size, "dram_sha256": self.ram_sha256,
            "page_bytes": PAGE_SIZE, "nonzero_pages": len(self._ram_pages),
            "retained_dram_bytes": sum(map(len, self._ram_pages.values())),
            "shared_sram_bytes": len(self._sram), "shared_vector_bytes": len(self._vectors),
            "private_memory_bytes": sum(len(data) for ranges in self._private for _, data in ranges),
            "private_memory_ranges": [[dict(address=hex(a),bytes=n) for a,n in ranges]
                                      for ranges in self._private_region_layout],
            "mapping_tracking_contract":self._layout_contract,
            "cpu_count": len(self._contexts), "device_classes": [type(d).__name__ for d in self._devices],
            "scheduled_closures": 0, "low_rom_alias": board.low_rom_alias,
            "scheduler_boundary": "completed natural quantum/yield and remaining cores not ready",
            "capture_seconds": self.capture_seconds,
            "limits": [
                "Caller must pause the session under its own lock; between-quantum concurrent capture is not safe",
                "Opaque Unicorn contexts restore only to the original Board and original engines in this process",
                "Pending scheduled closures, failed capture boundaries and unresolved in-flight operations are refused",
                "Device topology and shared native backing identities must remain unchanged",
                "All other native mappings/permissions and declared private MMIO pages must match the capture; obsolete mapping manifests refuse restore",
                "Only coordinated full low16KiB remaps to owned ROM/DRAM retain identity; arbitrary low-page host mapping/protection changes refuse restore",
                "Capture/restore scans physical DRAM; no dirty-page tracking is assumed",
                "This is executable rollback state; state.json reports remain diagnostic and cannot restore a board",
            ],
        }

    def restore(self, board=None):
        board = self._board if board is None else board
        if board is not self._board or len(board.cpus) != len(self._cpus) or any(
                actual is not saved for actual, saved in zip(board.cpus, self._cpus)):
            raise CheckpointError("Checkpoint belongs to different CPU engines; cross-engine restore is refused")
        if len(board.devices) != len(self._devices) or any(
                actual is not saved for actual, saved in zip(board.devices, self._devices)):
            raise CheckpointError("Device topology changed since checkpoint capture")
        if not hasattr(self,'_mapping_layout') or not hasattr(self,'_layout_contract'):
            raise CheckpointError('Checkpoint lacks current memory mapping manifest; fresh capture required')
        if (getattr(board,'memory_layout_token',None) is not self._layout_token
                or getattr(board,'memory_layout_tracking_contract',None)!=self._layout_contract
                or _mapping_layout(board)!=self._mapping_layout):
            raise CheckpointError('Native memory mapping topology/identity changed since checkpoint capture')
        if any(getattr(board, name) is not saved for name, saved in zip(
                ("ram", "sram", "vector_scratch", "rom_storage"), self._backings)):
            raise CheckpointError("Native memory backings changed since checkpoint capture")
        if board.ram_pointer != self._ram_pointer:
            raise CheckpointError("Physical DRAM pointer changed since checkpoint capture")
        _paused(board, capturing=False)
        if self._tcm is not None:
            for core,state in enumerate(self._tcm):board._native.tcm_restore(core,state)
        if tuple(_private_ranges(board,i) for i in range(len(board.cpus)))!=self._private_region_layout:
            raise CheckpointError('Private memory mapping layout differs after TCM restoration')
        started = perf_counter()
        # The later branch's pending closures will be discarded, never invoked.
        # The saved checkpoint itself contains no pending closure references.
        changed_pages = copied = 0
        zero = bytes(PAGE_SIZE)
        for offset in range(0, self.ram_size, PAGE_SIZE):
            count = min(PAGE_SIZE, self.ram_size - offset)
            saved = self._ram_pages.get(offset, zero[:count])
            current = ctypes.string_at(board.ram_pointer + offset, count)
            if current != saved:
                ctypes.memmove(board.ram_pointer + offset, saved, count)
                changed_pages += 1
                copied += count
        for backing, saved in ((board.sram, self._sram), (board.vector_scratch, self._vectors)):
            if bytes(backing) != saved:
                ctypes.memmove(ctypes.addressof(backing), saved, len(saved))
                copied += len(saved)
        for cpu, ranges in zip(board.cpus, self._private):
            for address, saved in ranges:
                if bytes(cpu.mem_read(address, len(saved))) != saved:
                    cpu.mem_write(address, saved)
                    copied += len(saved)
        saved_alias = self._python["low_rom_alias"]
        if board.low_rom_alias != saved_alias:
            board._remap_low_alias(saved_alias)
        preserved = BOARD_IDENTITIES | DERIVED_FIELDS
        for name in list(board.__dict__):
            if name not in preserved:
                del board.__dict__[name]
        memo = _memo(board)
        board.__dict__.update(copy.deepcopy(self._python, memo))
        for device, state in zip(board.devices, self._device_states):
            device.__dict__.clear()
            device.__dict__.update(copy.deepcopy(state, memo))
        for cpu, context in zip(board.cpus, self._contexts):
            cpu.context_restore(context)
        if hasattr(board, "invalidate_execution_caches"):
            board.invalidate_execution_caches()
        else:
            if hasattr(board, "invalidate_instruction_cache"):
                board.invalidate_instruction_cache()
            for cpu in board.cpus:
                cpu.ctl_flush_tb()
        self.last_restore = {
            "seconds": perf_counter() - started, "changed_dram_pages": changed_pages,
            "copied_bytes": copied, "discarded_branch_events": True,
        }
        return dict(self.last_restore)


def capture(board):
    from x70_native.checkpoint import capture as native_capture
    return native_capture(board)


def restore(board, checkpoint):
    return checkpoint.restore(board)


def _canonical(value, board, seen=None):
    """Stable semantic encoding for checkpoint and replay validation."""
    if value is board:
        return {"reference": "Board"}
    for index, device in enumerate(board.devices):
        if value is device:
            return {"device_reference": index, "class": type(device).__name__}
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, PurePath):
        return {"path": str(value)}
    if isinstance(value, (bytes, bytearray)):
        return {"bytes": len(value), "sha256": hashlib.sha256(value).hexdigest()}
    if callable(value):
        return {"callable": f"{getattr(value, '__module__', '')}.{getattr(value, '__qualname__', type(value).__name__)}"}
    if isinstance(value, Mapping):
        return [[_canonical(key, board), _canonical(item, board)]
                for key, item in sorted(value.items(), key=lambda pair: repr(pair[0]))]
    if isinstance(value, (list, tuple, deque)):
        return [_canonical(item, board) for item in value]
    if isinstance(value, (set, frozenset)):
        return sorted((_canonical(item, board) for item in value), key=repr)
    if is_dataclass(value):
        return {"class": type(value).__name__, "fields": _canonical(
            {field.name: getattr(value, field.name) for field in fields(value)}, board)}
    if hasattr(value, "__dict__"):
        return {"class": type(value).__name__, "fields": _canonical(value.__dict__, board)}
    native_module = sys.modules.get('x70_native._native')
    native_plan = getattr(native_module, 'DeclaredUIConversionPlan', None)
    if native_plan is not None and type(value) is native_plan:
        return {"class": type(value).__name__, "fields": _canonical({
            "source_physical": value.source_physical,
            "destination_physical": value.destination_physical,
            "width": value.width,
            "height": value.height,
            "stride_pixels": value.stride_pixels,
        }, board)}
    raise CheckpointError(f"Cannot fingerprint state type {type(value).__name__}")


def architecture_state(cpu):
    """Read visible general/VFP and banked registers, restoring the CPU after inspection."""
    context = cpu.context_save()
    try:
        cpsr = cpu.reg_read(UC_ARM_REG_CPSR)
        result = {"r": [cpu.reg_read(UC_ARM_REG_R0 + index) for index in range(13)],
                  "sp": cpu.reg_read(UC_ARM_REG_SP), "lr": cpu.reg_read(UC_ARM_REG_LR),
                  "pc": cpu.reg_read(UC_ARM_REG_PC), "cpsr": cpsr,
                  "fpscr": cpu.reg_read(UC_ARM_REG_FPSCR), "fpexc": cpu.reg_read(UC_ARM_REG_FPEXC),
                  "sctlr": cpu.cpr_read(15, 0, 1, 0, 0, 0, False),
                  "sctlr_banks": {"nonsecure":cpu.cpr_read(15,0,1,0,0,0,False),
                                  "secure":cpu.cpr_read(15,0,1,0,0,1,False)},
                  "q": [cpu.reg_read(UC_ARM_REG_Q0 + index) for index in range(16)], "banked": {}}
        for mode in (0x10, 0x11, 0x12, 0x13, 0x17, 0x1b, 0x1f):
            cpu.reg_write(UC_ARM_REG_CPSR, cpsr & ~0x1f | mode)
            result["banked"][hex(mode)] = {
                "r8_r12": [cpu.reg_read(UC_ARM_REG_R8 + index) for index in range(5)],
                "sp": cpu.reg_read(UC_ARM_REG_SP), "lr": cpu.reg_read(UC_ARM_REG_LR),
                "spsr": cpu.reg_read(UC_ARM_REG_SPSR) if mode not in (0x10, 0x1f) else None,
            }
        return result
    finally:
        cpu.context_restore(context)


def fingerprint(board):
    """Hash full modeled memory/Python state and visible CPU registers for parity."""
    if board.executing_cpu is not None:
        raise CheckpointError("Fingerprint requires a paused board")
    _size, _pages, ram_sha256 = _sparse_ram(board)
    python_state = {
        "board": _canonical(_board_state(board), board),
        "devices": [_canonical(device.__dict__, board) for device in board.devices],
    }
    encoded = json.dumps(python_state, sort_keys=True, separators=(",", ":")).encode()
    private = [{hex(address): hashlib.sha256(data).hexdigest() for address, data in ranges}
               for ranges in _private_memory(board)]
    result = {
        "instructions": board.instructions, "virtual_ticks": board.virtual_ticks,
        "dram_sha256": ram_sha256, "shared_sram_sha256": hashlib.sha256(bytes(board.sram)).hexdigest(),
        "vector_scratch_sha256": hashlib.sha256(bytes(board.vector_scratch)).hexdigest(),
        "private_memory": private, "python_state_sha256": hashlib.sha256(encoded).hexdigest(),
        "architecture": [architecture_state(cpu) for cpu in board.cpus],
    }
    native=getattr(board,'_native',None)
    # Sparse default encoding: the established EXRII reset layout is ATCM
    #8KiB/FFFF8000/enabled, BTCM absent/disabled. Its bytes are already covered
    # by every core's full high-private-memory hash. Every deviation records
    # all parameters/registers and all physical contents, including disabled RAM.
    default=getattr(native,'tcm_default_layout',None)
    if default is not None and not default():
        result['tcm']=[[[size,reset,reg,hashlib.sha256(bytes(data)).hexdigest()]
                        for size,reset,reg,data in state] for state in _tcm_state(board)]
    result["sha256"] = hashlib.sha256(json.dumps(result, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return result
