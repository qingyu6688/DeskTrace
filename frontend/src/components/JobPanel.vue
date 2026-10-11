<script setup lang="ts">
import { NButton, NEmpty, useMessage } from 'naive-ui';
import { useWorkspaceStore } from '../stores/workspace';
const workspace = useWorkspaceStore();
const message = useMessage();
const labels: Record<string, string> = { pending: '等待中', running: '执行中', completed: '已完成', failed: '失败', cancelled: '已取消' };
async function cancel(id: string): Promise<void> { try { await workspace.cancelJob(id); message.info('已请求取消，等待任务结束。'); } catch (error: unknown) { message.error(error instanceof Error ? error.message : '取消失败。'); } }
</script>
<template>
  <section class="panel">
    <div class="section-head">
      <h2>后台任务</h2>
    </div><ul
      v-if="workspace.jobs.length"
      class="plain-list"
    >
      <li
        v-for="job in workspace.jobs"
        :key="job.jobId"
      >
        <div class="grow">
          <strong>{{ job.kind === 'export' ? 'CSV 导出' : '清单刷新' }} · {{ labels[job.status] }}</strong><span class="sub wrap">{{ job.message ?? job.stage }} · {{ job.processed }}{{ job.total === null ? ' 项' : ' / ' + job.total + ' 项' }}</span><span
            v-if="job.resultPath"
            class="mono path"
          >{{ job.resultPath }}</span>
        </div><NButton
          v-if="job.status === 'pending' || job.status === 'running'"
          size="small"
          @click="cancel(job.jobId)"
        >
          取消任务
        </NButton>
      </li>
    </ul><NEmpty
      v-else
      class="compact-empty"
      description="暂无任务"
    />
  </section>
</template>
