CREATE TABLE settings (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    manual_pause INTEGER NOT NULL CHECK (manual_pause IN (0, 1))
) STRICT;
CREATE TABLE tasks (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    revision INTEGER NOT NULL CHECK (revision > 0),
    assistant_workspace_revision INTEGER NOT NULL DEFAULT 1 CHECK (assistant_workspace_revision > 0),
    text TEXT CHECK (text IS NULL OR length(CAST(text AS BLOB)) BETWEEN 1 AND 16384),
    lifecycle INTEGER NOT NULL DEFAULT 0 CHECK (lifecycle IN (0, 1)),
    initiative_suppressed INTEGER NOT NULL DEFAULT 0 CHECK (initiative_suppressed IN (0, 1)),
    plan_completeness INTEGER NOT NULL DEFAULT 0 CHECK (plan_completeness IN (0, 1, 2))
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
CREATE TABLE plan_steps (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    description TEXT CHECK (description IS NULL OR length(CAST(description AS BLOB)) BETWEEN 1 AND 16384),
    prompt TEXT CHECK (prompt IS NULL OR length(CAST(prompt AS BLOB)) BETWEEN 1 AND 16384),
    reference TEXT CHECK (reference IS NULL OR (length(CAST(reference AS BLOB)) BETWEEN 1 AND 2048 AND instr(reference, char(0)) = 0)),
    completed_at_utc INTEGER,
    UNIQUE (id, task_id),
    CHECK (description IS NOT NULL OR (prompt IS NULL AND reference IS NULL AND completed_at_utc IS NULL))
) STRICT;
CREATE TABLE plan_checks (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    step_id INTEGER NOT NULL,
    description TEXT CHECK (description IS NULL OR length(CAST(description AS BLOB)) BETWEEN 1 AND 16384),
    result INTEGER CHECK (result IN (0, 1, 2)),
    confirmed_at_utc INTEGER,
    FOREIGN KEY (step_id, task_id) REFERENCES plan_steps(id, task_id),
    CHECK ((description IS NULL AND result IS NULL AND confirmed_at_utc IS NULL)
        OR (description IS NOT NULL AND result IS NOT NULL AND ((result = 0 AND confirmed_at_utc IS NULL) OR (result IN (1, 2) AND confirmed_at_utc IS NOT NULL))))
) STRICT;
CREATE TABLE plan_dependencies (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    predecessor INTEGER,
    dependent INTEGER,
    FOREIGN KEY (predecessor, task_id) REFERENCES plan_steps(id, task_id),
    FOREIGN KEY (dependent, task_id) REFERENCES plan_steps(id, task_id),
    CHECK ((predecessor IS NULL AND dependent IS NULL) OR (predecessor IS NOT NULL AND dependent IS NOT NULL AND predecessor != dependent))
) STRICT;
CREATE TABLE useful_references (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    text TEXT CHECK (text IS NULL OR (length(CAST(text AS BLOB)) BETWEEN 1 AND 2048 AND instr(text, char(0)) = 0))
) STRICT;
CREATE INDEX steps_by_task ON plan_steps(task_id, id);
CREATE INDEX checks_by_step ON plan_checks(step_id, id);
CREATE INDEX dependencies_by_task ON plan_dependencies(task_id, id);
CREATE INDEX dependencies_by_predecessor ON plan_dependencies(predecessor) WHERE predecessor IS NOT NULL;
CREATE UNIQUE INDEX live_dependencies ON plan_dependencies(predecessor, dependent) WHERE predecessor IS NOT NULL;
CREATE INDEX references_by_task ON useful_references(task_id, id);
CREATE TABLE connection_checks (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    purpose INTEGER NOT NULL CHECK (purpose IN (1, 2)),
    at_utc INTEGER NOT NULL,
    stage INTEGER NOT NULL CHECK (stage IN (1, 2)),
    outcome INTEGER NOT NULL CHECK (outcome BETWEEN 0 AND 9),
    expected_model INTEGER NOT NULL CHECK (expected_model = 1),
    expected_effort INTEGER NOT NULL CHECK (expected_effort IN (1, 2)),
    expected_tier INTEGER NOT NULL CHECK (expected_tier = 1),
    actual_model INTEGER NOT NULL CHECK (actual_model BETWEEN 0 AND 2),
    actual_effort INTEGER NOT NULL CHECK (actual_effort BETWEEN 0 AND 3),
    actual_tier INTEGER NOT NULL CHECK (actual_tier BETWEEN 0 AND 4),
    input_tokens INTEGER,
    output_tokens INTEGER,
    total_tokens INTEGER,
    CHECK (purpose = 1 OR expected_effort = 2),
    CHECK ((stage = 1 AND outcome = 0 AND input_tokens IS NULL) OR (stage = 2 AND outcome != 0)),
    CHECK ((input_tokens IS NULL AND output_tokens IS NULL AND total_tokens IS NULL)
        OR (input_tokens IS NOT NULL AND output_tokens IS NOT NULL AND total_tokens IS NOT NULL
        AND input_tokens >= 0 AND output_tokens >= 0 AND total_tokens >= 0
        AND input_tokens <= total_tokens AND output_tokens = total_tokens - input_tokens))
) STRICT;

CREATE TABLE local_source_clock (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    generation INTEGER NOT NULL CHECK (generation >= 0)
) STRICT;
CREATE TABLE local_roots (
    path TEXT PRIMARY KEY CHECK (length(CAST(path AS BLOB)) BETWEEN 1 AND 4096 AND instr(path, char(0)) = 0),
    revision INTEGER NOT NULL UNIQUE CHECK (revision > 0)
) STRICT;
CREATE TABLE local_exclusions (
    root_path TEXT NOT NULL REFERENCES local_roots(path),
    position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 15),
    prefix TEXT NOT NULL CHECK (length(CAST(prefix AS BLOB)) BETWEEN 1 AND 1024 AND instr(prefix, char(0)) = 0),
    PRIMARY KEY (root_path, position),
    UNIQUE (root_path, prefix)
) STRICT;
CREATE TABLE codex_selections (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    revision INTEGER NOT NULL UNIQUE CHECK (revision > 0),
    path TEXT NOT NULL CHECK (length(CAST(path AS BLOB)) BETWEEN 1 AND 4096 AND instr(path, char(0)) = 0)
) STRICT;
CREATE TABLE codex_metadata (
    binding_id INTEGER PRIMARY KEY REFERENCES source_bindings(id),
    selection_id INTEGER NOT NULL CHECK (selection_id > 0),
    settings_revision INTEGER NOT NULL CHECK (settings_revision > 0),
    accepted_ordinal BLOB NOT NULL CHECK (length(accepted_ordinal) = 8),
    turn_id TEXT NOT NULL CHECK (length(CAST(turn_id AS BLOB)) BETWEEN 1 AND 2048 AND instr(turn_id, char(0)) = 0),
    item_id TEXT CHECK (item_id IS NULL OR (length(CAST(item_id AS BLOB)) BETWEEN 1 AND 2048 AND instr(item_id, char(0)) = 0)),
    kind INTEGER NOT NULL CHECK ((kind BETWEEN 1 AND 5 AND item_id IS NOT NULL) OR (kind BETWEEN 6 AND 9 AND item_id IS NULL))
) STRICT;

