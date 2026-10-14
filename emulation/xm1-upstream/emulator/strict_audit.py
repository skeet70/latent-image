"""Opt-in re-audits of device ledgers that only the device itself mutates.

Device models validate their ledgers where they change them (MMIO writes,
closes, resets). Re-deriving the same ledgers on every scheduler tick or
register read only catches host code that edits device state directly, and
cost more than half the run time of ordinary menu scenarios. Tests and
investigations that want those extra audits set X70_EMULATOR_AUDIT=1 or
patch ENABLED.
"""
import os
import weakref

ENABLED = bool(os.environ.get('X70_EMULATOR_AUDIT'))

# Audited ledger keys, outside device state so checkpoints and state hashes
# are unchanged. A restored device either matches an audited key or re-audits.
_validated = weakref.WeakKeyDictionary()


def needed(device, key):
    """True when the device's ledger summary changed since its last audit."""
    return ENABLED or _validated.get(device) != key


def passed(device, key):
    _validated[device] = key


def module_fixture():
    """setUpModule/tearDownModule for tests that corrupt device state directly.

    Those tests check that the opt-in audits reject host edits, so they run
    with audits on regardless of the environment.
    """
    previous = []

    def set_up():
        global ENABLED
        previous.append(ENABLED)
        ENABLED = True

    def tear_down():
        global ENABLED
        ENABLED = previous.pop()

    return set_up, tear_down
