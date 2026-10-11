import { beforeEach, describe, expect, it, vi } from 'vitest';
import { request, subscribe } from './client';
import * as api from './workspace';
import type { ObjectQuery } from '../types/ipc';
const bridge = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => bridge);
vi.mock('@tauri-apps/api/event', () => ({ listen: bridge.listen }));
describe('桌面接口边界', (): void => {
  beforeEach((): void => { bridge.isTauri.mockReturnValue(true); bridge.invoke.mockResolvedValue({ code: 200, message: '成功', data: {} }); });
  it('浏览器不能回退成样本或触发命令', async (): Promise<void> => {
    bridge.isTauri.mockReturnValue(false);
    await expect(api.getSettings()).rejects.toThrow('桌面');
    expect(bridge.invoke).not.toHaveBeenCalled();
  });
  it('保留业务错误码供凭证过期处理', async (): Promise<void> => {
    bridge.invoke.mockResolvedValue({ code: 409, message: '确认凭证已过期', data: null });
    await expect(request('clear_usage_data')).rejects.toMatchObject({ code: 409, message: '确认凭证已过期' });
  });
  it('传输错误转换为中文', async (): Promise<void> => {
    bridge.invoke.mockRejectedValue(new Error('内部堆栈'));
    await expect(api.getOverview(90)).rejects.toThrow('桌面请求失败');
  });
  it('目录只传选择凭证，更新范围参数与后端一致', async (): Promise<void> => {
    await api.addScope('选择凭证', true); await api.updateScope('范围', false, true);
    expect(bridge.invoke).toHaveBeenNthCalledWith(1, 'add_folder_scope', { selectionToken: '选择凭证', includeDescendants: true });
    expect(bridge.invoke).toHaveBeenNthCalledWith(2, 'update_folder_scope', { scopeId: '范围', enabled: false, includeDescendants: true });
  });
  it('导出传完整筛选与字段白名单，包含服务端其他分页', async (): Promise<void> => {
    const query: ObjectQuery = { objectType: 'location', keyword: '测试', candidateStatuses: ['unknown'], coverageStatus: null, thresholdDays: 90, page: 2, pageSize: 50, sortBy: 'lastUsedAt', sortDirection: 'desc' };
    await api.exportObjects(query, ['displayName', 'path']);
    expect(bridge.invoke).toHaveBeenCalledWith('request_export', { query, confirmedFields: ['displayName', 'path'] });
  });
  it('清除先准备，再用返回的凭证确认', async (): Promise<void> => {
    bridge.invoke.mockResolvedValueOnce({ code: 200, data: { confirmationToken: '清除凭证', affectedObjects: 2, expiresInSeconds: 60 } });
    const preparation = await api.prepareClear();
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    await api.clearUsage(preparation.confirmationToken);
    expect(bridge.invoke).toHaveBeenLastCalledWith('clear_usage_data', { confirmationToken: '清除凭证' });
  });
  it('事件只向处理函数传递载荷，并保留注销函数', async (): Promise<void> => {
    const unlisten = vi.fn(); const callback = vi.fn();
    bridge.listen.mockResolvedValue(unlisten);
    expect(await subscribe('job-progress', callback)).toBe(unlisten);
    const handler = bridge.listen.mock.calls[0]?.[1] as (event: { payload: unknown }) => void;
    handler({ payload: { status: 'completed' } }); expect(callback).toHaveBeenCalledWith({ status: 'completed' });
  });
});
