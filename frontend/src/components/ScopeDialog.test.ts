import { createPinia, setActivePinia } from 'pinia';
import { flushPromises, shallowMount } from '@vue/test-utils';
import { NButton, NModal } from 'naive-ui';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IpcError } from '../api/client';
import * as api from '../api/workspace';
import { useWorkspaceStore } from '../stores/workspace';
import ScopeDialog from './ScopeDialog.vue';
const ui = vi.hoisted(() => ({ error: vi.fn(), warning: vi.fn(), success: vi.fn() }));
vi.mock('naive-ui', async (original) => ({ ...await original<typeof import('naive-ui')>(), useMessage: (): typeof ui => ui }));
vi.mock('../api/workspace', () => ({ pickScope: vi.fn(), addScope: vi.fn() }));
describe('原生范围选择凭证', (): void => {
  beforeEach((): void => {
    setActivePinia(createPinia()); vi.spyOn(useWorkspaceStore(), 'refresh').mockResolvedValue();
    vi.mocked(api.pickScope).mockResolvedValue({ path: 'D:\\测试范围', selectionToken: '后端选择凭证' });
    vi.mocked(api.addScope).mockResolvedValue({ id: '范围', path: 'D:\\测试范围', enabled: true, includeDescendants: true, isSystemDesktop: false });
  });
  async function choose(): Promise<ReturnType<typeof shallowMount<typeof ScopeDialog>>> {
    const wrapper = shallowMount(ScopeDialog, { props: { show: true }, global: { renderStubDefaultSlot: true } });
    await (wrapper.findComponent(NButton).props('onClick') as () => Promise<void>)();
    await flushPromises(); return wrapper;
  }
  it('选择后只提交后端凭证，不提交输入路径', async (): Promise<void> => {
    const wrapper = await choose(); wrapper.findComponent(NModal).vm.$emit('positiveClick'); await flushPromises();
    expect(api.addScope).toHaveBeenCalledWith('后端选择凭证', true); wrapper.unmount();
  });
  it('取消系统选择不会新增范围', async (): Promise<void> => {
    vi.mocked(api.pickScope).mockResolvedValue(null); const wrapper = await choose();
    wrapper.findComponent(NModal).vm.$emit('positiveClick'); await flushPromises();
    expect(api.addScope).not.toHaveBeenCalled(); expect(ui.warning).toHaveBeenCalled(); wrapper.unmount();
  });
  it('过期后清空凭证，必须重新选择', async (): Promise<void> => {
    vi.mocked(api.addScope).mockRejectedValue(new IpcError('选择凭证已过期', 409));
    const wrapper = await choose(); wrapper.findComponent(NModal).vm.$emit('positiveClick'); await flushPromises();
    wrapper.findComponent(NModal).vm.$emit('positiveClick'); await flushPromises();
    expect(api.addScope).toHaveBeenCalledTimes(1); expect(ui.error).toHaveBeenCalledWith('选择凭证已过期'); wrapper.unmount();
  });
});
