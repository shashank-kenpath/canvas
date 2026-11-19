-- Accounts are Google Workspace identities; `id` is Google's stable `sub`.
CREATE TABLE users (
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  picture TEXT,
  created_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  disabled INTEGER NOT NULL DEFAULT 0
);

-- Only a SHA-256 of each session token is stored.
CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  device TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  revoked INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX sessions_user ON sessions(user_id);

-- Sign-ins in progress. Google's device code never leaves the server.
CREATE TABLE device_logins (
  handle TEXT PRIMARY KEY,
  device_code TEXT NOT NULL,
  device TEXT NOT NULL DEFAULT '',
  interval_s INTEGER NOT NULL,
  next_poll_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL
);

-- org_access: 'none' (members only), 'view' or 'edit' for everyone signed in.
CREATE TABLE boards (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id),
  org_access TEXT NOT NULL DEFAULT 'none',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  archived INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX boards_owner ON boards(owner_id);

-- Members are invited by email, so people can be added before their first sign-in.
CREATE TABLE members (
  board_id TEXT NOT NULL REFERENCES boards(id),
  email TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('editor', 'viewer')),
  added_by TEXT NOT NULL,
  added_at INTEGER NOT NULL,
  PRIMARY KEY (board_id, email)
);
CREATE INDEX members_email ON members(email);
