<script setup lang="ts">
import { NAlert, NButton, NEmpty, NPagination, NSkeleton, useDialog, useMessage } from 'naive-ui';
import { computed, ref, watch } from 'vue';
import * as api from '../api/workspace';
import { useResource } from '../hooks/useResource';
import { useWorkspaceStore } from '../stores/workspace';
import { detailView } from '../utils/adapters';
import { formatTime } from '../utils/objects';
import ObservationTrack from './ObservationTrack.vue';
import StatusTag from './StatusTag.vue';

const props = defineProps<{ objectId: string; standalone?: boolean }>();
defineEmits<{ close: [] }>();
const workspace = useWorkspaceStore();
const message = useMessage();
const dialog = useDialog();
const busy = ref(false);
const eventPage = ref(1);
const detail = useResource((): string => props.objectId + ':' + workspace.thresholdDays, (): boolean => workspace.ready, () => api.getDetail(props.objectId, workspace.thresholdDays), (): number => workspace.revision);
const events = useResource((): string => props.objectId + ':' + eventPage.value, (): boolean => workspace.ready, () => api.listEvents(props.objectId, eventPage.value, 20), (): number => workspace.revision);
const item = computed(() => detail.data.value ? detailView(detail.data.value) : null);
watch(() => props.objectId, (): void => { eventPage.value = 1; });

