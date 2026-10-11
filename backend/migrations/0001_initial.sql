PRAGMA foreign_keys = ON;

CREATE TABLE tracked_objects (
  id TEXT PRIMARY KEY,
  object_type TEXT NOT NULL CHECK (object_type IN ('folder', 'application')),
  identity_key TEXT NOT NULL,
  display_name TEXT NOT NULL,
  current_path TEXT,
  availability TEXT NOT NULL DEFAULT 'available'
    CHECK (availability IN ('available', 'unavailable', 'removed', 'unknown')),
  observation_capability TEXT NOT NULL DEFAULT 'unresolved'
    CHECK (observation_capability IN ('supported', 'unresolved', 'unsupported')),
  first_seen_at INTEGER NOT NULL,
  observation_started_at INTEGER,
  last_used_at INTEGER,
  observation_generation INTEGER NOT NULL DEFAULT 1 CHECK (observation_generation > 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (object_type, identity_key)
);

CREATE TABLE object_identifiers (
  id TEXT PRIMARY KEY,
  object_id TEXT NOT NULL REFERENCES tracked_objects(id) ON DELETE CASCADE,
  identifier_kind TEXT NOT NULL CHECK (identifier_kind IN
    ('folder_file_id', 'folder_path', 'executable', 'uninstall_key', 'package_entry')),
  identifier_key TEXT NOT NULL,
  match_quality TEXT NOT NULL CHECK (match_quality IN ('exact', 'verified', 'unresolved')),
  source TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (identifier_kind, identifier_key)
);

CREATE TABLE application_details (
  id TEXT PRIMARY KEY REFERENCES tracked_objects(id) ON DELETE CASCADE,
  application_kind TEXT NOT NULL CHECK (application_kind IN ('win32', 'msix', 'portable', 'unresolved')),
  publisher TEXT,
  display_version TEXT,
  last_catalog_seen_at INTEGER NOT NULL,
  missing_complete_scans INTEGER NOT NULL DEFAULT 0 CHECK (missing_complete_scans >= 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE installation_evidence (
  id TEXT PRIMARY KEY,
  object_id TEXT NOT NULL REFERENCES tracked_objects(id) ON DELETE CASCADE,
  source TEXT NOT NULL CHECK (source IN ('uninstall_registry', 'package_api')),
  source_key TEXT NOT NULL,
  raw_value TEXT,
  date_value TEXT,
  date_precision TEXT NOT NULL CHECK (date_precision IN ('day', 'instant', 'unknown')),
  date_meaning TEXT NOT NULL CHECK (date_meaning IN ('install_or_service', 'install_or_update', 'registered', 'unknown')),
  validation_status TEXT NOT NULL CHECK (validation_status IN ('valid', 'missing', 'invalid', 'future')),
  observed_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK (
    (validation_status = 'valid' AND date_value IS NOT NULL AND date_precision <> 'unknown')
    OR (validation_status <> 'valid' AND date_value IS NULL AND date_precision = 'unknown')
  )
);

CREATE TABLE usage_events (
  id TEXT PRIMARY KEY,
  object_id TEXT NOT NULL REFERENCES tracked_objects(id) ON DELETE CASCADE,
  event_kind TEXT NOT NULL CHECK (event_kind IN
    ('app_foreground', 'app_active_sample', 'folder_browsed', 'folder_visible_sample')),
  source TEXT NOT NULL CHECK (source IN ('foreground_hook', 'foreground_sample', 'explorer_shell')),
  observed_at INTEGER NOT NULL,
  observation_generation INTEGER NOT NULL CHECK (observation_generation > 0),
  dedup_key TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE collector_intervals (
  id TEXT PRIMARY KEY,
  module TEXT NOT NULL CHECK (module IN ('applications', 'folders')),
  interval_state TEXT NOT NULL CHECK (interval_state IN ('active', 'inactive', 'gap', 'unknown')),
  reason TEXT NOT NULL CHECK (reason IN (
    'collection_started', 'collection_resumed', 'collector_recovered',
    'session_locked', 'system_suspended', 'user_idle',
    'user_paused', 'collector_fault', 'permission_denied', 'unsupported_view',
    'queue_overflow', 'storage_error',
    'startup_unverified', 'unexpected_exit', 'collector_stopped', 'clock_change'
  )),
  started_at INTEGER NOT NULL,
  verified_until INTEGER NOT NULL,
  ended_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK (verified_until >= started_at),
  CHECK (ended_at IS NULL OR ended_at >= verified_until),
  CHECK (
    (interval_state = 'active' AND reason IN ('collection_started', 'collection_resumed', 'collector_recovered'))
    OR (interval_state = 'inactive' AND reason IN ('session_locked', 'system_suspended', 'user_idle'))
    OR (interval_state = 'gap' AND reason IN ('user_paused', 'collector_fault', 'permission_denied', 'unsupported_view', 'queue_overflow', 'storage_error'))
    OR (interval_state = 'unknown' AND reason IN ('startup_unverified', 'unexpected_exit', 'collector_stopped', 'clock_change'))
  )
);

CREATE TABLE folder_scopes (
  id TEXT PRIMARY KEY,
  path TEXT NOT NULL,
  normalized_key TEXT NOT NULL UNIQUE,
  include_descendants INTEGER NOT NULL DEFAULT 0 CHECK (include_descendants IN (0, 1)),
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE exclusion_rules (
  id TEXT PRIMARY KEY,
  rule_kind TEXT NOT NULL CHECK (rule_kind IN ('object', 'folder_tree')),
  object_id TEXT REFERENCES tracked_objects(id) ON DELETE CASCADE,
  path TEXT,
  normalized_key TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK (
    (rule_kind = 'object' AND object_id IS NOT NULL AND path IS NULL AND normalized_key IS NULL)
    OR (rule_kind = 'folder_tree' AND object_id IS NULL AND path IS NOT NULL AND normalized_key IS NOT NULL)
  )
);

CREATE TABLE settings (
  id TEXT PRIMARY KEY,
  setting_key TEXT NOT NULL UNIQUE,
  value_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE schema_migrations (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  checksum TEXT NOT NULL,
  applied_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX idx_objects_type_last_used ON tracked_objects(object_type, last_used_at);
CREATE INDEX idx_objects_observation_started ON tracked_objects(observation_started_at);
CREATE INDEX idx_identifiers_object ON object_identifiers(object_id);
CREATE INDEX idx_installation_object_observed ON installation_evidence(object_id, observed_at DESC);
CREATE INDEX idx_usage_object_observed ON usage_events(object_id, observed_at DESC);
CREATE INDEX idx_usage_retention ON usage_events(observed_at);
CREATE INDEX idx_intervals_module_started ON collector_intervals(module, started_at);
CREATE UNIQUE INDEX idx_intervals_one_open ON collector_intervals(module) WHERE ended_at IS NULL;
CREATE UNIQUE INDEX idx_exclusion_object ON exclusion_rules(object_id) WHERE rule_kind = 'object';
CREATE UNIQUE INDEX idx_exclusion_tree ON exclusion_rules(normalized_key) WHERE rule_kind = 'folder_tree';
