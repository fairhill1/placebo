-- The starter's demo. Its tasks are a short tour of what Placebo does.
CREATE TABLE tasks (
    id bigserial PRIMARY KEY,
    title text NOT NULL,
    notes text NOT NULL DEFAULT '',
    done boolean NOT NULL DEFAULT false,
    version bigint NOT NULL DEFAULT 1
);

INSERT INTO tasks (title, notes, done) VALUES
    ('Open this app in a second tab',
     'Mark a task done or add one in this tab, and the other tab follows at once. No code syncs them: each save tells open pages to read themselves again.',
     false),
    ('Click a task to open it',
     'Links swap the page in without a full reload, and Back and Forward work as usual. You are reading this on one.',
     false),
    ('Edit this task in two tabs at once',
     'Change the title in both tabs and save each. The second save gets a conflict instead of overwriting the first, and keeps what you typed.',
     false),
    ('Switch the theme',
     'Top right. The choice is kept in a cookie, and the server renders the page with it.',
     false),
    ('Create a Placebo app',
     'Done. This demo is src/tasks.rs and its migration; AGENTS.md says how to remove it when you start on your own app.',
     true);
