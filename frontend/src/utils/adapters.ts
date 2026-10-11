import type { Detail, ObjectSummary } from '../types/ipc';
import type { UsageObject } from '../types/objects';

const MEANINGS: Record<string, string> = { install_or_service: '安装/维护', install_or_update: '安装/更新', registered: '系统登记', unknown: '含义未知' };
const SOURCES: Record<string, string> = { uninstall_registry: '注册表登记', package_api: '应用包接口' };
export function summaryView(item: ObjectSummary): UsageObject {
  return {
    id: item.id, objectType: item.objectType, displayName: item.displayName, path: item.currentPath ?? '',
    subtitle: item.objectType === 'application' ? [item.displayVersion, item.applicationKind].filter(Boolean).join(' · ') : item.currentPath ?? '位置未知',
    publisher: item.publisher ?? '发布者未知', lastUsedAt: item.lastUsedAt, firstSeenAt: item.firstSeenAt,
    observationStartedAt: item.observationStartedAt, installationDate: item.installationDate,
    installationLabel: [MEANINGS[item.installationMeaning] ?? item.installationMeaning, item.installationSource ? SOURCES[item.installationSource] ?? item.installationSource : '来源未知', item.installationValidation === 'invalid' ? '日期无效' : item.installationValidation === 'future' ? '未来日期，未采用' : ''].filter(Boolean).join(' · '),
    coverage: item.coverageStatus, candidateStatus: item.candidateStatus, statusReason: item.statusReason,
    unknownReason: item.candidateStatus === 'unknown' ? item.statusReason : null, excluded: item.candidateStatus === 'excluded',
    unavailable: item.candidateStatus === 'unavailable', exclusion: item.exclusion, track: null,
  };
}
export function detailView(detail: Detail): UsageObject { return { ...summaryView(detail.summary), track: detail.track }; }
