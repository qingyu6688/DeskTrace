<script setup lang="ts">
import { NAlert, NButton, NCheckbox, NEmpty, NInputNumber, NModal, NSwitch, useDialog, useMessage } from 'naive-ui';
import { computed, ref, watch } from 'vue';
import { IpcError } from '../api/client';
import * as api from '../api/workspace';
import JobPanel from '../components/JobPanel.vue';
import ScopeDialog from '../components/ScopeDialog.vue';
import { useBootstrapStore } from '../stores/bootstrap';
import { useWorkspaceStore } from '../stores/workspace';
import type { ClearPreparation, ExclusionRule, Scope } from '../types/ipc';
const workspace = useWorkspaceStore();
const bootstrap = useBootstrapStore();
const message = useMessage();
const dialog = useDialog();
const threshold = ref<number | null>(90);
const retention = ref<number | null>(180);
const autoStart = ref(false);
const busy = ref(false);
const scopeDialog = ref(false);
const clearDialog = ref(false);
const preparation = ref<ClearPreparation | null>(null);
const clearDeadline = ref(0);
const hasDesktop = computed((): boolean => workspace.scopes.some((scope): boolean => scope.isSystemDesktop));
watch(() => workspace.settings, (settings): void => {
  if (settings) { threshold.value = settings.thresholdDays; retention.value = settings.eventRetentionDays; autoStart.value = settings.autoStartEnabled; }
}, { immediate: true });

