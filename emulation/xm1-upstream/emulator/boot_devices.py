"""Explicit ideal clock/DRAM and pin fixtures for captured X70 ROM execution.

These devices execute no firmware routines and invent no RTOS results.  Register
sets and poll bits come from the captured FF4227 ROM and the frozen X70 loader
corpus; reset values and successful settling are simulation fixtures, not
measurements of the powered camera. FFF88048 uses an explicit virtual delay;
other supported startup clocks settle instantly. Unknown registers reject.
"""
from dataclasses import dataclass
from enum import Enum
from typing import NewType

from .hardware import UnsupportedDevice

SHUTTER_IRQ_PROFILE = 'declared-shutter-gpio-falling-irq-unmeasured'

VirtualTick = NewType('VirtualTick', int)
TickDuration = NewType('TickDuration', int)


class AcquisitionActuatorProfile(Enum):
    XM1 = 'xm1-150-declared-acquisition-gpio-motion-unmeasured'


class AcquisitionDrive(Enum):
    IDLE = 0
    DRIVE = 1
    COAST = 2


@dataclass
class AcquisitionActuator:
    profile: AcquisitionActuatorProfile
    phase: AcquisitionDrive = AcquisitionDrive.IDLE
    due_tick: VirtualTick | None = None
    settle_ticks: TickDuration = TickDuration(3000)
    transitions: int = 0


def idle_gpio_profile():
    """Released buttons from the loader and stock application key table.

    Port0 bits2/3 release physical S1/S2 according to the actual 0x93f82c
    type4 predicate and 0x93f064 polarity table. Retain ROM source straps,
    release CMD_PUSH, FUNCTION3, DRIVE, PLAY, CMD_LEFT, CMD_RIGHT, DELETE
    and CARD; FLASH_EXT remains absent. The zero fixture for port2 bit11
    selects RECL_MANUAL and port1 bit1 selects LCD_TILT. These mechanical
    positions and other pins are unmeasured, rather than universal idle.
    """
    return {'gpio_inputs': {0: 0xc0c, 2: 0x1200, 3: 0x308, 12: 0x80},
            'profile_name': 'released known buttons including S1/S2 / absent card and external flash / manual lever and tilted LCD / unmeasured other pins'}


def normal_power_on_gpio_profile():
    """The selected idle-control fixture with GPIO0 bit5 held on.

    0x94774c permits selected boot modes2/3 only while saved port0 bit5 is
    high (or MCU boot status bit2 is set). The pin's electrical/reset state
    was not captured; this supplies the software-visible virtual switch-on.
    """
    profile = idle_gpio_profile()
    profile['gpio_inputs'][0] |= 0x20
    profile['profile_name'] = 'virtual power switch on / '+profile['profile_name']
    return profile


