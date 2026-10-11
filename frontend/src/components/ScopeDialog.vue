<script setup lang="ts">
import { NButton, NCheckbox, NInput, NModal, useMessage } from 'naive-ui';
import { ref, watch } from 'vue';
import { IpcError } from '../api/client';
import * as api from '../api/workspace';
import { useWorkspaceStore } from '../stores/workspace';
import type { PickedFolder } from '../types/ipc';
const show = defineModel<boolean>('show', { default: false });
const selected = ref<PickedFolder | null>(null);
const descendants = ref(true);
const busy = ref(false);
const workspace = useWorkspaceStore();
const message = useMessage();
watch(show, (value): void => { if (value) selected.value = null; });
async function pick(): Promise<void> {
  busy.value = true;
  try { const folder = await api.pickScope(); if (folder) selected.value = folder; }
  catch (cause: unknown) { message.error(cause instanceof Error ? cause.message : '选择文件夹失败。'); }
  finally { busy.value = false; }
}
async function add(): Promise<boolean> {
  if (!selected.value) { message.warning('请先选择文件夹。'); return false; }
  busy.value = true;
  try {
    await api.addScope(selected.value.selectionToken, descendants.value);
    await workspace.refresh(); message.success('已添加观察范围。'); return true;
  } catch (cause: unknown) {
    if (cause instanceof IpcError && cause.code === 409) selected.value = null;
    message.error(cause instanceof Error ? cause.message : '添加范围失败，请重新选择。'); return false;
  } finally { busy.value = false; }
}
</script>
<template>
  <NModal
    v-model:show="show"
    preset="dialog"
    :closable="!busy"
    :mask-closable="!busy"
    :close-on-esc="!busy"
    title="添加观察范围"
    positive-text="添加"
    negative-text="取消"
    :positive-button-props="{ loading: busy, disabled: busy || !selected }"
    :negative-button-props="{ disabled: busy }"
    @positive-click="add"
  >
    <p class="muted modal-intro">
      选择需要观察的文件夹。选择凭证在 60 秒后失效，过期时请重新选择。
    </p>
    <NInput
      :value="selected?.path ?? ''"
      readonly
      aria-label="已选择的范围路径"
      placeholder="请通过系统窗口选择文件夹"
    />
    <NButton
      class="scope-checkbox"
      :loading="busy"
      @click="pick"
    >
      选择文件夹…
    </NButton>
    <NCheckbox
      v-model:checked="descendants"
      class="scope-checkbox"
      :disabled="busy"
    >
      包含子目录
    </NCheckbox>
  </NModal>
</template>
