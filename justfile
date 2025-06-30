# Run cargo check, failing on warnings
check:
  cargo check --all-targets --all-features
  cargo clippy

# Run cargo fmt, failing on warnings
fmt:
  cargo fmt --all -- --check

# Run cargo nextest
test:
  cargo nextest run

# Run network integration tests. Requires a LOT of disk space and bandwidth!
integration:
  # don't nuke all storage, we need some of this
  # rm -rf test_data/ephemeral-storage/
  # TODO we should port the logic back into cargo-nextest
  # cargo nextest run --release --features network-integration --nocapture

  cargo run -- bootstrap --home test_data/ephemeral-storage
  # ideally we'd top up the archive based on remote, but this can add ~20m
  # cargo run -- archive --home test_data/ephemeral-storage --remote-rpc https://rpc-penumbra.radiantcommons.com

  # check that the archive is valid
  cargo run -- check --home test_data/ephemeral-storage
  # TODO: use picturesque to set up a local psql db for testing
  # cargo run -- regen --home test_data/ephemeral-storage --chain-id penumbra-1 --database-url postgresql://penumbra:penumbra@127.0.0.1:5432/regen

# Run expensive tests that require local files as input. Assumes integration tests have been run!
expensive-tests:
  REINDEXER_SQLITE_DB_FILEPATH=test_data/ephemeral-storage/network/penumbra-1/node0/reindexer_archive.bin \
    cargo nextest run --release --nocapture --features expensive-tests --test file

# Build the binary via nix
build:
  nix build

# Build the container image via nix
container:
  # Building container via nix...
  nix build .#container
  # To run this container locally:
  #
  #   docker load < result
  #   docker run -it localhost/penumbra-reindexer:0.5.0 bash

# dev-only helper to reset a local psql database
unsafe-reset-local-db:
  sudo -u postgres psql -c 'DROP DATABASE regen;' || true
  sudo -u postgres psql -c 'CREATE DATABASE regen WITH OWNER penumbra;'
