/* eslint-disable vue/one-component-per-file -- 每个用例独立挂载组合函数的宿主组件。 */
import { flushPromises, mount } from '@vue/test-utils';
import { defineComponent, h, nextTick, ref } from 'vue';
import { describe, expect, it, vi } from 'vitest';
import { useResource } from './useResource';
function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((finish): void => { resolve = finish; });
  return { promise, resolve };
}
describe('异步查询切换', (): void => {
  it('慢的旧请求不能覆盖新的查询结果', async (): Promise<void> => {
    const key = ref('旧查询'); const first = deferred<string>(); const second = deferred<string>();
    let resource!: ReturnType<typeof useResource<string>>;
    const wrapper = mount(defineComponent({ setup(): () => ReturnType<typeof h> { resource = useResource((): string => key.value, (): boolean => true, (): Promise<string> => key.value === '旧查询' ? first.promise : second.promise); return (): ReturnType<typeof h> => h('div'); } }));
    key.value = '新查询'; await nextTick(); second.resolve('新结果'); await flushPromises();
    first.resolve('旧结果'); await flushPromises(); expect(resource.data.value).toBe('新结果');
    wrapper.unmount();
  });
  it('断开连接清空旧数据且不发送请求', async (): Promise<void> => {
    const enabled = ref(true); const fetcher = vi.fn().mockResolvedValue('实际数据');
    let resource!: ReturnType<typeof useResource<string>>;
    const wrapper = mount(defineComponent({ setup(): () => ReturnType<typeof h> { resource = useResource((): string => '固定', (): boolean => enabled.value, fetcher); return (): ReturnType<typeof h> => h('div'); } }));
    await flushPromises(); enabled.value = false; await nextTick();
    expect(resource.data.value).toBeNull(); expect(fetcher).toHaveBeenCalledTimes(1); wrapper.unmount();
  });
  it('组件卸载后忽略在途响应', async (): Promise<void> => {
    const pending = deferred<string>(); let resource!: ReturnType<typeof useResource<string>>;
    const wrapper = mount(defineComponent({ setup(): () => ReturnType<typeof h> { resource = useResource((): string => '固定', (): boolean => true, (): Promise<string> => pending.promise); return (): ReturnType<typeof h> => h('div'); } }));
    wrapper.unmount(); pending.resolve('延迟结果'); await flushPromises(); expect(resource.data.value).toBeNull();
  });
});
