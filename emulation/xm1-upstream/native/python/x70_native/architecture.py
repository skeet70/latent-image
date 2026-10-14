"""Explicit R5F register state for fresh Rust engines, without opaque contexts.

Selectors are pinned to Unicorn 2.1.4 qemu/target/arm/{cpu,helper,unicorn_arm}.c.
The CP API silently accepts undefined selectors, so this module deliberately
uses a fixed, tested whitelist, never a scan of coprocessor register space.
The native runtime models TCM base/enable and physical RAM per core. Identity
compares its read-only size; explicit state carries mutable registers/contents.
Other CONST registers are compared and never written.

This is the register/TCM portion of a board checkpoint. DRAM, pending exceptions,
Rust PMU/exclusive monitors, device state and scheduler state belong to the board
serializer. QEMU PMU/debug extensions outside the Rust execution model are not
covered here. Translation/instruction caches are derived, not serialized.
"""
from __future__ import annotations

import unicorn
from unicorn.arm_const import (UC_ARM_REG_C1_C0_2, UC_ARM_REG_CPSR,
    UC_ARM_REG_ESR, UC_ARM_REG_FPEXC, UC_ARM_REG_FPSCR, UC_ARM_REG_FPSID,
    UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_Q0, UC_ARM_REG_R0,
    UC_ARM_REG_R8, UC_ARM_REG_SP, UC_ARM_REG_SPSR)

FORMAT = "unicorn-2.1.4-r5f-registers-v2-tcm"
MODES = (0x10, 0x11, 0x12, 0x13, 0x17, 0x1b, 0x1f)
# (opc1, CRn, CRm, opc2), CP15, 32-bit, API sec=0 NS / sec=1 S.
BANKED_CP15 = {
    "sctlr": (0, 1, 0, 0), "dacr": (0, 3, 0, 0),
    "csselr": (2, 0, 0, 0),
    "dfsr": (0, 5, 0, 0), "ifsr": (0, 5, 0, 1),
    "dfar": (0, 6, 0, 0), "ifar": (0, 6, 0, 2),
    "vbar": (0, 12, 0, 0),
    "fcseidr": (0, 13, 0, 0), "contextidr": (0, 13, 0, 1),
    "tpidrurw": (0, 13, 0, 2), "tpidruro": (0, 13, 0, 3),
    "tpidrprw": (0, 13, 0, 4),
    "mair0": (0, 10, 2, 0), "mair1": (0, 10, 2, 1),
    "par": (0, 7, 4, 0),
}
MPU_RGNR = (0, 6, 2, 0)
MPU_REGISTERS = {"drbar": (0, 6, 1, 0), "drsr": (0, 6, 1, 2),
                 "dracr": (0, 6, 1, 4)}
IDENTITY_CP15 = {"midr": (0, 0, 0, 0), "mpuir": (0, 0, 0, 4),
                  "tcmtr": (0, 0, 0, 2), "btcm": (0, 9, 1, 0),
                  "atcm": (0, 9, 1, 1)}
SCALAR_REGISTERS = {"cpacr": UC_ARM_REG_C1_C0_2,
                    "fpscr": UC_ARM_REG_FPSCR, "fpexc": UC_ARM_REG_FPEXC,
                    "esr": UC_ARM_REG_ESR}


class ArchitectureError(ValueError):
    """Register state does not match the supported, paused native engine."""


def _paused(cpu):
    from .board import NativeCore
    if not isinstance(cpu, NativeCore) or unicorn.__version__ != "2.1.4":
        raise ArchitectureError("Explicit state requires a Unicorn 2.1.4 NativeCore")
    board = cpu._board_ref()
    if board is None or board.executing_cpu is not None:
        raise ArchitectureError("Explicit register state requires a paused board")
    if cpu.ctl_get_cpu_model() != 13:
        raise ArchitectureError("Explicit register state requires the fixed R5F model")
    return board


def _read(cpu, selector, sec=0):
    return cpu.cpr_read(15, *selector, sec, False)


def _write(cpu, selector, value, sec=0):
    cpu.cpr_write(15, *selector, sec, False, value)


def _identity(cpu):
    result = {name: _read(cpu, selector) for name, selector in IDENTITY_CP15.items()}
    for name in ('atcm','btcm'):result[name]&=0x7c  # Immutable size, not mutable base/enable.
    result["fpsid"] = cpu.reg_read(UC_ARM_REG_FPSID)
    if result["midr"] != 0x411fc153 or result["mpuir"] != 0x1000:
        raise ArchitectureError("Unexpected R5F identity or MPU region count")
    return result


def export_tcm(cpu):
    getter=getattr(cpu._machine,'tcm_export',None)
    if getter is None:return None
    return [[size,reset,reg,bytes(data).hex()] for size,reset,reg,data in getter(cpu.core)]


