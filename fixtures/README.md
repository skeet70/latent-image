# Fixtures

Keep synthetic or explicitly redistributable cases here with origin, license, hash, expected result, and evidence scope. Binary fixtures are permitted by `.gitignore`. Proprietary firmware bytes, body-specific state, and private captures belong under `private-data/`; public cases can refer to required hashes and extraction offsets without embedding those bytes. Follow [the publication boundary](../docs/principles.md#publication-and-asset-boundaries) and [licensing requirements](../LICENSING.md).

Cases for the [native harness](../emulation/README.md#case-contract-and-execution-authority) must declare CPU/mode, memory permissions, registers, stubs, expected outputs, and finite budgets. Engine initialization alone is not a stock fixture and does not satisfy G3.

The current hand-encoded synthetic cases live in [`emulation/fixtures/`](../emulation/fixtures/PROVENANCE.md) alongside their provenance and independent expected arithmetic/state. They establish the bounded runner's behavior on a declared synthetic CPU; they contain no firmware-derived bytes and do not satisfy the stock milestone or G3.