CREATE TABLE assistant_workspaces (
    task_id INTEGER PRIMARY KEY REFERENCES tasks(id),
    task_revision INTEGER NOT NULL CHECK (task_revision > 0),
    selected TEXT NOT NULL CHECK (length(CAST(selected AS BLOB)) BETWEEN 2 AND 1024),
    sources TEXT NOT NULL CHECK (length(CAST(sources AS BLOB)) BETWEEN 2 AND 262144)
) STRICT;
CREATE TABLE assistant_proposals (
    id INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    task_id INTEGER NOT NULL REFERENCES assistant_workspaces(task_id) ON DELETE CASCADE,
    content_revision INTEGER NOT NULL CHECK (content_revision > 0),
    kind INTEGER NOT NULL CHECK (kind IN (1, 2)),
    message TEXT NOT NULL CHECK (length(CAST(message AS BLOB)) BETWEEN 1 AND 16384),
    rationale TEXT NOT NULL CHECK (length(CAST(rationale AS BLOB)) BETWEEN 1 AND 8192),
    citations TEXT NOT NULL CHECK (length(CAST(citations AS BLOB)) BETWEEN 2 AND 8192),
    step_id INTEGER,
    visible INTEGER NOT NULL CHECK (visible IN (0, 1)),
    FOREIGN KEY (step_id, task_id) REFERENCES plan_steps(id, task_id)
) STRICT;
CREATE INDEX assistant_proposals_by_task ON assistant_proposals(task_id, id);
CREATE TABLE assistant_replies (
    task_id INTEGER NOT NULL REFERENCES assistant_workspaces(task_id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 31),
    id TEXT NOT NULL UNIQUE CHECK (length(CAST(id AS BLOB)) BETWEEN 1 AND 64),
    proposal_id INTEGER NOT NULL CHECK (proposal_id > 0),
    content_revision INTEGER NOT NULL CHECK (content_revision > 0),
    model_question TEXT NOT NULL CHECK (length(CAST(model_question AS BLOB)) BETWEEN 1 AND 16384),
    user_reply TEXT NOT NULL CHECK (length(CAST(user_reply AS BLOB)) BETWEEN 1 AND 16384),
    PRIMARY KEY (task_id, position)
) STRICT;

CREATE TABLE store_identity (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    identity BLOB NOT NULL CHECK (length(identity) = 16)
) STRICT;
CREATE TABLE integration_profiles (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    kind TEXT NOT NULL CHECK (kind IN ('github', 'jira')),
    settings_revision INTEGER NOT NULL CHECK (settings_revision > 0),
    credential_generation INTEGER NOT NULL CHECK (credential_generation > 0),
    name TEXT NOT NULL CHECK (length(CAST(name AS BLOB)) BETWEEN 1 AND 256),
    scope TEXT CHECK (scope IS NULL OR length(CAST(scope AS BLOB)) BETWEEN 1 AND 128),
    config TEXT NOT NULL CHECK (length(CAST(config AS BLOB)) BETWEEN 2 AND 16384),
    not_before_utc INTEGER,
    server_deadline_known INTEGER NOT NULL CHECK (server_deadline_known IN (0,1))
) STRICT;
CREATE UNIQUE INDEX integration_profile_scopes ON integration_profiles(kind, scope);
CREATE TABLE integration_exclusions (
    kind TEXT NOT NULL CHECK (kind IN ('github', 'jira')),
    scope TEXT NOT NULL CHECK (length(CAST(scope AS BLOB)) BETWEEN 1 AND 128),
    PRIMARY KEY (kind, scope)
) STRICT;
CREATE TABLE integration_metadata (
    binding_id INTEGER PRIMARY KEY REFERENCES source_bindings(id),
    data TEXT NOT NULL CHECK (length(CAST(data AS BLOB)) BETWEEN 2 AND 16384)
) STRICT;
CREATE TABLE integration_switches (
    kind TEXT PRIMARY KEY CHECK (kind IN ('local-projects', 'codex', 'github', 'jira')),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
) STRICT;
