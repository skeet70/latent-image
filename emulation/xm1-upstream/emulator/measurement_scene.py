"""Declared defocus response of one supplied packed YYCbCr scene.

This optical experiment is independent of camera decisions. The firmware
receives energies derived from an image and lens coordinate; no focus flags,
callback, machine state or outcome appears in its output. Blur scale, filter
and correspondence with hardware windows are explicitly uncalibrated.
"""
import hashlib

try:
    import numpy as _numpy
except ImportError:
    _numpy = None

from .hardware import UnsupportedDevice
from .afs_scene_fixture import contrast_grid


PROFILE='declared-normalised-scene-horizontal-binomial-defocus-unmeasured'
MAX_BLUR_PASSES=16


def _blur_scalar(image, width, height, stride_bytes, passes):
    output=bytearray(image)
    for y in range(height):
        start=y*stride_bytes
        line=[image[start+(x//2)*4+(x&1)] for x in range(width)]
        for _ in range(passes):
            line=[(line[max(0,x-1)]+2*line[x]+line[min(width-1,x+1)]+2)//4 for x in range(width)]
        for x,value in enumerate(line):output[start+(x//2)*4+(x&1)]=value
    return bytes(output)


def _blur_numpy(image, width, height, stride_bytes, passes):
    output=bytearray(image)
    rows=_numpy.frombuffer(output,dtype=_numpy.uint8).reshape(height,stride_bytes)
    pairs=rows[:,:width*2].reshape(height,width//2,4)
    line=pairs[:,:,:2].reshape(height,width).astype(_numpy.uint16)
    for _ in range(passes):
        padded=_numpy.pad(line,((0,0),(1,1)),mode='edge')
        line=(padded[:,:-2]+2*line+padded[:,2:]+2)//4
    pairs[:,:,:2]=line.reshape(height,width//2,2)
    return bytes(output)


def lens_scene_grid(image,*,width,height,stride_bytes,focus_position,target_position,
                    pulses_per_blur_pass=8,columns=1,rows=1):
    # Contrast's checked geometry/sample bound applies before allocating any
    # blurred raster. Source padding/chroma stay unchanged.
    original=contrast_grid(image,width=width,height=height,stride_bytes=stride_bytes,columns=columns,rows=rows)
    if (any(type(value) is not int or not -32768<=value<=32767 for value in (focus_position,target_position))
            or type(pulses_per_blur_pass) is not int or not 1<=pulses_per_blur_pass<=4096):
        raise UnsupportedDevice('Declared measurement scene requires bounded lens coordinates/blur scale')
    defocus=target_position-focus_position
    passes=min(MAX_BLUR_PASSES,(abs(defocus)+pulses_per_blur_pass//2)//pulses_per_blur_pass)
    output=(_blur_numpy if _numpy is not None else _blur_scalar)(
        image,width,height,stride_bytes,passes) if passes else image
    grid=contrast_grid(output,width=width,height=height,stride_bytes=stride_bytes,
                       columns=columns,rows=rows) if passes else original
    return dict(profile=PROFILE,focus_position=focus_position,target_position=target_position,
                pulses_per_blur_pass=pulses_per_blur_pass,blur_passes=passes,
                source_sha256=hashlib.sha256(image).hexdigest(),image=output,grid=grid,
                assumptions='Supplied packed YYCbCr scene represents a normalised sensor field; horizontal binomial blur and lens-pulse scale are declared, not optical/ISP calibration. Pixel crop correspondence and exposure gain are unmeasured; stock firmware alone judges focus.')
