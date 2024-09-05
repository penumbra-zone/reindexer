# Run cargo check, failing on warnings
check:
  # The `-D warnings` option causes an error on warnings.
  RUSTFLAGS="-D warnings" \
    cargo check --release --all-targets

# Run cargo fmt, failing on warnings
fmt:
  cargo fmt --all -- --check

# Run cargo nextest
test:
  cargo nextest run
