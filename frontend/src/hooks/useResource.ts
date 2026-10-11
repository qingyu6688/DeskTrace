import { onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import type { Ref, ShallowRef } from 'vue';

// 查询条件变化立即失效；较慢的旧请求不能覆盖新结果。
export function useResource<T>(key: () => string, enabled: () => boolean, fetcher: () => Promise<T>, revision: () => number = (): number => 0): { data: ShallowRef<T | null>; loading: Ref<boolean>; error: Ref<string>; reload: () => Promise<void> } {
  const data = shallowRef<T | null>(null);
  const loading = ref(false);
  const error = ref('');
  let sequence = 0;
  let disposed = false;
  async function reload(): Promise<void> {
    const ticket = ++sequence;
    error.value = '';
    if (!enabled()) { data.value = null; loading.value = false; return; }
    loading.value = true;
    try { const next = await fetcher(); if (!disposed && sequence === ticket) data.value = next; }
    catch (cause: unknown) { if (!disposed && sequence === ticket) error.value = cause instanceof Error ? cause.message : '查询失败，请重试。'; }
    finally { if (sequence === ticket) loading.value = false; }
  }
  watch([key, enabled, revision], (next, previous): void => { if (next[0] !== previous?.[0] || !next[1]) data.value = null; void reload(); }, { immediate: true });
  onBeforeUnmount((): void => { disposed = true; sequence += 1; });
  return { data, loading, error, reload };
}
