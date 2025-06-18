# Unreleased

Nothing Yet!

# Version 0.7.0 (2025-06-18)

* chore: bump penumbra deps to 2.0.0

# Version 0.6.3 (2025-06-13)

* chore: bump penumbra deps to 1.5.3

# Version 0.6.2 (2025-05-01)

* fix: add support for tls db connections
* chore: cargo update

# Version 0.6.1 (2025-05-01)

* fix: really support app parameter change events

# Version 0.6.0 (2025-04-17)

* feat: add support for following remote nodes when regenerating
* feat: add support for exporting genesis files from archive
* build: add container image spec

# Version 0.5.0 (2025-04-15)

* feat: add support for exporting genesis files from archives
* feat: add support for `penumbra-testnet-phobos-3` chain
* feat: improve remote block streaming performance
* feat: add remote indexing support for regen
* feat: add support for mainnet3 migration on `penumbra-1` chain

# Version 0.4.0 (2025-04-07)

* feat: add LQT support for testnet
* feat: add remote indexing support

# Version 0.3.0 (2025-03-18)

* feat: support app parameter change events

# Version 0.2.1 (2025-03-18)

* fix: parsing bugs in BeginBlock conversions
* refactor: remove unused conversion fn
* fix(tests): fetch matching genesis per phase

# Version 0.2.0 (2025-03-05)

* feat: add support for v1 of penumbra crates
* feat: more logging by default
* docs: clarify --home argument in cli options
* chore: update cometbft to v0.37.15
* lint: cargo clippy
* test: add integration test suite

# Version 0.1.3 (2025-01-29)

* feat: add support for `penumbra-testnest-phobos-2` chain
* feat: add safeguard against plans not matching the archive
* build: fix builds on macos

# Version 0.1.2 (2025-01-21)

Adds support for the `v0.81.x` protocol changes for the `penumbra-1` chain.

# Version 0.1.1 (2024-12-18)

Bumps the `v0.80.x` deps to `v0.80.11`, to stay in sync with upstream changes.

# Version 0.1.0 (2024-08-25)

Initial release, supporting chain upgrades from `v0.79.x` to `v0.80.x` on the `penumbra-1` chain.
