<script setup lang="ts">
import { NInputNumber, NSelect } from 'naive-ui';
import { computed, onBeforeUnmount, ref } from 'vue';
import { useWorkspaceStore } from '../stores/workspace';
const workspace = useWorkspaceStore();
const presets = [30, 90, 180];
const customMode = ref(!presets.includes(workspace.thresholdDays));
const selected = computed((): number => customMode.value || !presets.includes(workspace.thresholdDays) ? 0 : workspace.thresholdDays);
const options = [...presets.map((value): { label: string; value: number } => ({ label: value + ' 天', value })), { label: '自定义…', value: 0 }];
let timer: ReturnType<typeof setTimeout> | undefined;
function apply(value: number): void {
  workspace.thresholdDays = value;
  if (timer) clearTimeout(timer);
  timer = setTimeout((): void => { void workspace.refresh(); }, 200);
}
function change(value: number): void { customMode.value = value === 0; if (value !== 0) apply(value); }
function custom(value: number | null): void { if (value !== null && Number.isInteger(value) && value >= 1 && value <= 3650) apply(value); }
onBeforeUnmount((): void => { if (timer) clearTimeout(timer); });
</script>
<template>
  <div class="threshold-control">
    <span class="muted small">阈值</span><NSelect
      aria-label="长期未记录阈值"
      :value="selected"
      :options="options"
      :disabled="!workspace.ready"
      style="width: 112px"
      @update:value="change"
    />
    <NInputNumber
      v-if="selected === 0"
      aria-label="自定义阈值天数"
      :value="workspace.thresholdDays"
      :min="1"
      :max="3650"
      :precision="0"
      :disabled="!workspace.ready"
      style="width: 110px"
      @update:value="custom"
    />
  </div>
</template>