async function nativeAction(action: () => Promise<unknown>): Promise<void> {
  busy.value = true;
  try { await action(); } catch (cause: unknown) { message.error(cause instanceof Error ? cause.message : '操作失败，请重试。'); }
  finally { busy.value = false; }
}
async function openLocation(): Promise<void> { if (item.value) await nativeAction(() => api.openLocation(props.objectId)); }
async function openSettings(): Promise<void> { await nativeAction(api.openApplicationSettings); }
function changeExclusion(tree = false): void {
  const current = detail.data.value?.summary;
  if (!current) return;
  const exclusion = current.exclusion;
  const restore = exclusion !== null;
  dialog.warning({
    title: restore ? '恢复观察' : '排除观察',
    content: restore ? (exclusion.mode === 'folder_tree' ? '此项目由文件夹树规则排除。移除规则会恢复整棵文件夹树的观察，并重新计算观察起点。' : '恢复后从新的观察起点记录，保留已有历史。') : (tree ? '排除此文件夹及其子项，保留已有历史记录。' : '排除此项目，保留已有历史记录。'),
    positiveText: restore ? '恢复观察' : '确认排除', negativeText: '取消',
    onPositiveClick: async (): Promise<boolean> => {
      try {
        if (exclusion) await api.removeExclusion(exclusion.ruleId);
        else await api.addExclusion(current.id, tree ? 'folder_tree' : 'object');
        await workspace.refresh();
        message.success(restore ? '已恢复观察。' : '已排除观察。');
        return true;
      } catch (cause: unknown) { message.error(cause instanceof Error ? cause.message : '更新排除规则失败。'); return false; }
    },
  });
}
const EVIDENCE_LABELS: Record<string, string> = { uninstall_registry: '注册表登记', package_api: '应用包接口', install_or_service: '安装/维护', install_or_update: '安装/更新', registered: '系统登记', unknown: '未知', day: '精确到日', instant: '时间点', valid: '有效', invalid: '无效', missing: '缺失' };
function evidenceLabel(value: string): string { return EVIDENCE_LABELS[value] ?? value; }
</script>
<template>
  <aside
    class="panel detail"
    aria-label="对象详情"
  >
    <NAlert
      v-if="detail.error.value"
      type="error"
      title="读取详情失败"
    >
      {{ detail.error.value }} <NButton
        size="small"
        @click="detail.reload"
      >
        重试
      </NButton>
    </NAlert>
    <NSkeleton
      v-if="detail.loading.value && !item"
      text
      :repeat="8"
    />
    <template v-if="item && detail.data.value">
      <div class="detail-head">
        <span class="avatar avatar--lg">{{ item.displayName.slice(0, 1) }}</span>
        <div>
          <h2 class="detail-title">
            {{ item.displayName }}
          </h2><span class="sub wrap">{{ item.objectType === 'application' ? item.publisher + ' · ' : '' }}{{ item.subtitle }}</span>
        </div>
        <NButton
          v-if="!standalone"
          quaternary
          aria-label="关闭详情"
          @click="$emit('close')"
        >
          ×
        </NButton>
      </div>
      <div class="verdict">
        <div class="verdict-line">
          <StatusTag :status="detail.data.value.summary.candidateStatus" /><span class="small">记录依据</span>
        </div>{{ item.statusReason }}
      </div>
      <ObservationTrack
        :track="item.track"
        :threshold="workspace.thresholdDays"
        :excluded="item.excluded"
        :unknown="item.candidateStatus === 'unknown'"
      />
      <dl class="facts">
        <dt>{{ item.objectType === 'folder' ? '最近浏览' : '最近使用' }}</dt><dd class="mono">
          {{ formatTime(item.lastUsedAt) }}
        </dd>
        <dt>开始观察</dt><dd class="mono">
          {{ formatTime(item.observationStartedAt) }}
        </dd>
        <template v-if="item.objectType === 'application'">
          <dt>安装/更新日期</dt><dd><span class="mono">{{ item.installationDate ? formatTime(item.installationDate, detail.data.value.summary.installationPrecision === 'day') : '未知' }}</span><span class="sub wrap">{{ item.installationLabel }}；不保证为原始安装时间</span></dd>
        </template>
        <dt>首次发现</dt><dd class="mono">
          {{ formatTime(item.firstSeenAt) }}
        </dd>
        <dt>{{ item.objectType === 'application' ? '启动入口' : '位置' }}</dt><dd class="mono path">
          {{ item.path || '没有可匹配入口' }}
        </dd>
      </dl>
      <details
        v-if="detail.data.value.evidence.length"
        class="gap-details"
      >
        <summary>安装日期证据（{{ detail.data.value.evidence.length }}）</summary>
        <ul class="plain-list">
          <li
            v-for="(evidence, index) in detail.data.value.evidence"
            :key="index"
            class="evidence-row"
          >
            <strong>{{ evidenceLabel(evidence.source) }}</strong><span>{{ evidenceLabel(evidence.meaning) }} · {{ evidenceLabel(evidence.precision) }} · {{ evidenceLabel(evidence.validation) }}</span>
            <span class="mono path">{{ evidence.rawValue ?? '原始值缺失' }}</span><span class="small muted">采集于 {{ formatTime(evidence.observedAt) }}</span>
          </li>
        </ul>
      </details>
      <details
        v-if="detail.data.value.identifiers.length"
        class="gap-details"
      >
        <summary>对象标识（{{ detail.data.value.identifiers.length }}）</summary>
        <ul class="plain-list">
          <li
            v-for="(identifier, index) in detail.data.value.identifiers"
            :key="index"
            class="evidence-row"
          >
            <span>{{ identifier.kind }} · {{ identifier.quality }} · {{ identifier.source }}</span><span class="mono path">{{ identifier.value ?? '未知' }}</span>
          </li>
        </ul>
      </details>
      <div>
        <h3 class="section-label">
          使用事件
        </h3>
        <NAlert
          v-if="events.error.value"
          type="error"
        >
          {{ events.error.value }} <NButton
            size="small"
            @click="events.reload"
          >
            重试
          </NButton>
        </NAlert>
        <ul
          v-if="events.data.value?.items.length"
          class="event-list"
        >
          <li
            v-for="(event, index) in events.data.value.items"
            :key="index"
          >
            <span class="mono">{{ formatTime(event.observedAt) }}</span><span class="muted">{{ event.label }}</span>
          </li>
        </ul>
        <NEmpty
          v-else
          class="compact-empty"
          :description="events.loading.value ? '正在读取事件…' : '当前没有保留的直接使用事件'"
        />
        <NPagination
          v-if="(events.data.value?.total ?? 0) > 20"
          v-model:page="eventPage"
          :page-size="20"
          :item-count="events.data.value?.total ?? 0"
          simple
        />
      </div>
      <RouterLink
        v-if="!standalone"
        :to="'/objects/' + item.id"
        class="small"
      >
        打开独立详情页
      </RouterLink>
      <div class="detail-actions">
        <NButton
          :disabled="!item.path || item.unavailable"
          :loading="busy"
          @click="openLocation"
        >
          打开所在位置
        </NButton>
        <NButton
          v-if="item.objectType === 'application'"
          :loading="busy"
          @click="openSettings"
        >
          Windows 应用设置
        </NButton>
        <NButton
          secondary
          @click="changeExclusion()"
        >
          {{ item.exclusion ? '恢复观察' : '排除此项目' }}
        </NButton>
        <NButton
          v-if="item.objectType === 'folder' && !item.exclusion"
          secondary
          @click="changeExclusion(true)"
        >
          排除整棵文件夹树
        </NButton>
      </div>
      <p class="small muted">
        排除只改变观察规则。DeskTrace 保留文件和应用；恢复观察会建立新的观察起点。
      </p>
    </template>
    <NEmpty
      v-else-if="!detail.loading.value && !detail.error.value"
      description="连接桌面程序后查看详情"
    />
  </aside>
</template>
