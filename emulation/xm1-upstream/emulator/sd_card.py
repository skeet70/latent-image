"""Bounded stock SD0 host transactions with a declared blank SDHC card.

The frozen 9B208C/9B0868 driver supplies command words, arguments, real DMA
addresses and IRQ enables. No task, guest callback, UI field or filesystem
status selects completion. Identity, capacity, zero-filled media and latency
are explicit virtual inputs, not measured X70/card electrical behavior.

BootControls retains the existing FFF5002C clock register. This owner covers
SD0 only; stock SDMA and finite word-fed PIO sector reads/writes are bounded.
MMC, UHS/tuning and erase remain strict stops.
"""
from .board import RAM_SIZE
from .hardware import UnsupportedDevice


BLANK_SDHC_PROFILE = 'declared-blank-sdhc-default-speed-unmeasured'
PROFILES = (BLANK_SDHC_PROFILE,)


class SparseCard:
    """512-byte sectors; absent entries read as zeros without a large image."""
    def __init__(self, sectors=0x800000, *, max_dirty_sectors=65536):
        if (type(sectors) is not int or not 0x400000 <= sectors <= 0x4000000
                or sectors % 1024 or type(max_dirty_sectors) is not int
                or not 1 <= max_dirty_sectors <= 65536):
            raise ValueError('SDHC capacity must be 2..32 GiB in 1024-sector units; dirty bound 1..65536')
        self.sectors = sectors
        self.max_dirty_sectors = max_dirty_sectors
        self.data = {}

    def _range(self, first, count):
        if (type(first) is not int or type(count) is not int or first < 0
                or not 1 <= count <= 65536 or first + count > self.sectors):
            raise UnsupportedDevice('SD sector range exceeds the declared card/transaction bound')

    def read(self, first, count):
        self._range(first, count)
        return b''.join(self.data.get(sector, bytes(512)) for sector in range(first, first + count))

    def _updates(self, first, payload):
        """Preflight a sector update without partially changing sparse media."""
        if not isinstance(payload, bytes) or len(payload) % 512:
            raise ValueError('SD fixture payload must contain whole byte sectors')
        count = len(payload) // 512
        self._range(first, count)
        updates = {first + index: payload[index * 512:(index + 1) * 512]
                   for index in range(count)}
        retained = set(self.data) - set(updates)
        if len(retained) + sum(bool(any(block)) for block in updates.values()) > self.max_dirty_sectors:
            raise UnsupportedDevice('SD sparse dirty-sector bound exceeded')
        return updates

    def write(self, first, payload):
        """Apply whole sectors atomically, including a bounded guest SDMA chunk."""
        updates = self._updates(first, payload)
        for sector, block in updates.items():
            if any(block):
                self.data[sector] = block
            else:
                self.data.pop(sector, None)