class BootControls:
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    CLOCK = 0xfff88000
    GPIO = 0xfff89000
    PHY = 0xfff01000
    DRAM = 0xfff99000
    SYSTEM = 0xfff98000
    RESET = 0xfffb2000
    CLOCK_OFFSETS = set(range(0, 0x3c, 4)) | {0x40, 0x48, 0x50, 0x54, 0x58, 0x5c, 0x80, 0x84}
    DRAM_OFFSETS = {0, 4, 8, 0xc, 0x10, 0x14, 0x18, 0x1c, 0x40, 0x44,
                    0xc0, 0xc8, 0xd0, 0xd8, 0xe0, 0xe4, 0xe8, 0xec, 0xf4, 0xf8}
    SYSTEM_OFFSETS = {0, 8, 0x100, 0x104, 0x208}
    RESET_OFFSETS = {4, 8, 0xc, 0x10, 0x14}
    # Native GPIO reads use the same retained status words as Python reads.
    native_registers = {0xfff89000 + port*0x40 + 0x14: ((0, 0, None), None) for port in range(16)}
    # Board wiring (port, pin, source) and firmware registration expectations
    # (source, vector, priority) are independent immutable profile inputs.
    _shutter_wiring = ((0, 2, 10), (0, 3, 11))
    _shutter_bindings = ((10, 0x9408e8, 7), (11, 0x9408d4, 7))

    def __init__(self, board, *, boot_source=1, boot_gpio=0xc00, gpio_inputs=None,
                 profile_name='inferred boot straps / CARD and K_PLAY released',
                 clock48_settle_ticks=2, shutter_irq_profile=None,
                 shutter_wiring=None, shutter_bindings=None, acquisition_actuator_profile=None):
        if isinstance(clock48_settle_ticks, bool) or not isinstance(clock48_settle_ticks, int) or clock48_settle_ticks < 1:
            raise ValueError('Clock48 settle delay must be a positive virtual-tick count')
        self.board = board
        self.profile_name = str(profile_name)
        self.registers = {}
        self.clock48_settle_ticks = clock48_settle_ticks
        if shutter_irq_profile not in (None, SHUTTER_IRQ_PROFILE):
            raise ValueError('Unknown shutter GPIO IRQ profile')
        if shutter_wiring is not None or shutter_bindings is not None:
            if (type(shutter_wiring) is not tuple or not shutter_wiring
                    or any(type(row) is not tuple or len(row) != 3
                           or any(type(v) is not int for v in row)
                           or not (0 <= row[0] < 16 and 0 <= row[1] < 16 and 0 <= row[2] < 256)
                           for row in shutter_wiring)
                    or len({row[:2] for row in shutter_wiring}) != len(shutter_wiring)
                    or len({row[2] for row in shutter_wiring}) != len(shutter_wiring)):
                raise ValueError('Explicit shutter wiring requires unique GPIO/source triples')
            if (type(shutter_bindings) is not tuple
                    or any(type(row) is not tuple or len(row) != 3
                           or any(type(v) is not int for v in row)
                           or not (0 <= row[0] < 256 and 0 <= row[1] <= 0xfffffffc
                                   and row[1] % 4 == 0 and 0 <= row[2] <= 255)
                           for row in shutter_bindings)
                    or len({row[0] for row in shutter_bindings}) != len(shutter_bindings)
                    or {row[0] for row in shutter_bindings} != {row[2] for row in shutter_wiring}):
                raise ValueError('Explicit shutter bindings must cover exactly the wired sources')
            self._shutter_wiring, self._shutter_bindings = shutter_wiring, shutter_bindings
        self.shutter_irq_profile = shutter_irq_profile
        self.acquisition_actuator = (AcquisitionActuator(AcquisitionActuatorProfile(acquisition_actuator_profile))
                                     if acquisition_actuator_profile is not None else None)
        self.clock48_ready_tick = None
        self.clock48_changes = 0
        # GPIO data writes use high-halfword write masks and low-halfword
        # levels, e.g. 0x91838c asserts CS with 1<<(pin+16)|level<<pin.
        # Keep known bits separate from unresolved reset output levels.
        self.gpio_output_levels, self.gpio_output_known = {}, {}
        self.boot_source, self.boot_gpio = boot_source, boot_gpio
        # The source is an inferred NAND type-1 normal-boot profile.  Loader
        # table14abc identifies CARD and K_PLAY as active-low inputs. Release
        # those two explicitly; other input bits remain unmeasured fixtures.
        # This does not establish the application key driver's full pin map.
        self.registers[self.SYSTEM] = boot_source
        inputs = {0: boot_gpio, 2: 0x200, 12: 0x80}
        if gpio_inputs is not None:
            if any(not 0 <= int(port) < 16 or not 0 <= int(word) <= 0xffffffff
                   for port, word in gpio_inputs.items()):
                raise ValueError('GPIO input fixtures require ports0..15 and 32-bit words')
            inputs.update(gpio_inputs)
        self.gpio_inputs = inputs
        for port in range(16):
            self.registers[self.GPIO+port*0x40+0x14] = inputs.get(port, 0)
        if self.acquisition_actuator is not None:
            if self.gpio_input_pin(0, 9) != 1 or self.gpio_input_pin(0, 6) != 1 or self.gpio_input_pin(2, 8) != 0:
                raise ValueError('X-M1 actuator fixture requires its declared initial sensor levels')
            self.board.record('Declared X-M1 acquisition actuator', profile=self.acquisition_actuator.profile.value,
                              drive_gpio=((1, 1), (1, 2)), sense_gpio=((0, 9), (0, 6), (2, 8)),
                              settle_ticks=self.acquisition_actuator.settle_ticks,
                              limits='Target GPIO/IRQ mapping; actuator identity, motion law and latency are unmeasured')
        self.registers[self.CLOCK+0x80] = 1 << 16
        self.phy_started = False
        self.rom_handoff_mode = None
        self.board.record('Boot profile fixture', boot_source=boot_source,
                          profile_name=self.profile_name,
                          boot_gpio=hex(boot_gpio), clock_settle='other clocks instantaneous; FFF88048 delayed',
                          clock48_settle_ticks=clock48_settle_ticks,
                          gpio_inputs={port: hex(word) for port, word in inputs.items()},
                          input_limits='Explicit input fixtures; named button levels follow loader/application predicates; mechanical positions and other pins unmeasured')

    def _kind(self, address, size):
        if size == 4 and address % 4 == 0:
            if address-self.CLOCK in self.CLOCK_OFFSETS:
                return 'clock'
            offset = address-self.GPIO
            if (0 <= offset < 18*0x40 and offset % 0x40 in range(0, 0x20, 4)
                    and (offset // 0x40 < 16 or offset % 0x40 != 0x14)):
                return 'gpio'
            if address-self.DRAM in self.DRAM_OFFSETS:
                return 'dram'
            if 0 <= address-self.PHY < 0x248:
                return 'phy'
            if address-self.SYSTEM in self.SYSTEM_OFFSETS:
                return 'system'
            if address-self.RESET in self.RESET_OFFSETS:
                return 'reset'
        if size == 2 and address in (0xfff5002c, 0xfff5102c):
            return 'sd_clock'
        return None

    def contains(self, address, size):
        return self._kind(address, size) is not None

    def mmio_ranges(self):
        """Superset of the start addresses _kind() accepts (RustBoard._mmio_device index)."""
        return ((self.CLOCK, self.CLOCK+max(self.CLOCK_OFFSETS)+1), (self.GPIO, self.GPIO+18*0x40),
                (self.DRAM, self.DRAM+max(self.DRAM_OFFSETS)+1), (self.PHY, self.PHY+0x248),
                (self.SYSTEM, self.SYSTEM+max(self.SYSTEM_OFFSETS)+1),
                (self.RESET, self.RESET+max(self.RESET_OFFSETS)+1),
                (0xfff5002c, 0xfff5002d), (0xfff5102c, 0xfff5102d))

    def _settle_clock48(self):
        # 0x917574 sets bit28 and then waits for bit24. Latency and the
        # zero-reset fixture are unmeasured. Keep this separately from all
        # other clock words: no unrelated status bits are fabricated.
        due = getattr(self, 'clock48_ready_tick', None)
        if due is not None and self.board.virtual_ticks >= due:
            self.clock48_ready_tick = None
            if self.registers.get(self.CLOCK+0x48, 0) & (1 << 28):
                self.registers[self.CLOCK+0x48] |= 1 << 24
                self.board.record('Output clock enable settled (modeled)',
                                  address=hex(self.CLOCK+0x48), ready_bit=24,
                                  due_tick=due, latency='unmeasured virtual delay')

    def tick(self, board):
        self._settle_clock48()
        actuator = self.acquisition_actuator
        if actuator is None or actuator.due_tick is None or board.virtual_ticks < actuator.due_tick:
            return
        actuator.due_tick = None
        if self.gpio_output_known.get(1, 0) & 6 != 6:
            return
        levels = self.gpio_output_levels[1] & 6
        phase = {0: AcquisitionDrive.IDLE, 2: AcquisitionDrive.DRIVE, 6: AcquisitionDrive.COAST}.get(levels)
        if phase is None:
            raise UnsupportedDevice('X-M1 actuator fixture supports only its declared active-high drive states')
        if phase is actuator.phase:
            return
        actuator.phase = phase
        actuator.transitions += 1
        level = 0 if phase is AcquisitionDrive.DRIVE else 1
        for pin in (9, 6):
            previous = self.gpio_input_pin(0, pin)
            self.set_gpio_input_pin(0, pin, level, source=actuator.profile.value)
            if previous == level or (pin == 9 and level != 1) or (pin == 6 and level != 0):
                continue
            routes = [cpu for cpu, bank in enumerate(board.interrupts.banks) if bank.enabled & (1 << 0x11)]
            if not routes:
                continue
            if len(routes) != 1:
                raise UnsupportedDevice('X-M1 actuator GPIO IRQ has competing enabled banks')
            cpu = routes[0]
            bank = board.interrupts.banks[cpu]
            mode = bank.config.get(0x184, 0) >> 2 & 3
            if mode != (1 if pin == 9 else 2):
                continue
            if bank.config.get(0x844) != 12 or bank.config.get(0x6c, 0) & 0x20:
                raise UnsupportedDevice('X-M1 actuator GPIO IRQ lacks its target priority or selects FIQ')
            board.queue_irq(cpu, 0x11)
            board.record('X-M1 actuator sensor edge', gpio=(0, pin), previous=previous, level=level,
                         cpu=cpu, irq=0x11, profile=actuator.profile.value)

    def next_event_tick(self, now):
        deadlines = (self.clock48_ready_tick, self.acquisition_actuator.due_tick if self.acquisition_actuator is not None else None)
        return min((tick for tick in deadlines if tick is not None), default=None)

    def read(self, core, address, size):
        kind = self._kind(address, size)
        if kind is None:
            raise UnsupportedDevice(f'Unsupported boot control read {address:#x}/{size}')
        if address == self.CLOCK+0x48:
            self._settle_clock48()
        value = self.registers.get(address, 0)
        # Stock 0x983008 programs +0x40 with bit28 clear, then enables
        # bit28 and waits for bit24 at 0x9831e0. The same ready contract
        # exists at +0x38 in the loader. Settling remains instantaneous.
        if address in (self.CLOCK+0x38, self.CLOCK+0x40) and value & (1 << 28):
            value |= 1 << 24
        if address == self.PHY+0x90 and self.phy_started:
            value |= 1 << 28
        if kind == 'sd_clock' and value & 1:
            value |= 2
        return value & ((1 << (8*size))-1)

    def write(self, core, address, size, value):
        kind = self._kind(address, size)
        if kind is None:
            raise UnsupportedDevice(f'Unsupported boot control write {address:#x}/{size}')
        if address == self.CLOCK+0x80:
            raise UnsupportedDevice('Clock-ready status is read-only in boot model')
        value &= (1 << (8*size))-1
        if address == self.CLOCK+0x48:
            self._settle_clock48()
            old = self.registers.get(address, 0)
            if value & ~0x11000000:
                raise UnsupportedDevice('Unsupported FFF88048 field; only bit28 request and bit24 status are evidenced')
            # Read-modify-write may echo status already read, but cannot
            # invent an acknowledgement or independently clear ready.
            if value & (1 << 24) and not old & (1 << 24):
                raise UnsupportedDevice('FFF88048 ready status cannot be set by firmware')
            requested, previous = bool(value & (1 << 28)), bool(old & (1 << 28))
            if requested != previous:
                self.registers[address] = (1 << 28) if requested else 0
                delay = getattr(self, 'clock48_settle_ticks', 2)
                self.clock48_ready_tick = self.board.virtual_ticks+delay if requested else None
                self.clock48_changes = getattr(self, 'clock48_changes', 0)+1
                self.board.record('Output clock request (modeled)', cpu=core,
                                  address=hex(address), requested=requested,
                                  ready_tick=self.clock48_ready_tick,
                                  evidence='0x917574 ORs C64AB8/C64ABC requester flags',
                                  limit='Reset, stop readiness and virtual latency are unmeasured')
            return
        if kind == 'gpio' and (address-self.GPIO) % 0x40 == 0x14:
            raise UnsupportedDevice('GPIO input status is read-only in boot model')
        # Divider changes settle immediately; firmware's busy polls still run.
        if kind == 'clock' and address-self.CLOCK <= 0x20:
            value &= ~(1 << 28)
        self.registers[address] = value
        if kind == 'gpio' and (address-self.GPIO) % 0x40 == 0:
            port = (address-self.GPIO)//0x40
            mask = value >> 16
            self.gpio_output_levels[port] = ((self.gpio_output_levels.get(port, 0) & ~mask)
                                             | (value & mask)) & 0xffff
            self.gpio_output_known[port] = self.gpio_output_known.get(port, 0) | mask
            if port == 1 and mask & 6 and self.acquisition_actuator is not None:
                self.acquisition_actuator.due_tick = VirtualTick(self.board.virtual_ticks + self.acquisition_actuator.settle_ticks)
        if address == self.PHY and value & 1:
            self.phy_started = True
            self.board.record('DRAM PHY training modeled complete', cpu=core,
                              status_address=hex(self.PHY+0x90), status_bit=28)
        if address == self.SYSTEM+8:
            self.rom_handoff_mode = value
            self.board.record('ROM memory mapping request', cpu=core, value=value,
                              interpretation='1 RAM / 2 storage; remap semantics unverified')
            if value in (1, 2) and hasattr(self.board, 'remap_low_ram'):
                self.board.remap_low_ram()
        if kind == 'sd_clock' and value & 1:
            self.registers[address] |= 2

    def gpio_input_pin(self, port, pin):
        if (type(port) is not int or not 0<=port<16
                or type(pin) is not int or not 0<=pin<16):
            raise ValueError('GPIO input selector out of range')
        return (self.gpio_inputs.get(port,0)>>pin)&1

    def _shutter_irq_route(self, port, pin, previous, level):
        """Preflight a wired S1/S2 falling edge against actual guest routing.

        Source numbers come from the unchanged type4 input records and
        AEAFE0 polarity/IRQ table. Mode2 in the controller's 180 family is
        the declared falling-edge interpretation of kernel attribute 87.
        This is an opt-in peripheral law, not a measured electrical model.
        No key/event, RAM flag, guest ISR or callback is supplied here.
        """
        if (getattr(self, 'shutter_irq_profile', None) is None
                or previous != 1 or level != 0):
            return None
        line = next((source for p, bit, source in self._shutter_wiring
                     if (p, bit) == (port, pin)), None)
        if line is None:
            return None
        interrupts = getattr(self.board, 'interrupts', None)
        if interrupts is None:
            raise UnsupportedDevice('Shutter GPIO edge requires the modeled interrupt controller')
        routes = [cpu for cpu, bank in enumerate(interrupts.banks) if bank.enabled >> line & 1]
        if not routes:
            # Type4 driver intentionally unregisters while held. The GPIO
            # input still changes; a disabled source does not receive an IRQ.
            return None
        if len(routes) != 1:
            raise UnsupportedDevice('Shutter GPIO source has competing enabled CPU routes')
        cpu = routes[0]
        bank = interrupts.banks[cpu]
        mode = bank.config.get(0x180 + (line // 16) * 4, 0) >> ((line % 16) * 2) & 3
        alternate = bank.config.get(0x140 + (line // 16) * 4, 0) >> ((line % 16) * 2) & 3
        _, vector, priority = next(row for row in self._shutter_bindings if row[0] == line)
        if (bank.source_config >> line & 1 or mode != 2 or alternate
                or bank.config.get(0x400 + line * 4) != vector
                or bank.config.get(0x800 + line * 4) != priority
                or bank.config.get(0x6c, 0) & 0x20):
            raise UnsupportedDevice('Shutter GPIO source lacks its stock falling-edge IRQ binding')
        return cpu, line

    def configure_shutter_irq_profile(self, profile):
        """Explicit host selection for a development branch, without guest writes."""
        if profile != SHUTTER_IRQ_PROFILE:
            raise ValueError('Unknown shutter GPIO IRQ profile')
        if getattr(self, 'shutter_irq_profile', None) not in (None, profile):
            raise UnsupportedDevice('Shutter GPIO input law changed outside its declared profile')
        self.shutter_irq_profile = profile
        self.board.record('Declared shutter GPIO IRQ profile', profile=profile,
                          mapping={f'GPIO{port}/{pin}': line for port, pin, line in self._shutter_wiring},
                          limits='Opt-in falling-edge peripheral interpretation; physical timing/electrical mapping unmeasured')

    def set_gpio_input_pin(self, port, pin, level, *, source):
        """External pin change; opt-in S1/S2 law may latch an architectural IRQ."""
        previous=self.gpio_input_pin(port,pin)
        if (type(level) is not int or level not in (0,1)
                or type(source) is not str or not source.strip()):
            raise ValueError('External GPIO input requires a typed level and named source')
        route = self._shutter_irq_route(port, pin, previous, level)
        word=(self.gpio_inputs.get(port,0)&~(1<<pin))|(level<<pin)
        self.gpio_inputs[port]=word
        self.registers[self.GPIO+port*0x40+0x14]=word
        self.board.record('Declared external GPIO input',port=port,pin=pin,
                          previous=previous,level=level,source=source,
                          limits='Virtual external pin level; physical timing unmeasured; only selected S1/S2 falling-edge law delivers IRQ')
        if route is not None:
            cpu, line = route
            self.board.queue_irq(cpu, line)
            self.board.record('Declared shutter GPIO falling edge', port=port, pin=pin,
                              cpu=cpu, irq=line, profile=self.shutter_irq_profile,
                              limits='Actual guest-programmed source/vector; architectural IRQ entry remains owned by the execution engine')

    def gpio_output_pin(self, port, pin):
        """Return a firmware-written output bit, or None for unknown reset state."""
        if not 0 <= port < 18 or not 0 <= pin < 16:
            raise ValueError('GPIO output selector out of range')
        if not self.gpio_output_known.get(port, 0) & (1 << pin):
            return None
        return self.gpio_output_levels.get(port, 0) >> pin & 1

    def snapshot(self):
        state = {
            'name': 'BootControls',
            'profile_name': self.profile_name,
            'registers': {hex(k): hex(v) for k, v in sorted(self.registers.items())},
            'phy_started': self.phy_started,
            'rom_handoff_mode': self.rom_handoff_mode,
            'clock48': {'requested': bool(self.registers.get(self.CLOCK+0x48, 0) & (1 << 28)),
                        'ready': bool(self.registers.get(self.CLOCK+0x48, 0) & (1 << 24)),
                        'ready_tick': getattr(self, 'clock48_ready_tick', None),
                        'settle_ticks': getattr(self, 'clock48_settle_ticks', 2),
                        'changes': getattr(self, 'clock48_changes', 0)},
            'gpio_inputs': {port: hex(word) for port, word in self.gpio_inputs.items()},
            'shutter_irq_profile': getattr(self, 'shutter_irq_profile', None),
            'acquisition_actuator': (None if self.acquisition_actuator is None else {
                'profile': self.acquisition_actuator.profile.value, 'phase': self.acquisition_actuator.phase.name,
                'due_tick': self.acquisition_actuator.due_tick, 'settle_ticks': self.acquisition_actuator.settle_ticks,
                'transitions': self.acquisition_actuator.transitions}),
            'gpio_output_levels': {port: hex(word) for port, word in self.gpio_output_levels.items()},
            'gpio_output_known': {port: hex(word) for port, word in self.gpio_output_known.items()},
            'limits': [
                'Boot straps and initial register values are simulation fixtures.',
                'Other clocks and DRAM training settle instantly; FFF88048 uses an explicit virtual acknowledgement delay.',
                'FFF88048 zero reset, stop-ready clearing and acknowledgement latency are unmeasured fixtures; no clock IRQ is generated.',
                'GPIO output/mux writes retain state; external pin behavior is not modeled.',
                'SD support here covers clock setup only, not a card/controller.',
                'System remapping is recorded; CPU reset/release belongs to the board harness.',
            ],
        }
        if '_shutter_wiring' in self.__dict__:
            state['shutter_configuration'] = {'wiring': list(self._shutter_wiring),
                                               'bindings': list(self._shutter_bindings),
                                               'edge': 'falling; controller mode2 interpretation unmeasured'}
        return state
