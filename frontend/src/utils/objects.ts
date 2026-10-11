import type { CandidateStatus, UsageObject } from '../types/objects';

export const DAY_MS = 86_400_000;
export const STATUS_ORDER: CandidateStatus[] = ['stale_candidate', 'never_observed', 'insufficient_observation', 'unknown', 'recent', 'excluded', 'unavailable'];
export const STATUS_LABELS: Record<CandidateStatus, string> = {
  recent: '近期有使用', stale_candidate: '较久未记录', never_observed: '观察期内未记录',
  insufficient_observation: '观察不足', unknown: '无法判断', excluded: '已排除', unavailable: '已失联',
};
export const STATUS_CLASSES: Record<CandidateStatus, string> = {
  recent: 'recent', stale_candidate: 'stale', never_observed: 'never', insufficient_observation: 'insufficient',
  unknown: 'unknown', excluded: 'excluded', unavailable: 'unavailable',
};

export function candidateStatus(item: UsageObject, thresholdDays: number, now: string): CandidateStatus {
  if (item.excluded) return 'excluded';
  if (item.unavailable) return 'unavailable';
  if (item.unknownReason) return 'unknown';
  const current = Date.parse(now);
  const observation = item.observationStartedAt ? Date.parse(item.observationStartedAt) : NaN;
  const last = item.lastUsedAt ? Date.parse(item.lastUsedAt) : null;
  if (!Number.isFinite(current) || !Number.isFinite(observation) || observation > current || (last !== null && (!Number.isFinite(last) || last > current))) return 'unknown';
  if (last !== null && last >= observation && last >= current - thresholdDays * DAY_MS) return 'recent';
  if (current - observation < thresholdDays * DAY_MS) return 'insufficient_observation';
  return item.lastUsedAt ? 'stale_candidate' : 'never_observed';
}

export function observedDays(item: UsageObject, now: string): number | null {
  return item.observationStartedAt ? Math.max(0, Math.floor((Date.parse(now) - Date.parse(item.observationStartedAt)) / DAY_MS)) : null;
}

export function formatTime(value: string | null, dateOnly = false): string {
  if (!value) return '未记录';
  const options: Intl.DateTimeFormatOptions = { timeZone: 'Asia/Shanghai', year: 'numeric', month: '2-digit', day: '2-digit' };
  if (!dateOnly) Object.assign(options, { hour: '2-digit', minute: '2-digit', hour12: false });
  return new Intl.DateTimeFormat('zh-CN', options).format(new Date(value)).replaceAll('/', '-');
}

export function timeNote(item: UsageObject, now: string): string {
  if (item.lastUsedAt) return `${Math.max(0, Math.floor((Date.parse(now) - Date.parse(item.lastUsedAt)) / DAY_MS))} 天前`;
  const days = observedDays(item, now);
  return days === null ? '尚无有效观察起点' : `观察 ${days} 天`;
}

export function compareNullableTime(first: string | null, second: string | null, descending: boolean): number {
  if (first === null) return second === null ? 0 : 1;
  if (second === null) return -1;
  return (Date.parse(first) - Date.parse(second)) * (descending ? -1 : 1);
}

export function filterObjects(items: UsageObject[], keyword: string, status: CandidateStatus | 'all', continuousOnly: boolean, threshold: number, now: string, descending: boolean): UsageObject[] {
  const needle = keyword.trim().toLocaleLowerCase('zh-CN');
  return items.filter((item): boolean => (!needle || `${item.displayName} ${item.path}`.toLocaleLowerCase('zh-CN').includes(needle))
    && (status === 'all' || candidateStatus(item, threshold, now) === status)
    && (!continuousOnly || item.coverage === 'continuous'))
    .sort((first, second): number => compareNullableTime(first.lastUsedAt, second.lastUsedAt, descending) || first.displayName.localeCompare(second.displayName, 'zh-CN'));
}

export function statusCounts(items: UsageObject[], threshold: number, now: string): Record<CandidateStatus, number> {
  const counts: Record<CandidateStatus, number> = { recent: 0, stale_candidate: 0, never_observed: 0, insufficient_observation: 0, unknown: 0, excluded: 0, unavailable: 0 };
  for (const item of items) counts[candidateStatus(item, threshold, now)] += 1;
  return counts;
}

export function explainObject(item: UsageObject, threshold: number, now: string): string {
  const status = candidateStatus(item, threshold, now);
  if (status === 'unknown') return item.unknownReason ?? '尚无有效观察起点，暂时无法判断。';
  if (status === 'excluded') return '此项目已排除，保留已有演示记录；恢复后从新的观察起点重新记录。';
  if (status === 'unavailable') return '演示清单中已失联，需要核对是否仍可访问；不能据此判断已卸载或已删除。';
  if (status === 'insufficient_observation') return `已观察 ${observedDays(item, now)} 天，尚不足 ${threshold} 天，暂不能下结论。`;
  if (status === 'recent') return `最近 ${threshold} 天内有直接记录，${timeNote(item, now)}观察到使用。`;
  const evidence = status === 'stale_candidate' ? `最近一次在 ${timeNote(item, now)}，超过 ${threshold} 天阈值。` : `达到 ${threshold} 天观察期，没有直接使用记录。`;
  return `${evidence}${item.coverage === 'continuous' ? '有效观察期内记录连续。' : '记录有中断，结论只作核对参考。'}`;
}

export function objectsCsv(items: UsageObject[], threshold: number, now: string): string {
  const escape = (value: string): string => `"${(/^[\t\r\n]|^\s*[=+@-]/.test(value) ? `'${value}` : value).replaceAll('"', '""')}"`;
  const rows = [['数据性质', '名称', '路径', '最近使用', '安装或更新日期', '状态', '记录覆盖'], ...items.map((item): string[] => ['演示数据', item.displayName, item.path, item.lastUsedAt ?? '未记录', item.installationDate ?? '未知', STATUS_LABELS[candidateStatus(item, threshold, now)], item.coverage])];
  return '\ufeff' + rows.map((row): string => row.map(escape).join(',')).join('\r\n');
}