def load_raw_image(card, path):
    """Copy a raw card image's nonzero sectors into a SparseCard (host fixture).

    For declared test media such as a FAT32 card with photos, built by a host
    formatter. Returns provenance for the run record; zero sectors stay sparse.
    """
    import hashlib
    digest = hashlib.sha256()
    loaded = 0
    with open(path, 'rb') as image:
        first = 0
        while True:
            chunk = image.read(512 * 2048)
            if not chunk:
                break
            digest.update(chunk)
            if len(chunk) % 512:
                raise ValueError('Card image must contain whole 512-byte sectors')
            # Empty FAT32 fixtures are almost entirely zero. Compare in C
            # before allocating millions of Python sector slices; every byte
            # still contributes to the logical image digest above.
            if chunk != bytes(len(chunk)):
                for index in range(len(chunk) // 512):
                    block = chunk[index * 512:(index + 1) * 512]
                    if block != bytes(512):
                        card.write(first + index, block)
                        loaded += 1
            first += len(chunk) // 512
    if first > card.sectors:
        raise ValueError('Card image is larger than the declared card capacity')
    return {'path': str(path), 'sha256': digest.hexdigest(), 'sectors': first, 'nonzero_sectors': loaded}


class SDCardController:
    BASE = 0xfff50000
    PHYSICAL_IRQ = 78  # decimal 78 / 0x4E; logical 0x4C at AEAFE0+0x4C*12.
    HALFWORDS = frozenset((4, 6, 8, 10, 12, 14, *range(16, 32, 2),
                          0x28, 0x2e, 0x30, 0x32, 0x34, 0x36, 0x38, 0x3a, 0x3e, 0x100, 0x104))
    WORDS = frozenset((0, 12, 0x20, 0x24))
    # Whole command/transfer words emitted by the recovered SD driver. These
    # are per-command grammar, not an entire boot trace or guest-PC whitelist.
    COMMANDS = {0: 0, 2: 0x02090000, 3: 0x031a0000, 7: 0x071b0000,
                8: 0x081a0000, 9: 0x09090000, 12: 0x0cdb0000,
                13: 0x0d1a0000, 16: 0x101a0000, 18: 0x123a0037, 25: 0x193a0027,
                55: 0x371a0000, 6: 0x063a0011}
    APPLICATION = {6: 0x061a0000, 13: 0x0d3a0011, 41: 0x29020000,
                   42: 0x2a1a0000, 51: 0x333a0011}

    def __init__(self, board, controls, *, profile=None, transfer_ticks=100,
                 sectors=0x800000):
        if profile not in (None, *PROFILES):
            raise ValueError('Unknown SD card profile')
        if type(transfer_ticks) is not int or transfer_ticks < 1:
            raise ValueError('SD transfer ticks must be a positive virtual delay')
        if not callable(getattr(controls, 'gpio_input_pin', None)):
            raise ValueError('SD card requires its explicit external GPIO owner')
        self.board, self.controls = board, controls
        self.profile, self.transfer_ticks = profile, transfer_ticks
        self.card = SparseCard(sectors)
        self.regs = {offset: 0 for offset in self.HALFWORDS | self.WORDS}
        self.state, self.rca, self.block_length, self.bus_width = 'idle', 0x1234, 512, 1
        self.application_pending = False
        self.generation = self.started = self.completed = self.irqs = self.dma_boundaries = 0
        self.request = None
        self.history = []
        self.response = bytes(16)

    @property
    def busy(self):
        # Same-engine checkpoint preflight also refuses a drained SDMA
        # boundary that still needs a real firmware ACK/resume transaction.
        return self.request is not None

    def contains(self, address, size):
        # No overlap with the existing BootControls halfword clock owner.
        if address < self.BASE + 0x2e and address + size > self.BASE + 0x2c:
            return False
        return (self.BASE <= address and address + size <= self.BASE + 0x40
                or self.BASE + 0x100 <= address and address + size <= self.BASE + 0x108)

    def _offset(self, address, size):
        offset = address - self.BASE
        if not (size == 2 and offset in self.HALFWORDS or size == 4 and offset in self.WORDS):
            raise UnsupportedDevice(f'Unknown SD0 register/access {address:#x}/{size}')
        return offset

    def _present(self):
        return self.controls.gpio_input_pin(12, 7) == 0

    def _physical(self, address, length):
        physical = address & 0x3fffffff if 0x40000000 <= address < 0x60000000 else address
        if address & 3 or not 0 <= physical <= RAM_SIZE - length or not 1 <= length <= 0x2000000:
            raise UnsupportedDevice('SD DMA address/quantity exceeds aligned modeled DRAM')
        return physical

    def _r1(self, *, app=False):
        state = {'idle': 0, 'ready': 1, 'identification': 2, 'standby': 3, 'transfer': 4}[self.state]
        return state << 9 | (0x100 if self.state == 'transfer' else 0) | (0x20 if app else 0)

    def _csd(self):
        # SDHC C_SIZE at bits69..48, READ_BL_LEN9, no write protection.
        value = (1 << 126 | 0x0e << 112 | 0x32 << 96 | 0x515 << 84
                 | 9 << 80 | (self.card.sectors // 1024 - 1) << 48
                 | 1 << 46 | 0x7f << 39 | 9 << 22 | 1)
        return (value >> 8).to_bytes(16, 'little')

    def _route(self):
        controller = getattr(self.board, 'interrupts', None)
        if controller is None:
            raise UnsupportedDevice('SD command requires the physical IRQ controller')
        owners = [cpu for cpu, bank in enumerate(controller.banks)
                  if bank.enabled & (1 << self.PHYSICAL_IRQ)]
        if len(owners) != 1:
            raise UnsupportedDevice('SD IRQ78 requires one initially enabled CPU owner')
        owner = owners[0]
        binding = self._binding(owner)
        # X70 1.10 registers handler 0x9b0eb4, X-M1 1.50 0x6f1cf0; the handler
        # is per-firmware and recorded, the source/mode/priority are pinned.
        if binding[owner][:4] + binding[owner][5:] != (0, 0, 0, 2, 12):
            raise UnsupportedDevice('SD IRQ78 requires recovered source0/mode0/priority12')
        if binding[owner] != getattr(self, '_recorded_binding', None):
            self._recorded_binding = binding[owner]
            self.board.record('SD IRQ78 binding', cpu=owner, source=binding[owner][0], mode=binding[owner][1:4],
                              handler=hex(binding[owner][4]), priority=binding[owner][5])
        return owner, binding

    def _binding(self, owner):
        line = self.PHYSICAL_IRQ
        group, shift = line // 32, line % 16 * 2
        mode = (0x140 + line // 16 * 4, 0x180 + line // 16 * 4)
        vector, priority = 0x400 + line * 4, 0x800 + line * 4
        result = []
        for cpu, bank in enumerate(self.board.interrupts.banks):
            required = (0x60 + group * 4, *mode, vector, priority)
            if cpu == owner and (any(offset not in bank.config for offset in required)
                    or type(bank.config[vector]) is not int or bank.config[vector] <= 0
                    or bank.config[vector] & 3):
                raise UnsupportedDevice('SD IRQ owner lacks complete source/vector/mode programming')
            result.append(((bank.config.get(0x60 + group * 4, 0) >> (line % 32)) & 1,
                           bank.source_config >> line & 1,
                           *(None if offset not in bank.config else bank.config[offset] >> shift & 3
                             for offset in mode), bank.config.get(vector), bank.config.get(priority)))
        return tuple(result)

    def _check_request(self, request):
        if self.profile != request['profile'] or not self._present():
            raise UnsupportedDevice('SD card/profile changed during an active transaction')
        if self.controls.registers.get(self.BASE + 0x2c, 0) != request['clock']:
            raise UnsupportedDevice('SD clock changed during an active transaction')
        if self._binding(request['owner']) != request['binding']:
            raise UnsupportedDevice('SD IRQ source/vector/priority binding changed during a transaction')
        if any(cpu != request['owner'] and bank.enabled & (1 << self.PHYSICAL_IRQ)
               for cpu, bank in enumerate(self.board.interrupts.banks)):
            raise UnsupportedDevice('SD transaction acquired a competing IRQ owner')
        if request['command'] == 25 and self.controls.gpio_input_pin(12, 6):
            raise UnsupportedDevice('SD write became protected during an active transaction')

    def _irq(self, request):
        normal = self.regs[0x30] & self.regs[0x34] & self.regs[0x38]
        error = self.regs[0x32] & self.regs[0x36] & self.regs[0x3a]
        if normal or error:
            self.board.queue_irq(request['owner'], self.PHYSICAL_IRQ)
            self.irqs += 1
            self.board.record('SD0 physical IRQ', cpu=request['owner'], line=self.PHYSICAL_IRQ,
                              status=self.regs[0x30], error=self.regs[0x32],
                              owner_enabled=bool(self.board.interrupts.banks[request['owner']].enabled &
                                                 (1 << self.PHYSICAL_IRQ)))

    def _plan(self, word, argument):
        command = word >> 24 & 0x3f
        application = self.application_pending and command != 55
        expected = (self.APPLICATION if application else self.COMMANDS).get(command)
        pio = not application and (command, word) in ((25, 0x193a0026), (18, 0x123a0036))
        if expected != word and not pio:
            raise UnsupportedDevice(f'Unsupported SD {"ACMD" if application else "CMD"}{command} word {word:#x}')
        response, next_state, payload = self._r1(), self.state, None
        next_app, block_length, width = False, self.block_length, self.bus_width
        if command == 0:
            if argument:
                raise UnsupportedDevice('SD CMD0 requires argument0')
            response, next_state, block_length, width = 0, 'idle', 512, 1
        elif command == 8:
            if self.state != 'idle' or argument != 0x1aa:
                raise UnsupportedDevice('SDHC CMD8 requires idle/voltage-pattern1AA')
            response = 0x1aa
        elif command == 55:
            if argument not in (0, self.rca << 16) or self.state not in ('idle', 'standby', 'transfer'):
                raise UnsupportedDevice('SD CMD55 has unsupported RCA/card state')
            response, next_app = self._r1(app=True), True
        elif application and command == 41:
            if self.state != 'idle' or argument != 0x41100000:
                raise UnsupportedDevice('SDHC ACMD41 requires recovered HCS/OCR request')
            response, next_state = 0xc0100000, 'ready'  # ready/CCS, no S18A.
        elif command == 2:
            if self.state != 'ready' or argument:
                raise UnsupportedDevice('SD CMD2 requires ready/argument0')
            cid = int.from_bytes(b'\xfeMCVIRT0\x10\x12\x34\x56\x78\x01\xaa\xff', 'big')
            response, next_state = (cid >> 8).to_bytes(16, 'little'), 'identification'
        elif command == 3:
            if self.state != 'identification' or argument:
                raise UnsupportedDevice('SD CMD3 requires identification/argument0')
            response, next_state = self.rca << 16 | 3 << 9, 'standby'
        elif command == 9:
            if self.state != 'standby' or argument != self.rca << 16:
                raise UnsupportedDevice('SD CMD9 requires standby/assigned RCA')
            response = self._csd()
        elif command == 7:
            if self.state not in ('standby', 'transfer') or argument not in (0, self.rca << 16):
                raise UnsupportedDevice('SD CMD7 requires assigned RCA or deselection')
            next_state = 'transfer' if argument else 'standby'
            response = 0x900 if argument else 0x600
        else:
            if self.state != 'transfer':
                raise UnsupportedDevice('SD selected-card command before CMD7')
            if command == 16:
                if argument not in (8, 64, 512):
                    raise UnsupportedDevice('SD recovered block lengths are8/64/512')
                block_length = argument
            elif application and command == 42:
                if argument:
                    raise UnsupportedDevice('SD ACMD42 only disconnects card-detect pull-up')
            elif application and command == 6:
                if argument != 2:
                    raise UnsupportedDevice('SD ACMD6 only selects recovered four-bit mode')
                width = 4
            elif application and command == 13:
                if argument or self.block_length != 64 or self.bus_width != 4:
                    raise UnsupportedDevice('SD ACMD13 requires four-bit/64-byte status request')
                payload = b'\x80' + bytes(9) + b'\x70' + bytes(53)
            elif application and command == 51:
                if argument or self.block_length != 8:
                    raise UnsupportedDevice('SD ACMD51 requires eight-byte SCR request')
                payload = bytes.fromhex('0205000000000000')  # SD_SPEC2, one/four-bit, erased zeros.
            elif command == 6:
                if argument != 1 or self.block_length != 64:
                    raise UnsupportedDevice('SD CMD6 only supports default-speed capability check')
                data = bytearray(64)
                data[13] = 1  # default speed supported; high-speed bit1 absent.
                payload = bytes(data)
            elif command in (18, 25):
                if self.block_length != 512:
                    raise UnsupportedDevice('SD sector transaction before restored512-byte block length')
                if command == 25 and self.controls.gpio_input_pin(12, 6):
                    raise UnsupportedDevice('SD sector write requires an unprotected declared card')
                count = self.regs[6] or 65536
                self.card._range(argument, count)
                payload = ('sector-write' if command == 25 else 'sectors', argument, count)
            elif command == 12:
                if argument:
                    raise UnsupportedDevice('SD CMD12 requires argument0')
            elif command == 13:
                if argument != self.rca << 16:
                    raise UnsupportedDevice('SD CMD13 requires assigned RCA')
            else:
                raise UnsupportedDevice('SD command class not implemented')
        plan = dict(command=command, application=application, argument=argument,
                    response=response, next_state=next_state, next_app=next_app,
                    block_length=block_length, bus_width=width, payload=payload)
        if pio:
            plan['pio'] = {'state': 'command-pending', 'buffer': b''}
        return plan

    def _start(self, word):
        if self.profile is None:
            raise UnsupportedDevice('SD command requires an explicit virtual-card profile')
        if not self._present() or self.request is not None:
            raise UnsupportedDevice('SD command while absent or prior transaction pending')
        clock = self.controls.registers.get(self.BASE + 0x2c, 0)
        if clock & 5 != 5 or not self.regs[0x28] & 1 << 8:
            raise UnsupportedDevice('SD command before stock clock and host-power setup')
        argument = self.regs[8] | self.regs[10] << 16
        plan = self._plan(word, argument)
        data = plan['payload'] is not None
        expected_status = 2 if data else 1
        # A command without data may run with its completion masked: X-M1 1.50
        # sends CMD12 with 0x34/0x38=0xee02 and polls present-state inhibit.
        if ((data and not self.regs[0x34] & self.regs[0x38] & expected_status)
                or self.regs[0x30] or self.regs[0x32]):
            raise UnsupportedDevice('SD command requires cleared status and enabled completion signal')
        owner, binding = self._route()
        quantity = 0
        if data:
            quantity = (plan['payload'][2] * 512 if isinstance(plan['payload'], tuple)
                        else len(plan['payload']))
            if self.regs[4] & 0xfff != (512 if plan['command'] in (18, 25) else quantity):
                raise UnsupportedDevice('SD block-size differs from the command payload')
            if self.regs[0x28] & 0x18:
                raise UnsupportedDevice('SD DMA mode is not the recovered SDMA path')
            if 'pio' in plan:
                signals = 0x13 if plan['command'] == 25 else 0x23
                if self.regs[0x34] & self.regs[0x38] & signals != signals:
                    raise UnsupportedDevice('SD PIO requires command/data/directional buffer-ready signals')
                # PIO write bytes arrive later through the data register.
                # Reserve the worst-case nonzero sector count up front, so
                # sparse capacity cannot fail after earlier sectors commit.
                if plan['command'] == 25:
                    first, count = plan['payload'][1:]
                    outside = sum(not first <= sector < first + count for sector in self.card.data)
                    if outside + count > self.card.max_dirty_sectors:
                        raise UnsupportedDevice('SD PIO write exceeds reserved sparse dirty-sector bound')
            else:
                physical = self._physical(self.regs[0], quantity)
                boundary = 4096 << (self.regs[4] >> 12 & 7)
                if (quantity > boundary - self.regs[0] % boundary
                        and not self.regs[0x34] & self.regs[0x38] & 8):
                    raise UnsupportedDevice('SDMA boundary crossing requires enabled DMA interrupt')
            if plan['command'] == 25 and 'pio' not in plan:
                # 9B1564 chooses the SDMA word only for this actual alignment;
                # its alternative 193A0026 feeds bytes through FFF50020.
                alignment_mask = 7 if self.regs[0] & 0x40000000 else 31
                if self.regs[0] & alignment_mask:
                    raise UnsupportedDevice('SD sector-write address is outside stock SDMA alignment')
                source = bytes(self.board.read_memory(physical, quantity))
                if len(source) != quantity:
                    raise UnsupportedDevice('SD sector-write source is truncated')
                # Refuse a known whole-job capacity violation before any card
                # byte, status, IRQ or scheduled completion changes.
                self.card._updates(plan['argument'], source)
        self.generation += 1
        plan.update(word=word, owner=owner, binding=binding, profile=self.profile,
                    generation=self.generation, clock=clock, quantity=quantity, cursor=0,
                    dma_start=self.regs[0], boundary_wait=False)
        self.request = plan
        self.regs[12], self.regs[14] = word & 0xffff, word >> 16
        self.started += 1
        self.board.record('SD0 command start', command=plan['command'], application=plan['application'],
                          argument=argument, word=word, bytes=quantity, profile=self.profile,
                          latency='explicit unmeasured virtual ticks')
        self._schedule(plan)

    def _schedule(self, request):
        def complete():
            if self.request is not request or request['generation'] != self.generation:
                return  # A real controller reset cancels its scheduled transaction.
            self._check_request(request)
            if 'pio' in request:
                # Command completion is separate from data completion. The
                # real ISR/W1C and 9B0CF0 inspect directional readiness before
                # 128 words fill or drain a sector; no guest RAM is read here.
                self.response = request['response'].to_bytes(4, 'little') + bytes(12)
                request['pio']['state'] = 'buffer-ready'
                if request['command'] == 18:
                    request['pio']['buffer'] = self.card.read(request['argument'], 1)
                self.regs[0x30] |= 0x11 if request['command'] == 25 else 0x21
                self._irq(request)
                return
            quantity, cursor = request['quantity'], request['cursor']
            end = quantity
            if quantity:
                boundary = 4096 << (self.regs[4] >> 12 & 7)
                address = request['dma_start'] + cursor
                end = min(quantity, cursor + boundary - address % boundary)
                payload = request['payload']
                if request['command'] == 25:
                    data = bytes(self.board.read_memory(self._physical(address, end - cursor), end - cursor))
                    if len(data) != end - cursor:
                        raise UnsupportedDevice('SD sector-write source is truncated at DMA completion')
                    # The recovered SDMA boundary may split a sector. Preserve
                    # its other bytes instead of rounding the transfer up.
                    first = cursor // 512
                    count = (end + 511) // 512 - first
                    sectors = bytearray(self.card.read(payload[1] + first, count))
                    sectors[cursor % 512:cursor % 512 + len(data)] = data
                    self.card.write(payload[1] + first, bytes(sectors))
                elif isinstance(payload, tuple):
                    # A DMA boundary can split a sector. Read only the bounded
                    # covering sectors, then select bytes actually transferred.
                    first = cursor // 512
                    count = (end + 511) // 512 - first
                    data = self.card.read(payload[1] + first, count)[cursor % 512:cursor % 512 + end - cursor]
                else:
                    data = payload[cursor:end]
                if request['command'] != 25:
                    self.board.write_memory(self._physical(address, len(data)), data)
                request['cursor'] = end
                self.regs[0] = request['dma_start'] + end
            response = request['response']
            self.response = response if isinstance(response, bytes) else response.to_bytes(4, 'little') + bytes(12)
            if end < quantity:
                self.regs[0x30] |= 9  # command-complete + SDMA boundary interrupt.
                request['boundary_wait'] = True
                self.dma_boundaries += 1
                self._irq(request)
                return
            self._finish(request, 3 if quantity else 1)
        self.board.schedule(self.transfer_ticks, complete)

    def _finish(self, request, status):
        self.state, self.application_pending = request['next_state'], request['next_app']
        self.block_length, self.bus_width = request['block_length'], request['bus_width']
        self.regs[0x30] |= status
        self.request = None
        self.completed += 1
        self.history.append({key: request[key] for key in
                             ('command', 'application', 'argument', 'word', 'quantity')})
        del self.history[:-32]
        self.board.record('SD0 command complete', command=request['command'],
                          application=request['application'], bytes=request['quantity'],
                          card_state=self.state, status=self.regs[0x30])
        self._irq(request)

    def _pio_word(self, cpu, value):
        request = self.request
        if (request is None or 'pio' not in request or request['command'] != 25
                or cpu != request['owner']
                or request['pio']['state'] != 'buffer-ready'
                or self.regs[0x30] & 0x10):
            raise UnsupportedDevice('SD PIO word requires its ready owner and real buffer-status ACK')
        self._check_request(request)
        pio = request['pio']
        if len(pio['buffer']) >= 512 or request['cursor'] >= request['quantity']:
            raise UnsupportedDevice('SD PIO word exceeds its finite sector count')
        pio['buffer'] += value.to_bytes(4, 'little')
        if len(pio['buffer']) != 512:
            return
        pio['state'] = 'sector-pending'

        def complete():
            if self.request is not request or request['generation'] != self.generation:
                return  # Real host reset discards the unfinished sector.
            self._check_request(request)
            self.card.write(request['argument'] + request['cursor'] // 512, pio['buffer'])
            request['cursor'] += 512
            pio['buffer'] = b''
            if request['cursor'] == request['quantity']:
                self._finish(request, 2)
            else:
                pio['state'] = 'buffer-ready'
                self.regs[0x30] |= 0x10
                self._irq(request)
        self.board.schedule(self.transfer_ticks, complete)

    def _pio_read_word(self, cpu):
        request = self.request
        if (request is None or 'pio' not in request or request['command'] != 18
                or cpu != request['owner'] or request['pio']['state'] != 'buffer-ready'
                or self.regs[0x30] & 0x20):
            raise UnsupportedDevice('SD PIO read requires its ready owner and real buffer-status ACK')
        self._check_request(request)
        pio = request['pio']
        if len(pio['buffer']) < 4 or request['cursor'] >= request['quantity']:
            raise UnsupportedDevice('SD PIO read exceeds its finite sector count')
        value = int.from_bytes(pio['buffer'][:4], 'little')
        pio['buffer'] = pio['buffer'][4:]
        request['cursor'] += 4
        if not pio['buffer']:
            pio['state'] = 'sector-pending'

            def complete():
                if self.request is not request or request['generation'] != self.generation:
                    return
                self._check_request(request)
                if request['cursor'] == request['quantity']:
                    self._finish(request, 2)
                else:
                    pio['buffer'] = self.card.read(request['argument'] + request['cursor'] // 512, 1)
                    pio['state'] = 'buffer-ready'
                    self.regs[0x30] |= 0x20
                    self._irq(request)
            self.board.schedule(self.transfer_ticks, complete)
        return value

    def read(self, cpu, address, size):
        offset = self._offset(address, size)
        if offset == 0x24:
            inhibit = 0
            if self.request is not None:
                inhibit = 1 | (2 if self.request['quantity'] or self.request['command'] in (7, 12) else 0)
            ready = (1 << (10 if self.request['command'] == 25 else 11)
                     if self.request is not None and 'pio' in self.request
                     and self.request['pio']['state'] == 'buffer-ready' else 0)
            return 1 << 17 | (1 << 16 if self._present() else 0) | inhibit | ready
        if 16 <= offset < 32:
            return int.from_bytes(self.response[offset - 16:offset - 14], 'little')
        if offset == 0x20:
            return self._pio_read_word(cpu)
        if offset == 12 and size == 4:
            return self.regs[12] | self.regs[14] << 16
        return self.regs[offset]

    def write(self, cpu, address, size, value):
        offset = self._offset(address, size)
        if type(value) is not int or not 0 <= value < 1 << (8 * size):
            raise UnsupportedDevice('SD register value exceeds its access width')
        if offset == 0x24 or 16 <= offset < 32:
            raise UnsupportedDevice('SD present-state/response registers are read-only')
        if offset == 0x20:
            self._pio_word(cpu, value)
            return
        if offset in (0x30, 0x32):
            self.regs[offset] &= ~value  # Real driver status ACK is W1C.
            return
        if offset == 0x2e:
            if value & ~0x70f:
                raise UnsupportedDevice('SD timeout/reset contains unsupported bits')
            if value & 0x700:
                self.generation += 1
                self.request = None
                self.regs[0x30] = self.regs[0x32] = 0
                # Reset settling is an explicit instantaneous fixture.
            self.regs[offset] = value & 0xf
            return
        if offset in (12, 14):
            if offset == 12 and size == 2:
                if self.request is not None:
                    raise UnsupportedDevice('SD transfer mode changed during a command')
                self.regs[12] = value
            else:
                self._start(value if size == 4 else self.regs[12] | value << 16)
            return
        if offset == 0 and self.request is not None:
            request = self.request
            if not request['boundary_wait'] or value != self.regs[0] or self.regs[0x30] & 8:
                raise UnsupportedDevice('SDMA resume requires acknowledged boundary/current DMA address')
            self._check_request(request)
            request['boundary_wait'] = False
            self._schedule(request)
            return
        if self.request is not None:
            raise UnsupportedDevice('SD command configuration changed while transaction pending')
        if offset == 0x28 and value & ~0xfff:
            raise UnsupportedDevice('SD host control contains unsupported bits')
        if offset == 4 and value & 0x8000:
            raise UnsupportedDevice('SD block-size contains an unsupported reserved bit')
        if offset == 0x3e and value or offset == 0x100 and value & ~7 or offset == 0x104 and value:
            raise UnsupportedDevice('SD UHS/tuning/unknown vendor mode is not supported')
        self.regs[offset] = value

    def snapshot(self):
        return {'name': 'SDCardController', 'profile': self.profile, 'present': self._present(),
                'state': self.state, 'sectors': self.card.sectors, 'dirty_sectors': len(self.card.data),
                'block_length': self.block_length, 'bus_width': self.bus_width,
                'started': self.started, 'completed': self.completed, 'irqs': self.irqs,
                'dma_boundaries': self.dma_boundaries, 'pending': self.request is not None,
                'history': list(self.history),
                'limits': ['Declared blank/default-speed SDHC; identity/capacity/latency are unmeasured inputs.',
                           'Clock remains owned by BootControls; physical GPIO CARD input is caller-owned.',
                           'Aligned CMD25 copies actual guest RAM into bounded sparse sectors; partial SDMA boundaries require real W1C ACK/resume. Completion latency and physical sampling are unmeasured.',
                           'CMD25 PIO admits only 193A0026, finite 512-byte sectors and owner-fed little-endian words after real W1C. Each sector has declared latency; partial sectors are not committed.',
                           'CMD18 PIO admits only 123A0036, finite 512-byte sectors and actual owner word reads after buffer-ready W1C; each sector has declared latency.',
                           'MMC/UHS/tuning/erase are unsupported.',
                           'Mount, FORMAT, a captured image/JPEG and stock file creation require separate runtime validation.']}
