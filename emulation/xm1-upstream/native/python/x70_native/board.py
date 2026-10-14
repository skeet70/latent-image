"""Rust-owned Unicorn engines with the existing Python scheduler and devices.

The scheduler is inherited unchanged. Its mutable execution fields belong to
Python while paused and to Rust during emu_start; handoff occurs at execution
boundaries and before/after Python MMIO/event callbacks. No global monkeypatch
or modification of emulator.board/emulator.boot is performed.
"""
from __future__ import annotations

from collections import deque
import ctypes
import functools
import hashlib
import json
from operator import itemgetter
import os
import sys
import warnings
from pathlib import Path
import subprocess
import weakref

import unicorn
from unicorn import UcError
from unicorn.arm_const import UC_ARM_REG_PC
from emulator import board as reference
from emulator import strict_audit
from emulator.board import BoardPlatform, ENTRY_LABELS, ROOT, ROM_FILE, ROM_HASH
from emulator.boards.bindings import firmware_bindings
from ._native import Machine, NativeError


@functools.lru_cache(maxsize=None)
def _pinned_unicorn(root: Path) -> str:
    """The native/unicorn submodule commit this checkout's HEAD pins, or ""."""
    result = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD:native/unicorn"],
                            capture_output=True, text=True)
    return result.stdout.strip() if result.returncode == 0 else ""


def unicorn_library() -> Path:
    """Resolve the Unicorn 2.1.4 library the engines load.

    Default: the patched build tools/build_unicorn.py installs in
    .tools/unicorn-x70 while its stamp equals the pinned native/unicorn
    commit, else the reference wheel's library. X70_UNICORN_LIBRARY=wheel
    forces the wheel; any other value names a library file. Results are
    bit-identical with either; the patched build only adds faster paths
    (native/README.md). The library's SHA-256 is part of every snapshot's
    native identity.
    """
    override = os.environ.get("X70_UNICORN_LIBRARY")
    if override and override != "wheel":
        path = Path(override).resolve()
        if not path.is_file():
            raise RuntimeError(f"X70_UNICORN_LIBRARY does not exist: {path}")
        return path
    if not override:
        root = Path(__file__).resolve().parents[3]
        patched = root / ".tools/unicorn-x70"
        library = patched / ("libunicorn.2.dylib" if sys.platform == "darwin" else "libunicorn.so.2")
        if library.is_file():
            stamp = (patched / "stamp").read_text().strip() if (patched / "stamp").is_file() else ""
            if stamp and stamp == _pinned_unicorn(root):
                return library
            warnings.warn("Patched Unicorn in .tools/unicorn-x70 was not built from the pinned "
                          "native/unicorn commit; using the slower wheel library until "
                          "tools/build_unicorn.py is rerun")
    if unicorn.__version__ != "2.1.4":
        raise RuntimeError("Native backend requires the reference Unicorn 2.1.4 wheel")
    candidates = [Path(unicorn.__file__).parent / "lib" / name
                  for name in ("libunicorn.2.dylib", "libunicorn.so.2", "unicorn.dll")]
    paths = [path.resolve() for path in candidates if path.is_file()]
    if len(paths) != 1:
        raise RuntimeError("Cannot uniquely resolve the installed Unicorn library")
    return paths[0]


EXECUTION_MODES = ("instructions", "fast")
# Process-wide default for boards constructed without an explicit `execution`.
# "fast" unless the environment or a CLI profile selects "instructions" (the
# historical mode, kept as a fallback); the mode is native-engine state, never
# board state, so it is outside checkpoints and fingerprints (both modes are
# bit-exact).
DEFAULT_EXECUTION = os.environ.get("X70_NATIVE_EXECUTION", "fast")


def set_default_execution(mode):
    """Select the execution mode used by boards constructed afterwards."""
    global DEFAULT_EXECUTION
    if mode not in EXECUTION_MODES:
        raise ValueError(f"execution must be one of {EXECUTION_MODES}")
    DEFAULT_EXECUTION = mode


