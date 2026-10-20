-- Which account set up which server (R08), so "delete my data" can find that owner's records.
-- One owner per server (DECISION-0053): a server that is claimed again changes owner. Removed with the server's record.
CREATE TABLE IF NOT EXISTS owner_servers (
  server_id TEXT PRIMARY KEY,        -- the server's public key, as in records
  owner TEXT NOT NULL,               -- "<provider>:<account id>" from the device certificate
  created_at INTEGER NOT NULL        -- ms
);
CREATE INDEX IF NOT EXISTS owner_servers_owner ON owner_servers (owner);
