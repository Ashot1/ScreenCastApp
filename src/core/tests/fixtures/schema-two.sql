CREATE TABLE settings (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    manual_pause INTEGER NOT NULL CHECK (manual_pause IN (0, 1))
) STRICT;
CREATE TABLE tasks (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    revision INTEGER NOT NULL CHECK (revision > 0),
    text TEXT CHECK (text IS NULL OR length(CAST(text AS BLOB)) BETWEEN 1 AND 16384)
) STRICT;
CREATE TABLE evidence (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    text TEXT CHECK (text IS NULL OR length(CAST(text AS BLOB)) BETWEEN 1 AND 16384),
    origin INTEGER CHECK (origin IN (0, 1)),
    observed_at_utc INTEGER,
    completeness INTEGER CHECK (completeness IN (0, 1, 2)),
    kind INTEGER CHECK (kind IN (0, 1, 2)),
    CHECK ((text IS NULL AND origin IS NULL AND observed_at_utc IS NULL AND completeness IS NULL AND kind IS NULL)
        OR (text IS NOT NULL AND origin IS NOT NULL AND completeness IS NOT NULL AND kind IS NOT NULL AND (origin = 1 OR observed_at_utc IS NOT NULL)))
) STRICT;
CREATE TABLE source_bindings (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    evidence_id INTEGER NOT NULL REFERENCES evidence(id),
    digest BLOB NOT NULL CHECK (length(digest) = 32),
    namespace TEXT,
    scope TEXT,
    object_id TEXT,
    version TEXT,
    UNIQUE (task_id, digest),
    CHECK ((namespace IS NULL AND scope IS NULL AND object_id IS NULL AND version IS NULL)
        OR (namespace IS NOT NULL AND scope IS NOT NULL AND object_id IS NOT NULL
        AND length(CAST(namespace AS BLOB)) BETWEEN 1 AND 64
        AND length(CAST(scope AS BLOB)) BETWEEN 1 AND 256
        AND length(CAST(object_id AS BLOB)) BETWEEN 1 AND 2048
        AND instr(namespace, char(0)) = 0 AND instr(scope, char(0)) = 0 AND instr(object_id, char(0)) = 0
        AND (version IS NULL OR (length(CAST(version AS BLOB)) BETWEEN 1 AND 256 AND instr(version, char(0)) = 0))))
) STRICT;
CREATE TABLE decisions (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    other_task_id INTEGER REFERENCES tasks(id),
    evidence_id INTEGER REFERENCES evidence(id),
    accepted INTEGER NOT NULL CHECK (accepted IN (0, 1)),
    action_at_utc INTEGER NOT NULL,
    CHECK ((other_task_id IS NOT NULL AND evidence_id IS NULL AND task_id < other_task_id)
        OR (other_task_id IS NULL AND evidence_id IS NOT NULL))
) STRICT;
CREATE INDEX evidence_by_task ON evidence(task_id, id);
CREATE INDEX sources_by_digest ON source_bindings(digest);
CREATE UNIQUE INDEX live_source_evidence ON source_bindings(evidence_id) WHERE namespace IS NOT NULL;
CREATE UNIQUE INDEX link_decisions ON decisions(task_id, other_task_id) WHERE other_task_id IS NOT NULL;
CREATE UNIQUE INDEX binding_decisions ON decisions(task_id, evidence_id) WHERE evidence_id IS NOT NULL;
CREATE INDEX decisions_by_evidence ON decisions(evidence_id);
CREATE INDEX decisions_by_other_task ON decisions(other_task_id);