class NativeCore:
    """Bounded Uc-compatible surface, with no exposed native engine pointer."""
    def __init__(self, board, machine, core):
        self._board_ref, self._machine, self.core = weakref.ref(board), machine, core

    def reg_read(self, reg):
        return int.from_bytes(self._machine.reg_read(self.core, reg), "little")

    def reg_write(self, reg, value):
        # Unicorn ARM registers are at most 128 bits in the supported surface.
        self._machine.reg_write(self.core, reg, int(value).to_bytes(16, "little"))

    def cpr_read(self, cp, opc1, crn, crm, opc2, sec, is64):
        return self._machine.cpr(self.core, cp, opc1, crn, crm, opc2, sec, is64)

    def cpr_write(self, cp, opc1, crn, crm, opc2, sec, is64, value):
        self._machine.cpr(self.core, cp, opc1, crn, crm, opc2, sec, is64, value)

    def mem_read(self, address, size):
        return self._machine.mem_read(self.core, address, size)

    def mem_write(self, address, data):
        self._machine.mem_write(self.core, address, bytes(data))

    def mem_regions(self):
        return iter(self._machine.mem_regions(self.core))

    def mem_map(self, address, size, perms=7):
        self._machine.mem_map(self.core, address, size, perms)

    def mem_map_ptr(self, address, size, perms, pointer):
        self._machine.mem_map_ptr(self.core, address, size, perms, pointer)

    def mem_unmap(self, address, size):
        self._machine.mem_unmap(self.core, address, size)

    def mem_protect(self, address, size, perms):
        self._machine.mem_protect(self.core, address, size, perms)

    def ctl_remove_cache(self, address, end):
        self._machine.remove_cache(self.core, address, end)

    def ctl_flush_tb(self):
        self._machine.flush_tb(self.core)

    def ctl_set_cpu_model(self, model):
        if model != 13:
            raise ValueError("Native backend owns fixed R5F engines")

    def ctl_get_cpu_model(self):
        return 13

    def hook_add(self, kind, callback, user_data=None, begin=1, end=0):
        if kind != (unicorn.UC_HOOK_MEM_READ | unicorn.UC_HOOK_MEM_WRITE):
            raise ValueError("NativeCore supports peripheral hooks; custom instruction hooks are unsupported by the Rust executor")
        if getattr(callback, "__name__", None) != "_mmio" or getattr(callback, "__self__", None) is not self._board_ref():
            raise ValueError("Only Board MMIO dispatch is supported")
        return self._machine.add_mmio_hook(self.core, begin, end)

    def emu_start(self, begin, until, timeout=0, count=0):
        board = self._board_ref()
        if board is None:
            raise RuntimeError("Board no longer exists")
        board._sync_plain_registers()
        board._push_native()
        previous = board.executing_cpu
        board.executing_cpu = self.core
        try:
            status = self._machine.emu_start(self.core, begin, until, timeout, count)
        except Exception as error:
            # Preserve fault evidence as well as the original exception. This
            # executes after the C engine has stopped, including native panics
            # caught at its callback boundary.
            board._pull_native()
            if board.failure is None:
                board.failure = dict(message="Native execution callback exception", cpu=self.core,
                    pc=hex(self.reg_read(UC_ARM_REG_PC)), error_type=type(error).__name__, error=str(error))
            raise
        finally:
            board.executing_cpu = previous
            board._pull_after_run(self.core)
        if status:
            raise UcError(status)

    def emu_stop(self):
        self._machine.emu_stop(self.core)

    def context_save(self):
        return self._machine.context_save(self.core)

    def context_restore(self, context):
        self._machine.context_restore(self.core, context)


def normalize_instructions_per_tick(value):
    """Return (numerator, denominator) for a declared CPU-speed ratio.

    The camera's true CPU clock is unknown; this is a declared host input
    (instructions executed per virtual timer tick). Accepts a positive int,
    ``"n/d"`` text, a Fraction or an ``(n, d)`` pair. Default 1 reproduces
    the historical one-tick-per-instruction behavior.
    """
    from fractions import Fraction
    if isinstance(value, bool):
        raise ValueError("instructions_per_tick must not be boolean")
    if isinstance(value, tuple):
        value = Fraction(*value)
    elif isinstance(value, str):
        value = Fraction(value)
    elif isinstance(value, int):
        value = Fraction(value)
    elif not isinstance(value, Fraction):
        raise ValueError("instructions_per_tick must be an int, 'n/d' text, Fraction or pair")
    if value < 1 or value.numerator > 1 << 32 or value.denominator > 1 << 32:
        raise ValueError("instructions_per_tick must be at least 1 and a bounded ratio")
    return value.numerator, value.denominator


