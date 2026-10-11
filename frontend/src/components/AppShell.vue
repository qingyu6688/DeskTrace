<script setup lang="ts">
import { NAlert, NButton, useMessage } from 'naive-ui';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import packageInfo from '../../package.json';
import { useBootstrapStore } from '../stores/bootstrap';
import { useWorkspaceStore } from '../stores/workspace';
import { formatTime } from '../utils/objects';
import UiIcon from './UiIcon.vue';
const bootstrap = useBootstrapStore();
const workspace = useWorkspaceStore();
const message = useMessage();
const changing = ref(false);
const stateLabel = computed((): string => !workspace.connected ? '未连接桌面程序' : workspace.collector?.collectionState === 'running' ? '正在记录' : workspace.collector?.collectionState === 'paused' ? '已暂停记录' : '尚未开始记录');
const navigation = [
  { to: '/', label: '概览', icon: 'overview' }, { to: '/folders', label: '桌面与文件夹', icon: 'folder' },
  { to: '/applications', label: '应用', icon: 'application' }, { to: '/settings', label: '设置', icon: 'settings' },
] as const;
function focusContent(): void { document.getElementById('main-content')?.focus(); }
async function retry(): Promise<void> { await Promise.all([bootstrap.refresh(), workspace.ready ? workspace.refresh() : workspace.connect()]); }
async function toggleCollection(): Promise<void> {
  changing.value = true;
  try { await workspace.changeCollection(workspace.collector?.collectionState === 'running' ? 'paused' : 'running'); }
  catch (error: unknown) { message.error(error instanceof Error ? error.message : '操作失败，请重试。'); }
  finally { changing.value = false; }
}
onMounted(retry);
onBeforeUnmount((): void => workspace.disconnect());
</script>
<template>
  <a
    class="skip-link"
    href="#main-content"
    @click.prevent="focusContent"
  >跳到内容</a>
  <header class="topbar">
    <div class="brand">
      <UiIcon
        name="brand"
        :size="22"
      />DeskTrace
    </div>
    <span class="tag tag--recent">本机记录</span><div class="topbar-spacer" />
    <div class="topbar-status">
      <span
        class="dot"
        :class="workspace.collector?.collectionState === 'running' ? 'dot--ok' : 'dot--idle'"
      /><span>{{ stateLabel }}</span>
    </div>
    <NButton
      :disabled="!workspace.ready"
      :loading="changing"
      @click="toggleCollection"
    >
      {{ workspace.collector?.collectionState === 'running' ? '暂停记录' : workspace.collector?.collectionState === 'paused' ? '恢复记录' : '开始记录' }}
    </NButton>
  </header>
  <div class="layout">
    <nav
      class="sidebar"
      aria-label="主导航"
    >
      <RouterLink
        v-for="item in navigation"
        :key="item.to"
        :to="item.to"
        class="nav-link"
        exact-active-class="is-active"
      >
        <UiIcon :name="item.icon" /><span class="nav-label">{{ item.label }}</span>
        <span
          v-if="item.icon === 'application'"
          class="nav-count"
        >{{ workspace.overview?.applications.total ?? '—' }}</span>
        <span
          v-if="item.icon === 'folder'"
          class="nav-count"
        >{{ workspace.overview ? workspace.overview.folders.total + workspace.overview.files.total : '—' }}</span>
      </RouterLink>
      <div class="sidebar-foot">
        <div>实际观察起点</div><div class="mono">
          {{ formatTime(workspace.overview?.observationStartedAt ?? null) }}
        </div>
        <div>关闭窗口隐藏到托盘</div><div class="small muted">
          {{ packageInfo.version }} · 本机使用记录
        </div>
      </div>
    </nav>
    <main
      id="main-content"
      class="content"
      tabindex="-1"
    >
      <NAlert
        v-if="!workspace.connected"
        title="尚未连接桌面程序"
        type="info"
      >
        浏览器无法读取本机记录。请在项目根目录执行 pnpm --dir frontend tauri dev，界面不会填充演示数据。
      </NAlert>
      <NAlert
        v-if="workspace.error || bootstrap.errorMessage"
        type="error"
        title="状态读取失败"
      >
        {{ workspace.error || bootstrap.errorMessage }} <NButton
          size="small"
          @click="retry"
        >
          重新连接
        </NButton>
      </NAlert>
      <NAlert
        v-if="workspace.collector?.collectionState === 'not_started'"
        type="info"
        :show-icon="false"
      >
        应用清单和观察范围已可查看，点击“开始记录”后才建立使用观察；启用前的完整历史无法恢复。
      </NAlert>
      <NAlert
        v-if="workspace.collector?.collectionState === 'paused'"
        type="warning"
        :show-icon="false"
      >
        记录已暂停，暂停期间会形成记录缺口。
      </NAlert>
      <slot />
    </main>
  </div>
</template>
