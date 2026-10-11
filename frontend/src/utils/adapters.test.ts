import { describe, expect, it } from 'vitest';
import { summary } from '../../tests/fixtures/ipc';
import { detailView, summaryView } from './adapters';
describe('后端对象展示', (): void => {
  it('直接展示后端状态及依据，不按前端当前日期重新判断', (): void => {
    const item = summaryView({ ...summary, candidateStatus: 'stale_candidate', statusReason: '后端判断依据', coverageStatus: 'partial' });
    expect(item.candidateStatus).toBe('stale_candidate'); expect(item.statusReason).toBe('后端判断依据'); expect(item.coverage).toBe('partial');
  });
  it('文件及来源关闭缺口保留真实类型和原因', (): void => {
    const track = { windowStart: '2026-10-11T00:00:00Z', windowEnd: '2026-10-11T01:00:00Z', lastUsedAt: null, usageDays: [], gaps: [{ module: 'files', state: 'gap' as const, reason: 'source_disabled' as const, reasonLabel: '最近使用项目来源已关闭', startedAt: '2026-10-11T00:00:00Z', endedAt: null }] };
    const item = detailView({ summary: { ...summary, objectType: 'file' }, identifiers: [], evidence: [], recentEvents: [], track });
    expect(item.objectType).toBe('file'); expect(item.track?.gaps[0]?.reason).toBe('source_disabled');
  });
});
