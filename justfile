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
