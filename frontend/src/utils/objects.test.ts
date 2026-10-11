import { describe, expect, it } from 'vitest';
import { DEMO_NOW, getDemoObjects } from '../../tests/fixtures/demo';
import { DAY_MS, candidateStatus, compareNullableTime, filterObjects, formatTime, objectsCsv, statusCounts } from './objects';

describe('演示查询规则', (): void => {
  it('阈值边界包含当前毫秒，未知与观察不足不误判为长期未用', (): void => {
    const item = getDemoObjects()[0]!;
    item.lastUsedAt = new Date(Date.parse(DEMO_NOW) - 90 * DAY_MS).toISOString();
    expect(candidateStatus(item, 90, DEMO_NOW)).toBe('recent');
    item.lastUsedAt = new Date(Date.parse(item.lastUsedAt) - 1).toISOString();
    expect(candidateStatus(item, 90, DEMO_NOW)).toBe('stale_candidate');
    item.observationStartedAt = DEMO_NOW;
    expect(candidateStatus(item, 90, DEMO_NOW)).toBe('insufficient_observation');
    item.unknownReason = '无法匹配';
    expect(candidateStatus(item, 90, DEMO_NOW)).toBe('unknown');
  });
  it('未知时间无论升降序都置后', (): void => {
    for (const descending of [true, false]) {
      expect(compareNullableTime(null, DEMO_NOW, descending)).toBeGreaterThan(0);
      expect(compareNullableTime(DEMO_NOW, null, descending)).toBeLessThan(0);
    }
  });
  it('UTC 时刻跨北京时间午夜时，日期和详细时间一致', (): void => {
    expect(formatTime('2026-06-01T18:21:00Z', true)).toBe('2026-06-02');
    expect(formatTime('2026-06-01T18:21:00Z')).toBe('2026-06-02 02:21');
  });
  it('名称路径搜索、状态和覆盖筛选组合生效且不修改样本', (): void => {
    const items = getDemoObjects();
    const firstId = items[0]?.id;
    const result = filterObjects(items, 'old', 'all', false, 90, DEMO_NOW, false);
    expect(result).toEqual([]);
    expect(filterObjects(items, 'BLENDER', 'stale_candidate', false, 90, DEMO_NOW, false)).toHaveLength(1);
    expect(filterObjects(items, '', 'all', true, 90, DEMO_NOW, false).every((item): boolean => item.coverage === 'continuous')).toBe(true);
    expect(items[0]?.id).toBe(firstId);
  });
  it('分布统计全量，阈值修改后总数不变且状态重算', (): void => {
    const items = getDemoObjects().filter((item): boolean => item.objectType === 'application');
    const first = statusCounts(items, 90, DEMO_NOW);
    const second = statusCounts(items, 180, DEMO_NOW);
    expect(Object.values(first).reduce((sum, value): number => sum + value, 0)).toBe(12);
    expect(second.insufficient_observation).toBeGreaterThan(first.insufficient_observation);
  });
  it('CSV 标记演示数据、转义逗号引号及阻断公式', (): void => {
    const item = getDemoObjects()[0]!;
    item.displayName = '=HYPERLINK("示例", "值")';
    const csv = objectsCsv([item], 90, DEMO_NOW);
    expect(csv.startsWith('\ufeff')).toBe(true);
    expect(csv).toContain('演示数据');
    expect(csv).toContain('"\'=HYPERLINK(""示例"", ""值"")"');
    item.displayName = '  =1+1';
    expect(objectsCsv([item], 90, DEMO_NOW)).toContain("'  =1+1");
  });
});
