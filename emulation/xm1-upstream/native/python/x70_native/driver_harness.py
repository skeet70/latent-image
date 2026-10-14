"""Strict isolated stock-driver calls through the Rust execution engine.

Choose mapped sections and explicit ``ram_fixture=((address, bytes), ...)``
spans, attach existing device models, then ``call(address_or_corpus_name,
args=(...), max_instructions=...)``. Empty fixture spans declare zero-initial
RAM outside mapped images. Fixtures cannot change mapped firmware bytes.
``sections`` may also name ``"rom"``: the captured SoC boot ROM (hash-checked
against ``emulator.board.ROM_HASH``) at 0x80000000, read-only and callable.

Declared substitutions are explicit and appear in every call result:
``patches=((address, bytes), ...)`` is the only way to change mapped firmware
*code*; ``data_override(address, bytes, reason)`` is the separately reported
class for globals/flags inside a section and capture-vs-image differences (an
executed fetch from an overridden byte fails the call; ``data_override_restore``
puts the image bytes back); ``payloads`` add executable code/data in DRAM
outside every manifest section (``replace_payload`` rewrites bytes inside one
while paused and records the replacement); ``alloc()`` hands
out zeroed scratch DRAM from a reserved window; ``write``/``read`` access DRAM,
SRAM and the core's TCM between calls; ``stub``/``observe``/``stop_at`` are
exact-address breakpoints. ``call`` can log guest stores (``record_stores``,
including exclusive STREX stores and, optionally, stack stores), bound them
(``allowed_stores``), collect executed PCs (``coverage``), confine fetch to
declared functions (``allowed_functions``/``allowed_code``), start in any ARM
mode with explicit registers, and run on either core. With ``fetch_guard=True``
every ``call`` confines instruction fetch to the chosen images and payloads
(off by default: older callers run code placed through ``ram_fixture``, which
the guard rightly rejects; use ``payloads`` for those).

This is one virtual caller, not ROM boot or an RTOS task. Device transactions,
scheduled completions and optional attached KernelInterrupts are real model
operations; no helper return or success callback is intercepted implicitly.
Scheduled completions are delivered at their due virtual tick (the engine
yields before the first instruction at or after it), independent of
``quantum``. Every explicit stub, observer, patch, override, payload and
allocation is recorded in the call result, and stubs are counted in its
``limits`` text.
"""
from __future__ import annotations

import bisect
import csv
import functools
import hashlib
import heapq

from unicorn import UC_ERR_INSN_INVALID, UC_MEM_READ, UcError

from emulator.board import RAM_SIZE, ROOT, ROM_HASH
from emulator.hardware import UnsupportedDevice
from emulator.loader_devices import DAT_SHA256
from emulator.boards.profile import ResolvedProfile
from tools import firmware_registry as registry
from . import registers as regs
from .board import RustBoard
from .registers import CPSR, LR, NAMES, PC, R0, SP


RETURN_ADDRESS = 0x1fff0000
STACK_POINTER = 0x1ff00000
STACK_BELOW = 0x1000
STACK_ABOVE = 64
# Harness scratch for alloc(): the 1 MiB just below the default caller stack
# window (STACK_POINTER - STACK_BELOW). Far above every mapped firmware image.
ALLOC_BASE = 0x1fe00000
ALLOC_LIMIT = STACK_POINTER - STACK_BELOW
STORE_LIMIT = 1 << 20
WORD = 1 << 32
NO_DEADLINE = (1 << 64) - 1
ROM_ADDRESS = 0x80000000
CALL_MODES = (0x10, 0x11, 0x12, 0x13, 0x17, 0x1b, 0x1f)
# Ordinary, shared and private regions that read()/write() model (not MMIO).
SRAM_RANGE = (0xfff00000, 0xfff01000)
VECTOR_PAGES = ((0x3fff0000, 0x3fff1000), (0x7fff0000, 0x7fff1000))
# KernelInterrupts trace records that are IRQ assertions or deliveries.
IRQ_REQUEST_RECORDS = ("Software IRQ request", "Peripheral IRQ request", "Architectural IRQ entry")
FINAL_REGISTERS = tuple(f"r{index}" for index in range(13)) + ("sp", "lr", "pc", "cpsr")
FAULT_RECORDS = ("Architectural MPU permission prefetch abort entry", "Architectural undefined instruction entry")


@functools.lru_cache(maxsize=None)
def _corpus_rows(firmware_id="x70-110"):
    with (registry.path(registry.load(firmware_id), "corpus") / "functions.tsv").open() as stream:
        rows = list(csv.DictReader(stream, delimiter="\t"))
    result = []
    for row in rows:
        ranges = tuple((int(lo, 16), int(hi, 16) + 1) for lo, hi in
                       (item.split("-") for item in row["body_ranges"].split(",")))
        result.append((int(row["entry"], 16), row["name"], ranges))
    return result


@functools.lru_cache(maxsize=None)
def _corpus_by_entry(firmware_id="x70-110"):
    return {entry: ranges for entry, _, ranges in _corpus_rows(firmware_id)}


@functools.lru_cache(maxsize=None)
def _corpus_entries(firmware_id="x70-110"):
    names = {}
    for entry, name, _ in _corpus_rows(firmware_id):
        names.setdefault(name, []).append(entry)
    return names


@functools.lru_cache(maxsize=None)
def _corpus_index(firmware_id="x70-110"):
    spans = sorted((lo, hi, entry, name) for entry, name, ranges in _corpus_rows(firmware_id) for lo, hi in ranges)
    return [span[0] for span in spans], spans


def corpus_function(pc, firmware_id="x70-110"):
    """``(entry, name)`` of the corpus function whose body contains ``pc``, or None."""
    starts, spans = _corpus_index(firmware_id)
    index = bisect.bisect_right(starts, pc) - 1
    # Bodies do not overlap in the corpus; look back a few spans for safety.
    for lo, hi, entry, name in spans[max(0, index - 3):index + 1][::-1]:
        if lo <= pc < hi:
            return entry, name
    return None


@functools.lru_cache(maxsize=None)
def _verified_section(path, mtime_ns, size, expected_size, expected_sha):
    # Keyed by path+mtime+size so one process hashes each section file once.
    with open(path, "rb") as stream:
        data = stream.read()
    if len(data) != expected_size or hashlib.sha256(data).hexdigest() != expected_sha:
        raise ValueError("Mapped section bytes differ: " + path)
    return data


def _section_data(section, root=ROOT):
    path = root / section["file"]
    status = path.stat()
    try:
        return _verified_section(str(path), status.st_mtime_ns, status.st_size,
                                 section["size"], section["sha256"])
    except ValueError:
        raise ValueError("Mapped section bytes differ: " + str(section["name"])) from None


def _physical(address):
    return address & 0x3fffffff if 0x40000000 <= address < 0x40000000 + RAM_SIZE else address


def _word(value, what):
    if type(value) is not int or not -(1 << 31) <= value < WORD:
        raise ValueError(what + " must be a 32-bit machine word")
    return value & 0xffffffff


def _merge(ranges):
    merged = []
    for lo, hi in sorted(ranges):
        if merged and lo <= merged[-1][1]:
            merged[-1] = (merged[-1][0], max(merged[-1][1], hi))
        else:
            merged.append((lo, hi))
    return merged


def _intersect(ranges, limits):
    result = []
    for lo, hi in ranges:
        for base, top in limits:
            if max(lo, base) < min(hi, top):
                result.append((max(lo, base), min(hi, top)))
    return _merge(result)


def _subtract(ranges, holes):
    result = []
    for lo, hi in ranges:
        pieces = [(lo, hi)]
        for hole_lo, hole_hi in holes:
            pieces = [part for piece in pieces for part in
                      ((piece,) if hole_hi <= piece[0] or piece[1] <= hole_lo else
                       tuple(item for item in ((piece[0], hole_lo), (hole_hi, piece[1])) if item[0] < item[1]))]
        result.extend(pieces)
    return _merge(result)


class _DriverBoard(RustBoard):
    # True only while DriverHarness.call drives the engine: owners that run
    # the board directly (board.run) keep the original quantum-boundary drain.
    deadline_armed = False
    # Every MMIO read is a logged transaction: never answer repeats natively.
    repeatable_mmio_reads = False

    def _mmio(self, cpu, kind, address, size, value, core):
        if len(self.transactions) >= self.transaction_limit:
            self._stop(cpu, core, "Isolated driver transaction bound reached")
            return
        pc = cpu.reg_read(PC)
        if sum(device.contains(address, size) for device in self.devices) > 1:
            self._stop(cpu, core, "Ambiguous isolated driver MMIO ownership", address=hex(address))
            return
        super()._mmio(cpu, kind, address, size, value, core)
        self.transactions.append(dict(cpu=core, pc=pc, address=address, size=size,
                                      operation="read" if kind == UC_MEM_READ else "write",
                                      value=(int.from_bytes(cpu.mem_read(address, size), "little")
                                             if kind == UC_MEM_READ and not self.failure else value),
                                      accepted=self.failure is None, tick=self.virtual_ticks))

    def schedule(self, delay_ticks, callback):
        super().schedule(delay_ticks, callback)
        if self.executing_cpu is not None and self.deadline_armed:
            # A completion scheduled by a device callback mid-quantum: make the
            # engine yield at its due tick instead of the end of the quantum.
            due = self.scheduled[0][0]
            if due < self._native.tick_deadline():
                self._native.set_tick_deadline(due)


