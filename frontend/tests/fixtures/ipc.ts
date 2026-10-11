import type { CollectorStatus, ObjectSummary, Overview, Settings } from '../../src/types/ipc';

export const settings: Settings = { thresholdDays: 90, eventRetentionDays: 180, autoStartEnabled: false };
export const collector: CollectorStatus = { collectionState: 'not_started', implemented: true, modules: [], catalog: { lastRefreshedAt: null, complete: null, runningJobId: null }, limitations: [] };
const counts = { recent: 0, stale_candidate: 0, never_observed: 0, insufficient_observation: 0, unknown: 1, excluded: 0, unavailable: 0 };
export const overview: Overview = { thresholdDays: 90, applications: { total: 1, counts }, folders: { total: 0, counts: { ...counts, unknown: 0 } }, files: { total: 0, counts: { ...counts, unknown: 0 } }, observationStartedAt: null, recentGaps: [], scopes: [] };
export const summary: ObjectSummary = {
  id: '测试对象', objectType: 'application', displayName: '测试应用', currentPath: null,
  firstSeenAt: '2026-10-11T00:00:00Z', observationStartedAt: null, lastUsedAt: null,
  candidateStatus: 'unknown', coverageStatus: 'unknown', statusReason: '尚未开始观察。', remainingObservationDays: null,
  installationDate: null, installationPrecision: 'unknown', installationMeaning: 'unknown', installationSource: null,
  installationValidation: null, publisher: null, displayVersion: null, applicationKind: null, exclusion: null,
};
