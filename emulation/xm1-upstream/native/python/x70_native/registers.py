"""ARM core register identifiers for the native board, without importing unicorn.

Values are Unicorn 2.1.4 ``UC_ARM_REG_*`` numbers, accepted by ``cpu.reg_read``
and ``cpu.reg_write`` on ``RustBoard.cpus``. ``NAMES`` maps lower-case names
(``r0``..``r12``, ``sp``, ``lr``, ``pc``, ``cpsr``, ``spsr``, ``fpexc``,
``cpacr``) to those identifiers; ``sp``/``lr``/``spsr`` select the bank of the
current CPSR mode.

Unicorn has no constants for the banked registers of the other modes, so
``BANKED`` names them (``sp_abt``, ``lr_irq``, ``spsr_und`` ...) as
``(mode, base register)``. ``read_register``/``write_register`` reach them by
selecting the mode through CPSR and restoring the original CPSR, which is only
valid on a paused core (never from inside a breakpoint handler). USR and SYS
share one bank (``sp_usr``/``sp_sys``, ``lr_usr``/``lr_sys``); neither has an SPSR.
"""
from unicorn.arm_const import (
    UC_ARM_REG_C1_C0_2 as CPACR, UC_ARM_REG_CPSR as CPSR, UC_ARM_REG_FPEXC as FPEXC,
    UC_ARM_REG_LR as LR, UC_ARM_REG_PC as PC, UC_ARM_REG_SP as SP, UC_ARM_REG_SPSR as SPSR,
    UC_ARM_REG_R0 as R0, UC_ARM_REG_R1 as R1, UC_ARM_REG_R2 as R2, UC_ARM_REG_R3 as R3,
    UC_ARM_REG_R4 as R4, UC_ARM_REG_R5 as R5, UC_ARM_REG_R6 as R6, UC_ARM_REG_R7 as R7,
    UC_ARM_REG_R8 as R8, UC_ARM_REG_R9 as R9, UC_ARM_REG_R10 as R10, UC_ARM_REG_R11 as R11,
    UC_ARM_REG_R12 as R12,
)

GENERAL = (R0, R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12)
NAMES = {**{f"r{index}": register for index, register in enumerate(GENERAL)},
         "sp": SP, "lr": LR, "pc": PC, "cpsr": CPSR, "spsr": SPSR, "fpexc": FPEXC, "cpacr": CPACR}

MODES = {"usr": 0x10, "fiq": 0x11, "irq": 0x12, "svc": 0x13, "abt": 0x17, "und": 0x1b, "sys": 0x1f}
BANKED = {}
for _mode, _number in MODES.items():
    BANKED[f"sp_{_mode}"] = (_number, SP)
    BANKED[f"lr_{_mode}"] = (_number, LR)
    if _mode not in ("usr", "sys"):
        BANKED[f"spsr_{_mode}"] = (_number, SPSR)
del _mode, _number


def read_register(cpu, name):
    """Read a plain or banked register on a paused core."""
    if name in NAMES:
        return cpu.reg_read(NAMES[name])
    if name not in BANKED:
        raise ValueError("Unknown register: " + str(name))
    mode, register = BANKED[name]
    cpsr = cpu.reg_read(CPSR)
    if cpsr & 31 == mode:
        return cpu.reg_read(register)
    cpu.reg_write(CPSR, (cpsr & ~31) | mode)
    try:
        return cpu.reg_read(register)
    finally:
        cpu.reg_write(CPSR, cpsr)


def write_register(cpu, name, value):
    """Write a plain or banked register on a paused core; CPSR is restored."""
    if name in NAMES:
        cpu.reg_write(NAMES[name], value)
        return
    if name not in BANKED:
        raise ValueError("Unknown register: " + str(name))
    mode, register = BANKED[name]
    cpsr = cpu.reg_read(CPSR)
    if cpsr & 31 == mode:
        cpu.reg_write(register, value)
        return
    cpu.reg_write(CPSR, (cpsr & ~31) | mode)
    try:
        cpu.reg_write(register, value)
    finally:
        cpu.reg_write(CPSR, cpsr)


__all__ = ["R0", "R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10", "R11", "R12",
           "SP", "LR", "PC", "CPSR", "SPSR", "FPEXC", "CPACR", "GENERAL", "NAMES", "MODES", "BANKED",
           "read_register", "write_register"]
