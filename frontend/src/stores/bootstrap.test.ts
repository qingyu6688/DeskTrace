import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { getBootstrapStatus } from '../api/bootstrap';
import { useBootstrapStore } from './bootstrap';

vi.mock('../api/bootstrap', () => ({ getBootstrapStatus: vi.fn() }));

describe('初始化状态管理', (): void => {
  beforeEach((): void => { setActivePinia(createPinia()); });

  it('浏览器预览显示未连接且结束加载', async (): Promise<void> => {
    vi.mocked(getBootstrapStatus).mockResolvedValue(null);
    const store = useBootstrapStore();
    await store.refresh();
    expect(store.browserPreview).toBe(true);
    expect(store.status).toBeNull();
    expect(store.loading).toBe(false);
  });

  it('错误后清除旧状态并保留可展示说明', async (): Promise<void> => {
    const store = useBootstrapStore();
    store.status = { appVersion: '0.2.0', platformSupported: true, collectorImplemented: false, database: { schemaVersion: 1, objectCount: 2, eventCount: 3, dataDirectory: '测试目录' }, dedupWindowSeconds: 60, intervalReasonCount: 16 };
    vi.mocked(getBootstrapStatus).mockRejectedValue(new Error('本地存储不可用'));
    await store.refresh();
    expect(store.status).toBeNull();
    expect(store.errorMessage).toBe('本地存储不可用');
    expect(store.loading).toBe(false);
  });
});