def import_tcm(cpu,state):
    setter=getattr(cpu._machine,'tcm_restore',None)
    if state is None:
        if setter is not None:raise ArchitectureError('Snapshot lacks current TCM state')
        return
    if setter is None:raise ArchitectureError('Runtime lacks snapshot TCM model')
    try:setter(cpu.core,[(size,reset,reg,bytes.fromhex(data)) for size,reset,reg,data in state])
    except RuntimeError as exc:raise ArchitectureError('TCM state/configuration differs: '+str(exc)) from exc


def export_cpu(cpu):
    """Export JSON-compatible registers, restoring banking and MPU selection."""
    _paused(cpu)
    identity = _identity(cpu)
    cpsr = cpu.reg_read(UC_ARM_REG_CPSR)
    if cpsr & 0x1f not in MODES:
        raise ArchitectureError("Unsupported R5F CPSR mode")
    result = dict(format=FORMAT, identity=identity, cpsr=cpsr,
        r=[cpu.reg_read(UC_ARM_REG_R0 + i) for i in range(13)],
        sp=cpu.reg_read(UC_ARM_REG_SP), lr=cpu.reg_read(UC_ARM_REG_LR),
        pc=cpu.reg_read(UC_ARM_REG_PC),
        scalar={name: cpu.reg_read(reg) for name, reg in SCALAR_REGISTERS.items()},
        q=[cpu.reg_read(UC_ARM_REG_Q0 + i) for i in range(16)], tcm=export_tcm(cpu),
        cp15={name: [_read(cpu, selector, sec) for sec in (0, 1)]
              for name, selector in BANKED_CP15.items()}, banked={})
    selected = _read(cpu, MPU_RGNR)
    if not 0 <= selected < 16:
        raise ArchitectureError("Unsupported MPU selector")
    try:
        for mode in MODES:
            cpu.reg_write(UC_ARM_REG_CPSR, (cpsr & ~0x1f) | mode)
            result["banked"][str(mode)] = dict(
                r8_r12=[cpu.reg_read(UC_ARM_REG_R8 + i) for i in range(5)],
                sp=cpu.reg_read(UC_ARM_REG_SP), lr=cpu.reg_read(UC_ARM_REG_LR),
                spsr=None if mode in (0x10, 0x1f) else cpu.reg_read(UC_ARM_REG_SPSR))
        regions = []
        for index in range(16):
            _write(cpu, MPU_RGNR, index)
            regions.append({name: _read(cpu, selector)
                            for name, selector in MPU_REGISTERS.items()})
        result["mpu"] = dict(selected=selected, regions=regions)
        return result
    finally:
        _write(cpu, MPU_RGNR, selected)
        cpu.reg_write(UC_ARM_REG_CPSR, cpsr)


def _keys(value, expected, label):
    if not isinstance(value, dict) or set(value) != set(expected):
        raise ArchitectureError(f"Invalid {label} fields")


def _uint(value, bits, label):
    if type(value) is not int or not 0 <= value < 1 << bits:
        raise ArchitectureError(f"Invalid {label}: expected unsigned {bits}-bit integer")


def _values(values, count, bits, label):
    if not isinstance(values, list) or len(values) != count:
        raise ArchitectureError(f"Invalid {label} length")
    for value in values:
        _uint(value, bits, label)


