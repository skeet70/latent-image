"""Opt-in offline board profiles; the canonical X70 factory is unchanged."""
from dataclasses import dataclass
from importlib import import_module
from pathlib import Path


@dataclass(frozen=True)
class BoardProfile:
    firmware_id: str
    rom_file: Path
    rom_hash: str
    ram_size: int
    dram_basis: str
    extra_mmio_ranges: tuple
    entry_labels: dict
    manifest: dict
    flash: bytes
    fixtures: dict
    device_map: tuple


def load_profile(firmware_id):
    """Only explicitly investigated profiles qualify; no family fallback."""
    modules = {'xe2s-111': 'xe2s_111'}
    if firmware_id not in modules:
        raise ValueError(f'No offline board profile for {firmware_id!r}')
    return import_module(f'{__name__}.{modules[firmware_id]}').load()


def make_board(firmware_id, **options):
    if firmware_id == 'x70-110':
        from x70_native.board import make_board as stock_factory
        return stock_factory(**options)
    module = import_module(f'{__name__}.xe2s_111')
    return module.make_board(load_profile(firmware_id), **options)
