import { defineStore } from 'pinia';
import { ref } from 'vue';

import { getBootstrapStatus } from '../api/bootstrap';
import type { BootstrapStatus } from '../types/bootstrap';

export const useBootstrapStore = defineStore('bootstrap', () => {
  const status = ref<BootstrapStatus | null>(null);
  const loading = ref(false);
  const errorMessage = ref('');
  const browserPreview = ref(false);

  async function refresh(): Promise<void> {
    if (loading.value) return;
    loading.value = true;
    errorMessage.value = '';
    browserPreview.value = false;
    try {
      const result = await getBootstrapStatus();
      status.value = result;
      browserPreview.value = result === null;
    } catch (error: unknown) {
      status.value = null;
      errorMessage.value = error instanceof Error ? error.message : '读取状态失败，请重试。';
    } finally {
      loading.value = false;
    }
  }

  return { status, loading, errorMessage, browserPreview, refresh };
});
