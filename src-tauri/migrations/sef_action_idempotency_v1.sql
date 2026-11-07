-- Local-only retry guard for SEF return/cancel requests made through PROGRAMERI POS.
-- Stores no clinical text or payment data; only a request hash and the bridge key.
CREATE TABLE IF NOT EXISTS sef_bridge_actions (
  request_key TEXT PRIMARY KEY,
  coupon_id TEXT NOT NULL,
  action TEXT NOT NULL CHECK (action IN ('RETURN', 'CANCEL')),
  request_hash TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('PENDING', 'COMPLETED', 'REJECTED')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_sef_bridge_actions_pending_coupon
  ON sef_bridge_actions(coupon_id) WHERE state='PENDING';

CREATE INDEX IF NOT EXISTS ix_sef_bridge_actions_state_created
  ON sef_bridge_actions(state, created_at);
