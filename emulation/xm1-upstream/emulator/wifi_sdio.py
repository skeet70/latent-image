"""Stock host1 SDIO enumeration with a declared one-function virtual card.

FFF51000 uses the frozen 1D7B448/1D7AB88/1D7A0B4 register grammar. Clock
2C remains owned by BootControls. Enumeration remains a separate strict
profile. The bootstrap profile accepts only the supplied firmware's framed
FIFO and phase-bound registers; readiness/metadata/latency are declared
unmeasured inputs. No guest calls or network/task/UI readiness substitution.
"""
import hashlib
from .board import RAM_SIZE
from .hardware import UnsupportedDevice


ENUMERATION_PROFILE = 'declared-wifi-sdio-one-function-enumeration-unmeasured'
BOOTSTRAP_PROFILE = 'declared-wifi-sdio-rom-firmware-bootstrap-unmeasured'
PROFILES = (ENUMERATION_PROFILE, BOOTSTRAP_PROFILE)


class WiFiSDIOController:
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    BASE = 0xfff51000
    PHYSICAL_IRQ = 79  # decimal79 /0x4F, direct9C8C14 physical API.
    ISR = 0x1d792c8
    HALFWORDS = frozenset((4, 6, 8, 10, 12, 14, *range(16, 32, 2),
                          0x28, 0x2e, 0x30, 0x32, 0x34, 0x36, 0x38,
                          0x3a, 0x3e, 0x100, 0x104))
    WORDS = frozenset((0, 12, 0x20, 0x24))
    # Whole native9C8C14(4F,priority,mode,1D792C8) output; not a
    # logical AEAFE0 descriptor. Initial priority0/mode2, later3/mode0.
    BINDINGS = frozenset(((0, 1, 2, 0, ISR, 0), (0, 0, 0, 2, ISR, 3)))
    COMMANDS = {5: 0x05020000, 3: 0x031a0000, 7: 0x071b0000}
    CCCR_METADATA = {0: 0x22, 1: 3, 5: 0, 8: 0x82, 9: 0, 10: 0, 11: 0,
                     12: 0, 13: 0, 14: 0, 15: 0, 0x100: 0, 0x109: 0,
                     0x10a: 0, 0x10b: 0}
    CCCR_WRITABLE = frozenset((2, 4, 6, 7, 0x10, 0x11, 0x110, 0x111))

    FIRMWARE_ADDRESS = 0x1eb6144
    FIRMWARE_SIZE = 0x215c0
    FIRMWARE_SHA256 = '80674bf44b30e51a14821740d06f662036131ac04e2a5312f235aed5c361f251'

    def __init__(self, board, controls, *, profile=None, transfer_ticks=100):
        if profile not in (None, *PROFILES):
            raise ValueError('Unknown WiFi SDIO profile')
        if type(transfer_ticks) is not int or transfer_ticks < 1:
            raise ValueError('WiFi SDIO transfer ticks must be positive')
        if not isinstance(getattr(controls, 'registers', None), dict):
            raise ValueError('WiFi SDIO needs the explicit BootControls clock owner')
        self.board, self.controls = board, controls
        self.profile, self.transfer_ticks = profile, transfer_ticks
        self.regs = {offset: 0 for offset in self.HALFWORDS | self.WORDS}
        self.response = bytes(16)
        self.card_registers = {**self.CCCR_METADATA, **{x: 0 for x in self.CCCR_WRITABLE}, 3: 0}
        self.state, self.rca = 'idle', 0x1234
        self.request = None
        self.generation = self.started = self.completed = self.irqs = self.resets = 0
        self.history = []
        if profile == BOOTSTRAP_PROFILE:
            self.vendor = dict(phase='idle', cursor=0, frames=[], words={}, direct=False)

    @property
    def busy(self):
        return self.request is not None

    def contains(self, address, size):
        if address < self.BASE + 0x2e and address + size > self.BASE + 0x2c:
            return False  # Existing BootControls clock halfword owns2C.
        return (self.BASE <= address and address + size <= self.BASE + 0x40
                or self.BASE + 0x100 <= address and address + size <= self.BASE + 0x108)

    def _offset(self, address, size):
        offset = address - self.BASE
        if not (size == 2 and offset in self.HALFWORDS or size == 4 and offset in self.WORDS):
            raise UnsupportedDevice(f'Unknown WiFi SDIO register/access {address:#x}/{size}')
        return offset

    def _binding(self):
        result = []
        line, shift = self.PHYSICAL_IRQ, self.PHYSICAL_IRQ % 16 * 2
        for bank in self.board.interrupts.banks:
            result.append(((bank.config.get(0x68, 0) >> (line % 32)) & 1,
                           bank.source_config >> line & 1,
                           *(None if offset not in bank.config else bank.config[offset] >> shift & 3
                             for offset in (0x150, 0x190)),
                           bank.config.get(0x53c), bank.config.get(0x93c)))
        return tuple(result)

    def _route(self):
        controller = getattr(self.board, 'interrupts', None)
        if controller is None:
            raise UnsupportedDevice('WiFi SDIO requires the physical IRQ controller')
        owners = [cpu for cpu, bank in enumerate(controller.banks)
                  if bank.enabled & (1 << self.PHYSICAL_IRQ)]
        if len(owners) != 1:
            raise UnsupportedDevice('WiFi SDIO IRQ79 needs one initially enabled CPU owner')
        owner, binding = owners[0], self._binding()
        if binding[owner] not in self.BINDINGS:
            raise UnsupportedDevice('WiFi SDIO IRQ79 needs the whole-driver source/mode/ISR/priority binding')
        return owner, binding

    def _check(self, request):
        if (request['profile'] != self.profile
                or self.controls.registers.get(self.BASE + 0x2c, 0) != request['clock']
                or self._binding() != request['binding']
                or any(cpu != request['owner'] and bank.enabled & (1 << self.PHYSICAL_IRQ)
                       for cpu, bank in enumerate(self.board.interrupts.banks))):
            raise UnsupportedDevice('WiFi SDIO card/clock/IRQ binding changed during a command')

    def _physical(self, address, quantity):
        physical = address & 0x3fffffff if 0x40000000 <= address < 0x60000000 else address
        if address & 31 or not 1 <= quantity <= 2560 or not 0 <= physical <= RAM_SIZE - quantity:
            raise UnsupportedDevice('WiFi bootstrap DMA requires bounded aligned DRAM')
        return physical

    def _validate_vendor(self):
        vendor = self.vendor
        phase, cursor = vendor['phase'], vendor['cursor']
        words = {0x8c: 0x8000}
        expected = {'idle': {}, 'reset-asserted': dict(words)}
        words[0x8c] = 0; expected['reset-ready'] = dict(words)
        words[0x90] = self.FIRMWARE_SIZE >> 6; expected['sized'] = dict(words)
        words[0x8c] = 1; expected['bootloader-ready'] = dict(words)
        words[0x88] = 1; expected['load-enabled'] = dict(words)
        words[0x84] = 0xff900000
        expected.update(loading=dict(words), **{'firmware-ready': dict(words)})
        words[0x88] = 0; expected['post-download'] = dict(words)
        words[0xc] = 0; words[0x14] = 0xffffff0f; expected['configured'] = dict(words)
        current_words = dict(vendor['words'])
        if phase == 'post-download' and current_words.get(0xc) == 0:
            current_words.pop(0xc)
        if (set(vendor) != {'phase', 'cursor', 'frames', 'words', 'direct'}
                or phase not in expected or current_words != expected[phase]
                or type(vendor['direct']) is not bool
                or vendor['direct'] and phase not in ('post-download', 'configured')
                or phase == 'configured' and not vendor['direct']
                or type(cursor) is not int or not 0 <= cursor <= self.FIRMWARE_SIZE
                or cursor != sum(frame[0] for frame in vendor['frames'])
                or len(vendor['frames']) > 67
                or any(frame != ((2048, 2560) if index < 66 else (1472, 1536))
                       for index, frame in enumerate(vendor['frames']))
                or phase not in ('loading', 'firmware-ready', 'post-download', 'configured') and cursor
                or phase == 'loading' and cursor == self.FIRMWARE_SIZE
                or phase in ('firmware-ready', 'post-download', 'configured') and cursor != self.FIRMWARE_SIZE):
            raise UnsupportedDevice('WiFi bootstrap phase/frame archive differs from bounded ROM transaction')
        if hashlib.sha256(self.board.read_memory(self.FIRMWARE_ADDRESS, self.FIRMWARE_SIZE)).hexdigest() != self.FIRMWARE_SHA256:
            raise UnsupportedDevice('WiFi bootstrap requires the exact supplied overlay firmware blob')

    def _vendor_plan(self, address, write, payload, quantity):
        self._validate_vendor()
        vendor = self.vendor
        phase, cursor = vendor['phase'], vendor['cursor']
        response, update = None, None
        if address == 0x80:
            if not write or phase != 'loading' or quantity not in (2560, 1536):
                raise UnsupportedDevice('WiFi FIFO is not the bounded firmware download')
            expected = min(2048, self.FIRMWARE_SIZE - cursor)
            length = (expected + 7) & ~3
            wire = (length + 511) // 512 * 512
            firmware = self.board.read_memory(self.FIRMWARE_ADDRESS + cursor, expected)
            if (quantity != wire or int.from_bytes(payload[:2], 'little') != length
                    or payload[2:4] != bytes(2) or payload[4:length] != firmware
                    or payload[length:length+4] != bytes(4)):
                raise UnsupportedDevice('WiFi firmware frame/header/source/termination differs')
            update = ('frame', expected, quantity)
        elif quantity != 4:
            raise UnsupportedDevice('WiFi bootstrap register is not one little-endian word')
        elif not write:
            allowed = {(0x8c, 'reset-ready'): 0x800, (0x8c, 'bootloader-ready'): 0x2000,
                       (0x8c, 'firmware-ready'): 0x8000, (0x88, 'loading'): 0x101,
                       (0x88, 'load-enabled'): 0x101, (0, 'post-download'): 0x205931,
                       (0, 'configured'): 0x205931, (0xc, 'post-download'): 0,
                       (0xc, 'configured'): vendor['words'].get(0xc, 0),
                       (0x10, 'configured'): 0}
            # Whole1D86CC0 queries word4 after the complete ROM download
            # and CMD52 function1 control. This is a declared awake input,
            # not CMD52 byte4 readback or a granted sleep/write4 protocol.
            if vendor['direct']:
                allowed[4, 'post-download'] = 0x100
            if (address, phase) not in allowed:
                raise UnsupportedDevice('WiFi vendor status read exceeds bootstrap phase')
            response = allowed[address, phase].to_bytes(4, 'little')
        else:
            value = int.from_bytes(payload, 'little')
            transitions = {(0x8c, 'idle', 0x8000): 'reset-asserted',
                           (0x8c, 'reset-asserted', 0): 'reset-ready',
                           (0x90, 'reset-ready', self.FIRMWARE_SIZE >> 6): 'sized',
                           (0x8c, 'sized', 1): 'bootloader-ready',
                           (0x88, 'bootloader-ready', 1): 'load-enabled',
                           (0x84, 'load-enabled', 0xff900000): 'loading',
                           (0x88, 'firmware-ready', 0): 'post-download',
                           (0xc, 'post-download', 0): 'post-download',
                           (0xc, 'configured', 0): 'configured',
                           (0x14, 'post-download', 0xffffff0f): 'configured'}
            if (address, phase, value) not in transitions:
                raise UnsupportedDevice('WiFi vendor bootstrap write/order/value differs')
            if phase == 'idle':
                firmware = self.board.read_memory(self.FIRMWARE_ADDRESS, self.FIRMWARE_SIZE)
                if hashlib.sha256(firmware).hexdigest() != self.FIRMWARE_SHA256:
                    raise UnsupportedDevice('WiFi bootstrap requires the exact supplied overlay firmware blob')
            if address == 0x14 and not vendor['direct']:
                raise UnsupportedDevice('WiFi post-download function1 control has not completed')
            update = ('word', address, value, transitions[address, phase, value])
        return dict(phase=phase, cursor=cursor, address=address, quantity=quantity,
                    response=response, update=update)

    def _vendor_complete(self, plan):
        self._validate_vendor()
        vendor = self.vendor
        if (vendor['phase'] != plan['phase'] or vendor['cursor'] != plan['cursor']
                or hashlib.sha256(self.board.read_memory(self.FIRMWARE_ADDRESS, self.FIRMWARE_SIZE)).hexdigest() != self.FIRMWARE_SHA256):
            raise UnsupportedDevice('WiFi firmware state changed while command pending')
        update = plan['update']
        if update is None:
            return
        if update[0] == 'direct':
            vendor['direct'] = True
        elif update[0] == 'word':
            _, address, value, vendor['phase'] = update
            vendor['words'][address] = value
        else:
            vendor['cursor'] += update[1]
            vendor['frames'].append((update[1], update[2]))
            if vendor['cursor'] == self.FIRMWARE_SIZE:
                vendor['phase'] = 'firmware-ready'

    def _plan(self, word, argument):
        command = word >> 24 & 0x3f
        state, response, update = self.state, 0, None
        if command in self.COMMANDS:
            if word != self.COMMANDS[command]:
                raise UnsupportedDevice('WiFi SDIO command grammar differs from the stock driver')
            if command == 5:
                if state not in ('idle', 'ready') or argument not in (0, 0x200000):
                    raise UnsupportedDevice('WiFi CMD5 requires idle/ready and recovered OCR request')
                # One I/O function, no memory card, supported voltage20.
                response = 0x10200000 | (0x80000000 if argument else 0)
                state = 'ready' if argument else 'idle'
            elif command == 3:
                if state != 'ready' or argument:
                    raise UnsupportedDevice('WiFi CMD3 requires ready card/argument0')
                response, state = self.rca << 16, 'standby'
            else:
                if state not in ('standby', 'transfer') or argument not in (0, self.rca << 16):
                    raise UnsupportedDevice('WiFi CMD7 requires assigned RCA or deselection')
                response, state = (0x900, 'transfer') if argument else (0x600, 'standby')
        elif command == 52:
            write, function, raw = bool(argument & 0x80000000), argument >> 28 & 7, bool(argument & 0x08000000)
            address, data = argument >> 9 & 0x1ffff, argument & 255
            if (state != 'transfer' or word != (0x341a0000 if write else 0x341a0010)
                    or argument & 0x04000100):
                raise UnsupportedDevice('WiFi CMD52 requires selected card and stock direct-I/O grammar')
            if function:
                if self.profile == BOOTSTRAP_PROFILE: self._validate_vendor()
                if (self.profile != BOOTSTRAP_PROFILE or function != 1 or not write or raw
                        or address != 4 or data != 2 or self.vendor['phase'] != 'post-download'
                        or self.vendor['direct']):
                    raise UnsupportedDevice('WiFi function1 direct-I/O exceeds bounded bootstrap control')
                vendor = dict(phase=self.vendor['phase'], cursor=self.vendor['cursor'],
                              address=4, quantity=1, response=None, update=('direct',))
                return dict(word=word, command=command, argument=argument, response=0,
                            next_state=state, update=None, vendor=vendor)
            if address not in self.card_registers:
                raise UnsupportedDevice(f'WiFi vendor/CIS register is not qualified: function{function}/address{address:#x}')
            old = self.card_registers[address]
            if write:
                if address not in self.CCCR_WRITABLE:
                    raise UnsupportedDevice('WiFi CCCR metadata is read-only')
                if ((address == 2 and data & ~2) or (address == 4 and data & ~3)
                        or (address == 6 and data not in (0, 1, 8))
                        or (address == 7 and data not in (0, 2))):
                    raise UnsupportedDevice('WiFi CCCR function/reset/bus choice exceeds the declared fixture')
                update = (address, data)
            response = data if write and raw else old
        elif command == 53 and self.profile == BOOTSTRAP_PROFILE:
            write, function = bool(argument & 0x80000000), argument >> 28 & 7
            block, increment = bool(argument & 0x08000000), bool(argument & 0x04000000)
            address, count = argument >> 9 & 0x1ffff, argument & 511
            pio = word == 0x353a0010
            expected = (0x353a0023 if write else 0x353a0033) if block else (0x353a0001 if write else 0x353a0011)
            if pio:
                expected = 0x353a0010
                if write or block or address != 0x10 or count != 4:
                    raise UnsupportedDevice('WiFi PIO is only the configured register10 word read')
            quantity = count * 512 if block else count or 512
            if (state != 'transfer' or function != 1 or increment or word != expected
                    or block and (count not in (3, 5) or self.regs[4] != 512 or self.regs[6] != count
                                  or self.card_registers[0x110] != 0 or self.card_registers[0x111] != 2)
                    or not block and (count != 4 or self.regs[4] != 4 or self.regs[6])):
                raise UnsupportedDevice('WiFi bootstrap CMD53 grammar/blocks/CCCR differs')
            if pio:
                vendor = self._vendor_plan(address, False, None, quantity)
                if (self.card_registers[0x110], self.card_registers[0x111]) != (0, 2):
                    raise UnsupportedDevice('WiFi PIO drain requires the declared512-byte function block')
                return dict(word=word, command=command, argument=argument, response=0,
                            next_state=state, update=None, vendor=vendor,
                            quantity=quantity, write=False, pio=True, pio_ready=False,
                            pio_reads=0, pio_gates=tuple(self.regs[x] for x in (0x34, 0x36, 0x38, 0x3a)))
            physical = self._physical(self.regs[0], quantity)
            if self.regs[0x28] & 0x18 or quantity > 4096 - physical % 4096:
                raise UnsupportedDevice('WiFi bootstrap SDMA mode/boundary is not qualified')
            payload = self.board.read_memory(physical, quantity) if write else None
            vendor = self._vendor_plan(address, write, payload, quantity)
            return dict(word=word, command=command, argument=argument, response=0,
                        next_state=state, update=None, vendor=vendor, dma=physical,
                        quantity=quantity, write=write, payload=payload)
        else:
            raise UnsupportedDevice(f'WiFi SDIO CMD{command} is not qualified; vendor data/DMA remains strict')
        return dict(word=word, command=command, argument=argument,
                    response=response, next_state=state, update=update)

    def _start(self, word):
        if self.profile is None:
            raise UnsupportedDevice('WiFi SDIO command needs an explicit virtual-card profile')
        if self.request is not None or self.regs[0x30] or self.regs[0x32]:
            raise UnsupportedDevice('WiFi SDIO command requires completed/ACKed prior status')
        clock = self.controls.registers.get(self.BASE + 0x2c, 0)
        if clock & 7 != 7 or self.regs[0x2e] & 15 != 14:
            raise UnsupportedDevice('WiFi SDIO command requires stock enabled clock/timeout configuration')
        if not self.regs[0x34] & (2 if word >> 24 & 0x3f == 53 and word != 0x353a0010 else 1):
            raise UnsupportedDevice('WiFi SDIO command-complete status is disabled')
        argument = self.regs[8] | self.regs[10] << 16
        plan = self._plan(word, argument)
        owner, binding = self._route()
        self.generation += 1
        plan.update(generation=self.generation, profile=self.profile, clock=clock,
                    owner=owner, binding=binding, due_tick=self.board.virtual_ticks+self.transfer_ticks)
        self.request = plan
        self.regs[12], self.regs[14] = word & 0xffff, word >> 16
        self.started += 1
        self.board.record('WiFi SDIO command start', command=plan['command'], argument=argument,
                          word=word, profile=self.profile, latency='declared unmeasured virtual ticks')

    def next_event_tick(self, now):
        return (max(now+1, self.request['due_tick'])
                if self.request is not None and not self.request.get('pio_ready') else None)

    def _signal(self, owner):
        if self.regs[0x30] & self.regs[0x34] & self.regs[0x38]:
            self.board.queue_irq(owner, self.PHYSICAL_IRQ)
            self.irqs += 1
            self.board.record('WiFi SDIO physical IRQ', cpu=owner, line=self.PHYSICAL_IRQ,
                              owner_enabled=bool(self.board.interrupts.banks[owner].enabled & (1 << self.PHYSICAL_IRQ)))

    def _finish(self, plan, status):
        self.regs[0x30] |= status
        self.request = None
        self.completed += 1
        self.history.append({key: plan[key] for key in ('word', 'command', 'argument', 'response')})
        del self.history[:-64]
        self.board.record('WiFi SDIO command complete', command=plan['command'],
                          card_state=self.state, status=self.regs[0x30])
        self._signal(plan['owner'])

    def tick(self, board):
        plan = self.request
        if plan is None or plan.get('pio_ready') or board.virtual_ticks < plan['due_tick']:
            return
        self._check(plan)
        owner = plan['owner']
        if plan['command'] == 53:
            if plan['write'] and self.board.read_memory(plan['dma'], plan['quantity']) != plan['payload']:
                raise UnsupportedDevice('WiFi bootstrap source DMA changed before physical completion')
        if 'vendor' in plan:
            self._vendor_complete(plan['vendor'])
        if plan['command'] == 53 and not plan.get('pio'):
            if not plan['write']:
                self.board.write_memory(plan['dma'], plan['vendor']['response'])
            self.regs[6] = 0
        self.response = plan['response'].to_bytes(4, 'little') + bytes(12)
        self.state = plan['next_state']
        if plan.get('pio'):
            # Command IRQ precedes the real caller's ACK/configuration and
            # read20. No DMA or transfer-complete is supplied before that read.
            plan['pio_ready'] = True
            self.regs[0x30] |= 1
            self.board.record('WiFi SDIO PIO command complete', address=0x10, quantity=4)
            self._signal(owner)
            return
        if plan['update'] is not None:
            address, data = plan['update']
            self.card_registers[address] = data
            if address == 2:
                self.card_registers[3] = data  # Declared immediate function-ready response.
            elif address == 6 and data == 8:
                self.card_registers[2] = self.card_registers[3] = self.card_registers[4] = 0
                self.card_registers[6] = 0
        self._finish(plan, 3 if plan['command'] == 53 else 1)

    def read(self, cpu, address, size):
        offset = self._offset(address, size)
        if offset == 0x24:
            if self.request is not None and self.request.get('pio_ready'):
                return 0x30800 if self.request['pio_reads'] == 0 else 0x30000
            return 0x30000 | (1 if self.request is not None else 0)
        if 16 <= offset < 32:
            return int.from_bytes(self.response[offset - 16:offset - 14], 'little')
        if offset == 0x20:
            plan = self.request
            if (plan is None or not plan.get('pio_ready')
                    or type(plan.get('pio_reads')) is not int or not 0 <= plan['pio_reads'] < 128
                    or self.regs[0x30] != (2 if plan['pio_reads'] else 0)
                    or self.regs[0x34] != 0xffff or self.regs[0x38] != 0):
                raise UnsupportedDevice('WiFi PIO read needs completed command ACK and stock data gates')
            self._check(plan)
            self._vendor_complete(plan['vendor'])
            # Byte-mode quantity is4, but whole1D7AFF0 passes retained512
            # as the drain size to1D7A48C. Only firstword is wire data;
            #127 additional empty-latch zeros are an explicit unmeasured
            # host fixture convention, not RX bytes or a512-byte transfer.
            value = int.from_bytes(plan['vendor']['response'], 'little') if plan['pio_reads'] == 0 else 0
            plan['pio_reads'] += 1
            self.regs[0x30] |= 2
            if plan['pio_reads'] == 128:
                self.regs[6] = 0
                self._finish(plan, 2)
            return value
        if offset == 12 and size == 4:
            return self.regs[12] | self.regs[14] << 16
        return self.regs[offset]

    def write(self, cpu, address, size, value):
        offset = self._offset(address, size)
        if type(value) is not int or not 0 <= value < 1 << (8 * size):
            raise UnsupportedDevice('WiFi SDIO value exceeds its access width')
        if offset == 0x24 or 16 <= offset < 32:
            raise UnsupportedDevice('WiFi SDIO present-state/response registers are read-only')
        if offset == 0x20:
            raise UnsupportedDevice('WiFi SDIO PIO/vendor data is not qualified')
        if offset in (0x30, 0x32):
            self.regs[offset] &= ~value
            return
        if offset == 0x2e:
            if value & ~0x70f:
                raise UnsupportedDevice('WiFi SDIO timeout/reset contains unsupported bits')
            if value & 0x700:
                self.generation += 1
                self.resets += 1
                self.request = None
                self.regs[0x30] = self.regs[0x32] = 0
                self.response = bytes(16)
            self.regs[offset] = value & 15  # Declared instantaneous host reset self-clear.
            return
        if offset in (12, 14):
            if offset == 12 and size == 2:
                if self.request is not None:
                    raise UnsupportedDevice('WiFi SDIO transfer mode changed while pending')
                self.regs[12] = value
            else:
                self._start(value if size == 4 else self.regs[12] | value << 16)
            return
        if self.request is not None:
            # Whole1D79FF8 tears down command IRQ gates, then1D7B1AC
            # enables polled data status and disables signals before read20.
            if self.request.get('pio_ready'):
                normal, error, signal, error_signal = self.request['pio_gates']
                # Whole1D792C8 masks async bits,1D79194 clears command
                # bits and restores the existing card-IRQ100 subscription.
                masked = normal & 0xfe3f & 0xffc4
                allowed = {0x34: {0, 0x100, 0xffff, normal, normal & 0xfe3f, masked, masked | 0x100},
                           0x36: {0, error}, 0x38: {0, 0x100, signal}, 0x3a: {0, error_signal}}
                if offset in allowed and value in allowed[offset]:
                    self.regs[offset] = value
                    return
            raise UnsupportedDevice('WiFi SDIO configuration changed while command pending')
        if (offset == 0x28 and value & ~0xfff or offset == 4 and value & 0x8000
                or offset == 0x3e and value != 0x4000
                or offset == 0x100 and value & ~7 or offset == 0x104 and value):
            raise UnsupportedDevice('WiFi SDIO unsupported host configuration')
        self.regs[offset] = value

    def snapshot(self):
        return dict(name='WiFiSDIOController', profile=self.profile, state=self.state,
                    pending=self.busy, started=self.started, completed=self.completed,
                    irqs=self.irqs, resets=self.resets, history=list(self.history),
                    **({'vendor': dict(self.vendor)} if hasattr(self, 'vendor') else {}),
                    limits=['Declared one-function SDIO enumeration/CCCR, not measured X70 WiFi hardware.',
                            'Clock2C remains BootControls; commands drive real IRQ79 when signaled.',
                            'Enumeration refuses vendor/DMA. Bootstrap accepts only pinned ROM frames and phase registers; CIS/RX/network packets remain strict.'])
