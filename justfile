# Run network integration tests, specifically the 'penumbra-reindexer regen' ones.
integration-regen:
  RUST_LOG=debug cargo nextest run --release --features network-integration --nocapture run_reindexer_regen_

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
  rm -rf test_data/ephemeral-storage/
  cargo nextest run --release --features network-integration --nocapture

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
  #
# Serve the local postgres db, created via integration tests
db:
  printf 'Use the following URL to connect to the db:\n\n\t%s\n\n' \
    "postgresql://?dbname=penumbra_raw&host=/tmp/penumbra-reindexer-regen-1/pgtown/postgres/sock"
  postgres \
    -D /tmp/penumbra-reindexer-regen-1/pgtown/postgres/data/ \
    -k /tmp/penumbra-reindexer-regen-1/pgtown/postgres/sock/
