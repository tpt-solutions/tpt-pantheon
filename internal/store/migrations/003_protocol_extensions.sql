-- 003: PAR, device codes, email notifications, privacy budget, password reset.

-- Pushed Authorization Requests (RFC 9126)
CREATE TABLE IF NOT EXISTS par_requests (
    request_uri  TEXT PRIMARY KEY,  -- urn:ietf:params:oauth:request_uri:<random>
    client_id    TEXT NOT NULL,
    params       TEXT NOT NULL,     -- JSON: all /authorize query params
    expires_at   DATETIME NOT NULL,
    created_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Device Authorization Grant codes (RFC 8628)
CREATE TABLE IF NOT EXISTS device_codes (
    device_code   TEXT PRIMARY KEY,
    user_code     TEXT NOT NULL UNIQUE,
    client_id     TEXT NOT NULL,
    scope         TEXT NOT NULL DEFAULT '',
    subject_did   TEXT,            -- set when user approves
    approved      INTEGER NOT NULL DEFAULT 0,
    denied        INTEGER NOT NULL DEFAULT 0,
    expires_at    DATETIME NOT NULL,
    interval_secs INTEGER NOT NULL DEFAULT 5,
    created_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_device_codes_user_code ON device_codes(user_code);

-- SMTP email notification configuration (one row per deployment).
CREATE TABLE IF NOT EXISTS email_config (
    id          INTEGER PRIMARY KEY CHECK (id = 1),  -- singleton
    smtp_host   TEXT NOT NULL,
    smtp_port   INTEGER NOT NULL DEFAULT 587,
    smtp_user   TEXT NOT NULL,
    smtp_pass   TEXT NOT NULL,      -- encrypted (same keystore as other secrets)
    from_addr   TEXT NOT NULL,
    from_name   TEXT NOT NULL DEFAULT 'tpt-identity',
    enabled     INTEGER NOT NULL DEFAULT 1,
    updated_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Outbound email queue (for retry / delivery tracking).
CREATE TABLE IF NOT EXISTS email_queue (
    id          TEXT PRIMARY KEY,
    to_addr     TEXT NOT NULL,
    subject     TEXT NOT NULL,
    body_html   TEXT NOT NULL,
    body_text   TEXT NOT NULL,
    event_type  TEXT NOT NULL,
    attempts    INTEGER NOT NULL DEFAULT 0,
    last_error  TEXT,
    sent_at     DATETIME,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_email_queue_sent ON email_queue(sent_at);

-- Privacy budget: per-subject, per-schema disclosure counter.
CREATE TABLE IF NOT EXISTS privacy_disclosures (
    id            TEXT PRIMARY KEY,
    subject_did   TEXT NOT NULL,
    schema_id     TEXT NOT NULL,
    verifier_did  TEXT NOT NULL,
    field_names   TEXT NOT NULL,  -- JSON array of disclosed field names
    disclosed_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_privacy_disclosures_subject ON privacy_disclosures(subject_did);
CREATE INDEX IF NOT EXISTS idx_privacy_disclosures_schema  ON privacy_disclosures(subject_did, schema_id);

-- Password reset tokens (one-time, 15-min TTL).
CREATE TABLE IF NOT EXISTS password_reset_tokens (
    hash       TEXT PRIMARY KEY,   -- sha256(raw_token)
    identifier TEXT NOT NULL,      -- email or subject DID
    expires_at DATETIME NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- OID4VCI credential offers (single-use, short-lived).
CREATE TABLE IF NOT EXISTS credential_offers (
    id          TEXT PRIMARY KEY,   -- random; embedded in openid-credential-offer:// URI
    client_id   TEXT,               -- optional: restrict to one client
    schema_ids  TEXT NOT NULL,      -- JSON array of schema IDs being offered
    issuer_did  TEXT NOT NULL,
    subject_did TEXT,               -- optional pre-binding
    expires_at  DATETIME NOT NULL,
    used        INTEGER NOT NULL DEFAULT 0,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
