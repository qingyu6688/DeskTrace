import type { FolderScope, TrackGap, UsageObject } from '../../src/types/objects';

// 演示时刻固定，日期与用户提供的静态原型一致，不读取本机应用或文件夹。
export const DEMO_NOW = '2026-10-11T10:14:05+08:00';
export const DEMO_START = '2026-06-02T10:21:00+08:00';
export const DEMO_GAPS: TrackGap[] = [
  { state: 'gap', reason: 'user_paused', reasonLabel: '手动暂停', startedAt: '2026-08-21T19:02:00+08:00', endedAt: '2026-08-24T08:40:00+08:00' },
];
export const DEMO_FOLDER_GAPS: TrackGap[] = [
  { state: 'gap', reason: 'collector_fault', reasonLabel: '资源管理器无响应（演示）', startedAt: '2026-07-05T14:20:00+08:00', endedAt: '2026-07-05T14:31:00+08:00' },
];

type Sample = [name: string, subtitle: string, last: string | null, installed: string | null, label: string, extra?: Partial<UsageObject>];
const applications: Sample[] = [
  ['Blender 4.2', '4.2.3 · Win32', '2026-06-18T21:40:00+08:00', '2026-03-02', '安装/维护 · MSI 登记', { publisher: 'Blender Foundation', path: 'C:\\Program Files\\Blender Foundation\\Blender 4.2\\blender.exe' }],
  ['OBS Studio', '31.1.2', '2026-06-25T08:12:00+08:00', '2025-12-14', '系统登记日期'],
  ['Zoom Workplace', '6.5.1', '2026-06-30T16:45:00+08:00', '2026-08-27', '系统登记日期', { excluded: true }],
  ['Microsoft To Do', '2.148 · MSIX', '2026-07-02T09:30:00+08:00', '2026-09-28', '包安装/更新'],
  ['Notepad++', '8.8.5', '2026-10-09T17:26:00+08:00', '2026-09-03', '系统登记日期', { coverage: 'continuous' }],
  ['微信', '4.1.0', '2026-10-11T09:58:00+08:00', null, '登记中没有日期'],
  ['Visual Studio Code', '1.104.2 · 当前用户', '2026-10-11T10:12:00+08:00', '2026-09-30', '系统登记日期'],
  ['Steam', '2.10.91', null, null, '登记中没有日期'],
  ['Clipchamp', '4.3 · MSIX', null, '2026-09-12', '包安装/更新'],
  ['draw.io', '28.1.2 · 便携', null, null, '便携应用，无登记', { observationStartedAt: '2026-09-21T10:14:05+08:00', coverage: 'continuous' }],
  ['Python 3.12.3 (64-bit)', '3.12.3', null, '2024-04-18', '安装/维护 · MSI 登记', { unknownReason: '没有可匹配入口', observationStartedAt: null, path: '' }],
  ['XMind 2023', '23.12', null, '2024-02-07', '系统登记日期', { unavailable: true }],
];
const folders: Sample[] = [
  ['旧网站', 'D:\\Projects\\旧网站', '2026-06-20T18:20:00+08:00', null, '', { coverage: 'continuous' }],
  ['活动素材', 'E:\\素材库\\活动素材', '2026-06-28T16:00:00+08:00', null, ''],
  ['下载', 'D:\\Downloads', '2026-10-11T09:40:00+08:00', null, '', { coverage: 'continuous' }],
  ['待归档', 'D:\\Projects\\待归档', null, null, ''],
  ['新项目', 'D:\\Projects\\新项目', null, null, '', { observationStartedAt: '2026-09-21T10:14:05+08:00', coverage: 'continuous' }],
  ['离线素材', 'E:\\素材库\\离线素材', null, null, '', { unavailable: true }],
];

function makeObjects(samples: Sample[], objectType: UsageObject['objectType']): UsageObject[] {
  return samples.map(([displayName, subtitle, lastUsedAt, installationDate, installationLabel, extra], index): UsageObject => {
    const item: UsageObject = {
      id: `${objectType}-${index + 1}`, objectType, displayName, subtitle,
      path: objectType === 'folder' ? subtitle : `C:\\Program Files\\${displayName}\\app.exe`,
      publisher: '演示清单', lastUsedAt, observationStartedAt: DEMO_START, firstSeenAt: DEMO_START,
      installationDate, installationLabel, coverage: 'partial', unknownReason: null,
      unavailable: false, excluded: false, track: null, ...extra,
    };
    if (item.observationStartedAt) item.track = {
      windowStart: item.observationStartedAt, windowEnd: DEMO_NOW, lastUsedAt,
      usageDays: lastUsedAt ? (index === 0 && objectType === 'application' ? ['2026-06-03', '2026-06-05', '2026-06-11', '2026-06-18'] : [lastUsedAt.slice(0, 10)]) : [],
      gaps: item.coverage === 'continuous' ? [] : (objectType === 'application' ? DEMO_GAPS : DEMO_FOLDER_GAPS),
    };
    return item;
  });
}

export function getDemoObjects(): UsageObject[] {
  return structuredClone([...makeObjects(applications, 'application'), ...makeObjects(folders, 'folder')]);
}

export function getDemoScopes(): FolderScope[] {
  return [
    { id: 'scope-1', path: 'D:\\Projects', includeDescendants: true, enabled: true },
    { id: 'scope-2', path: 'D:\\Downloads', includeDescendants: false, enabled: true },
    { id: 'scope-3', path: 'E:\\素材库', includeDescendants: true, enabled: true },
  ];
}