async function perform(action: () => Promise<unknown>, success: string): Promise<boolean> {
  busy.value = true;
  try { await action(); await workspace.refresh(); await bootstrap.refresh(); if (success) message.success(success); return true; }
  catch (cause: unknown) { message.error(cause instanceof Error ? cause.message : '操作失败，请重试。'); return false; }
  finally { busy.value = false; }
}
async function save(): Promise<void> {
  if (!threshold.value || !Number.isInteger(threshold.value) || threshold.value < 1 || threshold.value > 3650 || !retention.value || !Number.isInteger(retention.value) || retention.value < 30 || retention.value > 3650) { message.error('阈值需为 1～3650 天的整数，保留天数需为 30～3650 天的整数。'); return; }
  const patch = { thresholdDays: threshold.value, eventRetentionDays: retention.value, autoStartEnabled: autoStart.value };
  await perform(() => workspace.saveSettings(patch), '设置已保存。');
}
async function updateScope(scope: Scope, patch: Partial<Pick<Scope, 'enabled' | 'includeDescendants'>>): Promise<void> {
  await perform(() => api.updateScope(scope.id, patch.enabled ?? scope.enabled, patch.includeDescendants ?? scope.includeDescendants), '观察范围已更新。');
}
function removeScope(scope: Scope): void {
  dialog.warning({
    title: '移除观察范围', content: '移除后停止观察此范围，保留已有历史记录。文件和文件夹不会被删除。', positiveText: '移除范围', negativeText: '取消',
    onPositiveClick: () => perform(() => api.removeScope(scope.id), '观察范围已移除。'),
  });
}
function restore(rule: ExclusionRule): void {
  dialog.warning({
    title: '恢复观察', content: rule.mode === 'folder_tree' ? '恢复此文件夹及其子项的观察，并重新计算观察起点。' : '恢复此项目的观察，并重新计算观察起点。', positiveText: '恢复观察', negativeText: '取消',
    onPositiveClick: () => perform(() => api.removeExclusion(rule.id), '已恢复观察。'),
  });
}
async function prepareClear(): Promise<void> {
  busy.value = true;
  try { preparation.value = await api.prepareClear(); clearDeadline.value = Date.now() + preparation.value.expiresInSeconds * 1000; clearDialog.value = true; }
  catch (cause: unknown) { message.error(cause instanceof Error ? cause.message : '无法准备清除操作。'); }
  finally { busy.value = false; }
}
async function confirmClear(): Promise<boolean> {
  const current = preparation.value;
  if (!current || Date.now() >= clearDeadline.value) { preparation.value = null; clearDialog.value = false; message.warning('确认凭证已过期，请重新发起清除。'); return false; }
  busy.value = true;
  try {
    await api.clearUsage(current.confirmationToken);
    preparation.value = null;
    await workspace.refresh(); await bootstrap.refresh(); message.success('已清除使用记录，保留清单、设置与观察规则。'); return true;
  } catch (cause: unknown) {
    if (cause instanceof IpcError && cause.code === 409) { preparation.value = null; clearDialog.value = false; }
    message.error(cause instanceof Error ? cause.message : '清除失败，请重试。'); return false;
  } finally { busy.value = false; }
}
</script>
<template>
  <div class="page-head">
    <div>
      <h1 class="page-title">
        设置
      </h1><p class="page-desc">
        管理本机观察范围、记录保留和开机启动。
      </p>
    </div><NButton
      type="primary"
      :disabled="!workspace.ready || busy"
      :loading="busy"
      @click="save"
    >
      保存设置
    </NButton>
  </div>
  <section class="panel">
    <div class="section-head">
      <h2>观察范围</h2><div class="detail-actions">
        <NButton
          size="small"
          :disabled="!workspace.ready || busy || hasDesktop"
          @click="perform(api.addDesktopScope, '已添加系统桌面范围。')"
        >
          {{ hasDesktop ? '已包含系统桌面' : '添加系统桌面' }}
        </NButton><NButton
          size="small"
          :disabled="!workspace.ready || busy"
          @click="scopeDialog = true"
        >
          添加观察范围
        </NButton>
      </div>
    </div>
    <ul
      v-if="workspace.scopes.length"
      class="plain-list"
    >
      <li
        v-for="scope in workspace.scopes"
        :key="scope.id"
        class="scope-row"
      >
        <span class="mono grow path">{{ scope.path }}<span
          v-if="scope.isSystemDesktop"
          class="sub"
        >系统桌面（直接子项）</span></span>
        <NCheckbox
          :checked="scope.includeDescendants"
          :disabled="busy || scope.isSystemDesktop"
          @update:checked="(value: boolean) => updateScope(scope, { includeDescendants: value })"
        >
          含子目录
        </NCheckbox>
        <NSwitch
          :value="scope.enabled"
          :disabled="busy"
          :aria-label="'启用范围 ' + scope.path"
          @update:value="(value: boolean) => updateScope(scope, { enabled: value })"
        />
        <NButton
          size="small"
          :disabled="busy"
          @click="removeScope(scope)"
        >
          移除
        </NButton>
      </li>
    </ul><NEmpty
      v-else
      class="compact-empty"
      :description="workspace.ready ? '尚无观察范围' : '连接桌面程序后显示观察范围'"
    />
    <p class="panel-body small muted">
      系统桌面记录其直接子项；普通范围可包含子目录。目录扫描只登记存在的项目，使用记录来自实际采集。
    </p>
  </section>
  <section class="panel">
    <div class="section-head">
      <h2>记录与显示</h2>
    </div><div class="settings-form">
      <label class="setting-row"><span><strong>较久未记录阈值</strong><span class="sub wrap">保存为默认阈值；列表上方的阈值只影响当前查询。</span></span><NInputNumber
        v-model:value="threshold"
        aria-label="设置阈值天数"
        :min="1"
        :max="3650"
        :precision="0"
        :disabled="!workspace.ready || busy"
      /></label>
      <label class="setting-row"><span><strong>原始事件保留天数</strong><span class="sub wrap">到期原始事件由后端保留策略清理，对象最近使用摘要仍保留。</span></span><NInputNumber
        v-model:value="retention"
        aria-label="原始事件保留天数"
        :min="30"
        :max="3650"
        :precision="0"
        :disabled="!workspace.ready || busy"
      /></label>
      <div class="setting-row">
        <span><strong>开机启动</strong><span class="sub wrap">保存后更新当前 Windows 用户的启动项。</span></span><NSwitch
          v-model:value="autoStart"
          :disabled="!workspace.ready || busy"
          aria-label="开机启动"
        />
      </div>
    </div>
  </section>
  <section class="panel">
    <div class="section-head">
      <h2>排除规则</h2><span class="small muted">{{ workspace.exclusions.length }} 条</span>
    </div>
    <ul
      v-if="workspace.exclusions.length"
      class="plain-list"
    >
      <li
        v-for="rule in workspace.exclusions"
        :key="rule.id"
      >
        <span class="grow path">{{ rule.label }}<span class="sub">{{ rule.mode === 'folder_tree' ? '整棵文件夹树' : '单个项目' }}</span></span><NButton
          size="small"
          :disabled="busy"
          @click="restore(rule)"
        >
          恢复观察
        </NButton>
      </li>
    </ul>
    <NEmpty
      v-else
      class="compact-empty"
      description="没有排除规则"
    />
  </section>
  <section class="panel">
    <div class="section-head">
      <h2>本地存储</h2><NButton
        size="small"
        :loading="bootstrap.loading"
        @click="bootstrap.refresh"
      >
        刷新连接
      </NButton>
    </div>
    <dl class="facts storage-facts">
      <dt>数据目录</dt><dd class="mono path">
        {{ bootstrap.status?.database.dataDirectory ?? '连接桌面程序后显示' }}
      </dd>
      <dt>实际数据</dt><dd>{{ bootstrap.status ? bootstrap.status.database.objectCount + ' 个对象 / ' + bootstrap.status.database.eventCount + ' 条事件' : '尚未连接' }}</dd>
      <dt>存储规则</dt><dd>{{ bootstrap.status ? bootstrap.status.dedupWindowSeconds + ' 秒聚合，' + bootstrap.status.intervalReasonCount + ' 种原因与状态约束' : '连接后显示' }}</dd>
    </dl>
  </section>
  <section class="panel">
    <div class="section-head">
      <h2>数据操作</h2>
    </div><div class="panel-body settings-actions">
      <RouterLink
        to="/applications"
        class="btn"
      >
        前往应用列表导出 CSV
      </RouterLink><RouterLink
        to="/folders"
        class="btn"
      >
        前往桌面与文件夹导出 CSV
      </RouterLink>
      <NButton
        :disabled="!workspace.ready || busy"
        :loading="busy"
        @click="prepareClear"
      >
        清除本工具记录…
      </NButton>
      <NAlert type="info">
        清除前会显示影响对象数量，确认后清除使用记录与观察区间。保留清单、设置和观察规则，原始文件及应用不受影响。
      </NAlert>
    </div>
  </section>
  <JobPanel v-if="workspace.jobs.length" />
  <ScopeDialog v-model:show="scopeDialog" />
  <NModal
    v-model:show="clearDialog"
    preset="dialog"
    type="warning"
    title="确认清除使用记录"
    :closable="!busy"
    :mask-closable="!busy"
    :close-on-esc="!busy"
    positive-text="确认清除"
    negative-text="取消"
    :positive-button-props="{ loading: busy, disabled: busy || !preparation }"
    :negative-button-props="{ disabled: busy }"
    @positive-click="confirmClear"
  >
    <p>本次影响 {{ preparation?.affectedObjects ?? 0 }} 个对象。使用事件和观察区间清除后无法撤销，清单、设置、观察范围与排除规则保留。</p>
    <p class="small muted">
      确认凭证有效期 {{ preparation?.expiresInSeconds ?? 60 }} 秒。期间数据变化时需要重新发起确认。
    </p>
  </NModal>
</template>
