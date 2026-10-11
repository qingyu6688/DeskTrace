<script setup lang="ts">
import { computed } from 'vue';
import { useWorkspaceStore } from '../stores/workspace';
import type { ObjectType } from '../types/objects';
import { STATUS_CLASSES, STATUS_LABELS, STATUS_ORDER } from '../utils/objects';
const props = defineProps<{ objectType: ObjectType }>();
const workspace = useWorkspaceStore();
const result = computed(() => props.objectType === 'application' ? workspace.overview?.applications : props.objectType === 'folder' ? workspace.overview?.folders : workspace.overview?.files);
const title = computed((): string => ({ application: '应用', folder: '文件夹', file: '文件' })[props.objectType]);
const classes: Record<string, string> = { recent: 'recent', stale: 'stale', never: 'never', insufficient: 'insufficient', unknown: 'unknown', excluded: 'other', unavailable: 'other' };
const description = computed((): string => STATUS_ORDER.map((status): string => STATUS_LABELS[status] + ' ' + (result.value?.counts[status] ?? 0)).join('，'));
</script>
<template>
  <div class="dist">
    <div class="dist-head">
      <span class="primary-text">{{ title }}</span><span class="mono muted">{{ result?.total ?? 0 }} 个</span>
    </div>
    <div
      v-if="result?.total"
      class="dist-bar"
      role="img"
      :aria-label="description"
    >
      <template
        v-for="status in STATUS_ORDER"
        :key="status"
      >
        <span
          v-if="result.counts[status]"
          class="dist-seg"
          :class="'seg-' + classes[STATUS_CLASSES[status]]"
          :style="{ flex: result.counts[status] + ' 1 0' }"
          :title="STATUS_LABELS[status] + ' ' + result.counts[status]"
        />
      </template>
    </div><p
      v-else
      class="small muted"
    >
      {{ workspace.connected ? '尚无此类对象。' : '连接桌面程序后显示。' }}
    </p>
    <div class="legend">
      <RouterLink
        v-for="status in STATUS_ORDER"
        :key="status"
        :to="{ path: objectType === 'application' ? '/applications' : '/folders', query: { status, ...(objectType === 'file' ? { kind: 'file' } : objectType === 'folder' ? { kind: 'folder' } : {}) } }"
      >
        <span
          class="swatch"
          :class="'seg-' + classes[STATUS_CLASSES[status]]"
        />{{ STATUS_LABELS[status] }} <span class="mono">{{ result?.counts[status] ?? 0 }}</span>
      </RouterLink>
    </div>
  </div>
</template>
