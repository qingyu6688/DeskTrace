import { beforeEach, describe, expect, it, vi } from 'vitest';

import { getBootstrapStatus } from './bootstrap';

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => bridge);

describe('初始化状态接口', (): void => {
  beforeEach((): void => { bridge.isTauri.mockReturnValue(true); });

  it('浏览器预览不伪造数据库状态', async (): Promise<void> => {
    bridge.isTauri.mockReturnValue(false);
    expect(await getBootstrapStatus()).toBeNull();
    expect(bridge.invoke).not.toHaveBeenCalled();
  });

  it('业务错误保留后端中文说明', async (): Promise<void> => {
    bridge.invoke.mockResolvedValue({ code: 503, message: '本地存储不可用', data: null });
    await expect(getBootstrapStatus()).rejects.toThrow('本地存储不可用');
  });

  it('传输异常转换为中文且不泄露原始错误', async (): Promise<void> => {
    bridge.invoke.mockRejectedValue(new Error('private transport detail'));
    await expect(getBootstrapStatus()).rejects.toThrow('无法连接桌面程序，请重新打开 DeskTrace。');
  });

  it('成功时返回真实初始化计数', async (): Promise<void> => {
    const status = { appVersion: '0.2.0', platformSupported: true, collectorImplemented: false, database: { schemaVersion: 1, objectCount: 0, eventCount: 0, dataDirectory: '测试目录' }, dedupWindowSeconds: 60, intervalReasonCount: 16 };
    bridge.invoke.mockResolvedValue({ code: 200, message: '查询成功', data: status });
    expect(await getBootstrapStatus()).toEqual(status);
  });
});
