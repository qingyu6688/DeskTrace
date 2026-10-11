-- 0002：观察范围内的文件对象、“最近使用的项目”来源，以及独立的文件采集模块。
-- SQLite 不能修改 CHECK 约束，按官方“重建表”步骤执行：调用方在事务外关闭外键检查，
-- 本脚本在事务内建新表、复制数据、删除旧表、改名并重建索引，提交前做外键一致性检查。

CREATE TABLE tracked_objects_new (
  id TEXT PRIMARY KEY,
  object_type TEXT NOT NULL CHECK (object_type IN ('folder', 'application', 'file')),
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
INSERT INTO tracked_objects_new (id, object_type, identity_key, display_name, current_path, availability, observation_capability, first_seen_at, observation_started_at, last_used_at, observation_generation, created_at, updated_at)
  SELECT id, object_type, identity_key, display_name, current_path, availability, observation_capability, first_seen_at, observation_started_at, last_used_at, observation_generation, created_at, updated_at FROM tracked_objects;
DROP TABLE tracked_objects;
ALTER TABLE tracked_objects_new RENAME TO tracked_objects;
CREATE INDEX idx_objects_type_last_used ON tracked_objects(object_type, last_used_at);
CREATE INDEX idx_objects_observation_started ON tracked_objects(observation_started_at);

CREATE TABLE object_identifiers_new (
  id TEXT PRIMARY KEY,
  object_id TEXT NOT NULL REFERENCES tracked_objects(id) ON DELETE CASCADE,
  identifier_kind TEXT NOT NULL CHECK (identifier_kind IN
    ('folder_file_id', 'folder_path', 'file_id', 'file_path', 'executable', 'uninstall_key', 'package_entry')),
  identifier_key TEXT NOT NULL,
  match_quality TEXT NOT NULL CHECK (match_quality IN ('exact', 'verified', 'unresolved')),
  source TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (identifier_kind, identifier_key)
);
INSERT INTO object_identifiers_new (id, object_id, identifier_kind, identifier_key, match_quality, source, created_at, updated_at)
  SELECT id, object_id, identifier_kind, identifier_key, match_quality, source, created_at, updated_at FROM object_identifiers;
DROP TABLE object_identifiers;
ALTER TABLE object_identifiers_new RENAME TO object_identifiers;
CREATE INDEX idx_identifiers_object ON object_identifiers(object_id);

CREATE TABLE usage_events_new (
  id TEXT PRIMARY KEY,
  object_id TEXT NOT NULL REFERENCES tracked_objects(id) ON DELETE CASCADE,
  event_kind TEXT NOT NULL CHECK (event_kind IN
    ('app_foreground', 'app_active_sample', 'folder_browsed', 'folder_visible_sample', 'file_opened', 'folder_content_opened')),
  source TEXT NOT NULL CHECK (source IN ('foreground_hook', 'foreground_sample', 'explorer_shell', 'recent_items')),
  observed_at INTEGER NOT NULL,
  observation_generation INTEGER NOT NULL CHECK (observation_generation > 0),
  dedup_key TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
INSERT INTO usage_events_new (id, object_id, event_kind, source, observed_at, observation_generation, dedup_key, created_at, updated_at)
  SELECT id, object_id, event_kind, source, observed_at, observation_generation, dedup_key, created_at, updated_at FROM usage_events;
DROP TABLE usage_events;
ALTER TABLE usage_events_new RENAME TO usage_events;
CREATE INDEX idx_usage_object_observed ON usage_events(object_id, observed_at DESC);
CREATE INDEX idx_usage_retention ON usage_events(observed_at);

CREATE TABLE collector_intervals_new (
  id TEXT PRIMARY KEY,
  module TEXT NOT NULL CHECK (module IN ('applications', 'folders', 'files')),
  interval_state TEXT NOT NULL CHECK (interval_state IN ('active', 'inactive', 'gap', 'unknown')),
  reason TEXT NOT NULL CHECK (reason IN (
    'collection_started', 'collection_resumed', 'collector_recovered',
    'session_locked', 'system_suspended', 'user_idle',
    'user_paused', 'collector_fault', 'permission_denied', 'unsupported_view',
    'queue_overflow', 'storage_error', 'source_disabled',
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
    OR (interval_state = 'gap' AND reason IN ('user_paused', 'collector_fault', 'permission_denied', 'unsupported_view', 'queue_overflow', 'storage_error', 'source_disabled'))
    OR (interval_state = 'unknown' AND reason IN ('startup_unverified', 'unexpected_exit', 'collector_stopped', 'clock_change'))
  )
);
INSERT INTO collector_intervals_new (id, module, interval_state, reason, started_at, verified_until, ended_at, created_at, updated_at)
  SELECT id, module, interval_state, reason, started_at, verified_until, ended_at, created_at, updated_at FROM collector_intervals;
DROP TABLE collector_intervals;
ALTER TABLE collector_intervals_new RENAME TO collector_intervals;
CREATE INDEX idx_intervals_module_started ON collector_intervals(module, started_at);
CREATE UNIQUE INDEX idx_intervals_one_open ON collector_intervals(module) WHERE ended_at IS NULL;
