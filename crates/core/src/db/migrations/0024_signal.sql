-- Signal, linked as a device of the user's own account (connections/signal/). Only what
-- the Signal protocol needs is kept: the account's keys, sessions with other devices and
-- a few settings. No message is ever stored. Everything belongs to one connection, so
-- removing the connection removes all of it.

CREATE TABLE signal_kv (
    connection_id TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    key           TEXT NOT NULL,
    value         BLOB NOT NULL,
    PRIMARY KEY (connection_id, key)
) STRICT;

CREATE TABLE signal_sessions (
    connection_id TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    identity      TEXT NOT NULL CHECK (identity IN ('aci', 'pni')),
    address       TEXT NOT NULL,
    device_id     INTEGER NOT NULL,
    record        BLOB NOT NULL,
    PRIMARY KEY (connection_id, identity, address, device_id)
) STRICT;

CREATE TABLE signal_identities (
    connection_id TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    identity      TEXT NOT NULL CHECK (identity IN ('aci', 'pni')),
    address       TEXT NOT NULL,
    record        BLOB NOT NULL,
    PRIMARY KEY (connection_id, identity, address)
) STRICT;

CREATE TABLE signal_pre_keys (
    connection_id TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    identity      TEXT NOT NULL CHECK (identity IN ('aci', 'pni')),
    id            INTEGER NOT NULL,
    record        BLOB NOT NULL,
    PRIMARY KEY (connection_id, identity, id)
) STRICT;

CREATE TABLE signal_signed_pre_keys (
    connection_id TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    identity      TEXT NOT NULL CHECK (identity IN ('aci', 'pni')),
    id            INTEGER NOT NULL,
    record        BLOB NOT NULL,
    PRIMARY KEY (connection_id, identity, id)
) STRICT;

CREATE TABLE signal_kyber_pre_keys (
    connection_id  TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    identity       TEXT NOT NULL CHECK (identity IN ('aci', 'pni')),
    id             INTEGER NOT NULL,
    record         BLOB NOT NULL,
    is_last_resort INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (connection_id, identity, id)
) STRICT;

-- Base keys already used with a last-resort Kyber pre-key, so none is accepted twice.
CREATE TABLE signal_base_keys_seen (
    connection_id     TEXT NOT NULL,
    identity          TEXT NOT NULL,
    kyber_pre_key_id  INTEGER NOT NULL,
    signed_pre_key_id INTEGER NOT NULL,
    base_key          BLOB NOT NULL,
    PRIMARY KEY (connection_id, identity, kyber_pre_key_id, signed_pre_key_id, base_key),
    FOREIGN KEY (connection_id, identity, kyber_pre_key_id)
        REFERENCES signal_kyber_pre_keys (connection_id, identity, id) ON DELETE CASCADE,
    FOREIGN KEY (connection_id, identity, signed_pre_key_id)
        REFERENCES signal_signed_pre_keys (connection_id, identity, id) ON DELETE CASCADE
) STRICT;

CREATE TABLE signal_sender_keys (
    connection_id   TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    identity        TEXT NOT NULL CHECK (identity IN ('aci', 'pni')),
    address         TEXT NOT NULL,
    device_id       INTEGER NOT NULL,
    distribution_id TEXT NOT NULL,
    record          BLOB NOT NULL,
    PRIMARY KEY (connection_id, identity, address, device_id, distribution_id)
) STRICT;
