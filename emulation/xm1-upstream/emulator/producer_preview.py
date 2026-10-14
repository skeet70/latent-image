"""One stock92CC5C idle DMA stop after the completed preview GLOBAL reset.

The already-zero channel/control registers are retained through the complete
ten-access driver transaction. This creates no transfer, IRQ, result or new
producer lifetime, and keeps the earlier arm, histogram and shutdown ledgers.
"""
from copy import copy
from .hardware import UnsupportedDevice
from . import histogram_protocol as hp


class PreviewProducerStopMixin:
    PREVIEW_IDLE_STOP = (
        ('write', 0x884, 0), ('read', 0x890, 0),
        ('write', 0x984, 0), ('read', 0x990, 0),
        ('read', 0x880, 0), ('write', 0x880, 0),
        ('read', 0x980, 0), ('write', 0x980, 0),
        ('read', 0, 0), ('write', 0, 0),
    )

    def _preview_history_view(self, record):
        """A read-only prior-register view for checking the frozen old lifetime.

        The real device and RAM keep current words. Prior words are used only
        by existing validators, so no old arm or recorded result is replayed.
        """
        view = copy(self)
        view.registers = dict(self.registers)
        request = record.get('request')
        if request is not None:
            if request.get('close') is not None:
                view.registers.update({int(a,16):w for a,w in request['close']['prior_words'].items()})
            if request.get('data') is not None:
                view.registers.update({int(a,16):w for a,w in request['data']['prior_ack_words'].items()})
            view.registers.update({int(a, 16): word for a, word in request['prior_words'].items()})
            # The old close required IRQ2E absent. A separately validated new
            # registration must not rewrite that historical ownership fact.
            view.board = copy(self.board)
            view.board.interrupts = copy(self.board.interrupts)
            view.board.interrupts.banks = [copy(bank) for bank in self.board.interrupts.banks]
            for bank in view.board.interrupts.banks:
                bank.enabled &= ~(1 << 0x2e)
                if request.get('data') is not None:
                    # The separate current IRQ must not become an assertion
                    # about the archived released lifetime. This is a copied
                    # validation view, never a change to the real controller.
                    bank.pending &= ~(1 << 0x2e)
                    if bank.active==0x2e:bank.active=None
                    if bank.injected==0x2e:bank.injected=None
        view.registers.update({int(a, 16): word for a, word in record['prior_words'].items()})
        view.preview_configuration = None
        view._validate_rearm()
        view._validate_histogram()
        view._validate_post_shutdown_control()
        stop = view.preview_idle_stop
        if (type(stop) is not dict or stop.get('state') != 'completed'
                or type(stop.get('step')) is not int or stop['step'] != len(self.PREVIEW_IDLE_STOP)
                or type(stop.get('cpu')) is not int
                or stop.get('cpu') != view.post_shutdown_control['cpu']
                or stop.get('control_sha256') != hp.digest(view.post_shutdown_control)
                or stop.get('configuration_sha256') != view._config_hash()
                or stop.get('retained_configuration_sha256') != view._retained_config_hash()
                or type(stop.get('accesses')) is not list
                or any(type(e) is not dict or type(e.get('word')) is not int
                       or type(e.get('cpu')) is not int or type(e.get('step')) is not int
                       for e in stop['accesses'])
                or stop['accesses'] != [dict(operation=kind, address=hex(self.BASE + offset),
                    word=word, cpu=stop['cpu'], step=i)
                    for i, (kind, offset, word) in enumerate(self.PREVIEW_IDLE_STOP)]):
            raise UnsupportedDevice('Preview configuration requires the complete prior idle stop')
        return view

    def _validate_preview_configuration(self, record, core, size=4):
        if (type(record) is not dict or type(record.get('accesses')) is not list
                or len(record['accesses']) > self.REARM_KNOWN_CONFIG_LIMIT
                or type(record.get('cpu')) is not int or core != record['cpu']
                or type(core) is not int or size != 4
                or type(record.get('prior_words')) is not dict
                or type(record.get('current_words')) is not dict):
            raise UnsupportedDevice('Preview configuration owner or finite ledger changed')
        prior, current = record['prior_words'], {}
        for i, event in enumerate(record['accesses']):
            if (type(event) is not dict or set(event) != {'operation', 'address', 'cpu', 'step', 'old_word', 'word'}):
                raise UnsupportedDevice('Preview configuration access shape changed')
            a = event.get('address')
            try:target = int(a, 16) if type(a) is str else -1
            except ValueError:raise UnsupportedDevice('Preview configuration address changed') from None
            kind, old, word = event.get('operation'), event.get('old_word'), event.get('word')
            if (target not in self.KNOWN_DESIRED_CONFIG or a != hex(target)
                    or type(event.get('cpu')) is not int or event['cpu'] != record['cpu']
                    or type(event.get('step')) is not int or event['step'] != i
                    or type(old) is not int or old != current.get(a, prior.get(a))
                    or type(word) is not int or kind not in ('read', 'write')
                    or i < 3 and (kind != 'write' or target != self.IMAGE_GLOBAL_CONFIG_ORDER[i])
                    or kind == 'write' and not self._known_pair_valid(target, old, word)
                    or kind == 'read' and (target not in self.READABLE or word != old)):
                raise UnsupportedDevice('Preview configuration ordered mask/prior ledger changed')
            current[a] = word
        projection = dict(self.registers)
        if record.get('request') is not None:
            if record['request'].get('close') is not None:
                projection.update({int(a,16):w for a,w in record['request']['close']['prior_words'].items()})
            projection.update({int(a, 16): word for a, word in record['request']['prior_words'].items()})
        if (set(prior) != set(current) or current != record['current_words']
                or any(type(projection[int(a, 16)]) is not int
                       or projection[int(a, 16)] != word for a, word in current.items())):
            raise UnsupportedDevice('Preview configuration retained words differ from its ledger')
        view = self._preview_history_view(record)
        if core != view.post_shutdown_control['cpu']:
            raise UnsupportedDevice('Preview configuration differs from its prior lifetime owner')
        return view

    def _preview_configuration_access(self, operation, core, address, size, value):
        """Retain passive fields, then one separately owned stock request."""
        record = self.preview_configuration
        if record is None:
            if (operation != 'write' or address != self.IMAGE_GLOBAL_CONFIG_ORDER[0]
                    or self.preview_idle_stop is None
                    or self.preview_idle_stop.get('state') != 'completed'):
                return False
            record = dict(cpu=core, accesses=[], prior_words={}, current_words={})
        if self._preview_request_access(record, operation, core, address, size, value):return True
        self._validate_preview_configuration(record, core, size)
        step = len(record['accesses'])
        old = self.registers.get(address)
        actual = old if operation == 'read' else value
        if (step >= self.REARM_KNOWN_CONFIG_LIMIT
                or address not in self.KNOWN_DESIRED_CONFIG or type(old) is not int or type(actual) is not int
                or step < 3 and (operation != 'write' or address != self.IMAGE_GLOBAL_CONFIG_ORDER[step])
                or operation == 'write' and not self._known_pair_valid(address, old, actual)
                or operation == 'read' and address not in self.READABLE):
            raise UnsupportedDevice('Preview configuration excludes activation/results and requires audited passive fields')
        event = dict(operation=operation, address=hex(address), cpu=core, step=step,
                     old_word=old, word=actual)
        record['prior_words'].setdefault(hex(address), old)
        record['current_words'][hex(address)] = actual
        record['accesses'].append(event)
        if operation == 'write':
            self.registers[address] = actual
            self.writes.append(dict(cpu=core, address=hex(address), value=hex(actual), old=hex(old)))
        self.preview_configuration = record
        self.board.record('Preview producer configuration retained', **event,
                          limits='Audited passive fields only; prior lifetime preserved, no job/data/status/IRQ')
        return True

    def _preview_request_route(self, core, required=False, allow_inflight=False):
        interrupts = getattr(self.board, 'interrupts', None)
        if interrupts is None or len(interrupts.banks) != 2:
            raise UnsupportedDevice('Preview request requires the real interrupt controller')
        if any(type(bank.enabled) is not int or not 0 <= bank.enabled < 1 << 128 for bank in interrupts.banks):
            raise UnsupportedDevice('Preview request IRQ route has an untyped/out-of-range enable bank')
        targets = [cpu for cpu, bank in enumerate(interrupts.banks) if bank.enabled & (1 << 0x2e)]
        if not targets and not required:return None
        if targets != [core]:raise UnsupportedDevice('Preview request IRQ2E registration is missing/ambiguous/foreign')
        bank = interrupts.banks[core]
        if (type(getattr(bank,'config',None)) is not dict
                or type(getattr(bank,'source_config',None)) is not int or not 0 <= bank.source_config < 1 << 128
                or type(getattr(bank,'pending',None)) is not int or not 0 <= bank.pending < 1 << 128):
            raise UnsupportedDevice('Preview request IRQ registration has malformed typed fields')
        route = dict(cpu=core, vector=bank.config.get(0x400 + 4 * 0x2e),
                     priority=bank.config.get(0x800 + 4 * 0x2e),
                     source_configuration=(bank.source_config >> 0x2e) & 1)
        if (route != dict(cpu=core, vector=0x92c8cc, priority=13, source_configuration=1)
                or not allow_inflight and
                    (bank.pending & (1 << 0x2e) or bank.active == 0x2e or bank.injected == 0x2e)):
            raise UnsupportedDevice('Preview request differs from the stock idle IRQ2E registration')
        return route

    def _preview_request_sequence(self):
        return (tuple(('write', a, None) for a in self.REARM_DESCRIPTOR_ORDER)
                + tuple(('write', a, w) for a, w in self.DMA_SETUP[:-1])
                + (('read', self.BASE + 0x884, 0), ('write', self.BASE + 0x884, 1),
                   ('read', self.BASE + 4, self.PRODUCER_DISABLED_MASK),
                   ('read', self.BASE + 8, 0x10000),
                   ('write', self.BASE + 4, self.PRODUCER_SELECT_MASK),
                   ('write', self.BASE + 8, 0x10000)))

    @staticmethod
    def _preview_request_state(step):
        return ('descriptor-rebuild' if step < 10 else 'descriptor-ready' if step == 10
                else 'initializing' if step < 18 else 'armed-awaiting-fresh-input' if step == 18
                else 'desired-enable-retained' if step < 22 else 'enabled-awaiting-fresh-input')

    def _validate_preview_request(self, record):
        request = record['request']; sequence = self._preview_request_sequence()
        live = self
        close=request.get('close')
        if close is not None:
            self._validate_preview_close(request)
            live=copy(self);live.registers=dict(self.registers)
            live.registers.update({int(a,16):w for a,w in close['prior_words'].items()})
        data=request.get('data')
        if data is not None:
            from . import preview_statistics
            preview_statistics.validate(self,request,check_route=close is None or close['step']<8)
            if live is self:live=copy(self);live.registers=dict(self.registers)
            live.registers.update({int(a,16):w for a,w in data['prior_ack_words'].items()})
        if request.get('rotation') is not None:
            self._validate_preview_rotation(request)
            if live is self:live=copy(self);live.registers=dict(self.registers)
            live.registers.update({int(a,16):w for a,w in request['rotation']['prior_words'].items()})
        if (type(request) is not dict or type(request.get('cpu')) is not int
                or request['cpu'] != record['cpu'] or type(request.get('step')) is not int
                or not 1 <= request['step'] <= len(sequence)
                or request.get('state') != self._preview_request_state(request['step'])
                or type(request.get('accesses')) is not list or len(request['accesses']) != request['step']
                or type(request.get('prior_words')) is not dict
                or request.get('history_sha256') != hp.digest({k:v for k,v in record.items() if k != 'request'})
                or type(self.board.virtual_ticks) is not int or not 0 <= self.board.virtual_ticks < 1 << 64
                or self.busy is not False or self.dma_busy is not False
                or any(x is not None for x in (self.pending_data, self.pending_ack, self.pending_stop))):
            raise UnsupportedDevice('Preview request ownership/history/phase changed')
        current = {}; previous_tick = request.get('started_tick')
        if (type(previous_tick) is not int or not 0 <= previous_tick < 1 << 64
                or previous_tick != request['accesses'][0].get('tick')):
            raise UnsupportedDevice('Preview request start time changed')
        for step, event in enumerate(request['accesses']):
            kind, target, expected = sequence[step]
            if (type(event) is not dict or set(event) != {'operation','address','word','old_word','cpu','tick','step'}
                    or event['operation'] != kind or event['address'] != hex(target)
                    or type(event['cpu']) is not int or event['cpu'] != request['cpu']
                    or type(event['step']) is not int or event['step'] != step
                    or type(event['tick']) is not int or not previous_tick <= event['tick'] <= self.board.virtual_ticks
                    or type(event['word']) is not int or not 0 <= event['word'] <= 0xffffffff
                    or type(event['old_word']) is not int
                    or event['old_word'] != current.get(hex(target), request['prior_words'].get(hex(target)))
                    or (not self._config_valid(target,event['word']) if expected is None else event['word'] != expected)):
                raise UnsupportedDevice('Preview request ordered descriptor/setup/enable access changed')
            current[hex(target)] = event['word']; previous_tick = event['tick']
        if (set(request['prior_words']) != set(current) or request.get('current_words') != current
                or any(type(live.registers[int(a,16)]) is not int or live.registers[int(a,16)] != w
                       for a,w in current.items())
                or request.get('configuration_sha256') != live._config_hash()
                or request.get('retained_configuration_sha256') != live._retained_config_hash()
                or request.get('armed_tick') != (request['accesses'][17]['tick'] if request['step'] >= 18 else None)
                or request.get('enabled_tick') != (request['accesses'][21]['tick'] if request['step'] == 22 else None)):
            raise UnsupportedDevice('Preview request live words/timing differ from its ordered ledger')
        route = self._preview_request_route(request['cpu'], request['step'] >= 16 and (close is None or close['step']<8),
            allow_inflight=(data is not None and data.get('irq_tick') is not None))
        if route is None and close is not None and close['step']==8:route=request['route']
        if (request.get('route') != (route if request['step'] >= 16 else None)
                or request['step'] < 15 and route is not None):
            raise UnsupportedDevice('Preview request IRQ registration precedes initialization or changed')
        self._validate_preview_configuration(record, request['cpu'])

    def _preview_request_access(self, record, operation, core, address, size, value):
        request = record.get('request')
        if request is None and (operation, address) != ('write', self.REARM_DESCRIPTOR_ORDER[0]):return False
        if request is None:
            self._validate_preview_configuration(record, core)
            if (len(record['accesses']) < 3 or self.registers[self.GLOBAL] != 0x10030104
                    or self.busy is not False or self.dma_busy is not False
                    or any(x is not None for x in (self.pending_data,self.pending_ack,self.pending_stop))
                    or any(self.registers[self.BASE+o] != w for o,w in
                        ((4,self.PRODUCER_DISABLED_MASK),(8,0x10000),(0x884,0),(0x890,0)))
                    or self._preview_request_route(core) is not None):
                raise UnsupportedDevice('Preview descriptor requires the completed idle preview phase and absent IRQ2E')
            request = dict(cpu=core, step=0, state='descriptor-rebuild', accesses=[], prior_words={}, current_words={},
                history_sha256=hp.digest(record), route=None, started_tick=self.board.virtual_ticks,
                armed_tick=None, enabled_tick=None)
        else:
            self._validate_preview_request(record)
            if self._preview_close_access(request,operation,core,address,size,value):return True
            if request.get('data') is not None:
                from . import preview_statistics
                if preview_statistics.access(self,request,operation,core,address,size,value):return True
            if self._preview_rotation_access(request,operation,core,address,size,value):return True
            # Whole10CBDD8 queries packed desired fields after10CAEA0.
            # Pure getters return the retained word; they cannot clear status,
            # revise the request or replay its completed setup transactions.
            if (request['step'] == 22 and operation == 'read'
                    and type(core) is int and core == request['cpu'] and size == 4
                    and address in self.READABLE and address in self.KNOWN_DESIRED_CONFIG):
                return True
        sequence = self._preview_request_sequence();step = request['step']
        if (type(core) is not int or core != request['cpu'] or size != 4 or step >= len(sequence)):
            raise UnsupportedDevice('Preview request owner/WORD width/one-request budget changed')
        kind, target, expected = sequence[step]
        old = self.registers.get(address);word = old if operation == 'read' else value
        if (operation != kind or address != target or type(old) is not int or type(word) is not int
                or (not self._config_valid(target,word) if expected is None else word != expected)):
            raise UnsupportedDevice('Preview request requires the complete next descriptor/setup/enable access')
        tick = self.board.virtual_ticks
        if type(tick) is not int or not 0 <= tick < 1 << 64:
            raise UnsupportedDevice('Preview request requires bounded typed time')
        route = self._preview_request_route(core, step >= 15)
        if step < 15 and route is not None:
            raise UnsupportedDevice('Preview request registration precedes the five initialization writes')
        request['prior_words'].setdefault(hex(address), old)
        request['current_words'][hex(address)] = word
        request['accesses'].append(dict(operation=operation,address=hex(address),word=word,old_word=old,
                                       cpu=core,tick=tick,step=step))
        if operation == 'write':
            self.registers[address] = word
            self.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old)))
        request['step'] += 1;request['state'] = self._preview_request_state(request['step'])
        if step >= 15:request['route'] = route
        if request['step'] == 18:request['armed_tick'] = tick
        if request['step'] == 22:request['enabled_tick'] = tick
        request['configuration_sha256'] = self._config_hash()
        request['retained_configuration_sha256'] = self._retained_config_hash()
        record['request'] = request;self.preview_configuration = record
        self.board.record('Preview producer fresh request retained', **request['accesses'][-1], state=request['state'],
            limits='One stock descriptor/initialization/enable; earlier history unchanged, awaiting new input without data/ACK/IRQ replay')
        return True

    def _preview_rotation_epoch(self):
        devices=[d for d in self.board.devices if type(d).__name__=='InputProcessingControl']
        if len(devices)!=1 or getattr(devices[0],'board',None) is not self.board:
            raise UnsupportedDevice('Preview rotation requires one same-board input source')
        epoch=devices[0].preview_frame_epoch()
        if (type(epoch) is not dict or epoch.get('generation')!=6 or epoch.get('ordinal')!=2
                or epoch.get('attempt')!=11 or epoch.get('frame')!=4 or epoch.get('cpu')!=0
                or epoch.get('source')!=0 or type(epoch.get('asserted_tick')) is not int):
            raise UnsupportedDevice('Preview rotation requires the declared post-arm source0 frame')
        return epoch

    def _validate_preview_rotation(self,request):
        rotation=request['rotation'];order=self.REARM_DESCRIPTOR_ORDER[:-1]
        if (type(rotation) is not dict or type(rotation.get('step')) is not int
                or not 1<=rotation['step']<=len(order) or request['step']!=22
                or rotation.get('source_epoch')!=self._preview_rotation_epoch()
                or rotation.get('state')!=('completed' if rotation['step']==len(order) else 'in-progress')
                or type(rotation.get('cpu')) is not int or rotation['cpu']!=request['cpu']
                or type(rotation.get('accesses')) is not list or len(rotation['accesses'])!=rotation['step']
                or type(rotation.get('prior_words')) is not dict or type(rotation.get('current_words')) is not dict
                or rotation.get('request_sha256')!=hp.digest({k:v for k,v in request.items() if k not in ('rotation','data','close')})):
            raise UnsupportedDevice('Preview rotation identity, source or finite transaction changed')
        current={};prior={};previous=rotation['source_epoch']['asserted_tick']
        if previous<=request['enabled_tick']:
            raise UnsupportedDevice('Preview rotation source predates the fresh producer enable')
        for i,event in enumerate(rotation['accesses']):
            a=hex(order[i]);old=request['current_words'][a]
            if (type(event) is not dict or event.get('operation')!='write' or event.get('address')!=a
                    or type(event.get('cpu')) is not int or event['cpu']!=rotation['cpu']
                    or type(event.get('step')) is not int or event['step']!=i or event.get('old_word')!=old
                    or type(event.get('tick')) is not int or not previous<=event['tick']<=self.board.virtual_ticks
                    or not self._config_valid(order[i],event.get('word'))):
                raise UnsupportedDevice('Preview rotation ordered destination tuple changed')
            current[a]=event['word'];prior[a]=old;previous=event['tick']
        if (prior!=rotation['prior_words'] or current!=rotation['current_words']
                or any(type(self.registers[int(a,16)]) is not int or self.registers[int(a,16)]!=w for a,w in current.items())
                or rotation.get('configuration_sha256')!=self._config_hash()
                or rotation.get('retained_configuration_sha256')!=self._retained_config_hash()):
            raise UnsupportedDevice('Preview rotation live desired fields changed')

    def _preview_rotation_access(self,request,operation,core,address,size,value):
        rotation=request.get('rotation');order=self.REARM_DESCRIPTOR_ORDER[:-1]
        if rotation is None and (operation,address)!=('write',order[0]):return False
        if rotation is not None and rotation['step']==len(order):return False
        step=0 if rotation is None else rotation['step']
        if (request['step']!=22 or type(core) is not int or core!=request['cpu'] or size!=4
                or operation!='write' or address!=order[step] or not self._config_valid(address,value)):
            raise UnsupportedDevice('Preview rotation requires the whole next stock nine-destination transaction')
        epoch=self._preview_rotation_epoch()
        if epoch['asserted_tick']<=request['enabled_tick']:
            raise UnsupportedDevice('Preview rotation has no fresh post-enable input')
        if rotation is None:
            rotation=dict(cpu=core,step=0,state='in-progress',source_epoch=epoch,accesses=[],
                prior_words={},current_words={},request_sha256=hp.digest({k:v for k,v in request.items() if k not in ('data','close')}))
        old=self.registers[address];tick=self.board.virtual_ticks
        event=dict(operation='write',address=hex(address),word=value,old_word=old,cpu=core,tick=tick,step=step)
        rotation['prior_words'][hex(address)]=old;rotation['current_words'][hex(address)]=value
        rotation['accesses'].append(event);rotation['step']+=1
        rotation['state']='completed' if rotation['step']==len(order) else 'in-progress'
        self.registers[address]=value
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
        rotation['configuration_sha256']=self._config_hash()
        rotation['retained_configuration_sha256']=self._retained_config_hash()
        request['rotation']=rotation
        self.board.record('Preview destination rotation retained',**event,state=rotation['state'],
            limits='One whole10CC14C(mask3F) desired tuple after fresh input; no data/result/status/IRQ')
        return True

    def arm_preview_statistics(self,profile):
        """Select the separate bounded virtual statistics experiment."""
        from . import preview_statistics
        return preview_statistics.arm(self,profile)

    def _preview_close_candidates(self,prior,accesses,previous):
        #1203540 optionally passes the live160E4F8 maskD4 to10CB5C8;
        #11F9810 subsequently releases clock6 through10CAD2C(mask1E).
        families=('D4-zero','FC-zero') if not previous else ('FC-zero',)
        if previous and (len(previous)!=1 or previous[0]['family']!='D4-zero'
                         or previous[0]['state']!='completed'):return []
        return [family for family in families
                if len(accesses)<=len(self._post_shutdown_sequence(prior,family))
                and all((event['operation'],event['word'])==self._post_shutdown_sequence(prior,family)[i]
                        for i,event in enumerate(accesses))]

    def _validate_preview_close(self,request):
        """One10CB2F4(mask3F) close; conditional10CB5C8(D4),10CAD2C(FC)."""
        close=request['close'];data=request.get('data');now=self.board.virtual_ticks
        prior={hex(self.BASE+o):w for o,w in self.FRESH_SHUTDOWN_PRIOR.items()}
        prior[hex(self.GLOBAL)]=0x10030104
        if (type(close) is not dict or set(close)!={'cpu','step','state','accesses','prior_words',
                'current_words','data_sha256','started_tick','completed_tick','control'}
                or type(close.get('cpu')) is not int or close['cpu']!=request['cpu']
                or type(close.get('step')) is not int or not 1<=close['step']<=8
                or type(data) is not dict or data.get('state')!='acknowledged'
                or type(data.get('acknowledged_tick')) is not int
                or type(now) is not int or not 0<=now<1<<64
                or close.get('data_sha256')!=hp.digest(data) or close.get('prior_words')!=prior
                or type(close.get('accesses')) is not list or len(close['accesses'])!=close['step']
                or type(close.get('started_tick')) is not int
                or not data['acknowledged_tick']<=close['started_tick']<=now):
            raise UnsupportedDevice('Preview close requires the actual new consumed result and exact prior latches')
        current=dict(prior);previous=close['started_tick']
        for index,event in enumerate(close['accesses']):
            kind,offset,word=self.FRESH_SHUTDOWN_SEQUENCE[index];address=hex(self.BASE+offset)
            if (type(event) is not dict or set(event)!={'operation','address','word','cpu','tick','step'}
                    or event.get('operation')!=kind or event.get('address')!=address
                    or type(event.get('word')) is not int or event['word']!=word
                    or type(event.get('cpu')) is not int or event['cpu']!=close['cpu']
                    or type(event.get('step')) is not int or event['step']!=index
                    or type(event.get('tick')) is not int or not previous<=event['tick']<=now):
                raise UnsupportedDevice('Preview close whole desired-disable/DMA-release sequence changed')
            current[address]=word;previous=event['tick']
        completed=previous if close['step']==8 else None
        if close['completed_tick']!=completed:
            raise UnsupportedDevice('Preview close completion precedes the final F8 clear')
        self._preview_request_route(close['cpu'],required=close['step']<8,allow_inflight=True)
        control=close['control'];state='closing' if close['step']<8 else 'closed'
        if control is not None:
            if (close['step']!=8 or type(control) is not dict
                    or set(control)!={'step','state','requests','prior_word','current_word','close_sha256'}
                    or control.get('prior_word')!=prior[hex(self.GLOBAL)]
                    or control.get('close_sha256')!=hp.digest({k:v for k,v in close.items()
                        if k not in ('control','state','current_words')})
                    or type(control.get('requests')) is not list or not 1<=len(control['requests'])<=2
                    or type(control.get('step')) is not int
                    or any(bank.enabled&(1<<46) for bank in self.board.interrupts.banks)):
                raise UnsupportedDevice('Preview desired cleanup requires completed close and actual IRQ46 unregister')
            steps=0;prefix=[]
            for ordinal,record in enumerate(control['requests'],1):
                if (type(record) is not dict or set(record)!={'ordinal','step','state','accesses',
                        'prior_word','current_word','candidates','family'}
                        or type(record.get('ordinal')) is not int or record['ordinal']!=ordinal
                        or type(record.get('step')) is not int or not 1<=record['step']<=15
                        or record.get('prior_word')!=current[hex(self.GLOBAL)]
                        or type(record.get('accesses')) is not list or len(record['accesses'])!=record['step']):
                    raise UnsupportedDevice('Preview desired cleanup finite request ledger changed')
                for index,event in enumerate(record['accesses']):
                    if (type(event) is not dict or set(event)!={'operation','word','cpu','tick','step'}
                            or type(event.get('word')) is not int
                            or type(event.get('cpu')) is not int or event['cpu']!=close['cpu']
                            or type(event.get('step')) is not int or event['step']!=index
                            or type(event.get('tick')) is not int or not previous<=event['tick']<=now):
                        raise UnsupportedDevice('Preview desired cleanup getter/setter chronology changed')
                    current[hex(self.GLOBAL)]=event['word'];previous=event['tick']
                candidates=self._preview_close_candidates(record['prior_word'],record['accesses'],prefix)
                family=candidates[0] if len(candidates)==1 else None
                done=family is not None and record['step']==len(self._post_shutdown_sequence(record['prior_word'],family))
                if (not candidates or record['candidates']!=candidates or record['family']!=family
                        or record['state']!=('completed' if done else 'in-progress')
                        or record['current_word']!=current[hex(self.GLOBAL)]
                        or ordinal<len(control['requests']) and not done):
                    raise UnsupportedDevice('Preview desired cleanup mask/order/current word changed')
                steps+=record['step'];prefix.append(record)
            if (control['step']!=steps or control['state']!=prefix[-1]['state']
                    or control['current_word']!=current[hex(self.GLOBAL)]):
                raise UnsupportedDevice('Preview desired cleanup total phase changed')
            state='global-closed' if control['state']=='completed' else 'global-closing'
        if (close['state']!=state or close['current_words']!=current
                or any(type(self.registers.get(int(a,16))) is not int or self.registers[int(a,16)]!=word
                       for a,word in current.items())):
            raise UnsupportedDevice('Preview close live latches differ from its ordered history')

    def _preview_close_access(self,request,operation,core,address,size,value):
        close=request.get('close')
        if close is None:
            if (operation,address)!=('read',self.BASE+4):return False
            data=request.get('data')
            if type(data) is not dict or data.get('state')!='acknowledged':return False
            prior={hex(self.BASE+o):w for o,w in self.FRESH_SHUTDOWN_PRIOR.items()}
            prior[hex(self.GLOBAL)]=0x10030104
            if any(type(self.registers.get(int(a,16))) is not int or self.registers[int(a,16)]!=w for a,w in prior.items()):
                raise UnsupportedDevice('Preview close requires the exact enabled idle DMA latches')
            close=dict(cpu=request['cpu'],step=0,state='closing',accesses=[],prior_words=prior,
                current_words=dict(prior),data_sha256=hp.digest(data),started_tick=self.board.virtual_ticks,
                completed_tick=None,control=None)
        if close['step']<8:
            kind,offset,word=self.FRESH_SHUTDOWN_SEQUENCE[close['step']]
            actual=self.registers.get(address) if operation=='read' else value
            if (type(core) is not int or core!=close['cpu'] or type(size) is not int or size!=4
                    or (operation,address)!=(kind,self.BASE+offset) or type(actual) is not int or actual!=word):
                raise UnsupportedDevice('Preview close requires the whole next owner/WORD idle release access')
            event=dict(operation=operation,address=hex(address),word=actual,cpu=core,
                       tick=self.board.virtual_ticks,step=close['step'])
            close['accesses'].append(event);close['step']+=1;close['current_words'][hex(address)]=actual
            if close['step']==8:close.update(state='closed',completed_tick=self.board.virtual_ticks)
        else:
            if address!=self.GLOBAL:return False
            control=close['control']
            if (control is not None and control['state']=='completed'
                    and control['requests'][-1]['family']=='FC-zero'):return False
            if any(bank.enabled&(1<<46) for bank in self.board.interrupts.banks):
                raise UnsupportedDevice('Preview desired cleanup requires the real IRQ46 unregister')
            if control is None:
                control=dict(step=0,state='in-progress',requests=[],prior_word=close['prior_words'][hex(self.GLOBAL)],
                    current_word=close['prior_words'][hex(self.GLOBAL)],close_sha256=hp.digest({k:v for k,v in close.items()
                        if k not in ('control','state','current_words')}))
            beginning=not control['requests'] or control['state']=='completed'
            if beginning:
                record=dict(ordinal=len(control['requests'])+1,step=0,state='in-progress',accesses=[],
                    prior_word=control['current_word'],current_word=control['current_word'],candidates=[],family=None)
            else:record=control['requests'][-1]
            actual=self.registers[address] if operation=='read' else value
            if type(core) is not int or core!=close['cpu'] or type(size) is not int or size!=4 or type(actual) is not int:
                raise UnsupportedDevice('Preview desired cleanup requires the close owner and GLOBAL WORD')
            event=dict(operation=operation,word=actual,cpu=core,tick=self.board.virtual_ticks,step=record['step'])
            accesses=record['accesses']+[event]
            candidates=self._preview_close_candidates(record['prior_word'],accesses,
                control['requests'] if beginning else control['requests'][:-1])
            if not candidates:
                raise UnsupportedDevice('Preview desired cleanup requires optional D4 then one FC zero-mode transaction')
            family=candidates[0] if len(candidates)==1 else None
            done=family is not None and len(accesses)==len(self._post_shutdown_sequence(record['prior_word'],family))
            record.update(accesses=accesses,step=len(accesses),current_word=actual,candidates=candidates,
                          family=family,state='completed' if done else 'in-progress')
            if beginning:control['requests'].append(record)
            control.update(step=control['step']+1,current_word=actual,state=record['state'])
            close['state']='global-closed' if done else 'global-closing';close['control']=control
            close['current_words'][hex(address)]=actual
        if operation=='write':
            old=self.registers[address];self.registers[address]=actual
            self.writes.append(dict(cpu=core,address=hex(address),value=hex(actual),old=hex(old)))
        request['close']=close
        self.board.record('Preview consumed-result close retained',**event,state=close['state'],
            limits='One audited close, optional D4 then FC desired cleanup; no result/DMA/IRQ/guest callback')
        return True

    def _preview_idle_stop_access(self, operation, core, address, size, value):
        record = self.preview_idle_stop
        if record is None and (operation, address, value) != ('write', self.BASE + 0x884, 0):
            return False
        if record is not None and record['state'] == 'completed':
            return False
        self._validate_post_shutdown_control()
        control = self.post_shutdown_control
        if (self.stop_profile != 'stock-idle-stop-handshakes-unmeasured'
                or control is None or control['state'] != 'completed'
                or len(control['requests']) != 3
                or control['requests'][-1]['family'] != 'GLOBAL-zero-stop'
                or self.registers[self.GLOBAL] != 0x10000000
                or type(core) is not int or core != control['cpu'] or size != 4
                or self.busy is not False or self.dma_busy is not False
                or any(x is not None for x in (self.pending_data, self.pending_ack, self.pending_stop))
                or any(type(self.registers[self.BASE + offset]) is not int
                       or self.registers[self.BASE + offset] != word
                       for _, offset, word in self.PREVIEW_IDLE_STOP)):
            raise UnsupportedDevice('Preview idle stop requires the completed GLOBAL reset and zero idle channels')
        if record is None:
            record = dict(state='in-progress', step=0, cpu=core, accesses=[],
                          control_sha256=hp.digest(control),
                          configuration_sha256=self._config_hash(),
                          retained_configuration_sha256=self._retained_config_hash())
        step = record['step']
        if (type(step) is not int or not 0 <= step < len(self.PREVIEW_IDLE_STOP)
                or record['state'] != 'in-progress' or record['cpu'] != core
                or len(record['accesses']) != step
                or record['control_sha256'] != hp.digest(control)
                or record['configuration_sha256'] != self._config_hash()
                or record['retained_configuration_sha256'] != self._retained_config_hash()):
            raise UnsupportedDevice('Preview idle stop ownership or retained configuration changed')
        for index, event in enumerate(record['accesses']):
            kind, offset, word = self.PREVIEW_IDLE_STOP[index]
            if event != dict(operation=kind, address=hex(self.BASE + offset), word=word,
                             cpu=core, step=index):
                raise UnsupportedDevice('Preview idle stop ordered prefix changed')
        actual = self.registers.get(address) if operation == 'read' else value
        if type(actual) is not int or (operation, address - self.BASE, actual) != self.PREVIEW_IDLE_STOP[step]:
            raise UnsupportedDevice('Preview idle stop differs from the complete92CC5C transaction')
        event = dict(operation=operation, address=hex(address), word=actual, cpu=core, step=step)
        if operation == 'write':
            self.writes.append(dict(cpu=core, address=hex(address), value=hex(actual), old=hex(actual)))
            if address in (self.BASE + 0x884, self.BASE + 0x984):
                self.idle_channel_stops[(address - self.BASE - 0x884) // 0x100] += 1
        record['accesses'].append(event)
        record['step'] += 1
        if record['step'] == len(self.PREVIEW_IDLE_STOP):
            record['state'] = 'completed'
        self.preview_idle_stop = record
        self.board.record('Preview producer idle stop retained', **event, state=record['state'],
                          limits='Complete zero-only DMA stop; earlier history preserved, no data or IRQ')
        return True
