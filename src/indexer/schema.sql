-- I had to remove all the comments to make this work :(

CREATE TABLE IF NOT EXISTS blocks (
  rowid      BIGSERIAL PRIMARY KEY,
  height     BIGINT NOT NULL,
  chain_id   VARCHAR NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,

  UNIQUE (height, chain_id)
);

CREATE INDEX IF NOT EXISTS idx_blocks_height_chain ON blocks(height, chain_id);

CREATE TABLE IF NOT EXISTS tx_results (
  rowid BIGSERIAL PRIMARY KEY,
  block_id BIGINT NOT NULL REFERENCES blocks(rowid),
  index INTEGER NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  tx_hash VARCHAR NOT NULL,
  tx_result BYTEA NOT NULL,

  UNIQUE (block_id, index)
);

CREATE TABLE IF NOT EXISTS events (
  rowid BIGSERIAL PRIMARY KEY,
  block_id BIGINT NOT NULL REFERENCES blocks(rowid),
  tx_id    BIGINT REFERENCES tx_results(rowid),
  type VARCHAR NOT NULL
);

CREATE TABLE IF NOT EXISTS attributes (
   event_id      BIGINT NOT NULL REFERENCES events(rowid),
   key           VARCHAR NOT NULL,
   composite_key VARCHAR NOT NULL,
   value         VARCHAR NULL,

   UNIQUE (event_id, key)
);

CREATE INDEX IF NOT EXISTS idx_attributes_event_id ON attributes(event_id);

CREATE OR REPLACE VIEW event_attributes AS
  SELECT block_id, tx_id, type, key, composite_key, value
  FROM events LEFT JOIN attributes ON (events.rowid = attributes.event_id);

CREATE OR REPLACE VIEW block_events AS
  SELECT blocks.rowid as block_id, height, chain_id, type, key, composite_key, value
  FROM blocks JOIN event_attributes ON (blocks.rowid = event_attributes.block_id)
  WHERE event_attributes.tx_id IS NULL;

CREATE OR REPLACE VIEW tx_events AS
  SELECT height, index, chain_id, type, key, composite_key, value, tx_results.created_at
  FROM blocks JOIN tx_results ON (blocks.rowid = tx_results.block_id)
  JOIN event_attributes ON (tx_results.rowid = event_attributes.tx_id)
  WHERE event_attributes.tx_id IS NOT NULL;

CREATE SCHEMA IF NOT EXISTS debug;

CREATE TABLE IF NOT EXISTS debug.app_hash (
  rowid SERIAL PRIMARY KEY,
  block_id BIGINT NOT NULL REFERENCES blocks(rowid),
  hash BYTEA NOT NULL
);
