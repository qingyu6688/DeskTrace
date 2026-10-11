<script setup lang="ts">
import { NAlert, NButton, NEmpty, NSkeleton, useMessage } from 'naive-ui';
import * as api from '../api/workspace';
import DistributionBar from '../components/DistributionBar.vue';
import ThresholdSelect from '../components/ThresholdSelect.vue';
import { useBootstrapStore } from '../stores/bootstrap';
import { useWorkspaceStore } from '../stores/workspace';
import { formatTime } from '../utils/objects';
const workspace = useWorkspaceStore();
const bootstrap = useBootstrapStore();
const message = useMessage();
const moduleLabels: Record<string, string> = { applications: '应用使用记录', folders: '文件夹浏览记录', files: '文件打开记录' };
const stateLabels: Record<string, string> = { active: '记录中', inactive: '非活跃', gap: '记录中断', unknown: '无法验证' };
async function recentSettings(): Promise<void> { try { await api.openRecentSettings(); } catch (error: unknown) { message.error(error instanceof Error ? error.message : '打开设置失败。'); } }
</script>
<template>
  <div class="page-head">
    <div>
      <h1 class="page-title">
        概览
      </h1><p class="page-desc">
        本机实际记录 · 观察起点 {{ formatTime(workspace.overview?.observationStartedAt ?? null) }}
      </p>
    </div><ThresholdSelect /><NButton
      :loading="workspace.loading"
      :disabled="!workspace.connected"
      @click="workspace.refresh"
    >
      刷新状态
    </NButton>
  </div>
  <section class="panel">
    <div class="section-head">
      <h2>采集状态</h2><span class="small muted">支持范围内的直接观察</span>
    </div>
    <NSkeleton
      v-if="workspace.loading && !workspace.collector"
      :height="160"
    />
    <div
      v-else
      class="table-scroll"
    >
      <table class="table">
        <thead><tr><th>模块</th><th>状态</th><th>最近确认</th><th>说明</th></tr></thead><tbody>
          <tr
            v-for="module in workspace.collector?.modules ?? []"
            :key="module.module"
          >
            <td>{{ moduleLabels[module.module] ?? module.module }}</td><td>{{ module.state ? stateLabels[module.state] : '尚未启用' }}</td>
            <td class="mono">
              {{ formatTime(module.lastHeartbeatAt) }}
            </td><td>{{ module.reasonLabel ?? '启用后建立观察区间' }}</td>
          </tr>
          <tr>
            <td>本地数据库</td><td>{{ bootstrap.status ? '已连接' : '未连接' }}</td><td class="mono">
              {{ bootstrap.status ? '结构 v' + bootstrap.status.database.schemaVersion : '—' }}
            </td><td>{{ bootstrap.status ? bootstrap.status.database.objectCount + ' 个对象 / ' + bootstrap.status.database.eventCount + ' 条聚合事件' : '请通过桌面程序启动' }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <div class="panel-body">
      <p
        v-for="limitation in workspace.collector?.limitations ?? []"
        :key="limitation"
        class="small muted"
      >
        {{ limitation }}
      </p><NButton
        v-if="workspace.collector?.modules.some(module => module.reason === 'source_disabled')"
        size="small"
        @click="recentSettings"
      >
        打开 Windows 最近项目设置
      </NButton>
    </div>
  </section>
  <section class="panel">
    <div class="section-head">
      <h2>候选分布</h2><span class="small muted">点击图例进入列表，状态由后端计算</span>
    </div><div class="panel-body">
      <DistributionBar object-type="application" /><DistributionBar object-type="folder" /><DistributionBar object-type="file" />
    </div>
  </section>
  <div class="split">
    <section class="panel">
      <div class="section-head">
        <h2>记录缺口</h2>
      </div>
      <ul
        v-if="workspace.overview?.recentGaps.length"
        class="plain-list"
      >
        <li
          v-for="(gap, index) in workspace.overview.recentGaps"
          :key="index"
        >
          <span class="mono grow">{{ formatTime(gap.startedAt) }} → {{ gap.endedAt ? formatTime(gap.endedAt) : '进行中' }}</span><span>{{ moduleLabels[gap.module ?? ''] ?? '记录' }}</span><span class="muted">{{ gap.reasonLabel }}</span>
        </li>
      </ul><NEmpty
        v-else
        description="尚无记录缺口"
        class="compact-empty"
      />
      <p class="panel-note">
        未知和中断不会被当成“从未使用”。已知锁屏、休眠与空闲期另行区分。
      </p>
    </section>
    <section class="panel">
      <div class="section-head">
        <h2>文件夹观察范围</h2><RouterLink to="/settings">
          管理范围
        </RouterLink>
      </div><ul
        v-if="workspace.scopes.length"
        class="plain-list"
      >
        <li
          v-for="scope in workspace.scopes"
          :key="scope.id"
        >
          <span class="mono grow">{{ scope.path }}{{ scope.isSystemDesktop ? '（系统桌面）' : '' }}</span><span class="muted">{{ scope.enabled ? (scope.includeDescendants ? '含子目录' : '直接子项') : '已停用' }}</span>
        </li>
      </ul><NEmpty
        v-else
        description="尚无观察范围"
        class="compact-empty"
      />
    </section>
  </div>
  <NAlert
    v-if="workspace.collector?.catalog.complete === false"
    type="warning"
  >
    最近一次应用清单读取不完整，本次不会据此判断应用已卸载。
  </NAlert>
</template>
