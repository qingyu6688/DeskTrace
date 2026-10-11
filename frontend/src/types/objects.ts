export type ObjectType = 'application' | 'folder' | 'file';
export type CandidateStatus = 'recent' | 'stale_candidate' | 'never_observed' | 'insufficient_observation' | 'unknown' | 'excluded' | 'unavailable';
export type CoverageStatus = 'continuous' | 'partial' | 'unknown';

export type TrackGap = {
  module?: string;
  reasonLabel: string;
  startedAt: string;
  endedAt: string | null;
} & (
  | { state: 'gap'; reason: 'user_paused' | 'collector_fault' | 'permission_denied' | 'unsupported_view' | 'queue_overflow' | 'storage_error' | 'source_disabled' }
  | { state: 'unknown'; reason: 'startup_unverified' | 'unexpected_exit' | 'collector_stopped' | 'clock_change' | 'unverified' }
);

export interface ObservationTrack {
  windowStart: string;
  windowEnd: string;
  usageDays: string[];
  lastUsedAt: string | null;
  gaps: TrackGap[];
}

export interface UsageObject {
  candidateStatus?: CandidateStatus;
  statusReason?: string;
  exclusion?: { ruleId: string; mode: 'object' | 'folder_tree' } | null;
  id: string;
  objectType: ObjectType;
  displayName: string;
  subtitle: string;
  path: string;
  publisher: string;
  lastUsedAt: string | null;
  observationStartedAt: string | null;
  firstSeenAt: string;
  installationDate: string | null;
  installationLabel: string;
  coverage: CoverageStatus;
  unknownReason: string | null;
  unavailable: boolean;
  excluded: boolean;
  track: ObservationTrack | null;
}

export interface FolderScope {
  id: string;
  path: string;
  includeDescendants: boolean;
  enabled: boolean;
}
