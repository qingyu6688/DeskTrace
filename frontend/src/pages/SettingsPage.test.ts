import { createPinia, setActivePinia } from 'pinia';
import { flushPromises, shallowMount } from '@vue/test-utils';
import { NButton, NModal } from 'naive-ui';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../api/workspace';
import { IpcError } from '../api/client';
import { useWorkspaceStore } from '../stores/workspace';
import SettingsPage from './SettingsPage.vue';
const ui = vi.hoisted(() => ({ error: vi.fn(), warning: vi.fn(), success: vi.fn(), dialog: vi.fn() }));
vi.mock('naive-ui', async (original) => ({ ...await original<typeof import('naive-ui')>(), useMessage: (): typeof ui => ui, useDialog: (): { warning: typeof ui.dialog } => ({ warning: ui.dialog }) }));
vi.mock('../api/workspace', () => ({ prepareClear: vi.fn(), clearUsage: vi.fn() }));
describe('清除记录确认流程', (): void => {
  beforeEach((): void => {
    setActivePinia(createPinia()); const store = useWorkspaceStore(); store.ready = true;
    vi.spyOn(store, 'refresh').mockResolvedValue();
    vi.mocked(api.prepareClear).mockResolvedValue({ confirmationToken: '后端凭证', affectedObjects: 7, expiresInSeconds: 60 });
    vi.mocked(api.clearUsage).mockResolvedValue({ cleared: true });
  });
  afterEach((): void => { vi.useRealTimers(); });
  async function openDialog(): Promise<ReturnType<typeof shallowMount<typeof SettingsPage>>> {
    const wrapper = shallowMount(SettingsPage, { global: { renderStubDefaultSlot: true, stubs: { RouterLink: true, NButton: false } } });
    const button = wrapper.findAllComponents(NButton).find((item): boolean => item.text().includes('清除本工具记录'));
    expect(typeof button!.props('onClick')).toBe('function');
    expect(button!.props('disabled')).toBe(false);
    await (button!.props('onClick') as () => Promise<void>)(); await flushPromises();
    expect(api.prepareClear).toHaveBeenCalled(); expect(ui.error.mock.calls).toEqual([]);
    return wrapper;
  }
  it('准备只读取影响范围，取消不执行清除', async (): Promise<void> => {
    const wrapper = await openDialog(); expect(api.clearUsage).not.toHaveBeenCalled();
    const modal = wrapper.findComponent(NModal); expect(modal.props('show')).toBe(true);
    modal.vm.$emit('update:show', false); await flushPromises(); expect(api.clearUsage).not.toHaveBeenCalled(); wrapper.unmount();
  });
  it('明确确认后才传后端凭证清除', async (): Promise<void> => {
    const wrapper = await openDialog(); wrapper.findComponent(NModal).vm.$emit('positiveClick'); await flushPromises();
    expect(api.clearUsage).toHaveBeenCalledWith('后端凭证'); expect(ui.success).toHaveBeenCalled(); wrapper.unmount();
  });
  it('凭证超时不提交清除', async (): Promise<void> => {
    const wrapper = await openDialog(); const clock = vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 61000);
    wrapper.findComponent(NModal).vm.$emit('positiveClick'); await flushPromises();
    expect(api.clearUsage).not.toHaveBeenCalled(); expect(ui.warning).toHaveBeenCalled(); clock.mockRestore(); wrapper.unmount();
  });
  it('数据已变化的冲突需要重新准备，不自动重试清除', async (): Promise<void> => {
    vi.mocked(api.clearUsage).mockRejectedValue(new IpcError('数据发生变化，请重新确认', 409));
    const wrapper = await openDialog(); wrapper.findComponent(NModal).vm.$emit('positiveClick'); await flushPromises();
    expect(api.clearUsage).toHaveBeenCalledTimes(1); expect(wrapper.findComponent(NModal).props('show')).toBe(false);
    expect(ui.error).toHaveBeenCalledWith('数据发生变化，请重新确认'); wrapper.unmount();
  });
});