class CallContext:
    """Breakpoint view of the caller state: valid only inside its handler."""
    def __init__(self, harness, cpu, pc, label, kind):
        self._harness, self._cpu, self.pc, self.label, self.kind = harness, cpu, pc, label, kind
        self.returned = None
        self.stopped = None
        self._live = True

    def _check(self):
        if not self._live:
            raise RuntimeError("Breakpoint context is only valid while its handler runs")

    def reg(self, name):
        self._check()
        if name not in NAMES:
            raise ValueError("Unknown register: " + str(name))
        return self._cpu.reg_read(NAMES[name])

    def set_reg(self, name, value):
        self._check()
        if name not in NAMES:
            raise ValueError("Unknown register: " + str(name))
        self._cpu.reg_write(NAMES[name], _word(value, "Register value"))

    def arg(self, index):
        """Argument ``index``: r0-r3, then caller stack words at SP (hit-time)."""
        self._check()
        if type(index) is not int or not 0 <= index < 16:
            raise ValueError("Argument index must be 0..15")
        if index < 4:
            return self._cpu.reg_read(regs.GENERAL[index])
        return self.u32(self._cpu.reg_read(SP) + 4 * (index - 4))

    def read(self, address, size):
        self._check()
        return self._harness.board.read_memory(address, size, self._harness._core)

    def u32(self, address):
        return int.from_bytes(self.read(address, 4), "little")

    def write(self, address, data):
        """Coherent bus write (also invalidates cached translations)."""
        self._check()
        data = bytes(data)
        if _physical(address) < self._harness._return_address + 4 and self._harness._return_address < _physical(address) + len(data):
            raise ValueError("Breakpoint write covers the reserved return address")
        self._harness.board.write_memory(address, data, self._harness._core)

    def ret(self, value=None):
        """Observe-style early return: r0=value (if given) and PC=LR on exit."""
        self._check()
        if self.kind != "observe":
            raise RuntimeError("ret() is only available to observe handlers; stubs return automatically")
        if value is not None:
            _word(value, "Return value")
        self.returned = (value,)

    def stop(self, reason=None):
        """End the call before this instruction runs (like ``stop_at``).

        Valid in stub, observe and stop handlers. The instruction does not
        execute and is not counted; a stub's return value is discarded. The
        result records ``stopped_at`` and ``stop`` (pc, label, reason).
        """
        self._check()
        if reason is not None and not isinstance(reason, str):
            raise ValueError("Stop reason must be a string")
        self.stopped = (reason,)


