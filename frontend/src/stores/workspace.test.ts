import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { collector, overview, settings } from '../../tests/fixtures/ipc';
import * as api from '../api/workspace';
import type { CollectorStatus, Job, Settings } from '../types/ipc';
import { useWorkspaceStore } from './workspace';
const bridge = vi.hoisted(() => ({ desktopAvailable: vi.fn(), subscribe: vi.fn(), refresh: vi.fn() }));
vi.mock('../api/client', () => bridge);
vi.mock('./bootstrap', () => ({ useBootstrapStore: (): { refresh: typeof bridge.refresh } => ({ refresh: bridge.refresh }) }));
vi.mock('../api/workspace', () => ({ getSettings: vi.fn(), updateSettings: vi.fn(), getOverview: vi.fn(), getCollectorStatus: vi.fn(), listScopes: vi.fn(), listExclusions: vi.fn(), setCollectionState: vi.fn(), getJob: vi.fn() }));
describe('真实工作台状态', (): void => {
  beforeEach((): void => {
    vi.useFakeTimers(); setActivePinia(createPinia()); bridge.desktopAvailable.mockReturnValue(true); bridge.subscribe.mockResolvedValue(vi.fn());
    vi.mocked(api.getSettings).mockResolvedValue({ ...settings }); vi.mocked(api.getOverview).mockResolvedValue(overview);
    vi.mocked(api.getCollectorStatus).mockResolvedValue(collector); vi.mocked(api.listScopes).mockResolvedValue([]); vi.mocked(api.listExclusions).mockResolvedValue([]);
  });
  afterEach((): void => { useWorkspaceStore().disconnect(); vi.useRealTimers(); });
  it('浏览器没有样本也不调用后端', async (): Promise<void> => {
    bridge.desktopAvailable.mockReturnValue(false); const store = useWorkspaceStore(); await store.connect();
    expect(store.overview).toBeNull(); expect(store.scopes).toEqual([]); expect(api.getSettings).not.toHaveBeenCalled();
  });
  it('初始化读取持久化设置并订阅三个事件', async (): Promise<void> => {
    vi.mocked(api.getSettings).mockResolvedValue({ ...settings, thresholdDays: 180 });
    const store = useWorkspaceStore(); await store.connect();
    expect(store.thresholdDays).toBe(180); expect(api.getOverview).toHaveBeenCalledWith(180); expect(bridge.subscribe).toHaveBeenCalledTimes(3);
    expect(store.overview).toEqual(overview); expect(store.ready).toBe(true);
  });
  it('保存失败不改变已生效设置', async (): Promise<void> => {
    const store = useWorkspaceStore(); await store.connect();
    vi.mocked(api.updateSettings).mockRejectedValue(new Error('启动项更新失败'));
    await expect(store.saveSettings({ thresholdDays: 180, autoStartEnabled: true })).rejects.toThrow('启动项');
    expect(store.settings).toEqual(settings); expect(store.thresholdDays).toBe(90);
  });
  it('连接尚未完成时重复点击只注册一组订阅', async (): Promise<void> => {
    let finish!: (value: Settings) => void;
    vi.mocked(api.getSettings).mockReturnValue(new Promise<Settings>((resolve): void => { finish = resolve; }));
    const store = useWorkspaceStore(); const connecting = store.connect(); await store.connect();
    expect(api.getSettings).toHaveBeenCalledTimes(1); finish(settings); await connecting;
    expect(bridge.subscribe).toHaveBeenCalledTimes(3);
  });
  it('暂停失败保留后端确认的采集状态', async (): Promise<void> => {
    const store = useWorkspaceStore(); await store.connect();
    vi.mocked(api.setCollectionState).mockRejectedValue(new Error('存储不可用'));
    await expect(store.changeCollection('paused')).rejects.toThrow('存储');
    expect(store.collector?.collectionState).toBe('not_started');
  });
  it('对象通知合并刷新，采集状态和任务通知同步', async (): Promise<void> => {
    const store = useWorkspaceStore(); await store.connect();
    const changed = bridge.subscribe.mock.calls[0]?.[1] as () => void;
    changed(); changed(); await vi.advanceTimersByTimeAsync(300);
    expect(api.getOverview).toHaveBeenCalledTimes(2);
    const statusChanged = bridge.subscribe.mock.calls[1]?.[1] as (status: CollectorStatus) => void;
    statusChanged({ ...collector, collectionState: 'paused' }); expect(store.collector?.collectionState).toBe('paused');
    const jobChanged = bridge.subscribe.mock.calls[2]?.[1] as (job: Job) => void;
    jobChanged({ jobId: '任务', kind: 'export', status: 'completed', stage: '完成', processed: 5, total: 5, message: null, resultPath: null });
    expect(store.jobs[0]?.processed).toBe(5);
  });
  it('退出时注销事件和定时器', async (): Promise<void> => {
    const unlisten = vi.fn(); bridge.subscribe.mockResolvedValue(unlisten); const store = useWorkspaceStore(); await store.connect();
    store.disconnect(); expect(unlisten).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(31000); expect(api.getOverview).toHaveBeenCalledTimes(1);
  });
  it('部分订阅失败会清理成功订阅，重试不重复注册', async (): Promise<void> => {
    const unlisten = vi.fn(); bridge.subscribe.mockResolvedValueOnce(unlisten).mockRejectedValueOnce(new Error('订阅失败')).mockResolvedValueOnce(unlisten);
    const store = useWorkspaceStore(); await store.connect(); expect(unlisten).toHaveBeenCalledTimes(2); expect(store.ready).toBe(false);
    bridge.subscribe.mockResolvedValue(vi.fn()); await store.connect(); expect(store.ready).toBe(true);
  });
});