def ratio_option(ratio):
    """Factory-option spelling of a normalized ratio (int, or 'n/d')."""
    return ratio[0] if ratio[1] == 1 else f"{ratio[0]}/{ratio[1]}"


_CORE_AUX = itemgetter("waiting", "pmcr", "pmcntenset", "pmu_start", "pmu_frozen")
_MMIO_KINDS = {"mmio:16": 16, "mmio:17": 17}


class _MmioIndex:
    """Candidate devices per 4 KiB page, for the board's MMIO dispatch.

    A device opts in with `mmio_ranges()`: half-open (begin, end) ranges that
    contain the *start address* of every access its `contains()` accepts, in
    any device state (a superset; the `contains()` call stays authoritative).
    A device that does not declare ranges, whose class is not the one that
    declared them (a subclass may change contains()), or whose `contains` is
    patched on the instance, is a wildcard: a candidate on every page. The
    candidate tuple of each page keeps the attach order, so the first
    candidate whose contains() is true is the first attached device whose
    contains() is true.
    """
    __slots__ = ("devices", "pages", "wildcards")
    SPAN_LIMIT = 1 << 16   # pages; a larger declared range is not worth expanding

    def __init__(self, devices):
        self.devices = list(devices)
        pages_of = []
        for device in self.devices:
            pages_of.append(self._pages(device))
        self.wildcards = tuple(d for d, pages in zip(self.devices, pages_of) if pages is None)
        touched = set()
        for pages in pages_of:
            if pages:
                touched |= pages
        self.pages = {page: tuple(d for d, pages in zip(self.devices, pages_of)
                                  if pages is None or page in pages)
                      for page in touched}

    @staticmethod
    def _pages(device):
        declare = vars(type(device)).get("mmio_ranges")
        if declare is None or "contains" in vars(device) or "mmio_ranges" in vars(device):
            return None
        try:
            pages = set()
            for begin, end in declare(device):
                if type(begin) is not int or type(end) is not int or begin < 0:
                    return None
                if end > begin:
                    first, last = begin >> 12, (end - 1) >> 12
                    if last - first >= _MmioIndex.SPAN_LIMIT:
                        return None
                    pages.update(range(first, last + 1))
            return pages
        except Exception:
            return None


class _EventSync:
    """Board bookkeeping that must stay out of `board.__dict__`.

    Held in a slot of the board, so checkpoints, fingerprints and portable
    snapshots never see it. `seen` is the native `aux_version` the Python
    mirror of the scheduler state was synchronised at; `key`/`exclusive`/
    `stats` are copies of the Python-side inputs `_push_native` sends, taken at
    that moment, to detect a handler that changed them. `mmio` is the device
    dispatch index (rebuilt when the device list changes). `events` counts
    native events, so the scheduler can tell a quantum that made none.
    """
    __slots__ = ("seen", "key", "exclusive", "stats", "mmio", "instructions", "events", "batch",
                 "plain", "deferred")

    def __init__(self):
        self.seen = self.key = self.exclusive = self.stats = self.mmio = self.instructions = None
        self.events = 0
        self.batch = None  # emulator.board._QuietBatch in progress
        self.plain = None  # (key, device ids) of the plain registers declared to the engine
        # (virtual tick, ticking devices) of the latest quiet quantum boundary
        # whose device ticks were deferred (emulator.board.Board.flush_deferred_ticks)
        self.deferred = None


