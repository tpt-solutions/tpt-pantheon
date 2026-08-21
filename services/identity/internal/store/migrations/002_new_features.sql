-- 002: Duress codes, verifiable audit log, and guardian recovery tables.

-- Duress passphrase (silent coercion alert).
CREATE TABLE IF NOT EXISTS duress_configs (
    subject_did TEXT PRIMARY KEY,
    hash        TEXT NOT NULL,  -- hex(salt) + ":" + hex(argon2id_dk)
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Hash-chained verifiable audit log (append-only).
CREATE TABLE IF NOT EXISTS audit_log (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    event_type TEXT NOT NULL,
    payload    TEXT NOT NULL,  -- JSON
    hash       TEXT NOT NULL,  -- SHA-256(prev_hash || event_json)
    prev_hash  TEXT NOT NULL DEFAULT ''
);

-- Guardian recovery configuration.
CREATE TABLE IF NOT EXISTS recovery_configs (
    subject_did   TEXT PRIMARY KEY,
    threshold     INTEGER NOT NULL,
    guardian_dids TEXT NOT NULL,  -- JSON array
    created_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Active recovery requests.
CREATE TABLE IF NOT EXISTS recovery_requests (
    id               TEXT PRIMARY KEY,
    subject_did      TEXT NOT NULL,
    started_at       DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    shares_collected INTEGER NOT NULL DEFAULT 0,
    completed        INTEGER NOT NULL DEFAULT 0  -- boolean
);

-- Guardian approval shares.
CREATE TABLE IF NOT EXISTS recovery_shares (
    request_id   TEXT NOT NULL,
    guardian_did TEXT NOT NULL,
    share_hash   TEXT NOT NULL,
    collected_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (request_id, guardian_did)
);
