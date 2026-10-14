"""Bounded stock display timing configuration and virtual phase sources.

The firmware establishes registers, phase status and physical IRQ routing.
Pixel-clock cadence and the position of phase2 are not measured. A caller
must select a virtual frame profile to generate recurring phase events.
This device never invokes a firmware callback or commits image shadows.
"""
from dataclasses import dataclass, field
import weakref

from . import strict_audit
from .hardware import UnsupportedDevice

# Board -> weak reference to its BootControls clock device, kept outside
# device state. The device holds its board, so a strong value would keep the
# board (and its engines and DRAM) alive forever.
_clock_devices = weakref.WeakKeyDictionary()


@dataclass
class TimingBank:
    registers: dict = field(default_factory=dict)
    status: int = 2  # Explicit stopped-at-phase0 reset fixture, not a capture.
    enabled: bool = False
    next_phase_index: int = 0
    frame_start: int = 0
    stop_due: int | None = None
    phases: list = field(default_factory=lambda: [0, 0, 0])
    frames: int = 0
    stop_completions: int = 0
    inferred_running: bool = False


class DisplayTiming:
    EXTERNAL_CLOCK_PROFILE = 'declared-external-phase-clock-unmeasured'
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    BASES = (0xfff6a000, 0xfff6d000)
    PHYSICAL_IRQS = ((0x34, 0x34, 0x35), (0x37, 0x37, 0x38))
    ENABLE = 0x10000
    # Fields written by actual 0x960824/0x960570 and 0x961150.
    MASKS = {
        # Mode2 stores a full cfgdata byte in control bits0..7; those bits
        # are retained opaquely even though only bit16 drives this model.
        0x00: 0x000101ff, 0x04: 0x0fff0fff, 0x08: 0x1fff1fff,
        0x0c: 0x0fff0fff, 0x10: 0x1fff1fff, 0x14: 0x0fff0fff,
        0x18: 0x000000ff, 0x1c: 0x00001fff, 0x20: 0x0fff0fff,
        0x24: 0x00001fff, 0x28: 0x0fff0fff, 0x2c: 0x00001fff,
        0x34: 0x1fff0007, 0x38: 0x07ff0fff,
    }

    def __init__(self, board, *, frame_period_ticks=None, stop_delay_ticks=2,
                 phase_profile='bit16'):
        if phase_profile not in ('bit16', 'configured-clocked', self.EXTERNAL_CLOCK_PROFILE):
            raise ValueError('Unknown display phase profile')
        if phase_profile == self.EXTERNAL_CLOCK_PROFILE and frame_period_ticks is None:
            raise ValueError('External display clock requires an explicit period')
        if frame_period_ticks is not None and (
                not isinstance(frame_period_ticks, int) or frame_period_ticks < 4):
            raise ValueError("Virtual display frame period must be an integer >=4 or None")
        if not isinstance(stop_delay_ticks, int) or stop_delay_ticks < 1:
            raise ValueError("Virtual display stop delay must be positive")
        self.board = board
        self.phase_profile = phase_profile
        self.frame_period_ticks = frame_period_ticks
        self.stop_delay_ticks = stop_delay_ticks
        self.banks = [TimingBank(), TimingBank()]
        self.last_tick = board.virtual_ticks
        # Explicit inferred sequence: phase1 half-way, phase2 three-quarters,
        # phase0 at the next frame boundary. No register is called a clock.
        self.phase_order = (1, 2, 0)
        self.offsets = None if frame_period_ticks is None else (
            frame_period_ticks // 2, frame_period_ticks * 3 // 4, frame_period_ticks)
        self.trace = []

    def contains(self, address, size):
        return size > 0 and any(base <= address and address + size <= base + 0x1000
                               for base in self.BASES)

    def _address(self, address, size):
        for instance, base in enumerate(self.BASES):
            offset = address - base
            if 0 <= offset < 0x1000:
                allowed = set(self.MASKS) | {0x30}
                if instance == 0:
                    allowed -= {0x24, 0x28}
                if size != 4 or offset & 3 or offset not in allowed:
                    raise UnsupportedDevice(f"Unknown display timing register/access {address:#x}/{size}")
                return instance, offset
        raise UnsupportedDevice(f"Unknown display timing bank {address:#x}")

    def _record(self, name, **data):
        self.trace.append({"tick": self.board.virtual_ticks, "name": name, **data})
        if len(self.trace) > 2000:
            del self.trace[:1000]
        self.board.record(name, **data)

    def _targets(self, instance, phase):
        line = self.PHYSICAL_IRQS[instance][phase]
        targets = [cpu_id for cpu_id, bank in enumerate(self.board.interrupts.banks)
                   if bank.enabled & (1 << line)]
        if len(targets) > 1:
            raise UnsupportedDevice(
                f"Ambiguous display phase{phase} physical IRQ{line:#x}: enabled in both CPU banks")
        return line, targets

    def _validate_routes(self, instance):
        for phase in range(3):
            self._targets(instance, phase)

    def _clock_ready(self):
        # Read the modeled shared clock handshake, never callback/task RAM.
        # Both requested and acknowledged bits are required. A missing clock
        # device cannot make this optional profile run.
        devices = getattr(self.board, 'devices', ())
        clock = _clock_devices.get(self.board)
        clock = clock() if clock is not None else None
        if clock is None or clock not in devices:
            clock = next((device for device in devices
                          if device.__class__.__name__ == 'BootControls'), None)
            if clock is None:
                return False
            _clock_devices[self.board] = weakref.ref(clock)
        return clock.read(0, 0xfff88048, 4) & 0x11000000 == 0x11000000

    def _configured(self, instance):
        required = set(self.MASKS)
        if instance == 0:
            required -= {0x24, 0x28}
        registers = self.banks[instance].registers
        # This optional inference is limited to a fully programmed nonzero
        # timing shape like the stock startup capture; an all-zero bank is
        # not evidence of a running generator.
        dimensions = registers.get(4, 0)
        return (required <= registers.keys() and bool(dimensions & 0xfff) and
                bool(dimensions & 0x0fff0000) and bool(registers.get(0x2c, 0)))

    def _running(self, bank):
        if getattr(self, 'phase_profile', 'bit16') in ('configured-clocked', self.EXTERNAL_CLOCK_PROFILE):
            return getattr(bank, 'inferred_running', False)
        return bank.enabled

    def _refresh_inferred(self, instance):
        if getattr(self, 'phase_profile', 'bit16') not in ('configured-clocked', self.EXTERNAL_CLOCK_PROFILE):
            return
        bank = self.banks[instance]
        if self.phase_profile == self.EXTERNAL_CLOCK_PROFILE:
            # Separately declared external phase waveform, not an inference
            # from zero/unknown calibrated geometry. The guest must request
            # the shared clock and program the controller's bit8 clock input.
            running = bool(bank.registers.get(0, 0) & 0x100) and self._clock_ready()
        else:
            running = self._configured(instance) and self._clock_ready()
        if running and (strict_audit.ENABLED or not getattr(bank, 'inferred_running', False)):
            self._validate_routes(instance)
        if running != getattr(bank, 'inferred_running', False):
            bank.inferred_running = running
            if running:
                bank.next_phase_index = 0
                bank.frame_start = self.board.virtual_ticks
            self._record('Display external clock input changed' if self.phase_profile == self.EXTERNAL_CLOCK_PROFILE
                         else 'Display configured-clocked cadence changed', instance=instance,
                         running=running, phase_profile=self.phase_profile,
                         evidence=('declared external waveform; calibrated timing/dimensions remain unresolved'
                                   if self.phase_profile == self.EXTERNAL_CLOCK_PROFILE else
                                   'opt-in inference from complete timing programming and requested/ready clock48'),
                         virtual_frame_period_ticks=self.frame_period_ticks)

    def read(self, cpu_id, address, size):
        instance, offset = self._address(address, size)
        self.tick(self.board)
        bank = self.banks[instance]
        return bank.status if offset == 0x30 else bank.registers.get(offset, 0)

    def write(self, cpu_id, address, size, value):
        instance, offset = self._address(address, size)
        if offset == 0x30:
            raise UnsupportedDevice("Display timing phase status is read-only in the recovered contract")
        if not 0 <= value <= 0xffffffff or value & ~self.MASKS[offset]:
            raise UnsupportedDevice(f"Unrecovered display timing bits {address:#x}={value:#x}")
        self.tick(self.board)
        bank = self.banks[instance]
        if offset == 0 and value & self.ENABLE:
            self._validate_routes(instance)
        bank.registers[offset] = value
        self._record("Display timing register write", instance=instance, cpu=cpu_id,
                     address=hex(address), value=hex(value))
        if offset != 0:
            self._refresh_inferred(instance)
            return
        enabled = bool(value & self.ENABLE)
        if getattr(self, 'phase_profile', 'bit16') == 'configured-clocked':
            if not enabled and getattr(bank, 'inferred_running', False):
                bank.stop_due = self.board.virtual_ticks + self.stop_delay_ticks
                self._record('Display configured-clocked stop requested', instance=instance,
                             due_virtual_tick=bank.stop_due,
                             cadence_continues='explicit profile inference, bit16 meaning unresolved')
        if enabled and not bank.enabled:
            bank.enabled = True
            bank.stop_due = None
            bank.next_phase_index = 0
            bank.frame_start = self.board.virtual_ticks
            self._record("Display timing enabled", instance=instance,
                         virtual_frame_period_ticks=self.frame_period_ticks)
        elif not enabled and bank.enabled:
            bank.enabled = False
            bank.stop_due = self.board.virtual_ticks + self.stop_delay_ticks
            self._record("Display timing stop requested", instance=instance,
                         due_virtual_tick=bank.stop_due)
        elif not enabled and not bank.status & 2 and bank.stop_due is None:
            bank.stop_due = self.board.virtual_ticks + self.stop_delay_ticks
        self._refresh_inferred(instance)
        # A same-value control write is configuration; it does not restart an
        # active frame or repeatedly manufacture a stopped-ready event.

    def _phase_due(self, bank):
        if not self._running(bank) or self.offsets is None:
            return None
        return bank.frame_start + self.offsets[bank.next_phase_index]

    def _emit_phase(self, instance, phase, due_tick):
        bank = self.banks[instance]
        if phase == 0:
            bank.status |= 2
            bank.frames += 1
        elif phase == 1:
            bank.status &= ~2
        bank.phases[phase] += 1
        line, targets = self._targets(instance, phase)
        self._record("Display virtual phase", instance=instance, phase=phase,
                     source_tick=due_tick, physical_irq=hex(line),
                     target_cpus=targets, timing_evidence=("declared external waveform, no calibrated timing inference"
                        if self.phase_profile == self.EXTERNAL_CLOCK_PROFILE else "explicit inferred virtual cadence"))
        for cpu_id in targets:
            self.board.queue_irq(cpu_id, line)

    def tick(self, board):
        now = board.virtual_ticks
        if now < self.last_tick:
            raise UnsupportedDevice("Display virtual time moved backwards")
        for instance, bank in enumerate(self.banks):
            if bank.stop_due is not None and bank.stop_due <= now:
                due = bank.stop_due
                bank.stop_due = None
                bank.status |= 2
                bank.stop_completions += 1
                self._record("Display timing stopped at phase0", instance=instance,
                             source_tick=due)
            self._refresh_inferred(instance)
            # Each emitted phase resolves its route; idle re-checks are opt-in.
            if strict_audit.ENABLED and self._running(bank):
                self._validate_routes(instance)
            events = 0
            while (due := self._phase_due(bank)) is not None and due <= now:
                self._emit_phase(instance, self.phase_order[bank.next_phase_index], due)
                bank.next_phase_index += 1
                if bank.next_phase_index == 3:
                    bank.next_phase_index = 0
                    bank.frame_start += self.frame_period_ticks
                events += 1
                if events > 10000:
                    raise UnsupportedDevice("Display phase catch-up exceeds explicit event bound")
        self.last_tick = now

    def next_event_tick(self, now):
        due = [value for bank in self.banks
               for value in (bank.stop_due, self._phase_due(bank)) if value is not None]
        return min(due) if due else None

    def snapshot(self):
        state = {
            "kind": "stock display timing registers with explicit virtual phase profile",
            "phase_profile": getattr(self, 'phase_profile', 'bit16'),
            "frame_period_ticks": self.frame_period_ticks, "stop_delay_ticks": self.stop_delay_ticks,
            "phase_order": list(self.phase_order), "phase_offsets": self.offsets,
            "instances": [{"instance": instance, "base": hex(self.BASES[instance]),
                           "enabled": bank.enabled, "phase_status": hex(bank.status),
                           "inferred_cadence_active": getattr(bank, 'inferred_running', False),
                           "configuration_complete": self._configured(instance),
                           "registers": {hex(k): hex(v) for k, v in sorted(bank.registers.items())},
                           "next_phase_tick": self._phase_due(bank), "stop_due": bank.stop_due,
                           "phase_counts": bank.phases[:], "virtual_frames": bank.frames,
                           "stop_completions": bank.stop_completions}
                          for instance, bank in enumerate(self.banks)],
            "limitations": [
                "Reset registers zero and stopped phase0 status2 are explicit fixtures",
                "Frame period and phase2 placement are unmeasured virtual model parameters; timing counters do not determine cadence",
                "Without a virtual frame profile, only configuration and delayed stop completion are modeled",
                "Default bit16 profile conservatively gates cadence; bit16's full hardware meaning is unresolved",
                "Opt-in configured-clocked profile infers continued phase IRQ cadence from all recovered bank offsets and FFF88048 requested/ready even with bit16 clear; delayed stop/status completion remains, but bit16 is not a full cadence gate in this profile",
                "Masked physical sources do not generate a latched IRQ; enabled routing must identify one CPU bank",
                "Phase0/1 share physical0x34/0x37; phase2 uses0x35/0x38; logical IRQ types are not relabeled as edges",
                "The device raises physical sources only; actual firmware ISR/list dispatch must invoke callbacks and image commits",
                "No panel pins, HDMI signal, pixel clock, frame rendering or GPU operation is simulated by this device",
            ],
        }
        if self.phase_profile == self.EXTERNAL_CLOCK_PROFILE:
            state['limitations'].append('Selected external waveform needs requested/ready clock and controller bit8; calibrated geometry remains unresolved and does not determine this declared waveform')
        return state
