"""Shared interfaces for the explicit X70 board model."""


class UnsupportedDevice(RuntimeError):
    """An instruction requested behavior the board model does not define."""
