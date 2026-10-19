"""Bounded EXR-II interrupt/timer model recovered from X70 kernel code.

Register families are inferred from actual kernel accesses, not a hardware
manual. Unknown accesses and unsupported FIQ/nesting fail. The explicit
kernel-active-scan-order fixture admits strict-higher IRQ nesting only after
the outer source is acknowledged; physical priority gating remains inferred.
Timer units are
board virtual ticks, not a claimed oscillator frequency. See the companion
research/emulator-kernel/interrupts.md for addresses and remaining limits.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from enum import IntEnum

from unicorn.arm_const import (UC_ARM_REG_CPSR, UC_ARM_REG_SPSR,
                               UC_ARM_REG_LR, UC_ARM_REG_PC)

from .hardware import UnsupportedDevice


IRQ_BASES = (0xfffb3000, 0xfffb4000)
TIMER_BASE = 0xfff91000
IRQ_MODE = 0x12
IRQ_MASK = 0x80
ASYNC_ABORT_MASK = 0x100


class DeclaredUIIRQTarget(IntEnum):
    CPU0 = 0
    CPU1 = 1


@dataclass
class InterruptBank:
    """One 128-source controller, selected by its physical bank address."""

    config: dict[int, int] = field(default_factory=dict)
    enabled: int = 0
    pending: int = 0
    active: int | None = None
    injected: int | None = None
    active_priority: int | None = None
    injected_priority: int | None = None
    active_stack: list[dict[str, int]] = field(default_factory=list)
    source_config: int = 0


@dataclass
class PolledTimer:
    """Loader polled delays and the observed application timer contract."""
    reload: int = 0
    control: int = 0
    count: int = 0
    acknowledgements: int = 0
    phase: int = 0
    counter_written: bool = False
    application_mode: bool = False
    expirations: int = 0


@dataclass
class ApplicationTimer:
    reload: int = 0
    control: int = 0
    count: int = 0
    expirations: int = 0
    counter_written: bool = False
    phase: int = 0


class KernelInterrupts:
    """Device interface: contains/read/write, plus IRQ and virtual-time hooks.

    Board.queue_irq(cpu_id, line) should call raise_irq(). At an execution
    boundary, call inject(cpu_id, uc). tick(board) tracks board.virtual_ticks.
    A supplied board is only used for record()/queue_irq(); none of the stock
    kernel routines are intercepted by this model.
    """
    # Reads the native engine may make by calling read() directly, outside
    # the board's event path (x70_native RustBoard._sync_plain_registers):
    # the active bitmap words and the acknowledged top depend only on bank
    # state that ticks never change, and have no effects.
    native_registers = {**{base + 0x230 + 4*word: ('direct', None) for base in IRQ_BASES for word in range(4)},
                        **{base + offset: ('direct', None) for base in IRQ_BASES for offset in (0x1c0, 0x1c4)},
                        # Timer reads (native_read): the kernel's clock reads.
                        **{TIMER_BASE + timer + register: ('timed', None)
                           for timer in (0, 0x10, 0x20, 0x30) for register in (0, 4, 8)}}
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True

    def __init__(self, board=None, *, timer_targets=(0, 1), tie_profile=None):
        if not timer_targets or any(cpu_id not in (0, 1) for cpu_id in timer_targets):
            raise ValueError('Timer targets must contain CPU 0 and/or 1')
        if tie_profile not in (None, 'kernel-active-scan-order'):
            raise ValueError('Unknown IRQ equal-priority tie profile')
        self.board = board
        # The stock kernel scans an already selected active bitmap this way;
        # that does NOT prove the controller's physical arbitration rule.
        # Applying this order to pending ties is an explicit opt-in fixture.
        self.tie_profile = tie_profile
        self.tie_decisions = 0
        self.last_tie_decision = None
        self.banks = [InterruptBank(), InterruptBank()]
        self.timer_targets = tuple(timer_targets)
        self.timer_reload = 0
        self.timer_control = 0
        self.timer_count = 0
        self.last_tick = 0
        self.timer_expirations = 0
        self.timer_count_written = False
        self.timer_phase = 0
        self.polled_timers = [PolledTimer(), PolledTimer()]
        self.application_timer = ApplicationTimer()
        self.trace = []
        self.trace_dropped = 0

    @staticmethod
    def contains(address, size):
        return size > 0 and (any(base <= address and address + size <= base + 0x1000
                                for base in IRQ_BASES) or
                             TIMER_BASE <= address and address + size <= TIMER_BASE + 0x1000)

    def mmio_ranges(self):
        """Superset of the start addresses contains() accepts (RustBoard._mmio_device index)."""
        return tuple((base, base + 0x1000) for base in (*IRQ_BASES, TIMER_BASE))

    TRACE_LIMIT = 20000

    def _record(self, name, **data):
        item = {'name': name, **data}
        self.trace.append(item)
        if len(self.trace) > self.TRACE_LIMIT:
            dropped = self.TRACE_LIMIT // 2
            del self.trace[:dropped]
            self.trace_dropped += dropped
        if self.board is not None:
            self.board.record(name, **data)

    @property
    def trace_position(self):
        """Count of records ever appended, including dropped ones."""
        return self.trace_dropped + len(self.trace)

    def trace_since(self, position):
        """Retained records appended at or after trace_position `position`."""
        return self.trace[max(0, position - self.trace_dropped):]

    @staticmethod
    def _word(address, size):
        if size != 4 or address & 3:
            raise UnsupportedDevice(f'IRQ/timer only supports aligned 32-bit accesses: {address:#x}/{size}')

    @staticmethod
    def _bank(address):
        for cpu_id, base in enumerate(IRQ_BASES):
            if base <= address < base + 0x1000:
                return cpu_id, address - base
        raise UnsupportedDevice(f'No IRQ controller at {address:#x}')

    @staticmethod
    def _family(offset, start, words):
        return start <= offset < start + words * 4 and offset % 4 == 0

    @staticmethod
    def _divider(control):
        # 0x91cf10 converts requested units using 12000/6000/... counts per
        # ms; 0x12644/0x1259c use 3 per us and 375 per ms at indices2/5.
        return 1 << ((control >> 4) & 7)

    def _validate_bank(self, bank):
        """Bind a plain acknowledged ancestor stack to programmed priorities."""
        stack=bank.active_stack
        if (self.tie_profile not in (None,'kernel-active-scan-order')
                or not isinstance(stack,list) or len(stack)>15
                or self.tie_profile is None and stack
                or bank.active is None and stack):
            raise UnsupportedDevice('IRQ active stack has an unsupported type/depth/profile')
        entries=[]
        for entry in stack:
            if not isinstance(entry,dict) or set(entry)!={'line','priority'}:
                raise UnsupportedDevice('IRQ active stack entry is not a line/priority record')
            entries.append((entry['line'],entry['priority']))
        for name in ('active','injected'):
            line=getattr(bank,name);priority=getattr(bank,name+'_priority')
            if line is None:
                if priority is not None:
                    raise UnsupportedDevice('IRQ priority binding exists without its in-service source')
            else:entries.append((line,priority))
        seen=set();previous=None
        for line,priority in entries:
            if (type(line) is not int or not 0<=line<128
                    or type(priority) is not int or not 0<=priority<=15
                    or line in seen or previous is not None and priority>=previous
                    or type(bank.config.get(0x800+line*4)) is not int
                    or bank.config[0x800+line*4]!=priority):
                raise UnsupportedDevice('IRQ stack/source priority binding, uniqueness or strict nesting order changed')
            seen.add(line);previous=priority
        if (bank.active is not None and bank.injected is not None
                and self.tie_profile is None):
            raise UnsupportedDevice('Nested IRQ delivery is not implemented without the explicit profile')

    def read(self, cpu_id, address, size):
        self._word(address, size)
        if TIMER_BASE <= address < TIMER_BASE + 0x1000:
            if self.board is not None:
                self.tick(self.board)
            offset = address - TIMER_BASE
            if 0x30 <= offset < 0x40:
                timer = self.application_timer
                register = offset & 0xf
                if register == 0:
                    return timer.count
                if register == 4:
                    return timer.reload
                if register == 8:
                    return timer.control
                raise UnsupportedDevice(f'Unmodeled application timer read {address:#x}')
            if 0x10 <= offset < 0x30:
                timer = self.polled_timers[(offset >> 4)-1]
                register = offset & 0xf
                if register == 0:
                    return timer.count
                if register == 4:
                    return timer.reload
                if register == 8:
                    return timer.control
                raise UnsupportedDevice(f'Unmodeled polled timer read {address:#x}')
            if offset == 0:
                return self.timer_count
            if offset == 4:
                return self.timer_reload
            if offset == 8:
                return self.timer_control
            raise UnsupportedDevice(f'Unmodeled timer read {address:#x}')
        bank_id, offset = self._bank(address)
        bank = self.banks[bank_id]
        self._validate_bank(bank)
        if self._family(offset, 0x80, 4):
            return (bank.enabled >> ((offset - 0x80) * 8)) & 0xffffffff
        if self._family(offset, 0x100, 4):
            return (bank.source_config >> ((offset - 0x100) * 8)) & 0xffffffff
        if offset == 0x200:
            line=bank.injected if bank.injected is not None else bank.active
            if line is None:
                # No interrupt injected means no successful acknowledge.
                return 0
            handler_offset = 0x400 + line * 4
            if handler_offset not in bank.config:
                raise UnsupportedDevice(f'IRQ {line:#x} lacks a firmware-programmed vector')
            if bank.injected is not None:
                if bank.active is not None:
                    bank.active_stack.append(dict(line=bank.active,priority=bank.active_priority))
                bank.active = line
                bank.active_priority = bank.injected_priority
                bank.injected = None
                bank.injected_priority = None
                self._record('IRQ acknowledge', cpu_id=bank_id, line=line,
                             priority=bank.active_priority,depth=len(bank.active_stack)+1)
            return bank.config[handler_offset]
        if self._family(offset, 0x230, 4):
            active_bits = 0 if bank.active is None else 1 << bank.active
            return (active_bits >> ((offset - 0x230) * 8)) & 0xffffffff
        if offset == 0x1c0:
            # Only the acknowledged top is exposed. The stock kernel saves it
            # after acknowledge and its active-bitmap scan must not redispatch
            # a suspended ancestor. Physical register semantics are inferred.
            return 0xffff if bank.active is None else bank.active
        if offset == 0x1c4:
            return 0xffff
        if (self._family(offset, 0x60, 4) or
                self._family(offset, 0x140, 8) or
                self._family(offset, 0x180, 8) or
                self._family(offset, 0x400, 128) or
                self._family(offset, 0x800, 128)):
            # Reset configuration is an explicit zero fixture. The stock
            # initializer writes all vector/priority slots before enabling IRQs.
            return bank.config.get(offset, 0)
        raise UnsupportedDevice(f'Unmodeled IRQ read {address:#x}')

    def native_read(self, address, now):
        """`read()` of a timer register at virtual tick `now`, without effects, or None.

        The native engine serves the timer reads in `native_registers` with
        this. `read()` ticks first; that tick only advances elapsed-time
        bookkeeping, which a later tick reproduces exactly, unless a timer
        expires by `now` (next_event_tick) or a mode `tick` does not cover
        that way is active. Then, and for any other case, None sends the
        access through the board's event path. The counts are `tick`'s
        arithmetic at `now`, evaluated on copies.
        """
        offset = address - TIMER_BASE
        if self.board is None or not 0 <= offset < 0x40 or offset & 3 or offset & 0xf == 0xc:
            return None
        last = self.last_tick
        if now < last or (self.timer_control and self.timer_reload and self.timer_control not in (0x23, 0x63)):
            return None
        if any(timer.control and timer.application_mode and not timer.reload for timer in self.polled_timers):
            return None
        due = self.next_event_tick(last)
        if due is not None and now >= due:
            return None
        elapsed, register = now - last, offset & 0xf
        if offset >= 0x10:
            timer = self.application_timer if offset >= 0x30 else self.polled_timers[(offset >> 4)-1]
            if register == 4:
                return timer.reload
            if register == 8:
                return timer.control
            if offset >= 0x30:
                if not (elapsed and timer.control and timer.reload):
                    return timer.count
            elif not timer.control:
                return timer.count
            counts = (timer.phase+elapsed) // self._divider(timer.control)
            if offset >= 0x30 or timer.application_mode:
                # No expiry before `due`: divmod(count + counts, reload) keeps the sum.
                return timer.count + counts
            return (timer.count + counts) & 0xffffffff
        if register == 4:
            return self.timer_reload
        if register == 8:
            return self.timer_control
        if not elapsed or not self.timer_control or not self.timer_reload:
            return self.timer_count
        counts = (self.timer_phase+elapsed) // self._divider(self.timer_control)
        if not counts:
            return self.timer_count
        if self.timer_control == 0x63:
            return (self.timer_count + counts) % (self.timer_reload + 1)
        return (self.timer_count or self.timer_reload) - counts

    def write(self, cpu_id, address, size, value):
        self._word(address, size)
        if type(value) is not int or not 0 <= value <= 0xffffffff:
            raise UnsupportedDevice('IRQ/timer write exceeds 32 bits')
        if TIMER_BASE <= address < TIMER_BASE + 0x1000:
            if self.board is not None:
                self.tick(self.board)
            offset = address - TIMER_BASE
            if 0x30 <= offset < 0x40:
                timer = self.application_timer
                register = offset & 0xf
                if register == 0 and value == 0:
                    timer.count = 0
                    timer.counter_written = True
                    timer.phase = 0
                elif register == 4:
                    timer.reload = value
                elif register == 8 and (value == 0 or value in range(3, 0x74, 0x10)):
                    if value and (not timer.reload or not timer.counter_written):
                        raise UnsupportedDevice('Application timer start without observed counter-zero/load sequence')
                    timer.control = value
                    timer.phase = 0
                else:
                    raise UnsupportedDevice(f'Unmodeled application timer write {address:#x}={value:#x}')
                self._record('Application timer register write', cpu_id=cpu_id, channel=3,
                             address=hex(address), value=hex(value))
                if self.board is not None:
                    self.last_tick = self.board.virtual_ticks
                return
            if 0x10 <= offset < 0x30:
                channel = offset >> 4
                timer = self.polled_timers[channel-1]
                register = offset & 0xf
                if register == 0 and value == 0 and not timer.control:
                    # Actual 0x91d228 addresses base+handle_index*0x10.
                    # This distinct sequence permits the application driver
                    # to reuse loader delay channels with a finite reload.
                    timer.count = 0
                    timer.phase = 0
                    timer.counter_written = True
                    timer.application_mode = True
                elif register == 4 and (timer.application_mode or value in (0, 0xffffffff)):
                    timer.reload = value
                elif register == 8 and (value == 0 or value in range(3, 0x74, 0x10)):
                    if value:
                        if timer.control:
                            raise UnsupportedDevice('Timer channel start while already enabled')
                        if timer.counter_written and timer.reload:
                            timer.application_mode = True
                            timer.counter_written = False
                        elif timer.reload == 0xffffffff and value in (0x23, 0x53):
                            timer.application_mode = False
                            timer.count = 0
                        else:
                            raise UnsupportedDevice('Timer channel start lacks observed application reset/load or loader maximum-load sequence')
                        timer.phase = 0
                    timer.control = value
                    if not value:
                        timer.phase = 0
                        timer.counter_written = False
                elif register == 0xc and value == 1:
                    if timer.control or timer.reload:
                        raise UnsupportedDevice('Polled timer acknowledge before observed stop/clear sequence')
                    timer.count = 0
                    timer.phase = 0
                    timer.application_mode = False
                    timer.counter_written = False
                    timer.acknowledgements += 1
                else:
                    raise UnsupportedDevice(f'Unmodeled polled timer write {address:#x}={value:#x}')
                self._record('Timer channel register write', cpu_id=cpu_id, channel=channel,
                             address=hex(address), value=hex(value))
                if self.board is not None:
                    self.last_tick = self.board.virtual_ticks
                return
            if offset == 0:
                # Stock loader writes only zero before selecting mode 0x63.
                if value != 0:
                    raise UnsupportedDevice(f'Unmodeled timer counter write {value:#x}')
                self.timer_count = value
                self.timer_count_written = True
                self.timer_phase = 0
            elif offset == 4:
                self.timer_reload = value
                # Disabled countdown setup primes the next interval. The
                # loader's explicit counter write survives its reload write;
                # an active periodic reload affects the following interval.
                if ((self.timer_control == 0 and not self.timer_count_written) or
                        (self.timer_control == 0x23 and not self.timer_count)):
                    self.timer_count = value
            elif offset == 8:
                # Exact values in stock loader 0x2720 and kernel 0x40c40.
                if value not in (0, 0x23, 0x63):
                    raise UnsupportedDevice(f'Unmodeled timer control {value:#x}')
                if value == 0x63 and not self.timer_count_written:
                    raise UnsupportedDevice('Loader timer mode requires its explicit counter-zero write')
                self.timer_control = value
                self.timer_phase = 0
                if value != 0x63:
                    self.timer_count_written = False
            else:
                raise UnsupportedDevice(f'Unmodeled timer write {address:#x}')
            self._record('Timer register write', cpu_id=cpu_id, address=hex(address), value=hex(value))
            if self.board is not None:
                self.last_tick = self.board.virtual_ticks
            return
        bank_id, offset = self._bank(address)
        bank = self.banks[bank_id]
        self._validate_bank(bank)
        if self._family(offset, 0x80, 4):
            bank.enabled |= value << ((offset - 0x80) * 8)
        elif self._family(offset, 0xa0, 4):
            bank.enabled &= ~(value << ((offset - 0xa0) * 8))
        elif self._family(offset, 0xc0, 4):
            bank.pending |= value << ((offset - 0xc0) * 8)
            self._record('Software IRQ request', cpu_id=bank_id, bits=hex(value), group=(offset - 0xc0)//4)
        elif self._family(offset, 0xe0, 4):
            bank.pending &= ~(value << ((offset - 0xe0) * 8))
        elif self._family(offset, 0x100, 4):
            shift = (offset - 0x100) * 8
            bank.source_config = (bank.source_config & ~(0xffffffff << shift)) | (value << shift)
        elif self._family(offset, 0x120, 4):
            bank.pending &= ~(value << ((offset - 0x120) * 8))
        elif offset == 0x200:
            if value != 0:
                raise UnsupportedDevice(f'Unmodeled IRQ end-of-interrupt value {value:#x}')
            if bank.injected is not None:
                raise UnsupportedDevice('IRQ end of interrupt precedes acknowledgment of the injected source')
            completed=bank.active
            if bank.active_stack:
                restored=bank.active_stack.pop()
                bank.active,bank.active_priority=restored['line'],restored['priority']
            else:bank.active=bank.active_priority=None
            self._record('IRQ end of interrupt', cpu_id=bank_id, line=completed,
                         restored=bank.active,depth=len(bank.active_stack)+(bank.active is not None))
        elif offset in (0x1c0, 0x1c4):
            if value != 0xffff:
                raise UnsupportedDevice(f'Unmodeled IRQ current-source initialization {value:#x}')
        elif offset in (0x1c8, 0xc00):
            # Exact loader reset sequence (0x24f8/0x2564), retained without
            # inventing a physical meaning or accepting other operations.
            expected = 1 if offset == 0x1c8 else 0
            if value != expected:
                raise UnsupportedDevice(f'Unmodeled IRQ reset register {offset:#x} value {value:#x}')
            bank.config[offset] = value
        elif (self._family(offset, 0x60, 4) or
                self._family(offset, 0x140, 8) or
                self._family(offset, 0x180, 8) or
                self._family(offset, 0x400, 128) or
                self._family(offset, 0x800, 128)):
            if self._family(offset, 0x800, 128) and value > 0xf:
                raise UnsupportedDevice(f'Unmodeled source-priority value {value:#x}')
            if self._family(offset,0x800,128):
                line=(offset-0x800)//4
                bound={entry['line']:entry['priority'] for entry in bank.active_stack}
                if bank.active is not None:bound[bank.active]=bank.active_priority
                if bank.injected is not None:bound[bank.injected]=bank.injected_priority
                if line in bound and value!=bound[line]:
                    raise UnsupportedDevice('In-service IRQ priority cannot change before its end-of-interrupt')
            bank.config[offset] = value
        else:
            raise UnsupportedDevice(f'Unmodeled IRQ write {address:#x}')

    def raise_irq(self, cpu_id, line):
        if type(cpu_id) is not int or cpu_id not in (0, 1) or type(line) is not int or not 0 <= line < 128:
            raise UnsupportedDevice(f'Unmodeled IRQ destination {cpu_id}/{line}')
        self._validate_bank(self.banks[cpu_id])
        self.banks[cpu_id].pending |= 1 << line
        self._record('Peripheral IRQ request', cpu_id=cpu_id, line=line)

    def irq_candidate(self, cpu_id):
        """Whether inject() could enter an IRQ on this core now (ignoring CPSR).

        False means injection stays a no-op until a device raises an IRQ or
        the firmware changes this controller, so a scheduler may skip it.
        Bit arithmetic only: unlike inject(), it never validates or raises.
        """
        bank = self.banks[cpu_id]
        candidates = bank.pending & bank.enabled
        for line in (bank.active, bank.injected):
            if type(line) is int and 0 <= line < 128:
                candidates &= ~(1 << line)
        for ancestor in bank.active_stack:
            line = ancestor.get('line') if isinstance(ancestor, dict) else None
            if type(line) is int and 0 <= line < 128:
                candidates &= ~(1 << line)
        return bool(candidates)

    def _pending_selection(self, cpu_id):
        if type(cpu_id) is not int or cpu_id not in (0, 1):
            raise UnsupportedDevice(f'Unmodeled IRQ destination {cpu_id}')
        bank = self.banks[cpu_id]
        self._validate_bank(bank)
        candidates = bank.pending & bank.enabled
        # A source already acknowledged/injected is in service, not a nested
        # request merely because its pending latch has not yet been cleared.
        if bank.active is not None:
            candidates &= ~(1 << bank.active)
        if bank.injected is not None:
            candidates &= ~(1 << bank.injected)
        for ancestor in bank.active_stack:
            candidates &= ~(1 << ancestor['line'])
        if not candidates:
            return None, [], None
        lines = [line for line in range(128) if candidates & (1 << line)]
        priorities = {}
        for line in lines:
            offset = 0x800 + line * 4
            if offset not in bank.config:
                raise UnsupportedDevice(f'IRQ {line:#x} lacks a firmware-programmed priority')
            priorities[line] = bank.config[offset]
            if type(priorities[line]) is not int or not 0<=priorities[line]<=15:
                raise UnsupportedDevice(f'IRQ {line:#x} has an unknown source-priority value')
        servicing = bank.injected if bank.injected is not None else bank.active
        if servicing is not None:
            if self.tie_profile is not None:
                service_offset = 0x800 + servicing*4
                if service_offset not in bank.config:
                    raise UnsupportedDevice(f'In-service IRQ {servicing:#x} lacks a firmware-programmed priority')
                service_priority = bank.config[service_offset]
                # Firmware 0x402d0 unmasks IRQ while the handler runs. This
                # profile infers controller gating of equal/lower priorities;
                # it does not consume those requests or fake handler completion.
                higher = [line for line in lines if priorities[line] < service_priority]
                if not higher:
                    return None, [], None
                # An architectural entry not yet acknowledged cannot be
                # overwritten by a later request, even a higher-priority one.
                if bank.injected is None:
                    lines=higher
                else:
                    raise UnsupportedDevice('Nested IRQ delivery before source acknowledgment is not implemented')
            else:raise UnsupportedDevice('Nested IRQ delivery is not implemented without the explicit profile')
        best = min(priorities[line] for line in lines)
        winners = [line for line in lines if priorities[line] == best]
        if len(winners) != 1:
            if self.tie_profile is None:
                raise UnsupportedDevice('Equal-priority IRQ arbitration is not implemented')
            selected = min(winners, key=lambda line: (line // 32, -(line % 32)))
        else:
            selected = winners[0]
        return selected, winners, best

    def pending_line(self, cpu_id):
        # Selection is read-only: neither a WFI wake check nor repeated polling
        # acknowledges a source or consumes the other pending requests.
        return self._pending_selection(cpu_id)[0]

    def declared_ui_irq_target(self):
        """Read the fresh-request predicate for the declared IRQ67 peripheral."""
        line, bit = 67, 1 << 67
        targets = []
        for cpu_id, bank in enumerate(self.banks):
            self._validate_bank(bank)
            if (bank.pending & bit or bank.injected == line or bank.active == line
                    or any(entry['line'] == line for entry in bank.active_stack)):
                raise UnsupportedDevice('Declared UI conversion requires fresh IRQ67 state in both banks')
            if bank.enabled & bit:
                targets.append(DeclaredUIIRQTarget(cpu_id))
        if len(targets) != 1:
            raise UnsupportedDevice('Declared UI conversion requires exactly one enabled IRQ67 bank')
        return targets[0]

    def inject(self, cpu_id, cpu, *, vector_base=None, sctlr=None):
        """Enter architectural IRQ state; firmware performs dispatch/return.

        Must be called between Unicorn runs, with PC at the next instruction.
        No firmware vector or handler is bypassed. SCTLR.V selects the default
        vector base; board may override for a separately established mapping.
        """
        old_cpsr = cpu.reg_read(UC_ARM_REG_CPSR)
        if old_cpsr & IRQ_MASK:
            return False
        line, winners, priority = self._pending_selection(cpu_id)
        if line is None:
            return False
        if sctlr is None:
            sctlr = cpu.cpr_read(15, 0, 1, 0, 0, 0, False)
        if sctlr & ((1 << 30) | (1 << 25)):
            raise UnsupportedDevice('Thumb/big-endian exception entry is not implemented')
        old_pc = cpu.reg_read(UC_ARM_REG_PC)
        vector_base = (0xffff0000 if sctlr & (1 << 13) else 0) if vector_base is None else vector_base
        # IRQ entry selects the banked LR/SPSR, masks IRQ, clears Thumb/IT/J.
        new_cpsr = (old_cpsr & ~(0x1f | 0x20 | 0x0600fc00 | 0x01000000 | 0x200)) | IRQ_MODE | IRQ_MASK | ASYNC_ABORT_MASK
        cpu.reg_write(UC_ARM_REG_CPSR, new_cpsr)
        cpu.reg_write(UC_ARM_REG_SPSR, old_cpsr)
        cpu.reg_write(UC_ARM_REG_LR, (old_pc + 4) & 0xffffffff)
        cpu.reg_write(UC_ARM_REG_PC, vector_base + 0x18)
        self.banks[cpu_id].injected = line
        self.banks[cpu_id].injected_priority = priority
        if len(winners) > 1:
            self.tie_decisions += 1
            self.last_tie_decision = {
                'cpu_id': cpu_id, 'profile': self.tie_profile,
                'priority': priority, 'candidates': winners, 'selected': line,
                'pending_preserved': hex(self.banks[cpu_id].pending),
                'assumption': 'Pending ties use the stock active-bitmap scan order; physical arbitration is unmeasured',
            }
            self._record('Modeled equal-priority IRQ arbitration', **self.last_tie_decision)
        self._record('Architectural IRQ entry', cpu_id=cpu_id, line=line,
                     resume_pc=hex(old_pc), vector=hex(vector_base+0x18), old_cpsr=hex(old_cpsr),
                     priority=priority,acknowledged_ancestors=len(self.banks[cpu_id].active_stack),
                     suspended_active=self.banks[cpu_id].active)
        return True

    def tick(self, board):
        """Advance exact stock control modes using provisional virtual units."""
        now = board.virtual_ticks
        if now < self.last_tick:
            raise UnsupportedDevice('Virtual time moved backwards')
        elapsed = now - self.last_tick
        self.last_tick = now
        for channel, timer in enumerate(self.polled_timers, 1):
            if timer.control:
                # Actual loader delay compares the raw counter against a
                # growing threshold. Neither prescaler is hardware validated.
                counts, timer.phase = divmod(timer.phase+elapsed, self._divider(timer.control))
                if timer.application_mode:
                    self._advance_application_timer(board, channel, timer, counts, now)
                else:
                    timer.count = (timer.count + counts) & 0xffffffff
        timer = self.application_timer
        if elapsed and timer.control and timer.reload:
            counts, timer.phase = divmod(timer.phase+elapsed, self._divider(timer.control))
            self._advance_application_timer(board, 3, timer, counts, now)
        if not elapsed or not self.timer_control or not self.timer_reload:
            return
        elapsed, self.timer_phase = divmod(self.timer_phase+elapsed, self._divider(self.timer_control))
        if not elapsed:
            return
        if self.timer_control == 0x63:
            # Boot starts at zero and logs raw count; kernel seeds elapsed
            # common ticks from that count before selecting periodic 0x23.
            # Up-counting and absence of IRQ in this mode are inferred.
            self.timer_count = (self.timer_count + elapsed) % (self.timer_reload + 1)
            return
        remaining = self.timer_count or self.timer_reload
        if elapsed < remaining:
            self.timer_count = remaining - elapsed
            return
        expirations = 1 + (elapsed - remaining) // self.timer_reload
        self.timer_count = self.timer_reload - ((elapsed - remaining) % self.timer_reload)
        self.timer_expirations += expirations
        for cpu_id in self.timer_targets:
            board.queue_irq(cpu_id, 0x20)
        self._record('Modeled timer expiry', expirations=expirations, virtual_ticks=now,
                     targets=list(self.timer_targets), assumption='0x23 periodic mode; shared IRQ0x20 fanout inferred')

    def _advance_application_timer(self, board, channel, timer, counts, now):
        if not timer.reload:
            raise UnsupportedDevice('Enabled application timer has zero reload')
        expirations, timer.count = divmod(timer.count + counts, timer.reload)
        if not expirations:
            return
        timer.expirations += expirations
        line = 0x20 + channel
        targets = [cpu_id for cpu_id, bank in enumerate(self.banks)
                   if bank.enabled & (1 << line)]
        for cpu_id in targets:
            board.queue_irq(cpu_id, line)
        self._record('Modeled application timer expiry', channel=channel, line=line,
                     expirations=expirations, targets=targets, virtual_ticks=now,
                     assumption='Up-count/reload inferred; IRQ routed to firmware-enabled banks; prescaler units unknown')

    def next_event_tick(self, now):
        """Next modeled IRQ expiry; free-running/polled timers do not wake WFI."""
        if now < self.last_tick:
            raise UnsupportedDevice('Virtual time moved backwards')
        events = []
        if self.timer_control == 0x23 and self.timer_reload:
            remaining = self.timer_count or self.timer_reload
            events.append(max(now, self.last_tick + remaining*self._divider(self.timer_control)-self.timer_phase))
        for timer in (*[timer for timer in self.polled_timers if timer.application_mode],
                      self.application_timer):
            if timer.control and timer.reload:
                remaining = (timer.reload-timer.count)*self._divider(timer.control)-timer.phase
                events.append(max(now, self.last_tick + remaining))
        return min(events) if events else None

    def snapshot(self):
        return {
            'tie_arbitration': {
                'profile': self.tie_profile, 'decisions': self.tie_decisions,
                'last_decision': self.last_tie_decision,
                'strict_default': 'Equal-priority pending sources stop when no profile is selected',
                'provenance': '0x43058/0x43188 scan active groups0..3 and the highest set bit within the first nonzero group',
                'limit': 'Applying this active-source scan order to pending ties is a simulation fixture, not measured physical arbitration',
                'in_service_gating': 'With this profile, only strict-higher priorities can nest after the outer source is acknowledged. Equal/lower requests remain pending. Pre-ACK replacement fails; physical threshold semantics are inferred.',
            },
            'banks': [{'base': hex(base), 'enabled': hex(bank.enabled),
                       'pending': hex(bank.pending), 'active': bank.active,
                       'injected': bank.injected, 'source_config': hex(bank.source_config),
                       'active_priority':bank.active_priority,'injected_priority':bank.injected_priority,
                       'active_stack':bank.active_stack,
                       'configuration': {hex(offset): hex(value) for offset, value in sorted(bank.config.items())}}
                      for base, bank in zip(IRQ_BASES, self.banks)],
            'timer': {'reload': self.timer_reload, 'control': hex(self.timer_control),
                      'count': self.timer_count, 'expirations': self.timer_expirations,
                      'targets': list(self.timer_targets), 'last_virtual_tick': self.last_tick,
                      'source_tick_phase': self.timer_phase, 'divider': self._divider(self.timer_control)},
            'polled_timers': [{'channel': channel, 'reload': timer.reload,
                               'control': hex(timer.control), 'count': timer.count,
                               'acknowledgements': timer.acknowledgements,
                               'application_mode': timer.application_mode,
                               'expirations': timer.expirations,
                               'physical_irq': 0x20+channel if timer.application_mode else None,
                               'source_tick_phase': timer.phase, 'divider': self._divider(timer.control)}
                              for channel, timer in enumerate(self.polled_timers, 1)],
            'application_timer': {'channel': 3, 'reload': self.application_timer.reload,
                                  'control': hex(self.application_timer.control),
                                  'count': self.application_timer.count,
                                  'expirations': self.application_timer.expirations,
                                  'source_tick_phase': self.application_timer.phase,
                                  'divider': self._divider(self.application_timer.control),
                                  'physical_irq': 0x23,
                                  'routing': 'Firmware-enabled controller banks; inferred'},
            'limitations': ['Controller reset configuration is an explicit zero fixture.',
                'Priority order and current-source register interpretation are inferred.',
                'Timer 0x63 up-count/0x23 countdown, reload timing, frequency and broadcast are software-derived models, not hardware validated.',
                '12MHz timer source and powers-of-two prescalers are inferred from driver conversion constants; virtual source clock is one tick per executed instruction, with unknown real CPU/source ratio.',
                'FIQ fails explicitly; nested IRQs and equal-priority arbitration fail without the explicit unmeasured kernel-active-scan-order profile.',
                'Selected strict-higher nesting retains a typed, unique descending-priority ancestor stack. ACK200 promotes the injected source; only the acknowledged top appears in active bitmap230/current-source1C0. EOI200zero pops/restores the outer threshold, while pending bits change only through explicit guest clears or new requests. In-flight priority changes and unacknowledged-entry replacement/EOI remain unsupported.',
                'Native banked exception registers and the actual stock vector/prologue/return provide CPU nesting; this controller model supplies no handler callback, return or completion.'],
        }
