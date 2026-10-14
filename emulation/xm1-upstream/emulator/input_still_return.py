"""One declared four-channel stop ACK after actual still-resource cleanup.

Stock 1211028/10CC270/922EB4 writes3 then2 while input0 stays enabled.
Only the phase latch returns3; this produces no frame, pixels, IRQ or wake.
The two-tick acknowledgment is a simulation timing assumption.
"""
from copy import deepcopy
from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-post-still-enabled-input-four-phase-stop-ack-unmeasured'
ATTRIBUTE = 'still_return_phases'
OFFSETS = (0xc4, 0xd8, 0xec, 0x128)
RETIRED = ((0x15d502c,0),(0x15d50a4,1),(0x15d50a8,1),(0x15d50b8,0),
           (0x15d50c0,0),(0x15f6bf4,0),(0x15f66a4,0),(0x15f68a4,0),
           (0x15f68a8,0),(0x15f63a4,2),(0x15d5044,0),(0x15f6e5c,0))

def _seal(record):
    record['seal'] = digest({k:v for k,v in record.items() if k != 'seal'})

def _live(device):
    from .input_processing_devices import PHASE_PROFILE, CARD_STAGING_EVENT_PROFILE
    r = device.registers
    if (device.phase_profile != PHASE_PROFILE or device.event_profile != CARD_STAGING_EVENT_PROFILE
            or device.event_enable_generation != 10 or device.event_due is not None
            or device.event_pending or device.event_followup is not None
            or device.busy or device.dma_busy or device.pending_command is not None
            or device.command_armed is not None
            or any(r.get(k) != v for k,v in ((0x48,1),(0x4c,1),(0x70,0),(0x74,0),
                                           (0x13c,1),(0x38c,0x10002),(0x154,0)))):
        raise UnsupportedDevice('Post-still phase ACK requires its drained generation10 card input owner')
    if any(int.from_bytes(device.board.read_memory(a,4,cpu_id=0),'little') != v for a,v in RETIRED):
        raise UnsupportedDevice('Post-still phase ACK requires stock private0 and retired RAW/Image/numeric/subscribers')
    for bank in device.board.interrupts.banks:
        for irq in (0x24,0x27,0x29,44,45,46):
            if (bank.pending & (1 << irq) or bank.active == irq or bank.injected == irq
                    or any(x.get('line') == irq for x in bank.active_stack)):
                raise UnsupportedDevice('Post-still phase ACK cannot replace an outstanding data interrupt')

def arm(device, profile):
    if (profile != PROFILE or hasattr(device,ATTRIBUTE) or device.board.failure is not None
            or device.pending_phases or device.phase_busy):
        raise UnsupportedDevice('Post-still phase ACK requires its explicit single lifetime')
    _live(device)
    if (device.registers.get(OFFSETS[0]) not in (3,0x14003)
            or any(device.registers.get(o) != 0x14003 for o in OFFSETS[1:3])
            or device.registers.get(0x128) != 3):
        raise UnsupportedDevice('Post-still phase ACK requires the actual four-channel phase prefix')
    record = dict(profile=PROFILE, armed_tick=device.board.virtual_ticks, requests=[], completions=[], state='armed')
    _seal(record);setattr(device,ATTRIBUTE,record)
    return deepcopy(record)

def validate(device):
    record = getattr(device,ATTRIBUTE,None)
    if record is None:return None
    if (type(record) is not dict or record.get('profile') != PROFILE
            or record.get('seal') != digest({k:v for k,v in record.items() if k != 'seal'})
            or record['state'] not in ('armed','complete')
            or record['requests'] != list(OFFSETS[:len(record['requests'])])
            or not 0 <= len(record['completions']) <= len(record['requests']) <= 4
            or [x['offset'] for x in record['completions']] != record['requests'][:len(record['completions'])]
            or (record['state']=='complete') != (len(record['completions'])==4)):
        raise UnsupportedDevice('Post-still phase ACK lost its finite ordered history')
    return record

def request(device,core,address,size,offset,value,old):
    record=validate(device)
    if record is None:return False
    from unicorn.arm_const import UC_ARM_REG_PC,UC_ARM_REG_LR
    cpu=device.board.cpus[0]
    _live(device)
    if (core != 0 or size != 4 or value != 2 or old != 3
            or len(record['requests']) >= 4 or offset != OFFSETS[len(record['requests'])]
            or cpu.reg_read(UC_ARM_REG_PC) != 0x922f60 or cpu.reg_read(UC_ARM_REG_LR) != 0x10cc2cc):
        raise UnsupportedDevice('Post-still phase ACK requires unchanged10CC270/922EB4 ordered3-to2 stores')
    record['requests'].append(offset);_seal(record)
    device.pending_phases[offset]=dict(cpu=core,index=device._phase_index(offset),
        requested_tick=device.board.virtual_ticks,due_tick=device.board.virtual_ticks+device.phase_ticks,
        request=hex(value),profile=PROFILE)
    device.phase_busy=True;device.phase_requests+=1
    device.board.record('Post-still input phase stop requested',**device.pending_phases[offset])
    return True

def tick(device):
    record=validate(device)
    if record is None:return
    for offset in record['requests'][len(record['completions']):]:
        item=device.last_phases.get(offset)
        if item is None or item['profile'] != PROFILE:break
        if item['acknowledged'] != '0x3':
            raise UnsupportedDevice('Post-still phase ACK changed its low-field response')
        record['completions'].append(dict(offset=offset,completed_tick=item['completed_tick']))
    if len(record['completions'])==4:record['state']='complete'
    _seal(record)

def summary(device):
    return deepcopy(validate(device))
