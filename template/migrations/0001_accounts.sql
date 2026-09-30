-- The people who can sign in, and their sessions. src/auth.rs uses both.
CREATE TABLE users (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- Trimmed and lowercased by the app, so one address is one account.
    email TEXT NOT NULL UNIQUE,
    -- An Argon2id PHC string, never the password itself.
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A signed-in browser. Its cookie holds a random token and this table only
-- the token's SHA-256, so a copy of the table signs no one in.
CREATE TABLE sessions (
    token_hash BYTEA PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX sessions_user_id ON sessions (user_id);
