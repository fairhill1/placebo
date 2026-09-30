CREATE TABLE items (
    id bigserial PRIMARY KEY,
    title text NOT NULL,
    version bigint NOT NULL DEFAULT 1
);

INSERT INTO items (title) VALUES ('First item'), ('Second item');
