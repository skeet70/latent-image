"""Pure declared monochrome rows for a bounded image-bank destination.

Stock setters/getters expose dimensions, destination and a halfword-unit
pitch. They do not establish the silicon color packing. This explicit
experiment uses YYCbCr pairs (two lumas, neutral chroma128) and floor-nearest
full-field luma sampling. It returns active row spans only, without applying
bytes, arming a device, supplying a status or asserting an interrupt.
"""
import hashlib
from copy import deepcopy

try:
    import numpy as _numpy
except ImportError:
    _numpy = None

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from .image_work_devices import ImageWorkControl
from .virtual_source import ram_span


PROFILE = 'declared-still-imagebank-monochrome-yycbcr-nearest-luma-unmeasured'
MAX_PIXELS = 4 * 1024 * 1024
LIMITS = ('Declared two lumas/neutral chroma128 in four-byte YYCbCr pairs; '
    'full-field floor-nearest sampling and byte pitch=2*C4. Color packing, '
    'CFA, filter, source crop and allocation header use are unmeasured. '
    'Active row spans only; no DMA, status, IRQ or firmware result.')


def _profile(profile):
    if profile != PROFILE:
        raise UnsupportedDevice('Still image rows require the explicit declared monochrome profile')


def _int(value, low, high, name):
    if type(value) is not int or not low <= value <= high:
        raise UnsupportedDevice('Still image rows: invalid ' + name)
    return value


def geometry(device, *, channel, allocation, profile=None):
    """Read a typed desired tuple; require separately known allocation bounds."""
    _profile(profile)
    _int(channel, 0, 1, 'channel')
    if type(allocation) is not dict or set(allocation) != {'address', 'bytes'}:
        raise UnsupportedDevice('Still image rows require explicit allocation bounds; returned geometry word5 is not a size')
    address = _int(allocation['address'], 0, 0x5fffffff, 'allocation address')
    size = _int(allocation['bytes'], 1, 64 * 1024 * 1024, 'allocation capacity')
    low, high = ram_span(address, size)
    base = ImageWorkControl.BASES[channel]
    words = {offset: device.registers.get(base+offset) for offset in ImageWorkControl.CONFIG_ORDER}
    if any(not ImageWorkControl._config_valid(offset, value) for offset, value in words.items()):
        raise UnsupportedDevice('Still image rows: unknown or incomplete desired configuration')
    width, height = (words[0xb0]&0x7ff)+1, ((words[0xb0]>>16)&0x7ff)+1
    pitch, destination = 2*words[0xc4], words[0xc0]
    if (width&1 or width*height>MAX_PIXELS or pitch<width*2 or not destination):
        raise UnsupportedDevice('Still image rows await a nonzero bounded destination and byte pitch')
    begin, end = ram_span(destination, (height-1)*pitch+width*2)
    if not low <= begin < end <= high:
        raise UnsupportedDevice('Still image rows exceed their separately supplied allocation capacity')
    return dict(channel=channel, width=width, height=height, pitch=pitch,
        row_bytes=width*2, destination=destination, destination_offset=begin-low,
        allocation=dict(address=address,bytes=size), physical_begin=begin, physical_end=end,
        desired=words, desired_sha256=digest(words), packing='declared-YYCbCr-neutral128')


def encode_rows(image, *, source_width, source_height, source_stride, width, height,
                profile=None, numpy=True):
    """Return active output rows; omit every destination padding byte."""
    _profile(profile)
    for value, low, name in ((source_width,2,'source width'),(source_height,1,'source height'),
                            (width,2,'output width'),(height,1,'output height')):
        _int(value,low,8192,name)
    _int(source_stride,2*source_width,32768,'source stride')
    if (type(image) is not bytes or len(image)!=source_stride*source_height
            or source_width&1 or width&1 or type(numpy) is not bool
            or source_width*source_height>1048576 or len(image)>4*1024*1024
            or width*height>MAX_PIXELS):
        raise UnsupportedDevice('Still image rows: invalid bounded source/output raster')
    if numpy and _numpy is not None:
        pairs=_numpy.frombuffer(image,dtype=_numpy.uint8).reshape(source_height,source_stride)
        luma=pairs[:,:source_width*2].reshape(source_height,source_width//2,4)[:,:,:2].reshape(source_height,source_width)
        xx=_numpy.arange(width)*source_width//width
        result=[]
        for y in range(height):
            row=_numpy.full((width//2,4),128,dtype=_numpy.uint8)
            row[:,:2]=luma[y*source_height//height,xx].reshape(width//2,2)
            result.append(row.tobytes())
        return tuple(result)
    rows=[]
    for y in range(height):
        start=y*source_height//height*source_stride
        row=bytearray()
        for x in range(0,width,2):
            a=x*source_width//width; b=(x+1)*source_width//width
            row.extend((image[start+(a//2)*4+(a&1)],image[start+(b//2)*4+(b&1)],128,128))
        rows.append(bytes(row))
    return tuple(rows)


def prepare(device, *, channel, allocation, scene, profile=None, numpy=True):
    """Return a pure plan, refusing unresolved destination/pitch/capacity."""
    output=geometry(device,channel=channel,allocation=allocation,profile=profile)
    fields={'address','bytes','width','height','stride_bytes','payload_sha256'}
    if type(scene) is not dict or set(scene)!=fields:
        raise UnsupportedDevice('Still image rows require a frozen explicit YYCbCr scene')
    source=scene['address']
    if type(source) is str:
        try: source=int(source,16)
        except ValueError as error:
            raise UnsupportedDevice('Still image scene address is malformed') from error
    size=_int(scene['bytes'],1,4*1024*1024,'scene size')
    begin,end=ram_span(source,size)
    if begin<output['physical_end'] and output['physical_begin']<end:
        raise UnsupportedDevice('Still image scene aliases its output span')
    raw=bytes(device.board.read_memory(source,size,cpu_id=0))
    if hashlib.sha256(raw).hexdigest()!=scene['payload_sha256']:
        raise UnsupportedDevice('Still image frozen scene bytes changed')
    rows=encode_rows(raw,source_width=scene['width'],source_height=scene['height'],
        source_stride=scene['stride_bytes'],width=output['width'],height=output['height'],profile=profile,numpy=numpy)
    result=dict(profile=profile,state='prepared-unarmed',geometry=output,scene=deepcopy(scene),
        rows=tuple((output['destination']+y*output['pitch'],row) for y,row in enumerate(rows)),limits=LIMITS)
    result['input_sha256']=digest(dict(profile=profile,geometry=output,scene=scene,
        rows_sha256=hashlib.sha256(b''.join(rows)).hexdigest()))
    return result
