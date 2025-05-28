#!/bin/bash
set -euo pipefail


archive_file="test_data/ephemeral-storage/network/penumbra-1/node0/reindexer_archive.bin"
working_dir="mainnet-lqt-reindex-2"


penumbra-reindexer regen \
  --database-url 'postgresql://penumbra:penumbra@127.0.0.1:5432/penumbra' \
  --archive-file "$archive_file" \
  --working-dir "$working_dir" \
  --stop-height 501974

penumbra-reindexer regen \
  --database-url 'postgresql://penumbra:penumbra@127.0.0.1:5432/penumbra' \
  --archive-file "$archive_file" \
  --working-dir "$working_dir" \
  --stop-height 2611799

penumbra-reindexer regen \
  --database-url 'postgresql://penumbra:penumbra@127.0.0.1:5432/penumbra' \
  --archive-file "$archive_file" \
  --working-dir "$working_dir" \
  --stop-height 4378761

penumbra-reindexer regen \
  --database-url 'postgresql://penumbra:penumbra@127.0.0.1:5432/penumbra' \
  --archive-file "$archive_file" \
  --working-dir "$working_dir"