class DriverHarness:
    """Reusable native board with verified code and declared caller RAM state.

    ``profile`` accepts only an explicitly resolved profile; omitted means the
    exact supplied X70 1.10 DAT. Siblings have no implicit fallback.
    ``sections`` selects names in the selected firmware manifest, or ``"rom"``
    for the captured boot ROM. RAM spans may
    also come from existing captures via ``Path(...).read_bytes()``. Loading a
    span overlapping a mapped image requires identical bytes in that overlap.
    ``board`` is public so owners can construct their existing device models
    and explicitly configure CPU/MPU state when their driver requires it.

    ``patches``: explicit byte changes inside chosen images (non-overlapping;
    reported with original/patched SHA-256). ``payloads``: executable code or
    data in DRAM outside every manifest section; a RAM-patch payload whose
    address lies inside a mapped section must use ``patches`` instead.
    ``data_overrides=((address, bytes, reason), ...)``: see ``data_override``;
    they are applied before ``ram_fixture`` is validated, so a fixture that
    overlaps overridden bytes must agree with the overridden bytes.
    ``fetch_guard`` (default False): confine instruction fetch to images and
    payloads (plus the stock IRQ vector word when an interrupt controller is
    attached); fixture-placed code fails it, so declare such code as payloads.
    ``cpu_identity={core: value}``: CP14 CPU-identity word returned by the
    stock read on that core (defaults 0x4003 / 0x5003); reported in results.
    """
    _firmware_id = "x70-110"
    _profile_identity = None
    _return_address = RETURN_ADDRESS
    _stack_pointer = STACK_POINTER
    _alloc_base = ALLOC_BASE
    _alloc_limit = ALLOC_LIMIT

    def __init__(self, *, sections, ram_fixture, profile=None, cpu=0, quantum=1000, transaction_limit=4096,
                 patches=(), payloads=(), fetch_guard=False, data_overrides=(), cpu_identity=None):
        if type(cpu) is not int or cpu not in (0, 1):
            raise ValueError("Driver CPU must be 0 or 1")
        if any(type(value) is not int or value < 1 for value in (quantum, transaction_limit)):
            raise ValueError("Driver quantum and transaction bound must be positive integers")
        if type(fetch_guard) is not bool:
            raise ValueError("fetch_guard must be a bool")
        identity = self._identity_map(cpu_identity)
        if profile is not None:
            if not isinstance(profile, ResolvedProfile):
                raise TypeError("profile must be an explicitly resolved ResolvedProfile")
            registered = registry.load(profile.firmware_id)
            if profile.manifest["source_sha256"] != registered["source"]["sha256"]:
                raise ValueError("Resolved profile DAT differs from registry")
            if profile.ram_size < 0x200000:
                raise ValueError("Driver profile requires at least 2 MiB DRAM")
            self._firmware_id = profile.firmware_id
            self._profile_identity = dict(firmware_id=profile.firmware_id,
                profile_sha256=profile.profile_sha256, parts=dict(profile.part_sha256))
            shift = profile.ram_size - RAM_SIZE
            self._return_address = RETURN_ADDRESS + shift
            self._stack_pointer = STACK_POINTER + shift
            self._alloc_base = ALLOC_BASE + shift
            self._alloc_limit = ALLOC_LIMIT + shift
        # The historical loader start loads its verified boot section. A
        # registry application-only map has no loader; initialize the native
        # reset map, then switch to DRAM without executing any ROM instruction.
        has_boot = profile is None or any(s["name"] == "boot" for s in profile.manifest["sections"])
        self.board = _DriverBoard(entry="loader" if has_boot else "rom", quantum=quantum,
                                  **({} if profile is None else {"profile": profile}))
        if not has_boot:
            self.board.remap_low_ram()
        self.board.transactions, self.board.transaction_limit = [], transaction_limit
        self.cpu = self._core = cpu
        self.fetch_guard = fetch_guard
        self._default_identity = tuple(self.board._native.cp14_identity())
        for core, value in identity.items():
            self.board._native.set_cp14_identity(core, value)
        self._base_identity = tuple(self.board._native.cp14_identity())
        self._select_core(cpu)
        if self._firmware_id == "x70-110":
            if self.board.manifest["source_sha256"] != DAT_SHA256:
                raise ValueError("Driver harness requires the supplied X70 1.10 map")
        elif self.board.manifest["source_sha256"] != profile.manifest["source_sha256"]:
            raise ValueError("Driver harness mapped firmware differs from explicit profile")
        names = tuple(sections)
        if not names or len(set(names)) != len(names):
            raise ValueError("Choose distinct mapped section names")
        self.images, self.image_names = [], []
        for name in names:
            if name == "rom":
                if hashlib.sha256(self.board.rom).hexdigest() != (ROM_HASH if profile is None else profile.rom_hash):
                    raise ValueError("Captured ROM hash mismatch")
                if self.board.read_memory(ROM_ADDRESS, len(self.board.rom)) != self.board.rom:
                    raise ValueError("Mapped boot ROM differs from the verified capture")
                self.images.append((ROM_ADDRESS, bytes(self.board.rom)))
                self.image_names.append("rom")
                continue
            entries = [item for item in self.board.manifest["sections"] if item["name"] == name]
            if len(entries) != 1:
                raise ValueError("Unknown mapped section: " + str(name))
            section = entries[0]
            data = _section_data(section, ROOT if profile is None else profile.root)
            address = section["address"]
            self.board.write_memory(address, data)
            self.images.append((address, data))
            self.image_names.append(name)
        # Every manifest section (chosen or not) is the camera's memory map.
        self._manifest_ranges = [(item["address"], item["address"] + item["size"])
                                 for item in self.board.manifest["sections"]]
        self.patches, self.payloads, self.allocations, self.data_overrides = [], [], [], []
        self._fixture_ranges, self._alloc_next = [], self._alloc_base
        self._breakpoints, self._hits, self._stopped_at, self._hit_limit = {}, [], None, 0
        self._stop_info = None
        self.ram_fixture_spans = []
        self.data_override_events, self.payload_replacements = [], []
        self._override_originals, self._calls = {}, 0
        self._apply_patches(patches)
        # Overrides come before the fixture: a capture that overlaps overridden
        # image bytes is validated against the overridden bytes, not the image.
        for override in data_overrides:
            try:
                address, data, reason = override
            except (TypeError, ValueError):
                raise ValueError("A data override is (address, bytes, reason)") from None
            self.data_override(address, data, reason)
        self._declare_payloads(payloads)
        self.load_ram(ram_fixture)

    # -- helpers ---------------------------------------------------------
    @staticmethod
    def _identity_map(identity):
        if identity is None:
            return {}
        if not isinstance(identity, dict):
            raise ValueError("cpu_identity is {core: 32-bit value}")
        for core, value in identity.items():
            if type(core) is not int or core not in (0, 1):
                raise ValueError("cpu_identity names a nonexistent core: " + str(core))
            if type(value) is not int or not 0 <= value < WORD:
                raise ValueError("cpu_identity values are unsigned 32-bit words")
        return dict(identity)

    def _select_core(self, core):
        """Only ``core`` is runnable: the other stays parked for the call."""
        for index, state in enumerate(self.board.core_state):
            state.update(runnable=index == core, waiting=False if index == core else state["waiting"])
        self._core = core

    # -- declared state -------------------------------------------------
    def _apply_patches(self, patches):
        prepared = []
        for item in patches:
            try:
                address, data = item
            except (TypeError, ValueError):
                raise ValueError("A patch is (address, bytes)") from None
            if type(address) is not int or not isinstance(data, (bytes, bytearray, memoryview)) or not len(data):
                raise ValueError("A patch needs an integer address and nonempty bytes")
            data = bytes(data)
            owner = [index for index, (base, image) in enumerate(self.images)
                     if base <= address and address + len(data) <= base + len(image)]
            if not owner:
                raise ValueError("Patch must lie wholly inside one chosen mapped image: " + hex(address))
            if self.image_names[owner[0]] == "rom":
                raise ValueError("The boot ROM is read-only; it cannot be patched")
            prepared.append((address, data, owner[0]))
        ordered = sorted(prepared, key=lambda patch: patch[0])
        if any(a[0] + len(a[1]) > b[0] for a, b in zip(ordered, ordered[1:])):
            raise ValueError("Patches must not overlap")
        for address, data, index in ordered:
            base, image = self.images[index]
            original = image[address - base:address - base + len(data)]
            patched = image[:address - base] + data + image[address - base + len(data):]
            self.board.write_memory(address, data)
            self.images[index] = (base, patched)
            self.patches.append(dict(address=address, length=len(data), section=self.image_names[index],
                                     original_sha256=hashlib.sha256(original).hexdigest(),
                                     patched_sha256=hashlib.sha256(data).hexdigest()))

    def data_override(self, address, data, reason):
        """Declare runtime data written inside a chosen mapped image.

        For globals/flags that live inside a section and for capture-vs-image
        differences; not code (use ``patches`` for code). Reported separately
        as ``data_overrides``. The bytes may not overlap a patch or another
        override, and an executed instruction that fetches from an overridden
        byte fails the call (checked natively through coverage; the fetch guard
        also excludes them) for as long as the override is active;
        ``data_override_restore`` ends it. ``data_overrides`` lists the active
        overrides, ``data_override_events`` every override and restore in order.
        Returns the address.
        """
        if self.board.executing_cpu is not None:
            raise ValueError("data_override requires a paused harness")
        if type(address) is not int or not isinstance(data, (bytes, bytearray, memoryview)) or not len(data):
            raise ValueError("A data override needs an integer address and nonempty bytes")
        if not isinstance(reason, str) or not reason.strip():
            raise ValueError("A data override needs a reason")
        data = bytes(data)
        owner = [index for index, (base, image) in enumerate(self.images)
                 if base <= address and address + len(data) <= base + len(image)]
        if not owner:
            raise ValueError("Data override must lie wholly inside one chosen mapped image: " + hex(address))
        index = owner[0]
        if self.image_names[index] == "rom":
            raise ValueError("The boot ROM is read-only; it cannot be overridden")
        spans = [(p["address"], p["address"] + p["length"]) for p in self.patches + self.data_overrides]
        if any(address < hi and lo < address + len(data) for lo, hi in spans):
            raise ValueError("Data override overlaps a patch or another override")
        base, image = self.images[index]
        original = image[address - base:address - base + len(data)]
        self.board.write_memory(address, data)
        self.images[index] = (base, image[:address - base] + data + image[address - base + len(data):])
        record = dict(address=address, length=len(data), section=self.image_names[index],
                      reason=reason, original_sha256=hashlib.sha256(original).hexdigest(),
                      new_sha256=hashlib.sha256(data).hexdigest())
        self.data_overrides.append(record)
        self._override_originals[address] = original
        self.data_override_events.append(dict(record, event="override", call_index=self._calls))
        return address

    def data_override_restore(self, address):
        """Put the original image bytes back under the active override at ``address``.

        ``address`` is the exact start address of an active override. The image
        bytes (and the mapped memory) return to what they were before the
        override; the override leaves ``data_overrides`` and, once none remain,
        the executed-fetch check stops applying. The restore is appended to
        ``data_override_events`` (``event='restore'``, with ``call_index`` the
        number of calls made before it). Requires a paused harness. Returns
        the address.
        """
        if self.board.executing_cpu is not None:
            raise ValueError("data_override_restore requires a paused harness")
        matches = [item for item in self.data_overrides if item["address"] == address]
        if type(address) is not int or not matches:
            raise ValueError("No active data override starts at " + str(address))
        record = matches[0]
        original = self._override_originals.pop(address)
        index = self.image_names.index(record["section"])
        base, image = self.images[index]
        length = record["length"]
        self.board.write_memory(address, original)
        self.images[index] = (base, image[:address - base] + original + image[address - base + length:])
        self.data_overrides.remove(record)
        self.data_override_events.append(dict(
            record, event="restore", call_index=self._calls,
            restored_sha256=hashlib.sha256(original).hexdigest(),
            overridden_sha256=record["new_sha256"]))
        return address

    def replace_payload(self, address, data, *, reason):
        """Rewrite bytes inside one already-declared payload, between calls.

        ``data`` replaces exactly ``len(data)`` bytes at ``address`` (physical
        or 0x40000000 alias), which must lie wholly inside a single declared
        payload; the payload keeps its length and any bytes outside the
        replaced span (a shorter replacement is not zero-padded). Anything
        outside a declared payload is refused (use ``write`` for fixtures and
        allocations, ``patches``/``data_override`` for images). Translation
        caches for both the cached and 0x40000000 views are invalidated;
        registered breakpoints stay registered. Each replacement is appended to
        ``payload_replacements`` (also in every call result): ``address``,
        ``length``, ``old_sha256``, ``new_sha256`` of the span,
        ``payload`` (its address), ``payload_sha256_before/after``, ``reason``
        and ``call_index`` (the number of calls made before it). ``payloads``
        entries report the current whole-payload ``sha256``. Returns the record.
        """
        if self.board.executing_cpu is not None:
            raise ValueError("replace_payload requires a paused harness")
        if type(address) is not int or not isinstance(data, (bytes, bytearray, memoryview)) or not len(data):
            raise ValueError("A payload replacement needs an integer address and nonempty bytes")
        if not isinstance(reason, str) or not reason.strip():
            raise ValueError("A payload replacement needs a reason")
        data, physical = bytes(data), _physical(address)
        owner = [item for item in self.payloads
                 if item["address"] <= physical and physical + len(data) <= item["address"] + item["length"]]
        if not owner:
            raise ValueError("Replacement must lie wholly inside one declared payload: " + hex(address))
        payload = owner[0]
        old = self.board.read_memory(physical, len(data), self._core)
        before = payload["sha256"]
        self.board.write_memory(physical, data, self._core)
        payload["sha256"] = hashlib.sha256(
            self.board.read_memory(payload["address"], payload["length"], self._core)).hexdigest()
        record = dict(address=physical, length=len(data), old_sha256=hashlib.sha256(old).hexdigest(),
                      new_sha256=hashlib.sha256(data).hexdigest(), payload=payload["address"],
                      payload_sha256_before=before, payload_sha256_after=payload["sha256"],
                      reason=reason, call_index=self._calls)
        self.payload_replacements.append(record)
        return dict(record)

    def reg(self, name, cpu=None):
        """Read one register (``registers.NAMES`` or banked name) of ``cpu`` between calls."""
        if self.board.executing_cpu is not None:
            raise ValueError("reg requires a paused harness")
        return regs.read_register(self.board.cpus[self._core_arg(cpu)], name)

    def _declare_payloads(self, payloads):
        prepared = []
        for item in payloads:
            try:
                address, data = item
            except (TypeError, ValueError):
                raise ValueError("A payload is (address, bytes)") from None
            if type(address) is not int or not isinstance(data, (bytes, bytearray, memoryview)) or not len(data):
                raise ValueError("A payload needs an integer address and nonempty bytes")
            data, physical = bytes(data), _physical(address)
            if physical < 0 or physical + len(data) > len(self.board.ram):
                raise ValueError("Payload is outside modeled DRAM")
            if any(physical < hi and lo < physical + len(data) for lo, hi in self._manifest_ranges):
                raise ValueError("Payload overlaps a mapped firmware section; use patches for mapped bytes: " + hex(physical))
            if physical < self._return_address + 4 and self._return_address < physical + len(data):
                raise ValueError("Payload covers the reserved return address")
            if physical < self._alloc_limit and self._alloc_base < physical + len(data):
                raise ValueError("Payload overlaps the alloc() scratch window")
            self._check_stack_window(self._stack_pointer, physical, physical + len(data), "Payload")
            if any(physical < hi and lo < physical + len(data) for lo, hi in self._fixture_ranges):
                raise ValueError("Payload overlaps a RAM fixture")
            prepared.append((physical, data))
        ordered = sorted(prepared)
        if any(a[0] + len(a[1]) > b[0] for a, b in zip(ordered, ordered[1:])):
            raise ValueError("Payloads must not overlap")
        for physical, data in ordered:
            self.board.write_memory(physical, data)
            self.payloads.append(dict(address=physical, length=len(data), sha256=hashlib.sha256(data).hexdigest()))

    @staticmethod
    def _check_stack_window(stack_pointer, lo, hi, what, below=STACK_BELOW):
        if lo < stack_pointer + STACK_ABOVE and stack_pointer - below < hi:
            raise ValueError(what + " overlaps the caller stack window")

    def _code_ranges(self):
        return ([(base, base + len(image)) for base, image in self.images]
                + [(item["address"], item["address"] + item["length"]) for item in self.payloads])

    def load_ram(self, spans):
        """Preflight explicit DRAM fixtures before applying any span."""
        self._load_ram(spans, True)

    def _load_ram(self, spans, declare):
        prepared = []
        for address, data in spans:
            if type(address) is not int or not isinstance(data, (bytes, bytearray, memoryview)):
                raise ValueError("RAM fixture needs an integer address and bytes")
            data = bytes(data)
            physical = _physical(address)
            if physical < 0 or physical + len(data) > len(self.board.ram):
                raise ValueError("RAM fixture is outside modeled DRAM")
            if physical < self._return_address + 4 and self._return_address < physical + len(data):
                raise ValueError("RAM fixture covers the reserved return address")
            for base, image in self.images:
                lo, hi = max(base, physical), min(base + len(image), physical + len(data))
                if lo < hi and data[lo - physical:hi - physical] != image[lo - base:hi - base]:
                    raise ValueError("RAM fixture changes mapped firmware bytes")
            if data and any(physical < lo + size and lo < physical + len(data)
                            for lo, size in ((p["address"], p["length"]) for p in self.payloads + self.allocations)):
                raise ValueError("RAM fixture overlaps a declared payload or allocation")
            prepared.append((physical, data))
        for address, data in prepared:
            if data:
                self.board.write_memory(address, data)
                if declare:
                    self._fixture_ranges.append((address, address + len(data)))
            self.ram_fixture_spans.append(dict(address=address, bytes=len(data)))

    def alloc(self, size, *, align=8, data=None):
        """Zero-filled (or ``data``-initialized) scratch DRAM; returns its address.

        Bump allocation from [0x1fe00000, STACK_POINTER-0x1000): above every
        mapped image and below the default caller stack. Refused if it would
        overlap a fixture, payload or image. Allocations persist across calls
        and are listed in every result; ``write()`` may change their bytes
        between calls.
        """
        if type(size) is not int or not 0 < size <= self._alloc_limit - self._alloc_base:
            raise ValueError("Allocation size must be a positive integer within the scratch window")
        if type(align) is not int or align < 1 or align & (align - 1) or align > 0x1000:
            raise ValueError("Allocation alignment must be a power of two up to 4096")
        if data is not None:
            if not isinstance(data, (bytes, bytearray, memoryview)) or len(data) > size:
                raise ValueError("Allocation data must be bytes no longer than the allocation")
            data = bytes(data)
        address = (self._alloc_next + align - 1) & -align
        if address + size > self._alloc_limit:
            raise ValueError("Scratch window exhausted")
        spans = ([(lo, hi) for lo, hi in self._fixture_ranges] + self._manifest_ranges
                 + [(p["address"], p["address"] + p["length"]) for p in self.payloads])
        if any(address < hi and lo < address + size for lo, hi in spans):
            raise ValueError("Scratch allocation would overlap an image, payload or fixture")
        self.board.write_memory(address, bytes(size) if data is None else data.ljust(size, b"\0"))
        self._alloc_next = address + size
        self.allocations.append(dict(address=address, length=size, align=align,
                                     initialized_bytes=0 if data is None else len(data)))
        return address

    # -- memory API ------------------------------------------------------
    def _memory_class(self, core, address, size):
        """'dram', 'sram', 'vectors' or 'tcm' when [address, address+size) is
        wholly one modeled RAM region of ``core``; None otherwise (MMIO, unmapped)."""
        if type(address) is not int or type(size) is not int or size < 1 or address < 0:
            return None
        end = address + size
        physical = _physical(address)
        if physical + size <= len(self.board.ram) and (address < len(self.board.ram) or address >= 0x40000000):
            return "dram" if _physical(end - 1) == physical + size - 1 else None
        if SRAM_RANGE[0] <= address and end <= SRAM_RANGE[1]:
            return "sram"
        if any(lo <= address and end <= hi for lo, hi in VECTOR_PAGES):
            return "vectors"
        tcm_base = 0xffff0000 if core == 0 else 0xffff1000
        if core == 1 and 0xffff0000 <= address and end <= 0xffff1000:
            return "vectors"
        if tcm_base <= address and end <= WORD:
            return "tcm"
        return None

    def _mapped_rw(self, core, lo, hi):
        """True when [lo, hi) lies in merged readable+writable regions of ``core``."""
        merged = _merge((begin, end + 1) for begin, end, perms in self.board.cpus[core].mem_regions()
                        if perms & 3 == 3)
        return any(base <= lo and hi <= top for base, top in merged)

    def _core_arg(self, cpu):
        core = self.cpu if cpu is None else cpu
        if type(core) is not int or core not in (0, 1):
            raise ValueError("cpu must be 0 or 1")
        return core

    def read(self, address, size, *, cpu=None):
        """Read modeled RAM (DRAM, SRAM, vector pages, ``cpu``'s TCM); never MMIO."""
        core = self._core_arg(cpu)
        if self.board.executing_cpu is not None:
            raise ValueError("read requires a paused harness")
        kind = self._memory_class(core, address, size)
        if kind is None or (kind in ("tcm", "vectors") and not self._mapped_rw(core, address, address + size)):
            raise ValueError(f"{address:#x}+{size} is not modeled RAM (DRAM, SRAM or core {core} TCM)")
        return self.board.read_memory(address, size, core)

    def write(self, address, data, *, cpu=None):
        """Coherent write to modeled RAM between calls (invalidates caches).

        DRAM (either alias), SRAM, vector pages and ``cpu``'s TCM. Refused over
        chosen mapped images (use ``patches`` for code, ``data_override`` for
        data inside a section), payloads and the reserved return address.
        Declared fixtures and ``alloc()`` buffers may be written freely.
        """
        core = self._core_arg(cpu)
        if self.board.executing_cpu is not None:
            raise ValueError("write requires a paused harness")
        if not isinstance(data, (bytes, bytearray, memoryview)) or not len(data):
            raise ValueError("write needs nonempty bytes")
        data = bytes(data)
        kind = self._memory_class(core, address, len(data))
        if kind is None or (kind in ("tcm", "vectors") and not self._mapped_rw(core, address, address + len(data))):
            raise ValueError(f"{address:#x}+{len(data)} is not modeled RAM (DRAM, SRAM or core {core} TCM)")
        if kind == "dram":
            physical = _physical(address)
            end = physical + len(data)
            if any(physical < base + len(image) and base < end for base, image in self.images):
                raise ValueError("write would change a mapped image; use patches or data_override")
            if any(physical < p["address"] + p["length"] and p["address"] < end for p in self.payloads):
                raise ValueError("write would change a declared payload")
            if physical < self._return_address + 4 and self._return_address < end:
                raise ValueError("write covers the reserved return address")
        self.board.write_memory(address, data, core)

    def attach(self, device):
        """Attach one declared model; a KernelInterrupts model remains optional.

        A ``KernelInterrupts`` constructed without ``board=`` is bound to this
        harness's board; any other device must already belong to it.
        """
        from emulator.interrupts import KernelInterrupts
        owner = getattr(device, "board", self.board)
        if owner is None and isinstance(device, KernelInterrupts):
            device.board = self.board
        elif owner is None:
            raise ValueError("Device has no board; construct it with this harness's board")
        elif owner is not self.board:
            raise ValueError("Device belongs to another board")
        if device in self.board.devices:
            raise ValueError("Device is already attached")
        if isinstance(device, KernelInterrupts):
            if hasattr(self.board, "interrupts"):
                raise ValueError("An interrupt controller is already attached")
            self.board.interrupts = device
        return self.board.attach(device)

    def _resolve(self, entry):
        label = None
        if isinstance(entry, str):
            try:
                entry = int(entry, 16)
            except ValueError:
                matches = _corpus_entries(self._firmware_id).get(entry, [])
                if len(matches) != 1:
                    raise ValueError("Choose an exact unique corpus function name")
                label, entry = entry, matches[0]
        if type(entry) is not int or entry & 3:
            raise ValueError("Driver entry must be an aligned address in a chosen mapped image or payload")
        return label, _physical(entry)

    def _entry(self, entry):
        entry = self._resolve(entry)[1]
        if not any(lo <= entry < hi for lo, hi in self._code_ranges()):
            raise ValueError("Driver entry must be an aligned address in a chosen mapped image or payload")
        return entry

    def functions(self, executed):
        """Map executed PCs to corpus functions (``research/arm/functions.tsv``).

        Returns ``[{'entry', 'name', 'pcs': n, 'first_pc', 'last_pc'}, ...]``
        sorted by entry; PCs outside every corpus function form a final record
        with ``entry`` and ``name`` None.
        """
        found, unknown = {}, []
        for pc in executed:
            hit = corpus_function(pc, self._firmware_id)
            if hit is None:
                unknown.append(pc)
            else:
                found.setdefault(hit, []).append(pc)
        result = [dict(entry=entry, name=name, pcs=len(pcs), first_pc=min(pcs), last_pc=max(pcs))
                  for (entry, name), pcs in sorted(found.items())]
        if unknown:
            result.append(dict(entry=None, name=None, pcs=len(unknown), first_pc=min(unknown), last_pc=max(unknown)))
        return result

    def _function_ranges(self, functions):
        if not functions:
            return []   # no corpus parse unless a function was actually named
        by_entry = _corpus_by_entry(self._firmware_id)
        result = []
        for item in functions:
            if isinstance(item, str):
                try:
                    entry = int(item, 16)
                except ValueError:
                    matches = _corpus_entries(self._firmware_id).get(item, [])
                    if len(matches) != 1:
                        raise ValueError("allowed_functions needs an exact unique corpus function name: " + item)
                    entry = matches[0]
            else:
                entry = item
            if type(entry) is not int or entry not in by_entry:
                raise ValueError("allowed_functions entry is not a corpus function: " + str(item))
            result.extend(by_entry[entry])
        return result

    # -- breakpoints -----------------------------------------------------
    def _register(self, target, kind, label, **action):
        name, address = self._resolve(target)
        if not any(lo <= address < hi for lo, hi in self._code_ranges()):
            raise ValueError("Breakpoint target must be an aligned address in a chosen mapped image or payload")
        if address in self._breakpoints:
            raise ValueError("Breakpoint target is already registered: " + hex(address))
        if len(self._breakpoints) >= 4096:
            raise ValueError("Too many breakpoints")
        self._breakpoints[address] = dict(kind=kind, label=label or name or f"{address:#010x}", **action)
        return address

    def stub(self, target, returns=0, *, label=None):
        """Replace a callee: its first instruction never runs; PC=LR, r0=returns.

        ``returns`` is an int (r0), or a callable ``fn(ctx)`` yielding an int
        or None (r0 unchanged). LR with the Thumb bit set fails the call.
        """
        if not callable(returns):
            _word(returns, "Stub return value")
        return self._register(target, "stub", label, returns=returns)

    def observe(self, target, fn, *, label=None):
        """Call ``fn(ctx)`` at ``target``, then run the instruction normally."""
        if not callable(fn):
            raise ValueError("Observer must be callable")
        return self._register(target, "observe", label, fn=fn)

    def stop_at(self, target, fn=None, *, label=None):
        """End the call successfully (returned=False) before ``target`` runs.

        The instruction at ``target`` neither executes nor counts in
        ``instructions``. ``fn(ctx)``, if given, runs first (observe and stop
        on one address share this single registration); ``ctx.stop(reason)``
        from any handler has the same effect.

        A stop at the call's own entry PC is not armed until execution has
        left the entry (at least one instruction executed, an interrupt
        injected, or an exception taken), like Unicorn's ``until``. So
        ``call(P)`` with ``stop_at(P)`` and a pending IRQ injects at the start,
        runs the ISR and stops when execution returns to P; with nothing
        pending it runs from P until control returns to P. A stop at any other
        PC fires before the next IRQ injection.
        """
        if fn is not None and not callable(fn):
            raise ValueError("stop_at handler must be callable")
        return self._register(target, "stop", label, fn=fn)

    def substitute(self, target, fn, *, label=None):
        """Replace one ARM instruction's effect: ``fn(ctx)`` runs instead of it.

        Firmware bytes stay unchanged; the instruction at ``target`` neither
        executes nor counts in ``instructions``, and execution continues at
        ``target + 4``. ``fn`` supplies the effect through ``ctx.set_reg`` /
        ``ctx.write`` (e.g. a CP15 ID read the engine cannot answer) and must
        honour the instruction's condition itself. Each hit is reported with
        the replaced instruction word. Thumb state fails the call.
        """
        if not callable(fn):
            raise ValueError("Substitution handler must be callable")
        return self._register(target, "substitute", label, fn=fn)

    def unstub(self, target):
        """Drop one registered stub; refuses other kinds. Returns its address."""
        address = self._resolve(target)[1]
        record = self._breakpoints.get(address)
        if record is None or record["kind"] != "stub":
            raise ValueError("No stub is registered at " + hex(address))
        del self._breakpoints[address]
        return address

    def remove(self, target):
        """Drop whatever breakpoint (stub, observer or stop) is registered at ``target``."""
        address = self._resolve(target)[1]
        if address not in self._breakpoints:
            raise ValueError("No breakpoint is registered at " + hex(address))
        del self._breakpoints[address]
        return address

    def clear_breakpoints(self):
        self._breakpoints.clear()

    def _breakpoint(self, cpu, core, address):
        board, record = self.board, self._breakpoints.get(address)
        if record is None:
            board._stop(cpu, core, "Native breakpoint at an unregistered address", pc=hex(address))
            return
        if len(self._hits) >= self._hit_limit:
            board._stop(cpu, core, "Isolated driver breakpoint-hit bound reached")
            return
        kind, lr = record["kind"], cpu.reg_read(LR)
        hit = dict(pc=address, label=record["label"], kind=kind,
                   args=tuple(cpu.reg_read(register) for register in regs.GENERAL[:4]),
                   return_value=None)
        self._hits.append(hit)
        context = CallContext(self, cpu, address, record["label"], kind)
        value = None
        try:
            if kind == "stub":
                value = record["returns"](context) if callable(record["returns"]) else record["returns"]
                if value is not None:
                    _word(value, "Stub return value")
            elif record.get("fn") is not None:
                record["fn"](context)
                if kind == "observe" and context.returned is not None:
                    hit["kind"], value = "observe_return", context.returned[0]
        finally:
            context._live = False
        if context.stopped is not None or kind == "stop":
            if cpu.reg_read(PC) != address:
                board._stop(cpu, core, f"Breakpoint handler {record['label']} changed PC; use stub() or ret()", pc=hex(address))
                return
            reason = context.stopped[0] if context.stopped is not None else None
            self._stopped_at = address
            self._stop_info = dict(pc=address, label=record["label"], reason=reason, kind=kind)
            if reason is not None:
                hit["stop_reason"] = reason
            # Not cpu.emu_stop(): the native hook must also skip counting and
            # executing the instruction.
            board._native.breakpoint_halt(core)
            return
        if cpu.reg_read(PC) != address:
            board._stop(cpu, core, f"Breakpoint handler {record['label']} changed PC; use stub() or ret()", pc=hex(address))
            return
        if kind == "substitute":
            if cpu.reg_read(regs.CPSR) & 0x20:
                board._stop(cpu, core, "Instruction substitution requires ARM state", pc=hex(address))
                return
            hit["word"] = int.from_bytes(board.read_memory(address, 4, core), "little")
            cpu.reg_write(PC, address + 4)
            return
        if kind == "stub" or hit["kind"] == "observe_return":
            if lr & 1:
                board._stop(cpu, core, "Stub return address has the Thumb bit set", lr=hex(lr), pc=hex(address))
                return
            if value is not None:
                cpu.reg_write(R0, value & 0xffffffff)
                hit["return_value"] = value & 0xffffffff
            else:
                hit["return_value"] = cpu.reg_read(R0)
            cpu.reg_write(PC, lr)

    # -- execution -------------------------------------------------------
    def _drain(self):
        board = self.board
        while board.scheduled and board.scheduled[0][0] <= board.virtual_ticks:
            _, _, callback = heapq.heappop(board.scheduled)
            callback()
        for device in board.devices:
            if hasattr(device, "tick"):
                device.tick(board)

    def advance(self, ticks):
        """Advance virtual time by ``ticks`` between calls and deliver completions.

        Scheduled completions fire in due order, each with ``virtual_ticks`` at
        its due tick; then ``virtual_ticks`` is the target and every device's
        ``tick`` runs once. Use this instead of assigning ``board.virtual_ticks``.
        Returns ``dict(virtual_ticks, delivered, pending_scheduled_completions)``.
        """
        board = self.board
        if type(ticks) is not int or ticks < 0:
            raise ValueError("advance needs a nonnegative integer tick count")
        if board.executing_cpu is not None:
            raise ValueError("advance requires a paused harness")
        if board.failure:
            raise ValueError("Cannot advance a failed isolated driver")
        target, delivered = board.virtual_ticks + ticks, 0
        try:
            while board.scheduled and board.scheduled[0][0] <= target:
                board.virtual_ticks = max(board.virtual_ticks, board.scheduled[0][0])
                _, _, callback = heapq.heappop(board.scheduled)
                callback()
                delivered += 1
            board.virtual_ticks = target
            for device in board.devices:
                if hasattr(device, "tick"):
                    device.tick(board)
        except UnsupportedDevice as error:
            board._stop(board.cpus[self.cpu], self.cpu, str(error))
        return dict(virtual_ticks=board.virtual_ticks, delivered=delivered,
                    pending_scheduled_completions=len(board.scheduled))

    def enable_flat_mpu(self, cpu=None):
        """Program MPU region 0 as 4 GiB, full access, executable; set SCTLR.M.

        The default (MPU-disabled) PMSAv7 map makes addresses at or above
        0x80000000 execute-never, so calling code in the ``rom`` image needs an
        explicit region. Only region 0 and the SCTLR.M bit change (other SCTLR
        bits and regions are kept); the selected region number is restored.
        This is declared caller CPU state, not boot-time MPU behavior.
        """
        core = self._core_arg(cpu)
        if self.board.executing_cpu is not None:
            raise ValueError("enable_flat_mpu requires a paused harness")
        target = self.board.cpus[core]
        selected = target.cpr_read(15, 0, 6, 2, 0, 0, False)
        try:
            target.cpr_write(15, 0, 6, 2, 0, 0, False, 0)
            for crm, opc2, value in ((1, 0, 0), (1, 2, 0x3f), (1, 4, 0x303)):
                target.cpr_write(15, 0, 6, crm, opc2, 0, False, value)
        finally:
            target.cpr_write(15, 0, 6, 2, 0, 0, False, selected)
        target.cpr_write(15, 0, 1, 0, 0, 0, False, target.cpr_read(15, 0, 1, 0, 0, 0, False) | 1)

    def _guard_configuration(self, cpu, core, ranges, deliver_faults):
        vectors, opcode = [], 0xe59ff018
        needed = []
        sctlr = cpu.cpr_read(15, 0, 1, 0, 0, 0, False)
        base = 0xffff0000 if sctlr & (1 << 13) else 0
        if hasattr(self.board, "interrupts"):
            needed.append(base + 0x18)      # IRQ
        if deliver_faults:
            needed.extend((base + 4, base + 0xc))   # undefined, prefetch abort
        words = set()
        for vector in needed:
            # Exception entry fetches the vector word, which may live in the
            # loader image (low vectors) or a copied high vector. Permit exactly
            # that word, and only while it is an LDR PC,[PC,#imm] dispatch.
            word = int.from_bytes(self.board.read_memory(vector, 4, core), "little")
            if not any(lo <= vector and vector + 4 <= hi for lo, hi in ranges) and word & 0xfffff000 == 0xe59ff000:
                vectors.append((vector, vector + 4))
                words.add(word)
        if len(words) > 1:
            raise ValueError("Fetch guard vector words differ; one dispatch opcode is allowed per call")
        if words:
            opcode = words.pop()
        return vectors, opcode

    def _plan_stack(self, core, stack_pointer, stack_bytes, align_sp):
        """Validate SP for ``core``; returns (space, store-log window)."""
        if stack_pointer & 3 or (align_sp and stack_pointer & 7):
            raise ValueError("Driver stack needs aligned DRAM or TCM with room for arguments")
        below = max(stack_bytes, STACK_BELOW)
        physical = _physical(stack_pointer)
        in_dram = (stack_pointer < len(self.board.ram) or stack_pointer >= 0x40000000) and 0x1000 <= physical <= len(self.board.ram) - 64
        if in_dram:
            window = (physical - below, physical + STACK_ABOVE)
            if window[0] < self._return_address + 4 and self._return_address < window[1]:
                raise ValueError("Driver caller stack window covers the reserved return address")
            if any(physical - 0x1000 < base + len(image) and base < physical + 64 for base, image in self.images):
                raise ValueError("Driver caller stack overlaps a mapped image")
            for item in self.payloads + self.allocations:
                self._check_stack_window(physical, item["address"], item["address"] + item["length"],
                                         "Declared payload/allocation", physical - window[0])
            return "dram", window, physical
        window = (stack_pointer - below, stack_pointer + STACK_ABOVE)
        if (self._memory_class(core, window[0], window[1] - window[0]) == "tcm"
                and self._mapped_rw(core, *window)):
            return "tcm", window, stack_pointer
        raise ValueError("Driver stack needs aligned DRAM with room for arguments, or the calling core's mapped TCM")

    def _parse_registers(self, registers, stack_pointer, lr):
        parsed = {}
        if registers is None:
            return parsed
        if not isinstance(registers, dict):
            raise ValueError("registers is {name: value}")
        for name, value in registers.items():
            if name == "pc":
                raise ValueError("registers cannot set pc; the call entry is the program counter")
            if name not in NAMES and name not in regs.BANKED:
                raise ValueError("Unknown register: " + str(name))
            parsed[name] = _word(value, "Register " + name)
        if "sp" in parsed and stack_pointer is not None and parsed["sp"] != stack_pointer:
            raise ValueError("registers['sp'] and stack_pointer disagree")
        if "lr" in parsed and lr is not None and parsed["lr"] != lr:
            raise ValueError("registers['lr'] and lr disagree")
        return parsed

    def call(self, entry, args=(), *, max_instructions=10000, stack_pointer=None, cpsr=0xd3,
             record_stores=False, allowed_stores=None, stack_bytes=STACK_BELOW,
             registers=None, lr=None, cpu=None, align_sp=True, coverage=False,
             allowed_code=None, allowed_functions=None, include_stack=False,
             inject_interrupts=True, deliver_faults=False, cpu_identity=None,
             count_ranges=None, trace=0, store_limit=STORE_LIMIT):
        """Run a whole ARM routine to its real LR return or a strict failure.

        Arguments use r0-r3 and then the ordinary caller stack. Interrupts are
        masked by default. Enabling them requires explicitly prepared vectors,
        kernel state and an attached interrupt controller. A stopped/failing
        harness must be replaced; failures are never cleared automatically.

        Registered breakpoints apply to this call. ``record_stores`` logs ordered
        stores ``(address, width, value, pc)`` outside the caller stack window
        [SP-stack_bytes, SP+64): ordinary stores and successful STREX stores
        in one order, DRAM/alias stores by physical address. ``store_records``
        carries the same entries as dicts that add the guest ``virtual``
        address (cached vs 0x40000000 alias evidence), ``exclusive`` and
        ``stack`` flags. ``include_stack=True`` also logs stack-window stores
        (``stack`` True) so exact stack write ranges can be asserted.
        ``allowed_stores`` is a tuple of half-open ``(lo, hi)`` ranges.
        ``store_limit`` bounds recorded stores (default 1048576); a changed
        bound appears in the result. A store outside ``allowed_stores``
        fails the call after the offending instruction (its
        effect is not prevented).

        ``registers={'r4': ..., 'sp': ..., 'lr': ..., 'cpsr': ..., 'spsr': ...,
        'sp_abt': ...}`` sets initial state after the usual setup (explicit
        values win; ``cpsr`` selects the entry mode, usr/fiq/irq/svc/abt/und/
        sys, and banked names reach the other modes' registers; ``pc`` is the
        entry). ``cpu=0|1`` runs this call on that core of the same board
        (shared DRAM/SRAM, private TCM, per-core CP14 identity); the other core
        stays non-runnable. ``stack_pointer`` may be DRAM or the core's TCM;
        4-byte alignment is always required, 8-byte only with ``align_sp``
        (disable it for mid-routine entry). ``lr=addr`` ends the call when PC
        reaches ``addr`` (before that instruction executes, so it is not
        counted; the call counts as returned) instead of RETURN_ADDRESS. The
        address may be inside a chosen image: control reaching it by any path
        ends the call, exactly like ``stop_at`` but reported as a return.

        ``coverage=True`` returns ``executed`` (sorted unique PCs of counted
        instructions, collected natively).
        ``count_ranges=((lo, hi), ...)`` (half-open, at most 256, either DRAM
        view) returns ``range_instructions``: a list, one int per range in the
        order given, of *executed* instructions whose address lies in the range
        (ranges may overlap; an instruction counts in each). It is collected
        natively on the same path as ``instructions``: a stub, stop, ``lr=`` or
        redirecting-observer hit whose instruction does not execute is not
        counted, whereas an observer that lets the instruction run is. The
        fetch guard's ``accepted_fetches``/``range_fetches`` are different:
        they count fetch *attempts* accepted by the guard, including
        breakpoint addresses (stubs, stops), and are not instruction counts.
        ``trace=N`` (0..1048576) returns ``trace``: the first N executed PCs in
        execution order (repeats kept, addresses as fetched so an alias
        fetch shows its alias address) and ``trace_truncated``; ``executed``
        stays sorted and unique. Without these options nothing extra runs.
        The result's ``registers`` are r0-r12, sp, lr, pc and cpsr of the
        running core when the call ended; ``call_index`` is the call's 0-based
        sequence number on this harness. ``allowed_code=((lo, hi), ...)`` /
        ``allowed_functions=(names or entries)`` narrow the fetch guard (which
        they enable for this call) to those ranges plus payloads and stub/stop
        addresses. ``cpu_identity={core: value}`` overrides the CP14 identity
        for this call. ``inject_interrupts=False`` never injects a pending IRQ
        (an injection is also skipped when PC is at an armed ``stop_at``
        address other than the call entry, so that stop fires first; a stop
        or ``lr=`` address equal to the entry is not armed until execution has
        left it). ``deliver_faults=True`` routes a native MPU
        prefetch abort / disabled-VFP undefined through the board's existing
        exception entry and continues in the stock vector (recorded in
        ``delivered_faults``); the vector word must be fetchable.
        """
        entry = self._entry(entry)
        args = tuple(args)
        if len(args) > 16 or any(type(arg) is not int or not -(1 << 31) <= arg < 1 << 32 for arg in args):
            raise ValueError("Driver arguments must be at most 16 machine words")
        if type(max_instructions) is not int or max_instructions < 1:
            raise ValueError("Driver instruction budget must be positive")
        if type(store_limit) is not int or not 1 <= store_limit <= 1 << 24:
            raise ValueError("Store bound must be an integer in 1..16777216")
        core = self._core_arg(cpu)
        parsed = self._parse_registers(registers, stack_pointer, lr)
        if lr is not None:
            if type(lr) is not int or not 0 <= lr < WORD or lr & 3:
                raise ValueError("lr must be an aligned 32-bit ARM address")
        return_address = self._return_address if lr is None else lr
        initial_lr = parsed.get("lr", self._return_address if lr is None else lr)
        if "cpsr" in parsed:
            if cpsr != 0xd3 and cpsr != parsed["cpsr"]:
                raise ValueError("registers['cpsr'] and cpsr disagree")
            cpsr = parsed["cpsr"]
        if (type(cpsr) is not int or not 0 <= cpsr <= 0xffffffff
                or cpsr & 0x20 or cpsr & 31 not in CALL_MODES):
            raise ValueError("Driver caller must use an ARM user, FIQ, IRQ, supervisor, abort, undefined or system mode")
        if cpsr & 31 in (0x10, 0x1f) and "spsr" in parsed:
            raise ValueError("User and system modes have no SPSR")
        sp = parsed.get("sp", self._stack_pointer if stack_pointer is None else stack_pointer)
        if type(sp) is not int:
            raise ValueError("stack_pointer must be an integer")
        if type(stack_bytes) is not int or not 0x100 <= stack_bytes <= 0x100000 or stack_bytes & 7:
            raise ValueError("Driver stack window must be an aligned size of 256 B..1 MiB")
        if type(align_sp) is not bool or type(coverage) is not bool or type(include_stack) is not bool:
            raise ValueError("align_sp, coverage and include_stack must be bools")
        if type(inject_interrupts) is not bool or type(deliver_faults) is not bool:
            raise ValueError("inject_interrupts and deliver_faults must be bools")
        if type(record_stores) is not bool:
            raise ValueError("record_stores must be a bool")
        if include_stack and not record_stores:
            raise ValueError("include_stack requires record_stores")
        if type(trace) is not int or not 0 <= trace <= 1 << 20:
            raise ValueError("trace must be an integer 0..1048576")
        probe_ranges = None
        if count_ranges is not None:
            probe_ranges = []
            for item in count_ranges:
                try:
                    lo, hi = item
                except (TypeError, ValueError):
                    raise ValueError("count_ranges needs (lo, hi) ranges") from None
                if type(lo) is not int or type(hi) is not int or not 0 <= lo < hi <= WORD:
                    raise ValueError("count_ranges needs half-open 32-bit (lo, hi) ranges")
                probe_ranges.append((_physical(lo), _physical(hi - 1) + 1))
            if len(probe_ranges) > 256:
                raise ValueError("count_ranges accepts at most 256 ranges")
        space, window, physical_sp = self._plan_stack(core, sp, stack_bytes, align_sp)
        board, cpu_obj = self.board, self.board.cpus[core]
        machine = board._native
        if board.failure:
            raise ValueError("Cannot resume a failed isolated driver")
        if allowed_stores is not None:
            allowed_stores = [tuple(item) for item in allowed_stores]
            if any(len(item) != 2 or any(type(v) is not int for v in item) or item[0] >= item[1]
                   for item in allowed_stores):
                raise ValueError("allowed_stores needs half-open (lo, hi) address ranges")
            allowed_stores = [(_physical(lo), _physical(hi - 1) + 1) for lo, hi in allowed_stores]
        identity = dict(enumerate(self._base_identity))
        identity.update(self._identity_map(cpu_identity))
        restricted = None
        if allowed_code is not None or allowed_functions is not None:
            restricted = []
            for item in allowed_code or ():
                try:
                    lo, hi = item
                except (TypeError, ValueError):
                    raise ValueError("allowed_code needs (lo, hi) ranges") from None
                if type(lo) is not int or type(hi) is not int or not 0 <= lo < hi <= WORD:
                    raise ValueError("allowed_code needs half-open 32-bit (lo, hi) ranges")
                restricted.append((_physical(lo), _physical(hi - 1) + 1))
            restricted.extend(self._function_ranges(allowed_functions or ()))
            if not restricted:
                raise ValueError("allowed_code/allowed_functions named no ranges")
        use_guard = self.fetch_guard or restricted is not None
        if args[4:]:
            data = b"".join((arg & 0xffffffff).to_bytes(4, "little") for arg in args[4:])
            if space == "dram":
                self._load_ram(((physical_sp, data),), False)
            else:
                board.write_memory(sp, data, core)
        self._select_core(core)
        cpu_obj.reg_write(CPSR, cpsr)
        for name, value in parsed.items():
            if name in regs.BANKED:
                regs.write_register(cpu_obj, name, value)
        cpu_obj.reg_write(SP, sp)
        cpu_obj.reg_write(LR, initial_lr)
        cpu_obj.reg_write(PC, entry)
        for index, register in enumerate(regs.GENERAL[:4]):
            cpu_obj.reg_write(register, args[index] & 0xffffffff if index < len(args) else 0)
        for name, value in parsed.items():
            if name not in regs.BANKED and name not in ("cpsr", "sp", "lr"):
                cpu_obj.reg_write(NAMES[name], value)
        board.transactions.clear()
        board.core_state[core]["waiting"] = False
        irq_start = len(board.interrupts.trace) if hasattr(board, "interrupts") else 0
        start, ticks = board.instructions, board.virtual_ticks
        self._hits, self._stopped_at, self._stop_info, self._hit_limit = [], None, None, max_instructions
        logging = record_stores or allowed_stores is not None
        track = coverage or bool(self.data_overrides)
        probe = bool(probe_ranges) or trace > 0
        call_index, self._calls = self._calls, self._calls + 1
        range_instructions = trace_pcs = trace_truncated = None
        guard = dict(enabled=False)
        stores = records = executed = None
        faults = []
        guard_ranges = None
        stop_addresses = {address for address, item in self._breakpoints.items() if item["kind"] == "stop"}
        # A stop (or lr=) address equal to the entry is unarmed until execution has left it.
        entry_unarmed = entry in stop_addresses
        entry_lr = entry == return_address
        left_entry = False
        armed_stops = stop_addresses - {entry}
        board.breakpoint_handler = self._breakpoint
        board.deadline_armed = True
        try:
            for index, value in identity.items():
                if value != machine.cp14_identity()[index]:
                    machine.set_cp14_identity(index, value)
            if self._breakpoints:
                machine.set_breakpoints(core, sorted(set(self._breakpoints) - ({entry} if entry_unarmed else set())))
            if logging:
                machine.store_log_begin(core, window[0], window[1], allowed_stores, store_limit, include_stack)
            if track:
                machine.coverage_begin(core)
            if probe:
                machine.exec_probe_begin(core, probe_ranges or [], trace)
            if use_guard:
                ranges = self._code_ranges()
                if restricted is not None:
                    payload_ranges = [(p["address"], p["address"] + p["length"]) for p in self.payloads]
                    ranges = _intersect(restricted, [(base, base + len(image)) for base, image in self.images])
                    ranges = _merge(ranges + payload_ranges
                                    + [(a, a + 4) for a, item in self._breakpoints.items()
                                       if item["kind"] in ("stub", "stop", "substitute")])
                if self.data_overrides:
                    ranges = _subtract(_merge(ranges), [(o["address"], o["address"] + o["length"])
                                                        for o in self.data_overrides])
                guard_ranges = ranges
                vectors, opcode = self._guard_configuration(cpu_obj, core, ranges, deliver_faults)
                # Unicorn does not execute the `until` address; no hook sees it.
                machine.fetch_guard_begin(core, ranges, vectors, opcode)
            while (not board.failure and (cpu_obj.reg_read(PC) != return_address or (entry_lr and not left_entry))
                   and self._stopped_at is None):
                if board.instructions - start >= max_instructions:
                    board._stop(cpu_obj, core, "Isolated driver instruction bound reached")
                    break
                if board.core_state[core]["waiting"]:
                    board._stop(cpu_obj, core, "Isolated driver entered WFI; no RTOS caller continuation supplied")
                    break
                before, hits, delivered = board.instructions, len(self._hits), len(faults)
                failed = None
                try:
                    if board.scheduled and board.scheduled[0][0] <= board.virtual_ticks:
                        self._drain()
                    # Completions fire at their due tick: yield there, whatever the quantum.
                    machine.set_tick_deadline(board.scheduled[0][0] if board.scheduled else NO_DEADLINE)
                    if (inject_interrupts and hasattr(board, "interrupts")
                            and cpu_obj.reg_read(PC) not in (stop_addresses if left_entry else armed_stops)
                            and board.interrupts.inject(core, cpu_obj)):
                        board.exclusive[core] = None
                        left_entry = True
                    if left_entry and entry_unarmed and self._breakpoints:
                        machine.set_breakpoints(core, sorted(self._breakpoints))
                        entry_unarmed = False
                    budget = min(board.quantum, max_instructions - (board.instructions - start))
                    try:
                        if not left_entry and (entry_lr or entry_unarmed):
                            # Take one step with the entry stop/lr unarmed (Unicorn would stop
                            # at once on until == PC), then arm it for the rest of the call.
                            cpu_obj.emu_start(cpu_obj.reg_read(PC),
                                              entry + 4 if entry_lr else return_address, count=1)
                        else:
                            cpu_obj.emu_start(cpu_obj.reg_read(PC), return_address, count=budget)
                    except UcError as error:
                        if (deliver_faults and error.errno == UC_ERR_INSN_INVALID and not board.failure
                                and board._enter_lazy_vfp_undefined(cpu_obj, core)):
                            faults.append(dict(board.event_log[-1]))
                        else:
                            raise
                    board._flush_execution_invalidations()
                    if board.core_state[core].get("pending_native_exception"):
                        if deliver_faults and not board.failure and board._enter_native_prefetch_abort(cpu_obj, core):
                            faults.append(dict(board.event_log[-1]))
                        else:
                            sctlr = cpu_obj.cpr_read(15, 0, 1, 0, 0, 0, False)
                            hint = {} if sctlr & 1 else dict(
                                hint="MPU disabled: the default PMSAv7 map makes addresses >= 0x80000000 execute-never; "
                                     "call enable_flat_mpu() or program an MPU region")
                            board._stop(cpu_obj, core, "Native synchronous fault in isolated driver",
                                        sctlr=hex(sctlr), **hint)
                    self._drain()
                except (UcError, UnsupportedDevice) as error:
                    failed = error
                if (not left_entry and (board.instructions != before or cpu_obj.reg_read(PC) != entry
                                        or len(faults) != delivered)):
                    left_entry = True
                if left_entry and entry_unarmed and self._breakpoints and not board.failure:
                    machine.set_breakpoints(core, sorted(self._breakpoints))
                    entry_unarmed = False
                if use_guard and not board.failure:
                    # The guard names the ownership failure; Unicorn's own fetch
                    # error for the same event is a consequence, not the cause.
                    halt = machine.fetch_guard_state(core)["halt"]
                    if halt is not None:
                        board._stop(cpu_obj, core, "Isolated driver fetch guard: " + halt["reason"],
                                    fetch_guard=dict(halt, registers=list(halt["registers"])))
                if failed is not None and not board.failure:
                    board._stop(cpu_obj, core, str(failed))
                if logging and not board.failure:
                    violation = machine.store_log_state(core, False)["violation"]
                    if violation is not None:
                        board._stop(cpu_obj, core, "Isolated driver store log: " + violation)
                if (board.instructions == before and len(self._hits) == hits and len(faults) == delivered
                        and self._stopped_at is None and not board.failure):
                    board._stop(cpu_obj, core, "Isolated driver made no instruction progress")
            if use_guard:
                guard = machine.fetch_guard_state(core)
                guard = dict(guard, halt=guard["halt"] and dict(guard["halt"], registers=list(guard["halt"]["registers"])))
            if record_stores:
                state = machine.store_log_state(core, True, True)
                stores = [tuple(item) for item in state["stores"]]
                records = [dict(physical=p, virtual=v, width=w, value=value, pc=pc,
                                stack=bool(flags & 1), exclusive=bool(flags & 2))
                           for p, w, value, pc, v, flags in state["records"]]
            if track:
                executed = machine.coverage_state(core)
            if probe:
                counts, trace_pcs, trace_total = machine.exec_probe_state(core)
                trace_pcs = trace_pcs if trace else None
                trace_truncated = trace_total > len(trace_pcs) if trace else None
                range_instructions = counts if probe_ranges is not None else None
            elif probe_ranges is not None:
                range_instructions = []
            if self.data_overrides and executed and not board.failure:
                for pc in executed:
                    for item in self.data_overrides:
                        if _physical(pc) < item["address"] + item["length"] and item["address"] < _physical(pc) + 4:
                            board._stop(cpu_obj, core, "Executed instruction fetched from a data_override byte",
                                        fetch_pc=hex(pc), override_address=hex(item["address"]),
                                        override_reason=item["reason"])
                            break
                    if board.failure:
                        break
        finally:
            board.deadline_armed = False
            machine.set_tick_deadline(NO_DEADLINE)
            machine.set_breakpoints(core, [])
            if logging:
                machine.store_log_end(core)
            if track:
                machine.coverage_end(core)
            if probe:
                machine.exec_probe_end(core)
            if use_guard:
                machine.fetch_guard_end(core)
            for index, value in enumerate(self._base_identity):
                if machine.cp14_identity()[index] != value:
                    machine.set_cp14_identity(index, value)
            board.breakpoint_handler = None
        final = {name: cpu_obj.reg_read(NAMES[name]) for name in FINAL_REGISTERS}
        returned = (not board.failure and self._stopped_at is None and final["pc"] == return_address)
        stubbed = sum(item["kind"] in ("stub", "observe_return") for item in self._hits)
        limits = "Isolated declared caller/model state; no full boot, physical timing or hardware success established"
        if self._breakpoints:
            counts = [sum(r["kind"] == k for r in self._breakpoints.values())
                      for k in ("stub", "observe", "stop", "substitute")]
            limits += f"; {counts[0]} callees substituted ({stubbed} stub returns taken), {counts[1]} observers, {counts[2]} stop points"
            if counts[3]:
                replaced = sum(item["kind"] == "substitute" for item in self._hits)
                limits += f", {counts[3]} instructions substituted ({replaced} substitutions taken)"
        if self.patches:
            limits += f"; {len(self.patches)} mapped firmware byte patches"
        if self.data_overrides:
            limits += f"; {len(self.data_overrides)} runtime data overrides inside mapped images"
        if self.payloads:
            limits += f"; {len(self.payloads)} DRAM payload images"
        if restricted is not None:
            limits += f"; fetch restricted to {len(guard_ranges)} declared code ranges"
        if identity != dict(enumerate(self._default_identity)):
            limits += "; CP14 CPU identity overridden"
        if deliver_faults:
            limits += f"; {len(faults)} native faults delivered through the board's exception entry"
        irq_trace = board.interrupts.trace[irq_start:] if hasattr(board, "interrupts") else []
        result = dict(entry=entry, cpu=core, returned=returned, return_address=return_address,
                    return_value=final["r0"] if returned else None,
                    registers=final, call_index=call_index,
                    instructions=board.instructions - start, virtual_ticks=board.virtual_ticks - ticks,
                    failure=board.failure, transactions=list(board.transactions),
                    irq_events=irq_trace,
                    irq_requests=[item for item in irq_trace if item.get("name") in IRQ_REQUEST_RECORDS],
                    pending_scheduled_completions=len(board.scheduled),
                    breakpoints=list(self._hits), stopped_at=self._stopped_at,
                    stop=dict(self._stop_info) if self._stop_info else None,
                    stores=stores, store_records=records,
                    executed=executed if coverage else None,
                    range_instructions=range_instructions, trace=trace_pcs, trace_truncated=trace_truncated,
                    delivered_faults=faults,
                    cpu_identity=identity,
                    patches=[dict(item) for item in self.patches],
                    data_overrides=[dict(item) for item in self.data_overrides],
                    data_override_events=[dict(item) for item in self.data_override_events],
                    payloads=[dict(item) for item in self.payloads],
                    payload_replacements=[dict(item) for item in self.payload_replacements],
                    allocations=[dict(item) for item in self.allocations],
                    allowed_code=None if restricted is None else list(guard_ranges),
                    fetch_guard=guard if use_guard else dict(enabled=False),
                    limits=limits)
        if self._profile_identity is not None:
            result["profile"] = dict(self._profile_identity)
        if store_limit != STORE_LIMIT:
            result["store_limit"] = store_limit
            result["limits"] += f"; explicit store bound {store_limit}"
        return result
