import type { CandidateStatus, CoverageStatus, ObservationTrack } from './objects';

export type ObjectKind = 'application' | 'folder' | 'file';
export type ListKind = ObjectKind | 'location';
export type CollectionState = 'not_started' | 'running' | 'paused';
export type IntervalState = 'active' | 'inactive' | 'gap' | 'unknown';
export interface Settings { thresholdDays: number; eventRetentionDays: number; autoStartEnabled: boolean }
export interface Scope { id: string; path: string; enabled: boolean; includeDescendants: boolean; isSystemDesktop: boolean }
export interface ExclusionRef { ruleId: string; mode: 'object' | 'folder_tree' }
export interface ExclusionRule { id: string; mode: 'object' | 'folder_tree'; objectId: string | null; label: string; objectType: ObjectKind }
export interface ObjectSummary {
  id: string; objectType: ObjectKind; displayName: string; currentPath: string | null;
  firstSeenAt: string; observationStartedAt: string | null; lastUsedAt: string | null;
  candidateStatus: CandidateStatus; coverageStatus: CoverageStatus; statusReason: string;
  remainingObservationDays: number | null; installationDate: string | null; installationPrecision: string;
  installationMeaning: string; installationSource: string | null; installationValidation: string | null;
  publisher: string | null; displayVersion: string | null; applicationKind: string | null; exclusion: ExclusionRef | null;
}
export interface ObjectQuery {
  objectType: ListKind; keyword: string; candidateStatuses: CandidateStatus[]; coverageStatus: CoverageStatus | null;
  thresholdDays: number; sortBy: 'displayName' | 'lastUsedAt' | 'firstSeenAt' | 'installationDate';
  sortDirection: 'asc' | 'desc'; page: number; pageSize: number;
}
export interface PageResult<T> { items: T[]; total: number; page: number; pageSize: number }
export interface EventView { observedAt: string; eventKind: string; label: string }
export interface EvidenceView { source: string; rawValue: string | null; dateValue: string | null; precision: string; meaning: string; validation: string; observedAt: string }
export interface IdentifierView { kind: string; value: string | null; quality: string; source: string }
export interface Detail { summary: ObjectSummary; identifiers: IdentifierView[]; evidence: EvidenceView[]; recentEvents: EventView[]; track: ObservationTrack | null }
export interface TypeCounts { total: number; counts: Record<CandidateStatus, number> }
export interface Overview { thresholdDays: number; applications: TypeCounts; folders: TypeCounts; files: TypeCounts; observationStartedAt: string | null; recentGaps: ObservationTrack['gaps']; scopes: Scope[] }
export interface ModuleStatus { module: string; state: IntervalState | null; reason: string | null; reasonLabel: string | null; healthy: boolean; lastHeartbeatAt: string | null }
export interface CollectorStatus { collectionState: CollectionState; implemented: boolean; modules: ModuleStatus[]; catalog: { lastRefreshedAt: string | null; complete: boolean | null; runningJobId: string | null }; limitations: string[] }
export interface Job { jobId: string; kind: 'catalog_refresh' | 'export'; status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled'; stage: string; processed: number; total: number | null; message: string | null; resultPath: string | null }
export interface PickedFolder { path: string; selectionToken: string }
export interface ClearPreparation { confirmationToken: string; affectedObjects: number; expiresInSeconds: number }
