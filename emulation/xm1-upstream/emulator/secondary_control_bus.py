"""Recovered bus1 transport with separate explicit usbcharge/PWIC inputs.

The target's identity and physical register defaults are unknown. This opt-in
input fixture supplies zero bytes only at the five recovered read indices,
retains the recovered RMW fields, and routes completion through actual
firmware IRQ0x4F/physical0x66. No firmware callback or RAM flag is synthesized.
"""
from .hardware import UnsupportedDevice
from .control_bus_contracts import ControlBusRegisters
from .secondary_control_bus_contracts import SecondaryBusReplies, DEFAULT_REGISTERS, DEFAULT_REPLIES

PROFILE = 'usbcharge-disconnected-zero-registers'
PWIC_PROFILE = 'usbcharge-disconnected-pwic61-zero'
PWIC_STARTUP_PROFILE = 'usbcharge-disconnected-pwic-startup-tagged'
DECLARED_PROFILE = 'declared-bus1-register-replies-unmeasured'
LEGACY_PROFILES = (PROFILE, PWIC_PROFILE, PWIC_STARTUP_PROFILE)
PROFILES = (*LEGACY_PROFILES, DECLARED_PROFILE)


class SecondaryControlBus:
    BASE = 0xfff85040
    OFFSETS = frozenset((0, 4, 8, 0x10, 0x18))
    READ_REGISTERS = frozenset((2, 5, 0x0c, 0x11, 0x13))
    WRITE_REGISTERS = frozenset((0x0c, 0x11))
    PHYSICAL_IRQ = 0x66
    _register_map = DEFAULT_REGISTERS
    _reply_models = DEFAULT_REPLIES
    _endpoint_table = _polarity_view = None

    def __init__(self, board, power, *, profile=None, transfer_ticks=4,
                 registers=None, endpoints=None, polarity=None, replies=None):
        if profile not in (None, *PROFILES):
            raise ValueError('Unknown secondary control-bus input profile')
        if not isinstance(transfer_ticks, int) or transfer_ticks < 1:
            raise ValueError('Secondary control-bus transfer ticks must be positive')
        if not isinstance(power.usb_connected, bool):
            raise ValueError('Secondary control-bus cable input must be boolean')
        if profile in LEGACY_PROFILES and power.usb_connected:
            raise ValueError('Disconnected usbcharge inputs cannot model a connected USB cable')
        if profile == DECLARED_PROFILE and replies is None:
            raise ValueError('Declared secondary bus inputs require explicit reply contracts')
        if any(value is not None for value in (registers, endpoints, polarity, replies)):
            from .boards.control_bus_parameters import EndpointDescriptor, FirmwareView
            registers = DEFAULT_REGISTERS if registers is None else registers
            if not isinstance(registers, ControlBusRegisters):
                raise TypeError('Secondary bus registers must be an immutable register contract')
            if endpoints is not None:
                endpoints = tuple(endpoints)
                if (not endpoints or any(not isinstance(row, EndpointDescriptor)
                        or type(row.channel) is not int or row.channel != 1
                        or type(row.index) is not int or row.index < 0
                        or type(row.address) is not int or not 0 <= row.address <= 0x7f
                        or type(row.polarity_selector) is not int or row.polarity_selector not in (0, 1)
                        for row in endpoints)
                        or len({row.index for row in endpoints}) != len(endpoints)
                        or len({row.address for row in endpoints}) != len(endpoints)):
                    raise ValueError('Secondary bus endpoints must be unique channel1 descriptors')
                if replies is None and profile is not None:
                    raise ValueError('Explicit endpoint tables require explicit reply contracts')
            if polarity is not None and (endpoints is None or not isinstance(polarity, FirmwareView)
                    or polarity.sampling != 'transaction' or len(polarity.data) != 8):
                raise ValueError('Secondary bus polarity requires a typed transaction view and endpoint table')
            replies = DEFAULT_REPLIES if replies is None else replies
            if not isinstance(replies, SecondaryBusReplies):
                raise TypeError('Secondary bus replies must be an immutable reply contract')
            if profile in LEGACY_PROFILES and replies != DEFAULT_REPLIES:
                raise ValueError('Changed reply maps require the declared secondary bus profile')
            if profile == DECLARED_PROFILE and power.usb_connected != replies.usb_connected:
                raise ValueError('Declared secondary bus reply cable state differs')
            self._register_map, self._reply_models = registers, replies
            self._endpoint_table, self._polarity_view = endpoints, polarity
            self.BASE, self.OFFSETS, self.PHYSICAL_IRQ = registers.base, registers.offsets, registers.irq
            self.READ_REGISTERS = frozenset(register for register, _ in replies.register_values)
            self.WRITE_REGISTERS = frozenset(register for register, _ in replies.register_write_masks)
        self.board, self.power = board, power
        self.profile, self.transfer_ticks = profile, transfer_ticks
        self.regs = dict.fromkeys(self.OFFSETS, 0)
        self.target_registers = dict.fromkeys(self.READ_REGISTERS, 0)
        self.phase, self.busy = 'idle', False
        self.payload, self.transactions = [], []
        self.generation = self.operations_started = self.operations_completed = self.irqs_raised = 0
        self.rx_ready = self.rx_consumed = self.receive_configured = self.last_byte_nack = False
        self.received_bytes = 0
        self.rx_byte = None
        self.target_address, self.pwic_prefix = None, None
        self.pwic_registers = ({0x61: 0, 0x20: 0x80} if profile == PWIC_STARTUP_PROFILE
                              else {0x61: 0} if profile == PWIC_PROFILE else {})
        if profile == DECLARED_PROFILE:
            self.target_registers = dict(self._reply_models.register_values)
            self.pwic_registers = dict(self._reply_models.pwic_values)
        self.rx_bytes, self.rx_index = b'', 0

    def _admitted(self, address):
        return (address is not None and (self._endpoint_table is None
                or any(row.address == address for row in self._endpoint_table)))

    def _modes(self):
        if '_register_map' in self.__dict__:
            return self._register_map.modes
        return (1, 9) if self.pwic_registers else (9,)

    def contains(self, address, size):
        return self.BASE <= address and address+size <= self.BASE+0x40

    def _offset(self, address, size):
        offset = address-self.BASE
        if size != 4 or address & 3 or offset not in self.OFFSETS:
            raise UnsupportedDevice(f'Unknown secondary control-bus access {address:#x}/{size}')
        return offset

    def _input(self):
        if self.profile not in PROFILES:
            raise UnsupportedDevice('Bus1 target response requires explicit disconnected usbcharge input profile')
        if self.profile == DECLARED_PROFILE:
            if self.power.usb_connected != self._reply_models.usb_connected:
                raise UnsupportedDevice('Declared secondary bus cable input changed')
        elif self.power.usb_connected:
            raise UnsupportedDevice('Connected USB charge state is outside the disconnected input profile')

    def _target(self):
        routes = [index for index, bank in enumerate(self.board.interrupts.banks)
                  if bank.enabled & (1 << self.PHYSICAL_IRQ)]
        if len(routes) != 1:
            raise UnsupportedDevice(f'Bus1 physical IRQ{self.PHYSICAL_IRQ:#x} requires one enabled CPU route')
        return routes[0]

    def _start(self, kind, *, status, phase, finish=None):
        self._input()
        if self.busy:
            raise UnsupportedDevice('Bus1 operation while prior shift is pending')
        target = self._target()
        self.generation += 1
        generation = self.generation
        self.busy, self.regs[0x18] = True, 0
        self.operations_started += 1
        self.board.record('Secondary control-bus operation start', operation=kind, profile=self.profile,
            transfer_ticks=self.transfer_ticks, provenance='explicit selected virtual target input and unmeasured controller latency')
        def complete():
            if generation != self.generation:
                return
            self._input()
            if self._target() != target:
                raise UnsupportedDevice('Bus1 IRQ route changed during transfer')
            self.busy, self.regs[0x18], self.phase = False, status, phase
            if finish:
                finish()
            self.operations_completed += 1
            self.board.queue_irq(target, self.PHYSICAL_IRQ)
            self.irqs_raised += 1
            self.board.record('Secondary control-bus operation complete', operation=kind,
                status=hex(status), cpu=target, physical_irq=hex(self.PHYSICAL_IRQ), profile=self.profile)
        self.board.schedule(self.transfer_ticks, complete)

    def _remember(self, item):
        self.transactions.append(item)
        if len(self.transactions) > 512:
            del self.transactions[:-512]

    def _pwic_write(self, register, value):
        mask = dict(self._reply_models.pwic_write_masks).get(register)
        if register not in self.pwic_registers or mask is None or (value ^ self.pwic_registers[register]) & ~mask:
            raise UnsupportedDevice('PWIC write changes unsupported register or bits outside recovered field mask')

    def _receive_input(self):
        if not self._admitted(self.target_address):
            raise UnsupportedDevice('Secondary bus receive has no admitted target')
        if self.target_address == self._reply_models.pwic_address:
            byte = self.pwic_registers[self.pwic_prefix]
            # AEBA68[20]=1, AF4AAC[0]=4: three virtual untagged bytes,
            # followed by the chosen tagged last byte. No power-ready bit.
            return dict(self._reply_models.pwic_read_prefixes).get(self.pwic_prefix, b'') + bytes((byte,))
        return bytes((self.target_registers[self.payload[0]],))

    def _register_write(self, register, value):
        mask = dict(self._reply_models.register_write_masks).get(register)
        if (register not in self.target_registers or mask is None
                or (value ^ self.target_registers[register]) & ~mask):
            raise UnsupportedDevice('Bus1 write changes bits outside the selected register contract')

    def read(self, cpu_id, address, size):
        offset = self._offset(address, size)
        if offset == 4:
            self._input()
            if self.phase != 'reading-data' or self.busy or not self.rx_ready:
                raise UnsupportedDevice('Bus1 target receive byte is not ready')
            self.rx_ready, self.rx_consumed = False, True
            self.received_bytes += 1
            self.rx_index += 1
            return self.rx_byte
        return self.regs[offset]

    def write(self, cpu_id, address, size, value):
        offset = self._offset(address, size)
        if not isinstance(value, int) or not 0 <= value <= 0xffffffff:
            raise UnsupportedDevice('Bus1 value exceeds word access width')
        if offset == 0x18:
            raise UnsupportedDevice('Bus1 status is read-only in the recovered subset')
        if offset == 0x10:
            allowed = self._modes()
            if self.busy or self.phase != 'idle' or value not in allowed:
                raise UnsupportedDevice('Only idle bus1 mode9, or explicit PWIC mode1, configuration is supported')
            self.regs[offset] = value
            return
        if offset == 0:
            if self.busy or value not in (0, 1) or self.phase != 'idle':
                raise UnsupportedDevice('Bus1 enable/disable requires an idle completed transaction')
            allowed = self._modes()
            if value == 1 and self.regs[0x10] not in allowed:
                raise UnsupportedDevice('Bus1 enable precedes supported configuration')
            self.regs[offset] = value
            return
        if not self.regs[0] & 1 or self.busy:
            raise UnsupportedDevice('Bus1 operation while disabled or prior shift pending')
        if offset == 8:
            if self.phase == 'idle' and value in (0x19, 0x1b):
                self._start('start', status=1, phase='awaiting-address')
                self.regs[8], self.payload = 8, []
                self.target_address = None
                self.rx_ready = self.rx_consumed = self.receive_configured = self.last_byte_nack = False
                self.rx_byte = None
                self.rx_bytes, self.rx_index = b'', 0
            elif self.phase == 'awaiting-address' and value == 0xa:
                self.regs[8] = value
            elif self.phase == 'writing-data' and len(self.payload) == 1 and value == 0xa:
                if self.target_address != self._reply_models.register_address:
                    raise UnsupportedDevice('PWIC uses recovered separate prefix and read transactions')
                if self.payload[0] not in self.READ_REGISTERS:
                    raise UnsupportedDevice('Unknown bus1 target read register')
                self.phase, self.regs[8] = 'awaiting-read-address', value
            elif self.phase == 'writing-data' and len(self.payload) == 2 and value == 0xb:
                register, byte = self.payload
                pwic = self.target_address == self._reply_models.pwic_address
                if pwic:
                    self._pwic_write(register, byte)
                if not pwic and register not in self.WRITE_REGISTERS:
                    raise UnsupportedDevice('Bus1 only supports recovered0C/11 RMW writes')
                if not pwic:
                    self._register_write(register, byte)
                def finish_write():
                    registers = self.pwic_registers if pwic else self.target_registers
                    registers[register] = byte
                    self._remember({'address': hex(self.target_address), 'register': hex(register),
                        'write_hex': bytes((register, byte)).hex(), 'data_byte': byte})
                self._start('stop/write', status=2, phase='idle', finish=finish_write)
                self.regs[8] = 0xa
            elif (self.phase == 'writing-data' and self.target_address == self._reply_models.pwic_address
                  and len(self.payload) == 1 and self.payload[0] in self.pwic_registers and value == 0xb):
                def finish_prefix():
                    self.pwic_prefix = self.payload[0]
                    self._remember({'address': hex(self.target_address), 'register': hex(self.pwic_prefix), 'prefix_hex': bytes(self.payload).hex()})
                self._start('stop/prefix', status=2, phase='idle', finish=finish_prefix)
                self.regs[8] = 0xa
            elif self.phase == 'reading-data':
                if self.rx_ready:
                    raise UnsupportedDevice('Bus1 receive byte must be consumed before another command')
                if value == 0xe and not self.receive_configured:
                    self.receive_configured, self.regs[8] = True, value
                elif (value == 0xa and self.receive_configured and self.regs[8] == 0xe
                      and self.rx_index == len(self.rx_bytes)-1 and not self.last_byte_nack):
                    self.last_byte_nack, self.regs[8] = True, value
                elif ((value == 0x2a and self.last_byte_nack and self.rx_index == len(self.rx_bytes)-1)
                      or (value == 0x2e and not self.last_byte_nack and self.receive_configured
                          and self.rx_index < len(self.rx_bytes)-1 and self.regs[8] == 0xe)):
                    byte = self.rx_bytes[self.rx_index]
                    self.rx_consumed = False
                    def receive():
                        self.rx_byte, self.rx_ready, self.regs[4] = byte, True, byte
                    self._start('receive-byte', status=4, phase='reading-data', finish=receive)
                    self.regs[8] = 0xa if value == 0x2a else 0xe
                elif value == 0xb and self.rx_consumed and self.rx_index == len(self.rx_bytes):
                    register = self.pwic_prefix if self.target_address == self._reply_models.pwic_address else self.payload[0]
                    self._start('stop/read', status=2, phase='idle', finish=lambda:
                        self._remember({'address': hex(self.target_address), 'register': hex(register), 'read_hex': self.rx_bytes.hex()}))
                    self.regs[8] = 0xa
                else:
                    raise UnsupportedDevice(f'Unknown bus1 receive command {value:#x}')
            else:
                raise UnsupportedDevice(f'Unknown bus1 command {value:#x} in phase {self.phase}')
            return
        if value > 0xff:
            raise UnsupportedDevice('Bus1 transmit requires one byte')
        if (self.phase == 'awaiting-address' and self.regs[8] == 0xa
                and self._admitted(self._reply_models.register_address)
                and value == self._reply_models.register_address << 1
                and self.regs[0x10] in self._reply_models.register_modes):
            self._start('address/write', status=0xc, phase='writing-data')
            self.target_address = self._reply_models.register_address
        elif (self.phase == 'awaiting-address' and self.regs[8] == 0xa
              and self._admitted(self._reply_models.pwic_address)
              and value in (self._reply_models.pwic_address << 1, self._reply_models.pwic_address << 1 | 1)
              and self.regs[0x10] in self._reply_models.pwic_modes and self.pwic_registers):
            if value & 1 and self.pwic_prefix not in self.pwic_registers:
                raise UnsupportedDevice('PWIC direct read requires an actual completed supported prefix transaction')
            self._start('address/read' if value & 1 else 'address/write',
                status=4 if value & 1 else 0xc,
                phase='reading-data' if value & 1 else 'writing-data')
            self.target_address = self._reply_models.pwic_address
            if value & 1:
                self.rx_bytes = self._receive_input()
        elif (self.phase == 'awaiting-read-address'
              and self._admitted(self._reply_models.register_address)
              and value == (self._reply_models.register_address << 1 | 1)):
            self._start('address/read', status=4, phase='reading-data')
            self.rx_bytes = self._receive_input()
        elif self.phase == 'writing-data':
            if not self._admitted(self.target_address):
                raise UnsupportedDevice('Secondary bus data has no admitted target')
            if self.target_address == self._reply_models.pwic_address:
                if not self.payload:
                    if value not in self.pwic_registers:
                        raise UnsupportedDevice('PWIC profile rejects unsupported register prefix')
                elif len(self.payload) == 1:
                    self._pwic_write(self.payload[0], value)
                else:
                    raise UnsupportedDevice('PWIC profile supports only a prefix or recovered field pair')
            elif not self.payload:
                if value not in self.READ_REGISTERS:
                    raise UnsupportedDevice('Unknown bus1 target register prefix')
            elif len(self.payload) != 1 or self.payload[0] not in self.WRITE_REGISTERS:
                raise UnsupportedDevice('Bus1 only supports one-byte prefixes or0C/11 register/data pairs')
            elif len(self.payload) == 1:
                self._register_write(self.payload[0], value)
            self._start('write-byte', status=4, phase='writing-data', finish=lambda: self.payload.append(value))
        else:
            raise UnsupportedDevice(f'Unknown bus1 transmit {value:#x} in phase {self.phase}')
        self.regs[4] = value

    def snapshot(self):
        state = {'device': type(self).__name__, 'base': hex(self.BASE), 'profile': self.profile,
            'usb_connected': self.power.usb_connected, 'physical_irq': hex(self.PHYSICAL_IRQ),
            'registers': {hex(self.BASE+offset): hex(value) for offset, value in self.regs.items()},
            'target_registers': {hex(register): hex(value) for register, value in self.target_registers.items()},
            'target_address': None if self.target_address is None else hex(self.target_address),
            'pwic_prefix': self.pwic_prefix, 'pwic_registers': {hex(register): hex(value) for register, value in self.pwic_registers.items()},
            'phase': self.phase, 'busy': self.busy, 'payload_hex': bytes(self.payload).hex(),
            'rx_ready': self.rx_ready, 'rx_consumed': self.rx_consumed, 'rx_byte': self.rx_byte,
            'rx_bytes_hex': self.rx_bytes.hex(), 'rx_index': self.rx_index,
            'received_bytes': self.received_bytes, 'operations_started': self.operations_started,
            'operations_completed': self.operations_completed, 'irqs_raised': self.irqs_raised,
            'transactions': list(self.transactions), 'transfer_ticks': self.transfer_ticks,
            'limits': ['Endpoint8 belongs to firmware usbcharge task; physical target/chip and register defaults are unknown.',
                'Distinct opt-in disconnected input supplies virtual zeros only for02/05/0C/11/13 and retains virtual0C/11 writes.',
                'Requires explicit PowerDevices.usb_connected=False throughout each transaction; connected charging is unsupported.',
                'Bus1 mode9, address0A, one-byte prefix/single-byte read or0C/11 pair only; unknown registers, bulk writes and other targets reject.',
                'Separate combined PWIC profile additionally permits mode1/address30 with split61-prefix/read and61 bit4 writes only; initial61=0 is a chosen virtual input, not measured reset state.',
                'Separate startup-tagged profile additionally supplies20 as00000080 on four-byte read, retaining last-byte bit7 while permitting bit1-only changes; chosen framing is not a measured power state.',
                'PhysicalIRQ0x66 wakes actual firmware ISR/RTOS; no host callback, RAM flag or target ready signal is synthesized.',
                'ACKs, zero register state, write retention and four-tick latency are explicit unmeasured virtual inputs, not physical observations.']}
        if '_register_map' in self.__dict__:
            state['configuration'] = {
                'registers': {'base': hex(self.BASE), 'irq': self.PHYSICAL_IRQ,
                              'offsets': sorted(self.OFFSETS), 'modes': list(self._register_map.modes)},
                'endpoints': None if self._endpoint_table is None else
                    [{'index': row.index, 'address': row.address, 'polarity_selector': row.polarity_selector,
                      'channel': row.channel} for row in self._endpoint_table],
                'polarity_view': None if self._polarity_view is None else
                    {'address': hex(self._polarity_view.address), 'sha256': self._polarity_view.sha256,
                     'source_sha256': self._polarity_view.source_sha256,
                     'sampling': self._polarity_view.sampling,
                     'use': 'typed fixture validation; controller observes MMIO mode/address phases'},
                'replies': {'usb_connected': self._reply_models.usb_connected,
                            'register_address': self._reply_models.register_address,
                            'pwic_address': self._reply_models.pwic_address,
                            'register_values': list(self._reply_models.register_values),
                            'register_write_masks': list(self._reply_models.register_write_masks),
                            'pwic_values': list(self._reply_models.pwic_values),
                            'pwic_write_masks': list(self._reply_models.pwic_write_masks),
                            'pwic_read_prefixes': [(r, data.hex()) for r, data in self._reply_models.pwic_read_prefixes],
                            'register_modes': list(self._reply_models.register_modes),
                            'pwic_modes': list(self._reply_models.pwic_modes)}}
        if self.profile == DECLARED_PROFILE:
            state['limits'] = ['Explicit cable/register/field-mask/framing inputs, all unmeasured; no chip identity is inferred.',
                               'Only addresses admitted by both endpoint topology and the reply contract can schedule address ACK.',
                               'Cable state must match the selected contract throughout every transaction.',
                               'MMIO state and actual enabled IRQ route select completion; no task/PC/callback RAM target inference.',
                               'Unknown registers, unsupported field changes, incomplete ACK/NACK framing and other accesses reject.']
        return state