def validate_cpu_state(state):
    """Preflight schema, widths and register aliases before any CPU mutation."""
    _keys(state, ("format", "identity", "cpsr", "r", "sp", "lr", "pc",
                  "scalar", "q", "cp15", "banked", "mpu", "tcm"), "CPU state")
    if state["format"] != FORMAT:
        raise ArchitectureError("Unsupported register state format")
    tcm=state['tcm']
    if tcm is not None:
        if not isinstance(tcm,list) or len(tcm)!=2:raise ArchitectureError('Invalid TCM bank count')
        for row in tcm:
            if not isinstance(row,list) or len(row)!=4:raise ArchitectureError('Invalid TCM bank')
            size,reset,reg,data=row
            for value in (size,reset,reg):_uint(value,32,'TCM register/size')
            if (size and (not 0x1000<=size<=0x800000 or size&(size-1))) or not isinstance(data,str) or len(data)!=size*2:
                raise ArchitectureError('Invalid TCM physical contents')
            try:bytes.fromhex(data)
            except ValueError as exc:raise ArchitectureError('Invalid TCM hex contents') from exc
    _keys(state["identity"], (*IDENTITY_CP15, "fpsid"), "identity")
    for value in state["identity"].values():
        _uint(value, 32, "identity")
    for name in ("cpsr", "sp", "lr", "pc"):
        _uint(state[name], 32, name)
    if state["cpsr"] & 0x1f not in MODES:
        raise ArchitectureError("Unsupported R5F CPSR mode")
    _values(state["r"], 13, 32, "general registers")
    _values(state["q"], 16, 128, "VFP registers")
    _keys(state["scalar"], SCALAR_REGISTERS, "scalar registers")
    for name, value in state["scalar"].items():
        _uint(value, 32, name)
    _keys(state["cp15"], BANKED_CP15, "CP15")
    for name, values in state["cp15"].items():
        _values(values, 2, 32, name)
    if any(value & ~0xf for value in state["cp15"]["csselr"]):
        raise ArchitectureError("CSSELR cannot preserve bits outside its write mask")
    if any(value & 31 for value in state["cp15"]["vbar"]):
        raise ArchitectureError("VBAR cannot preserve bits outside its write mask")
    if any(value & ~0xfffff6ff for value in state["cp15"]["par"]):
        raise ArchitectureError("PAR cannot preserve bits outside its R5F write mask")
    _keys(state["banked"], map(str, MODES), "banked registers")
    for mode in MODES:
        bank = state["banked"][str(mode)]
        _keys(bank, ("r8_r12", "sp", "lr", "spsr"), "bank")
        _values(bank["r8_r12"], 5, 32, "bank r8-r12")
        for name in ("sp", "lr"):
            _uint(bank[name], 32, "bank " + name)
        if mode in (0x10, 0x1f):
            if bank["spsr"] is not None:
                raise ArchitectureError("USR/SYS have no SPSR")
        else:
            _uint(bank["spsr"], 32, "bank SPSR")
        if mode != 0x11 and bank["r8_r12"] != state["banked"]["16"]["r8_r12"]:
            raise ArchitectureError("Inconsistent shared non-FIQ r8-r12 banks")
    if state["banked"]["16"] != state["banked"]["31"]:
        raise ArchitectureError("Inconsistent shared USR/SYS banks")
    current = state["banked"][str(state["cpsr"] & 0x1f)]
    if (state["r"][8:] != current["r8_r12"] or
            (state["sp"], state["lr"]) != (current["sp"], current["lr"])):
        raise ArchitectureError("Inconsistent current register bank")
    _keys(state["mpu"], ("selected", "regions"), "MPU")
    _uint(state["mpu"]["selected"], 4, "MPU selector")
    regions = state["mpu"]["regions"]
    if not isinstance(regions, list) or len(regions) != 16:
        raise ArchitectureError("Expected all 16 MPU regions")
    for region in regions:
        _keys(region, MPU_REGISTERS, "MPU region")
        for value in region.values():
            _uint(value, 32, "MPU region register")


def import_cpu(cpu, state):
    """Restore validated register state; flush derived execution caches.

    The surrounding board serializer restores memory, pending exceptions and
    Rust execution state separately. Identity/schema errors fail before writes.
    """
    board = _paused(cpu)
    validate_cpu_state(state)
    if state["identity"] != _identity(cpu):
        raise ArchitectureError("CPU identity/immutable TCM configuration mismatch")
    import_tcm(cpu,state['tcm'])
    # Host register access bypasses memory translation; restore MPU first and
    # SCTLR last. Region writes also purge Unicorn's translation lookaside cache.
    for index, region in enumerate(state["mpu"]["regions"]):
        _write(cpu, MPU_RGNR, index)
        for name, selector in MPU_REGISTERS.items():
            _write(cpu, selector, region[name])
    _write(cpu, MPU_RGNR, state["mpu"]["selected"])
    for name, selector in BANKED_CP15.items():
        if name != "sctlr":
            for sec, value in enumerate(state["cp15"][name]):
                _write(cpu, selector, value, sec)
    for mode in MODES:
        cpu.reg_write(UC_ARM_REG_CPSR, 0xd0 | mode)
        bank = state["banked"][str(mode)]
        for index, value in enumerate(bank["r8_r12"]):
            cpu.reg_write(UC_ARM_REG_R8 + index, value)
        cpu.reg_write(UC_ARM_REG_SP, bank["sp"])
        cpu.reg_write(UC_ARM_REG_LR, bank["lr"])
        if bank["spsr"] is not None:
            cpu.reg_write(UC_ARM_REG_SPSR, bank["spsr"])
    for name, reg in SCALAR_REGISTERS.items():
        cpu.reg_write(reg, state["scalar"][name])
    for index, value in enumerate(state["q"]):
        cpu.reg_write(UC_ARM_REG_Q0 + index, value)
    for sec, value in enumerate(state["cp15"]["sctlr"]):
        _write(cpu, BANKED_CP15["sctlr"], value, sec)
    for index, value in enumerate(state["r"][:8]):
        cpu.reg_write(UC_ARM_REG_R0 + index, value)
    # A PC host write changes Thumb state according to bit0, so do it before
    # restoring the exact CPSR (which also preserves in-flight Thumb IT state).
    cpu.reg_write(UC_ARM_REG_PC, state["pc"])
    cpu.reg_write(UC_ARM_REG_CPSR, state["cpsr"])
    board.invalidate_execution_caches()