class RustBoard(BoardPlatform):
    """Sole executor: Python platform/devices and direct Rust CPU hooks."""
    # Bookkeeping that must stay out of the board's __dict__ (checkpoints,
    # fingerprints and portable snapshots copy that).
    __slots__ = ("_ev",)
    tick_ratio, tick_phase = (1, 1), 0
    _entry_labels = ENTRY_LABELS
    _loader_validation = reference._FIRMWARE_BINDINGS.loader

    def __init__(self, *, entry="rom", quantum=1000, cache_instructions=True, instructions_per_tick=1,
                 execution=None, profile=None):
        if entry not in ("rom", "loader"):
            raise ValueError("Entry must be rom or loader")
        execution = DEFAULT_EXECUTION if execution is None else execution
        if execution not in EXECUTION_MODES:
            raise ValueError(f"execution must be one of {EXECUTION_MODES}")
        # (num, den): instructions per virtual tick; tick_phase is the native fractional accumulator.
        # Class defaults (1/1, phase 0) keep default boards' state and snapshots unchanged.
        ratio = normalize_instructions_per_tick(instructions_per_tick)
        if ratio != (1, 1):
            self.tick_ratio = ratio
        self._ev = _EventSync()
        # Optional offline board profile; the default keeps X70 state/layout.
        ram_size = reference.RAM_SIZE if profile is None else profile.ram_size
        rom_file = ROM_FILE if profile is None else profile.rom_file
        rom_hash = ROM_HASH if profile is None else profile.rom_hash
        if profile is not None:
            if profile.entry_labels != self._entry_labels:
                self._entry_labels = profile.entry_labels
            # Legacy probe profiles retain the original binding contract.
            if hasattr(profile, 'loader_validation') and hasattr(profile, 'reset_release'):
                bindings = firmware_bindings(profile)
                if bindings.loader != self._loader_validation:
                    self._loader_validation = bindings.loader
                if bindings.reset != self._reset_release:
                    self._reset_release = bindings.reset
        self.entry, self.quantum = entry, quantum
        self.private_mmio_ranges = (getattr(reference, "EXTRA_MMIO_RANGES", ())
                                    if profile is None else profile.extra_mmio_ranges)
        self.memory_layout_token = object()
        self.cache_instructions = cache_instructions
        self._instruction_cache, self._instruction_pages = [{}, {}], [{}, {}]
        self._execution_invalidations = [set(), set()]
        self.rom = rom_file.read_bytes()
        if hashlib.sha256(self.rom).hexdigest() != rom_hash:
            raise ValueError("Captured ROM hash mismatch")
        self.low_rom_alias = entry == "rom"
        self._native = Machine(str(unicorn_library()), ram_size, self.rom,
                               self.low_rom_alias, list(self._entry_labels))
        if execution != "instructions":
            self._native.set_execution(execution)

        def view(name, size):
            result = (ctypes.c_ubyte * size).from_address(self._native.memory_address(name))
            # A retained ctypes view must keep the native allocation alive.
            result._native_owner = self._native
            return result

        self.ram, self.sram = view("ram", ram_size), view("sram", 0x1000)
        self.ram_pointer = ctypes.addressof(self.ram)
        self.rom_storage = view("rom", len(self.rom))
        self.vector_scratch = view("vectors", 0x1000)
        self.manifest = (json.loads((ROOT / "firmware/mapped/manifest.json").read_text())
                         if profile is None else profile.manifest)
        self.devices, self.core_state = [], []
        self.virtual_ticks, self.instructions = 0, 0
        self.scheduled, self.schedule_order = [], 0
        self.event_log, self.recent = [], [deque(maxlen=32), deque(maxlen=32)]
        self.failure, self.active_cpu, self.quiescent = None, 0, False
        self.executing_cpu, self.remap_requested = None, False
        self.last_quantum_complete, self._checkpoint_boundary = True, False
        self.milestones, self.loader_memory_validation = {}, []
        self.breakpoint_handler = None
        self.exclusive = [None, None]
        self.exclusive_stats = dict(loads=0, stores_succeeded=0, stores_failed=0, invalidations=0)
        self.cp14_sites = set()
        self.cpus = [NativeCore(self, self._native, core) for core in range(2)]
        for cpu in self.cpus:
            for begin, length in self.private_mmio_ranges:
                cpu.mem_map(begin, length, 3)
                cpu.hook_add(unicorn.UC_HOOK_MEM_READ | unicorn.UC_HOOK_MEM_WRITE, self._mmio,
                             user_data=cpu.core, begin=begin, end=begin + length - 1)
            if hasattr(self, "_track_memory_layout"):
                self._track_memory_layout(cpu)
        for core in range(2):
            self.core_state.append(dict(instructions=0, started=False,
                runnable=core == 0 or entry == "loader", waiting=False,
                pmcr=0, pmcntenset=0, pmu_start=0, pmu_frozen=0))
        board_ref = weakref.ref(self)

        def handler(*args):
            board = board_ref()
            if board is None:
                raise RuntimeError("Native callback owner no longer exists")
            return board._native_event(*args)

        # Counted events: the native side passes the counters with each event
        # (see _native_event), so no state is copied for most of them.
        self._native.set_handler(handler, True)
        if entry == "loader":
            section = next(s for s in self.manifest["sections"] if s["name"] == "boot")
            boot = (ROOT / section["file"]).read_bytes()
            if hashlib.sha256(boot).hexdigest() != section["sha256"]:
                raise ValueError("Boot section hash mismatch")
            self.write_memory(0, boot)
        else:
            self.record("Assumed reset ROM alias", low_address="0x0", physical_address="0x80000000",
                        note="Supported by ROM low-address data reads and high-address remap handoff; hardware capture absent.")
        self.record("Secondary high-vector reset fixture", vector_alias="0xffff0000 <-> 0x3fff0000",
                    sctlr_v=1, note="Inferred from loader vector copy and kernel high-vector literals; physical reset/ITCM mapping unvalidated")
        self.record("DRAM extent fixture", bytes=ram_size, alias="0x40000000",
                    evidence=("Stock 0x915bc8..0x915bcc selects workspace0x5d700000; ROM MPU permits both1GiB windows."
                              if profile is None else profile.dram_basis),
                    limit="512MiB populated backing is inferred from firmware address use, not a measured physical capacity.")

    def _push_native(self, *, sites=False):
        cores = [(s["instructions"], s["waiting"], s["pmcr"], s["pmcntenset"],
                  s["pmu_start"], s["pmu_frozen"], s.get("cp14_reads", 0)) for s in self.core_state]
        monitors = [None if m is None else (m["address"], m["valid"]) for m in self.exclusive]
        seen = ([(core, address) for core in range(2) for address, label in self._entry_labels.items()
                 if f"{core}:{label}" in self.milestones] if sites else None)
        self._native.load_state(self.instructions, self.virtual_ticks, cores, monitors,
            tuple(self.exclusive_stats[key] for key in ("loads", "stores_succeeded", "stores_failed", "invalidations")),
            self.remap_requested, self.cache_instructions, list(self.cp14_sites) if sites else None, seen,
            self.tick_ratio, self.tick_phase)
        self._mark_synchronised(self._native.aux_version())

    def _aux_key(self):
        """Cheap value of every per-core input `_push_native` sends besides the counters."""
        c0, c1 = self.core_state
        return (_CORE_AUX(c0), _CORE_AUX(c1), c0.get("cp14_reads", 0), c1.get("cp14_reads", 0),
                self.remap_requested, self.cache_instructions, self.tick_ratio)

    def _mark_synchronised(self, version):
        """Record that Python and the native state agree as of `version`."""
        ev = self._ev
        ev.seen, ev.key = version, self._aux_key()
        ev.instructions = (self.core_state[0]["instructions"], self.core_state[1]["instructions"])
        ev.exclusive = [None if m is None else dict(m) for m in self.exclusive]
        ev.stats = dict(self.exclusive_stats)

    # Set X70_MMIO_INDEX_CHECK=1 to cross-check every dispatch against the linear scan.
    MMIO_INDEX_CHECK = bool(os.environ.get("X70_MMIO_INDEX_CHECK"))

    def _mmio_device(self, address, size):
        """The first attached device whose contains() accepts the access, or None.

        Exactly the reference linear scan's answer, evaluated over the
        devices that could accept an access at this page (see _MmioIndex).
        """
        ev = self._ev
        index = ev.mmio
        devices = self.devices
        if index is None or index.devices != devices:
            index = ev.mmio = _MmioIndex(devices)
        for device in index.pages.get(address >> 12, index.wildcards):
            if device.contains(address, size):
                found = device
                break
        else:
            found = None
        if self.MMIO_INDEX_CHECK:
            expected = super()._mmio_device(address, size)
            if expected is not found:
                raise RuntimeError(f"MMIO index chose {found!r} for {address:#x}/{size}; the scan chooses {expected!r}")
        return found

    def set_execution(self, mode):
        """Select "fast" (default) or "instructions" execution while paused.

        "fast" runs one inline instruction hook with natively enforced limits
        instead of Unicorn's per-instruction count hook; every value Python
        or a device can observe is identical (see native/README.md). The mode
        belongs to the native engine, not the board, so checkpoints and
        fingerprints do not carry it.
        """
        if self.executing_cpu is not None:
            raise RuntimeError("execution mode can change only while paused")
        if mode not in EXECUTION_MODES:
            raise ValueError(f"execution must be one of {EXECUTION_MODES}")
        self._native.set_execution(mode)

    def set_instructions_per_tick(self, value):
        """Change the declared CPU-speed ratio of a loaded board at a run boundary.

        The fractional tick accumulator restarts from zero (less than one
        tick is dropped). Recorded in the board events and in the portable
        factory options so later snapshots carry the new ratio.
        """
        if self.executing_cpu is not None or not self.last_quantum_complete:
            raise RuntimeError("instructions_per_tick can change only at a run boundary")
        ratio = normalize_instructions_per_tick(value)
        old = self.tick_ratio
        self.tick_ratio, self.tick_phase = ratio, 0
        options = getattr(self, "_portable_factory_options", None)
        if isinstance(options, dict):
            options = dict(options)
            if ratio == (1, 1):
                options.pop("instructions_per_tick", None)
            else:
                options["instructions_per_tick"] = ratio_option(ratio)
            self._portable_factory_options = options
        self._push_native()
        self.record("Declared CPU-speed ratio changed", old=ratio_option(old), new=ratio_option(ratio),
                    instructions=self.instructions, note="Declared instructions per virtual tick; the camera's CPU clock is unknown")

    def _pull_native(self, *, trace=True):
        state = self._native.state(trace)
        self.instructions, self.virtual_ticks = state["instructions"], state["virtual_ticks"]
        if state["tick_phase"] or "tick_phase" in vars(self):
            self.tick_phase = state["tick_phase"]
        fields = ("instructions", "waiting", "pmcr", "pmcntenset", "pmu_start", "pmu_frozen")
        for core, values in enumerate(state["cores"]):
            self.core_state[core].update(zip(fields, values[:6]))
            if values[6] or "cp14_reads" in self.core_state[core]:
                self.core_state[core]["cp14_reads"] = values[6]
        self.exclusive = [None if m is None else dict(address=m[0], valid=m[1]) for m in state["monitors"]]
        self.exclusive_stats.update(zip(("loads", "stores_succeeded", "stores_failed", "invalidations"), state["counts"]))
        self.cp14_sites = set(state["cp14_sites"])
        self._execution_invalidations = [set(p) for p in state["pending"]]
        if trace:
            self.recent = [deque(t, maxlen=32) for t in state["recent"]]
        self._mark_synchronised(state["aux_version"])

    def _pull_after_run(self, core):
        """`_pull_native()` after a run of `core`, reading only what moved.

        While the native `aux_version` stands still only the counters and the
        running core's trace can differ from the mirror (see `_native_event`);
        the other core's trace did not move. X70_EMULATOR_AUDIT=1 compares
        the result with a full pull.
        """
        instructions, ticks, phase, first, second, aux, ring = self._native.run_state(core)
        ev = self._ev
        if aux != ev.seen:
            self._pull_native()
            return
        self.instructions, self.virtual_ticks = instructions, ticks
        if phase or "tick_phase" in self.__dict__:
            self.tick_phase = phase
        c0, c1 = self.core_state
        c0["instructions"], c1["instructions"] = ev.instructions = (first, second)
        self.recent[core] = deque(ring, maxlen=32)
        if strict_audit.ENABLED:
            mirror = self._mirror()
            self._pull_native()
            if self._mirror() != mirror:
                raise RuntimeError("Native state moved without its aux_version")

    def _mirror(self):
        return (self.instructions, self.virtual_ticks, self.tick_phase, [dict(c) for c in self.core_state],
                self.exclusive, dict(self.exclusive_stats), set(self.cp14_sites), self._execution_invalidations,
                [list(r) for r in self.recent])

    def write_memory(self, address, data, cpu_id=0):
        self._push_native()
        self._native.write_bus(cpu_id, address, bytes(data))
        self._pull_native(trace=False)
        if self.executing_cpu is None:
            for cpu in self.cpus:
                cpu.ctl_remove_cache(address, address + len(data))
                if address < len(self.ram):
                    cpu.ctl_remove_cache(address | 0x40000000, (address | 0x40000000) + len(data))
                elif 0x40000000 <= address < 0x40000000 + len(self.ram):
                    cpu.ctl_remove_cache(address & 0x3fffffff, (address & 0x3fffffff) + len(data))

    def invalidate_execution_caches(self):
        self._native.clear_caches()
        self._instruction_cache, self._instruction_pages = [{}, {}], [{}, {}]
        self._execution_invalidations = [set(), set()]
        self._push_native(sites=True)
        self._native.load_trace([[(int(pc, 16), bytes.fromhex(raw)) for pc, raw in trace] for trace in self.recent])

    def _flush_execution_invalidations(self):
        self._native.flush_pending()
        self._execution_invalidations = [set(), set()]

    def _instruction(self, cpu, address, size, core):
        size, raw, word, ordinary = self._native.instruction(core, address, size)
        result = (size, raw, word if size == 4 else None, ordinary, (hex(address), raw.hex()))
        if self.cache_instructions:
            self._instruction_cache[core][address] = result
        self._instruction_pages[core].setdefault(address >> 12, set()).add(address)
        return result

    def _stop(self, cpu, core, message, **details):
        self._pull_native()
        return super()._stop(cpu, core, message, **details)

    def _native_event(self, kind, core, address, size, value, instructions=None, ticks=0, phase=0,
                      core_instructions=0, aux=None):
        """Handle one native event.

        With the counters passed by the native side (the board's own handler),
        Python's instructions/virtual_ticks/tick_phase and the executing core's
        instruction count are set from the arguments, and everything else the
        mirror holds (waiting/PMU/CP14 fields, exclusive monitors and counts,
        CP14 sites, pending invalidations) is re-read only when the native
        `aux_version` moved. After the handler, state is pushed back only if
        Python changed a counter or one of the pushed inputs. Without counters
        (a five-argument handler wrapper) the whole mirror is copied each way.
        """
        self._ev.events += 1
        if instructions is None:
            self._pull_native(trace=False)
            self._end_quiet_batch()
            self.flush_deferred_ticks()
            self._dispatch_event(kind, core, address, size, value)
            self._push_native()
            return
        self.instructions, self.virtual_ticks = instructions, ticks
        if phase or "tick_phase" in self.__dict__:
            self.tick_phase = phase
        core_states = self.core_state
        core_states[core]["instructions"] = core_instructions
        ev = self._ev
        if aux != ev.seen:
            self._pull_native(trace=False)
        # Device ticks deferred from quiet boundaries run before any event
        # can observe device state (emulator.board.Board.flush_deferred_ticks).
        self._end_quiet_batch()
        self.flush_deferred_ticks()
        self._dispatch_event(kind, core, address, size, value)
        core_states = self.core_state
        if (self.instructions != instructions or self.virtual_ticks != ticks or self.tick_phase != phase
                or core_states[core]["instructions"] != core_instructions
                or core_states[1 - core]["instructions"] != ev.instructions[1 - core]
                or self._aux_key() != ev.key or self.exclusive != ev.exclusive
                or self.exclusive_stats != ev.stats):
            self._push_native()

    def _sync_plain_registers(self):
        """Declare the attached devices' plain registers to the engine.

        A device's `native_registers` maps an address to (read, write): read
        None; "direct" for a read the engine makes by calling
        `device.read(core, address, 4)` itself (independent of time, deferred
        ticks and other devices, without effects); "timed" for one it makes by
        calling `device.native_read(address, virtual_ticks)`, which returns
        what `read` would at that tick without effects, or None to send the
        access through the event path; or (clear, set, flag) for
        a 32-bit read returning
        `(registers.get(address, 0) & ~clear) | (set if flag is None or
        getattr(device, flag) else 0)`; write None or `keep` for a 32-bit
        write storing `(value & keep) | (old & ~keep & ~value)` (keep
        0xffffffff: a plain store), with no other effect either way. The
        device's tick, if any, must not touch these registers. The engine
        serves them itself while this board's own
        `_mmio` handles them (directly or through wrappers that pass them
        through, e.g. the testbed RAM watcher) and the device owns them.
        Recomputed when the devices or the MMIO handler change.
        """
        devices, handler = self.devices, self._mmio
        hooked = getattr(getattr(handler, "__self__", None), "hooked", None)
        key = (len(devices), handler, frozenset(hooked) if hooked is not None else None)
        ev = self._ev
        if ev.plain is not None and ev.plain[0] == key and ev.plain[1] == [id(d) for d in devices]:
            return
        entries = []
        for device in devices:
            for address, (read, write) in getattr(device, "native_registers", {}).items():
                if (self._mmio_device(address, 4) is device and address != self._reset_release.register
                        and self._mmio_passes(address)):
                    entries.append((address, device, read, write))
        self._native.set_plain_registers(entries)
        ev.plain = (key, [id(d) for d in devices])

    def _mmio_passes(self, address):
        """True when an access at `address` reaches this board's own `_mmio` unchanged."""
        handler = self._mmio
        own = type(self)._mmio
        for _ in range(8):
            owner = getattr(handler, "__self__", None)
            if owner is self and getattr(handler, "__func__", None) is own:
                return True
            passes = getattr(owner, "native_passthrough", None)
            if passes is None or not passes(address):
                return False
            handler = owner.previous
        return False

    def _end_quiet_batch(self):
        batch = self._ev.batch
        if batch is not None:
            self._ev.batch = None
            batch.interrupt(self)

    def _dispatch_event(self, kind, core, address, size, value):
        cpu = self.cpus[core]
        mmio = _MMIO_KINDS.get(kind)
        if mmio is not None:
            self._mmio(cpu, mmio, address, size, value, core)
        elif kind.startswith("mmio:"):
            self._mmio(cpu, int(kind.split(":")[1]), address, size, value, core)
        elif kind.startswith("invalid:"):
            self._invalid(cpu, int(kind.split(":")[1]), address, size, value, core)
        elif kind.startswith("interrupt:"):
            self._native_interrupt(cpu, int(kind.split(":")[1]), core)
        elif kind == "milestone":
            self._entry(core, address)
        elif kind == "breakpoint":
            # Registers written by the handler (including PC) reach the engine
            # directly; the push after the event restores the scheduler's counters.
            if self.breakpoint_handler is None:
                self._stop(cpu, core, "Native breakpoint hit with no breakpoint handler", pc=hex(address))
            else:
                self.breakpoint_handler(cpu, core, address)
        elif kind == "cp14":
            self.record("CP14 CPU identity", cpu=core, pc=hex(address))
        elif kind == "wfi":
            self.record("CPU waiting for interrupt", cpu=core, pc=hex(address))
        elif kind == "halt":
            self._stop(cpu, core, "Firmware entered an unconditional self-branch",
                       reason="Firmware halt/error/power-off path; no instruction bypass applied.")
        elif kind == "exclusive_fault":
            self._stop(cpu, core, "Unsupported exclusive memory reservation", address=hex(value))
        elif kind == "pmu_fault":
            self._stop(cpu, core, "Unknown PMU register write", word=hex(value))
        elif kind == "cp14_fault":
            self._stop(cpu, core, "Unknown CP14 operation", word=hex(value))
        else:
            raise NativeError(f"Unknown native event {kind}")

    def _entry(self, core, address):
        label = self._entry_labels[address]
        self.milestones[f"{core}:{label}"] = dict(tick=self.virtual_ticks, pc=hex(address), cpu=core)
        self.record("Stock entry reached", entry=label, pc=hex(address), cpu=core)
        if address == self._loader_validation.entry:
            for name in self._loader_validation.sections:
                sec = next(s for s in self.manifest["sections"] if s["name"] == name)
                observed = self.read_memory(sec["address"], sec["size"])
                digest = hashlib.sha256(observed).hexdigest()
                check = dict(section=name, address=hex(sec["address"]), bytes=sec["size"], tick=self.virtual_ticks,
                    observed_sha256=digest, mapped_sha256=sec["sha256"], matches_mapped_file=digest == sec["sha256"])
                if digest != sec["sha256"]:
                    original = (ROOT / sec["file"]).read_bytes()
                    changes = [i for i, (a, b) in enumerate(zip(observed, original)) if a != b]
                    check.update(different_bytes=len(changes),
                        first_difference_addresses=[hex(sec["address"] + i) for i in changes[:32]],
                        note="Compare before application execution; runtime descriptor/header writes require separate explanation.")
                self.loader_memory_validation.append(check)


def make_board(*args, execution=None, **kwargs):
    """Construct the sole executor through the canonical device factory.

    `execution` ("instructions" or "fast") selects the native execution mode
    for this board; the default follows `DEFAULT_EXECUTION`.
    """
    from emulator.boot import make_board as factory
    board = factory(*args, **kwargs)
    if execution is not None:
        board.set_execution(execution)
    return board
