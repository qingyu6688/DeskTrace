import { describe, expect, it } from 'vitest';
import type { ObservationTrack, TrackGap } from '../types/objects';
import { trackGeometry, windowedTrack } from './track';

const track: ObservationTrack = { windowStart: '2026-06-02T00:00:00+08:00', windowEnd: '2026-10-11T00:00:00+08:00', usageDays: ['2026-06-03', '2026-06-03', '2026-06-18'], lastUsedAt: '2026-06-18T21:40:00+08:00', gaps: [] };
describe('观察轨迹', (): void => {
  it('日期去重、阈值按时间计算，不能把缺口当记录', (): void => {
    const result = trackGeometry(track, 90, 280)!;
    expect(result.marks).toHaveLength(2);
    expect(result.threshold).toBeCloseTo(41 / 131 * 100);
    expect(result.last).toBeLessThan(result.threshold!);
    expect(result.gaps).toEqual([]);
  });
  it('观察不足不画阈值，更早的聚合时间贴左端', (): void => {
    const result = trackGeometry({ ...track, lastUsedAt: '2025-01-01T00:00:00+08:00' }, 180, 280)!;
    expect(result.threshold).toBeNull(); expect(result.last).toBe(0); expect(result.earlier).toBe(true);
  });
  it('同像素短缺口合并，不越过窗口，未知状态独立保留', (): void => {
    const gaps: TrackGap[] = [
      { state: 'gap' as const, reason: 'user_paused', reasonLabel: '暂停', startedAt: '2026-08-01T10:00:00+08:00', endedAt: '2026-08-01T10:01:00+08:00' },
      { state: 'gap' as const, reason: 'collector_fault', reasonLabel: '故障', startedAt: '2026-08-01T10:02:00+08:00', endedAt: '2026-08-01T10:03:00+08:00' },
      { state: 'unknown' as const, reason: 'unexpected_exit', reasonLabel: '异常退出', startedAt: '2026-10-10T23:59:00+08:00', endedAt: null },
    ];
    const result = trackGeometry({ ...track, gaps }, 90, 280)!;
    expect(result.gaps).toHaveLength(2); expect(result.gaps[0]?.label).toBe('暂停、故障');
    expect(result.gaps.every((gap): boolean => gap.width >= 2 / 280 * 100 - 0.0001 && gap.left + gap.width <= 100)).toBe(true);
  });
  it('零长度与非法窗口不画伪造坐标', (): void => {
    expect(trackGeometry({ ...track, windowStart: track.windowEnd }, 90, 280)).toBeNull();
    expect(trackGeometry({ ...track, windowEnd: '无效日期' }, 90, 280)).toBeNull();
  });
  it('当天上午的记录仍显示当天刻度', (): void => {
    const result = trackGeometry({ ...track, windowEnd: '2026-10-11T10:14:05+08:00', usageDays: ['2026-10-11'] }, 90, 280)!;
    expect(result.marks).toEqual([100]);
  });
  it('窗口跨午夜但不足一天时，两个自然日各保留一条刻度', (): void => {
    const result = trackGeometry({ ...track, windowStart: '2026-10-10T10:21:00+08:00', windowEnd: '2026-10-11T10:14:05+08:00', usageDays: ['2026-10-10', '2026-10-11'] }, 90, 280)!;
    expect(result.marks).toHaveLength(2);
  });
  it('保留窗口隐藏旧明细，但保留更早的最近时间且不修改原数据', (): void => {
    const original: ObservationTrack = { ...track, usageDays: ['2026-06-03', '2026-10-09'], gaps: [{ state: 'gap', reason: 'user_paused', reasonLabel: '暂停', startedAt: '2026-08-01T00:00:00+08:00', endedAt: '2026-08-02T00:00:00+08:00' }] };
    const result = windowedTrack(original, 30);
    expect(result.usageDays).toEqual(['2026-10-09']);
    expect(result.gaps).toEqual([]);
    expect(result.lastUsedAt).toBe(track.lastUsedAt);
    expect(trackGeometry(result, 90, 280)?.earlier).toBe(true);
    expect(original.usageDays).toHaveLength(2);
  });
  it('超过一年按周合并，同一周内多天不重复画点', (): void => {
    const result = trackGeometry({ ...track, windowStart: '2025-01-01T00:00:00+08:00', usageDays: ['2025-01-02', '2025-01-03', '2025-01-10'] }, 90, 280)!;
    expect(result.weekly).toBe(true);
    expect(result.marks).toHaveLength(2);
  });
});
