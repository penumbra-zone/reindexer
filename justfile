# Run cargo check, failing on warnings
check:
  cargo check --release --all-targets --all-features

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
