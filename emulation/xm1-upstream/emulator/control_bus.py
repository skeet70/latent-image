"""Bounded bus0 endpoint7/9 transport with explicit virtual target inputs.

The stock 947BCC driver performs start, 7-bit address55/write, 1Cxx register
prefix/data or endpoint7 register/data, stop; actual callbacks wake through
physical IRQ0x65. A separate opt-in profile permits only endpoint7's recovered
register01/six-byte read, with an ideal stationary +Z gravity input. No firmware
task/PC/callback RAM contents select completion.

A distinct heartbeat profile adds only endpoint9 register1C06/one-byte reads.
Its counter advances at each completed read, modulo256: an explicit unmeasured
external liveness input, not a measured touch-controller clock or reset state.
"""
from .hardware import UnsupportedDevice
from .control_bus_contracts import (ControlBusRegisters, ControlBusReplies,
                                    RegisterStatusReply, DEFAULT_REGISTERS, DEFAULT_REPLIES)

PROFILE = 'endpoint9-1cxx-virtual-write-ack'
EXTENDED_PROFILE = 'endpoint7-and9-virtual-write-ack'
STATIONARY_PROFILE = 'endpoint7-stationary-z-plus-1g'
HEARTBEAT_PROFILE = 'endpoint7-stationary-and9-heartbeat-counter'
LATCHED_HEARTBEAT_PROFILE = 'endpoint7-stationary-and9-heartbeat-latched-irq-unmeasured'
HEARTBEAT_PROFILES = (HEARTBEAT_PROFILE, LATCHED_HEARTBEAT_PROFILE)
SENSOR_PROFILES = (STATIONARY_PROFILE, *HEARTBEAT_PROFILES)
REGISTER_STATUS_PROFILE = 'declared-register-status-pair-latched-irq-unmeasured'
PROFILES = (PROFILE, EXTENDED_PROFILE, *SENSOR_PROFILES, REGISTER_STATUS_PROFILE)


