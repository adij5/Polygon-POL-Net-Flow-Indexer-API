-- Raw transfers we care about (native POL only)
CREATE TABLE IF NOT EXISTS raw_transfers (
  id BIGSERIAL PRIMARY KEY,
  block_number BIGINT NOT NULL,
  tx_hash TEXT NOT NULL,
  "from" TEXT NOT NULL,
  "to" TEXT NOT NULL,
  value_wei TEXT NOT NULL,
  ts TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Running totals snapshot after each processed block
CREATE TABLE IF NOT EXISTS net_flows_snapshots (
  id BIGSERIAL PRIMARY KEY,
  block_number BIGINT NOT NULL,
  cumulative_in_wei TEXT NOT NULL,
  cumulative_out_wei TEXT NOT NULL,
  net_flow_wei TEXT NOT NULL,
  ts TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Single row keeping last processed block and totals (for quick reads)
CREATE TABLE IF NOT EXISTS indexer_state (
  id SMALLINT PRIMARY KEY DEFAULT 1,
  last_block BIGINT NOT NULL,
  cumulative_in_wei TEXT NOT NULL,
  cumulative_out_wei TEXT NOT NULL
);