class ControlBus:
    BASE = 0xfff85000
    OFFSETS = {0, 4, 8, 0x10, 0x18}
    PHYSICAL_IRQ = 0x65
    # 25 initializer rows AF5264..AF5384; 9523B4 also writes register2A.
    ENDPOINT7_REGISTERS = DEFAULT_REPLIES.sensor_registers
    _register_map = DEFAULT_REGISTERS
    _reply_models = DEFAULT_REPLIES
    _endpoint_table = _polarity_view = None
    _status_targets = ()
    _stationary_input = False

    def __init__(self, board, *, profile=None, transfer_ticks=4,
                 registers=None, endpoints=None, polarity=None, replies=None, status_targets=None, stationary_input=False):
        if profile not in (None, *PROFILES):
            raise ValueError('Unknown control-bus input profile')
        if not isinstance(transfer_ticks, int) or transfer_ticks < 1:
            raise ValueError('Control-bus transfer ticks must be positive')
        if type(stationary_input) is not bool or (stationary_input and
                (profile != REGISTER_STATUS_PROFILE or not isinstance(replies, ControlBusReplies)
                 or replies.sensor_address is None)):
            raise ValueError('Composed stationary input requires an explicit status profile and sensor endpoint')
        if stationary_input:
            self._stationary_input = True
        if profile == REGISTER_STATUS_PROFILE or status_targets is not None:
            if (profile != REGISTER_STATUS_PROFILE or type(status_targets) is not tuple or not status_targets
                    or any(not isinstance(row, RegisterStatusReply) for row in status_targets)
                    or len({row.address for row in status_targets}) != len(status_targets)
                    or endpoints is None or not isinstance(replies, ControlBusReplies)):
                raise ValueError('Register/status profile requires explicit topology and reply contracts')
            if any(row.address in (replies.sensor_address, replies.prefix_address) for row in status_targets):
                raise ValueError('Register/status target overlaps another reply model')
            self._status_targets = status_targets
            self.status_target_registers = {row.address: dict.fromkeys(dict(row.write_masks), 0) for row in status_targets}
            self.status_target_frames = {row.address: row.status for row in status_targets}
        if any(value is not None for value in (registers, endpoints, polarity, replies)):
            from .boards.control_bus_parameters import EndpointDescriptor, FirmwareView
            registers = DEFAULT_REGISTERS if registers is None else registers
            if not isinstance(registers, ControlBusRegisters):
                raise TypeError('Control-bus registers must be an immutable register contract')
            if endpoints is not None:
                endpoints = tuple(endpoints)
                if (not endpoints or any(not isinstance(row, EndpointDescriptor)
                        or type(row.channel) is not int or row.channel != 0
                        or type(row.index) is not int or row.index < 0
                        or type(row.address) is not int or not 0 <= row.address <= 0x7f
                        or type(row.polarity_selector) is not int or row.polarity_selector not in (0, 1)
                        for row in endpoints)
                        or len({row.index for row in endpoints}) != len(endpoints)
                        or len({row.address for row in endpoints}) != len(endpoints)):
                    raise ValueError('Control-bus endpoints must be unique channel0 descriptors')
                if replies is None and profile is not None:
                    raise ValueError('Explicit endpoint tables require explicit reply contracts')
            if polarity is not None and (endpoints is None or not isinstance(polarity, FirmwareView)
                    or polarity.sampling != 'transaction' or len(polarity.data) != 8):
                raise ValueError('Control-bus polarity requires a typed transaction view and endpoint table')
            if replies is None:
                replies = DEFAULT_REPLIES if endpoints is None else ControlBusReplies(None, None)
            if not isinstance(replies, ControlBusReplies):
                raise TypeError('Control-bus replies must be an immutable reply contract')
            if ((profile in SENSOR_PROFILES or stationary_input) and replies.sensor_address is not None
                    and not {0x0e, 0x2a, 0x2b, 0x2f, 0x30, 0x31} <= replies.sensor_registers):
                raise ValueError('Stationary reply lacks its interpreted register fields')
            self._register_map, self._reply_models = registers, replies
            self._endpoint_table, self._polarity_view = endpoints, polarity
            self.BASE, self.OFFSETS, self.PHYSICAL_IRQ = registers.base, registers.offsets, registers.irq
        self.board, self.profile, self.transfer_ticks = board, profile, transfer_ticks
        self.regs = {offset: 0 for offset in self.OFFSETS}
        self.phase = 'idle'
        self.busy = False
        self.generation = self.operations_started = self.operations_completed = self.irqs_raised = 0
        self.payload = []
        self.transactions = []
        self.target_address = None
        self.sensor_registers = {}
        self.rx_bytes = b''
        self.rx_index = 0
        self.rx_ready = False
        self.rx_configured = False
        self.received_bytes = 0
        self.sensor_sample = None
        self.rx_kind = None
        self.rx_metadata = None
        self.endpoint9_prefix = None
        self.heartbeat_counter = 0
        self.heartbeat_reads = 0

    def _admitted(self, address):
        # Mode and address come from hardware phases. The immutable table is
        # topology/provenance; mutable polarity may be sampled by an explicit
        # fixture through its typed view, never to infer a target/task here.
        return (address is not None and (self._endpoint_table is None
                or any(row.address == address for row in self._endpoint_table)))

    def _write_addresses(self):
        replies = self._reply_models
        addresses = [replies.prefix_address]
        if self.profile in (EXTENDED_PROFILE, *SENSOR_PROFILES, REGISTER_STATUS_PROFILE):
            addresses.append(replies.sensor_address)
        addresses.extend(row.address for row in self._status_targets)
        return tuple(address << 1 for address in addresses if self._admitted(address))

    def _status_target(self, address):
        return next((row for row in self._status_targets if row.address == address), None)

    def set_status_frame(self, address, frame, *, source):
        if self._status_target(address) is None or type(frame) is not bytes or len(frame) != 2 or not isinstance(source, str) or not source:
            raise ValueError('Status input requires an admitted target, two bytes and source')
        self.status_target_frames[address] = frame
        self.board.record('External control-bus status input', address=hex(address), frame_hex=frame.hex(), source=source)

    def contains(self, address, size):
        return self.BASE <= address and address+size <= self.BASE+0x40

    def _offset(self, address, size):
        offset = address-self.BASE
        if size != 4 or address & 3 or offset not in self.OFFSETS:
            raise UnsupportedDevice(f'Unknown control-bus register/access {address:#x}/{size}')
        return offset

    def read(self, cpu_id, address, size):
        offset = self._offset(address, size)
        if offset == 4:
            if (self.profile not in (*SENSOR_PROFILES, REGISTER_STATUS_PROFILE) or self.phase != 'reading-data'
                    or self.busy or not self.rx_ready):
                raise UnsupportedDevice('Control-bus target receive data is not modeled or not ready')
            value = self.regs[4]
            self.rx_ready = False
            self.rx_index += 1
            self.received_bytes += 1
            return value
        return self.regs[offset]

    def _read_command(self, value):
        """Exact ACK/NACK framing for the two separately proven receive sizes."""
        if self.rx_kind not in ('stationary-xyz', 'touch-heartbeat', 'register-status'):
            raise UnsupportedDevice('Control-bus receive has no explicit external input')
        length = 6 if self.rx_kind == 'stationary-xyz' else 2 if self.rx_kind == 'register-status' else 1
        if len(self.rx_bytes) != length:
            raise UnsupportedDevice('Control-bus receive buffer differs from its bounded input class')
        if self.rx_ready:
            raise UnsupportedDevice('Control-bus receive byte must be consumed before another command')
        if value == 0xe and self.rx_index == 0 and self.regs[8] == 0xa:
            self.regs[8] = value
            self.rx_configured = True
        elif value == 0xa and self.rx_index == length-1 and self.regs[8] == 0xe:
            self.regs[8] = value  # Actual driver clears ACK bit2 before final byte.
        elif (self.rx_configured and value == (0x2e if self.rx_index < length-1 else 0x2a)
                and self.rx_index < length and self.regs[8] == (value & ~0x20)):
            index = self.rx_index
            def receive():
                self.regs[4] = self.rx_bytes[index]
                self.rx_ready = True
            self._start('receive-byte', status=4, phase='reading-data', finish=receive)
            self.regs[8] = value & ~0x20
        elif value == 0xb and self.rx_index == length and self.regs[8] == 0xa:
            payload, kind, metadata = self.rx_bytes, self.rx_kind, dict(self.rx_metadata)
            target = self.target_address
            def finish_read():
                item = {'address': hex(target), 'read_hex': payload.hex(), 'input_kind': kind}
                if kind == 'stationary-xyz':
                    item.update(register='0x1', sample=metadata)
                elif kind == 'touch-heartbeat':
                    item.update(register=hex(self._reply_models.heartbeat_register), heartbeat=metadata)
                    self.heartbeat_reads += 1
                    self.heartbeat_counter = (metadata['counter_value']+1) & 0xff
                else:
                    item.update(status_input=metadata)
                self.transactions.append(item)
                if len(self.transactions) > 512:
                    del self.transactions[:-512]
            self._start('stop/read', status=2, phase='idle', finish=finish_read)
            self.regs[8] = 0xa
        else:
            raise UnsupportedDevice(f'Unsupported {self.rx_kind} receive command {value:#x}/index{self.rx_index}')

    def _stationary_sample(self):
        """Ideal input under inferred MMA8452Q register semantics, not identity."""
        registers = self.sensor_registers
        if set(registers) != self._reply_models.sensor_registers:
            raise UnsupportedDevice(f'Stationary endpoint7 read requires all{len(self._reply_models.sensor_registers)} recovered initialization writes')
        data_cfg, control1, control2 = (registers[offset] for offset in (0x0e, 0x2a, 0x2b))
        if data_cfg not in (0, 1, 2):
            raise UnsupportedDevice('Stationary endpoint7 range/filter configuration is unsupported')
        if not control1 & 1 or control1 & 2:
            raise UnsupportedDevice('Stationary endpoint7 requires active six-byte12-bit mode')
        if control2 & 0xc0 or any(registers[offset] for offset in (0x2f, 0x30, 0x31)):
            raise UnsupportedDevice('Stationary endpoint7 self-test/reset/nonzero offsets are unsupported')
        counts_per_g = (1024, 512, 256)[data_cfg]
        self.sensor_sample = {'axes_g': [0, 0, 1], 'counts': [0, 0, counts_per_g],
                              'full_scale_g': (2, 4, 8)[data_cfg], 'resolution_bits': 12,
                              'interpretation': 'inferred MMA8452Q-compatible registers; unmeasured +Z pose'}
        return b'\0\0\0\0' + (counts_per_g << 4).to_bytes(2, 'big')

    def _target(self):
        targets = [index for index, bank in enumerate(self.board.interrupts.banks)
                   if bank.enabled & (1 << self.PHYSICAL_IRQ)]
        if len(targets) != 1:
            raise UnsupportedDevice(f'Control-bus physical IRQ{self.PHYSICAL_IRQ:#x} requires one enabled CPU route')
        return targets[0]

    def _irq_bindings(self, target):
        """Selected source fields only: neighboring IRQ registration is allowed.

        42B00/42E60 temporarily mask an entire32-source group. The separate
        latched profile binds the physical request to the original owner while
        that mask changes. It does not infer a route from callback/task RAM.
        """
        line = self.PHYSICAL_IRQ
        group, shift = line // 32, (line % 16) * 2
        mode_offsets = (0x140 + line // 16 * 4, 0x180 + line // 16 * 4)
        vector, priority = 0x400 + line * 4, 0x800 + line * 4
        required = (0x60 + group * 4, *mode_offsets, vector, priority)
        bindings = []
        for cpu, bank in enumerate(self.board.interrupts.banks):
            if not hasattr(bank, 'source_config') or not isinstance(getattr(bank, 'config', None), dict):
                raise UnsupportedDevice('Latched control-bus IRQ requires modeled controller source configuration')
            if cpu == target and (any(offset not in bank.config for offset in required)
                    or type(bank.config[priority]) is not int or not 0 <= bank.config[priority] <= 15
                    or type(bank.config[vector]) is not int or not 0 < bank.config[vector] <= 0xffffffff
                    or bank.config[vector] & 3):
                raise UnsupportedDevice('Latched control-bus IRQ owner lacks complete vector/priority/mode programming')
            bindings.append((
                (bank.config.get(0x60 + group * 4, 0) >> (line % 32)) & 1,
                (bank.source_config >> line) & 1,
                *(None if offset not in bank.config else (bank.config[offset] >> shift) & 3
                  for offset in mode_offsets),
                bank.config.get(vector), bank.config.get(priority)))
        return tuple(bindings)

    def _start(self, kind, *, status, phase, finish=None):
        if self.profile is None:
            raise UnsupportedDevice('Control-bus external ACK behavior requires an explicit input profile')
        if self.busy:
            raise UnsupportedDevice('Control-bus operation while a prior shift is pending')
        target = self._target()
        profile = self.profile
        latched = profile in (LATCHED_HEARTBEAT_PROFILE, REGISTER_STATUS_PROFILE)
        binding = self._irq_bindings(target) if latched else None
        self.generation += 1
        generation = self.generation
        self.busy = True
        self.regs[0x18] = 0
        self.operations_started += 1
        self.board.record('Control-bus operation start', operation=kind, profile=self.profile,
                          transfer_ticks=self.transfer_ticks,
                          **({'irq_owner': target, 'irq_binding': binding} if latched else {}),
                          provenance='explicit unmeasured virtual target ACK and controller latency')
        def complete():
            if generation != self.generation:
                if latched:
                    raise UnsupportedDevice('Latched control-bus operation generation changed during transfer')
                return
            if latched:
                if self.profile != profile or self._irq_bindings(target) != binding:
                    raise UnsupportedDevice('Latched control-bus IRQ profile/source/vector/priority binding changed')
                if any(cpu != target and bank.enabled & (1 << self.PHYSICAL_IRQ)
                       for cpu, bank in enumerate(self.board.interrupts.banks)):
                    raise UnsupportedDevice('Latched control-bus IRQ gained a competing enabled owner')
            elif self._target() != target:
                raise UnsupportedDevice('Control-bus IRQ route changed during transfer')
            self.busy = False
            self.regs[0x18] = status
            self.phase = phase
            if finish:
                finish()
            self.operations_completed += 1
            self.board.queue_irq(target, self.PHYSICAL_IRQ)
            self.irqs_raised += 1
            self.board.record('Control-bus operation complete', operation=kind, status=hex(status),
                              cpu=target, physical_irq=hex(self.PHYSICAL_IRQ), profile=self.profile,
                              **({'irq_owner_enabled': bool(self.board.interrupts.banks[target].enabled &
                                                           (1 << self.PHYSICAL_IRQ)),
                                  'irq_binding': binding} if latched else {}))
        self.board.schedule(self.transfer_ticks, complete)

    def write(self, cpu_id, address, size, value):
        offset = self._offset(address, size)
        if not isinstance(value, int) or not 0 <= value <= 0xffffffff:
            raise UnsupportedDevice('Control-bus value is not a 32-bit word')
        if offset == 0x18:
            raise UnsupportedDevice('Control-bus status is read-only in the recovered write subset')
        if offset == 0x10:
            if self.busy or self.phase != 'idle' or value not in self._register_map.modes:
                message = ('Only idle bus0 mode9 configuration is supported' if self._register_map == DEFAULT_REGISTERS
                           else f'Only idle bus configuration modes {self._register_map.modes} are supported')
                raise UnsupportedDevice(message)
            self.regs[offset] = value
            return
        if offset == 0:
            if self.busy or value not in (0, 1):
                raise UnsupportedDevice('Unsupported control-bus enable/disable transaction')
            if value == 1:
                if self.phase != 'idle' or self.regs[0x10] not in self._register_map.modes:
                    message = ('Control-bus enable before mode9 configuration' if self._register_map == DEFAULT_REGISTERS
                               else 'Control-bus enable before supported mode configuration')
                    raise UnsupportedDevice(message)
            elif self.phase != 'idle':
                raise UnsupportedDevice('Control-bus disable before completed stop')
            self.regs[offset] = value
            return
        if not self.regs[0] & 1:
            raise UnsupportedDevice('Control-bus operation while disabled')
        if self.busy:
            raise UnsupportedDevice('Control-bus operation while a prior shift is pending')
        if offset == 8:
            allowed = 0x3f if self.profile in (*SENSOR_PROFILES, REGISTER_STATUS_PROFILE) else 0x1b
            if value & ~allowed:
                raise UnsupportedDevice('Control-bus receive/unknown command bits are unsupported')
            if self.phase == 'idle' and value in (0x19, 0x1b):
                self._start('start', status=1, phase='awaiting-address')
                self.regs[8] = 8  # Start request bits0/4 complete; bit3 retained.
                self.payload = []
                self.target_address = None
                self.rx_bytes, self.rx_index, self.rx_ready = b'', 0, False
                self.rx_configured = False
                self.rx_kind, self.rx_metadata = None, None
            elif self.phase == 'awaiting-address' and value == 0xa and not self.busy:
                self.regs[8] = value
            elif ((self.profile in SENSOR_PROFILES or self._stationary_input) and self.phase == 'writing-data'
                    and self._admitted(self.target_address)
                    and self.target_address == self._reply_models.sensor_address and self.payload == [1] and value == 0xa):
                self.phase = 'awaiting-read-address'
                self.regs[8] = value
            elif self.profile in (*SENSOR_PROFILES, REGISTER_STATUS_PROFILE) and self.phase == 'reading-data':
                self._read_command(value)
            elif self.phase == 'writing-data' and value == 0xb and len(self.payload) in (2, 3):
                payload = bytes(self.payload)
                target = getattr(self, 'target_address', None)
                if target == self._reply_models.sensor_address and len(payload) != 2:
                    raise UnsupportedDevice('Endpoint7 stop requires exactly one register/data pair')
                status_target = self._status_target(target)
                if (not self._admitted(target) or (status_target is None and
                        target not in (self._reply_models.prefix_address, self._reply_models.sensor_address))):
                    raise UnsupportedDevice('Control-bus stop has no supported target address')
                if status_target is not None and len(payload) != 2:
                    raise UnsupportedDevice('Register/status stop requires one register/data pair')
                def finish():
                    item = {'address': hex(target), 'write_hex': payload.hex()}
                    if status_target is not None:
                        self.status_target_registers[target][payload[0]] = payload[1]
                        item.update(register=hex(payload[0]), data_byte=payload[1], input_kind='declared register/status target')
                        reply = next((frame for register, value, frame in status_target.write_replies
                                      if payload == bytes((register, value))), None)
                        if reply is not None:
                            self.status_target_frames[target] = reply
                            item.update(status_transition_hex=reply.hex(),
                                        basis='explicit completed wire-command/status rule; no guest task RAM')
                    elif target == self._reply_models.prefix_address:
                        item.update(register_prefix=hex(int.from_bytes(payload[:2], 'big')),
                                    data_byte=payload[2] if len(payload) == 3 else None)
                        self.endpoint9_prefix = int.from_bytes(payload, 'big') if len(payload) == 2 else None
                    else:
                        item.update(register=hex(payload[0]), data_byte=payload[1])
                        if (self.profile in SENSOR_PROFILES or self._stationary_input):
                            self.sensor_registers[payload[0]] = payload[1]
                    self.transactions.append(item)
                    if len(self.transactions) > 512:
                        del self.transactions[:-512]
                self._start('stop', status=2, phase='idle', finish=finish)
                self.regs[8] = 0xa
            else:
                raise UnsupportedDevice(f'Unsupported control-bus command {value:#x} in phase {self.phase}')
            return
        if offset == 4:
            if value > 0xff:
                raise UnsupportedDevice('Control-bus transmit requires one byte')
            if self.phase == 'awaiting-address' and self.regs[8] == 0xa:
                status_target = self._status_target(value >> 1)
                if status_target is not None and value & 1 and self._admitted(status_target.address):
                    sample = self.status_target_frames[status_target.address]
                    def finish_status_address():
                        self.target_address = status_target.address
                        self.rx_bytes, self.rx_index, self.rx_ready = sample, 0, False
                        self.rx_kind = 'register-status'
                        self.rx_metadata = {'frame_hex': sample.hex(), 'provenance': 'explicit external input; no chip identity or autonomous measurement'}
                    self._start('address/read', status=4, phase='reading-data', finish=finish_status_address)
                    self.regs[4] = value
                    return
                if self._admitted(self._reply_models.prefix_address) and value == (self._reply_models.prefix_address << 1 | 1) and self.profile in HEARTBEAT_PROFILES:
                    if self.endpoint9_prefix != self._reply_models.heartbeat_register:
                        raise UnsupportedDevice(f'Endpoint9 heartbeat requires completed register{self._reply_models.heartbeat_register:04X} prefix')
                    sample = bytes((self.heartbeat_counter,))
                    metadata = {'counter_value': self.heartbeat_counter, 'completed_read_index': self.heartbeat_reads,
                        'progression': 'advance by1 modulo256 only after completed one-byte read stop',
                        'provenance': 'explicit unmeasured external liveness input; physical rate/reset/identity unknown'}
                    def finish_heartbeat_address():
                        self.target_address = self._reply_models.prefix_address
                        self.endpoint9_prefix = None
                        self.rx_bytes, self.rx_index, self.rx_ready = sample, 0, False
                        self.rx_kind, self.rx_metadata = 'touch-heartbeat', metadata
                    self._start('address/read', status=4, phase='reading-data', finish=finish_heartbeat_address)
                    self.regs[4] = value
                    return
                supported = self._write_addresses()
                if value not in supported:
                    raise UnsupportedDevice('Control-bus target outside explicit write profile; target reads are unresolved')
                self._start('address/write', status=0xc, phase='writing-data',
                            finish=lambda: setattr(self, 'target_address', value >> 1))
            elif ((self.profile in SENSOR_PROFILES or self._stationary_input) and self.phase == 'awaiting-read-address'
                    and self.regs[8] == 0xa and self._admitted(self._reply_models.sensor_address)
                    and value == (self._reply_models.sensor_address << 1 | 1)):
                sample = self._stationary_sample()
                def finish_address():
                    self.rx_bytes, self.rx_index, self.rx_ready = sample, 0, False
                    self.rx_kind, self.rx_metadata = 'stationary-xyz', dict(self.sensor_sample)
                self._start('address/read', status=4, phase='reading-data', finish=finish_address)
            elif self.phase == 'writing-data':
                target = getattr(self, 'target_address', None)
                if not self._admitted(target):
                    raise UnsupportedDevice('Control-bus data has no supported target address')
                status_target = self._status_target(target)
                if status_target is not None:
                    masks = dict(status_target.write_masks)
                    if (len(self.payload) >= 2 or (not self.payload and value not in masks)
                            or (self.payload and value & ~masks[self.payload[0]])):
                        raise UnsupportedDevice('Register/status target rejects unadmitted register/data bits')
                elif target == self._reply_models.prefix_address:
                    if len(self.payload) >= 3 or (not self.payload and value != self._reply_models.prefix_high):
                        raise UnsupportedDevice(f'Control-bus supports only two-byte{self._reply_models.prefix_high:02X}xx prefixes and an optional one-byte write')
                elif target == self._reply_models.sensor_address:
                    prefix_read = (self.profile in SENSOR_PROFILES or self._stationary_input) and not self.payload and value == 1
                    if (len(self.payload) >= 2 or self.payload == [1]
                            or (not self.payload and value not in self._reply_models.sensor_registers and not prefix_read)):
                        raise UnsupportedDevice('Endpoint7 accepts only recovered register/data pairs')
                else:
                    raise UnsupportedDevice('Control-bus data has no supported target address')
                self._start('write-byte', status=4, phase='writing-data',
                            finish=lambda: self.payload.append(value))
            else:
                raise UnsupportedDevice(f'Control-bus transmit in unsupported phase {self.phase}')
            self.regs[4] = value

    def snapshot(self):
        state = {'device': type(self).__name__, 'base': hex(self.BASE), 'profile': self.profile,
                'target_address': (hex(self.target_address) if getattr(self, 'target_address', None) is not None else None),
                'registers': {hex(self.BASE+offset): hex(value) for offset, value in self.regs.items()},
                'phase': self.phase, 'busy': self.busy, 'payload_hex': bytes(self.payload).hex(),
                'operations_started': self.operations_started, 'operations_completed': self.operations_completed,
                'irqs_raised': self.irqs_raised, 'physical_irq': hex(self.PHYSICAL_IRQ),
                'transactions': list(self.transactions), 'transfer_ticks': self.transfer_ticks,
                'sensor_registers': {hex(key): hex(value) for key, value in getattr(self, 'sensor_registers', {}).items()},
                'received_bytes': getattr(self, 'received_bytes', 0), 'rx_index': getattr(self, 'rx_index', 0),
                'rx_buffer_hex': getattr(self, 'rx_bytes', b'').hex(),
                'rx_ready': getattr(self, 'rx_ready', False), 'sensor_sample': getattr(self, 'sensor_sample', None),
                'rx_kind': self.rx_kind, 'rx_metadata': self.rx_metadata,
                'rx_configured': self.rx_configured,
                'endpoint9_prefix': hex(self.endpoint9_prefix) if self.endpoint9_prefix is not None else None,
                'heartbeat_counter': self.heartbeat_counter, 'heartbeat_reads': self.heartbeat_reads,
                'limits': ['Endpoint9 writes to0x55 with 1Cxx prefixes; extended profile also accepts endpoint7/address0x1C recovered register/data writes',
                           'Two write-only profiles retain no sensor effects/readback; stationary and heartbeat profiles permit only recovered register01/six-byte XYZ read',
                           'Separate heartbeat profile additionally permits only endpoint9 register1C06/one-byte final-NACK reads, counter0..255 advances per completed read as an unmeasured fixture',
                           'Stationary profile ideal +Z1g and MMA8452Q register interpretation are explicit inputs/inference; physical camera axes/identity/noise are unproved',
                           'Target ACK, controller command completion and latency are explicit virtual inputs, not physical capture',
                           'Actual enabled IRQ routing wakes firmware; no host callback invocation or RAM completion flag',
                           'The separate latched-IRQ profile requires a unique enabled owner at request, binds its source/mode/vector/priority fields at request/completion, then queues the real pending IRQ to that same owner even temporarily masked. Competing owners and changed bindings/profile/operation generation reject before completion. Later enable/delivery/ACK/EOI remain stock-kernel owned; physical masking/latching is unmeasured.',
                           'Binding checks compare selected fields at request/completion; controller rewrites restored between those boundaries are not independently tracked.',
                           'Bus1, other targets/register reads, touch packets/events/reset/panel effects, high-pass/self-test/reset/offset effects, sensor IRQ/data-ready and electrical arbitration remain unsupported']}
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
                'replies': {'sensor_address': self._reply_models.sensor_address,
                            'prefix_address': self._reply_models.prefix_address,
                            'sensor_registers': sorted(self._reply_models.sensor_registers),
                            'prefix_high': self._reply_models.prefix_high,
                            'heartbeat_register': self._reply_models.heartbeat_register},
                'limits': 'Selected protocol laws are explicit unmeasured inputs; descriptor equality proves no chip identity.'}
        if self._status_targets:
            state['register_status_inputs'] = [
                {'address': hex(row.address), 'write_masks': list(row.write_masks),
                 'registers': dict(self.status_target_registers[row.address]),
                 'status_hex': self.status_target_frames[row.address].hex(),
                 'write_replies': [(register, value, frame.hex()) for register, value, frame in row.write_replies]}
                for row in self._status_targets]
            state['limits'].append('Register/status targets admit only declared register-byte writes and two-byte status input reads; no task RAM, chip identity, calibration or autonomous conversion is inferred.')
        if self._stationary_input:
            state['composed_stationary_input'] = 'Explicit unmeasured +Z1g; same bounded register01/six-byte law as the stationary profile'
        return state
