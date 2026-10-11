import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { desktopAvailable, subscribe } from '../api/client';
import * as api from '../api/workspace';
import { useBootstrapStore } from './bootstrap';
import type { CollectorStatus, ExclusionRule, Job, Overview, Scope, Settings } from '../types/ipc';

export const useWorkspaceStore = defineStore('workspace', () => {
  const connected = ref(desktopAvailable());
  const ready = ref(false);
  const loading = ref(false);
  const error = ref('');
  const thresholdDays = ref(90);
  const settings = ref<Settings | null>(null);
  const overview = ref<Overview | null>(null);
  const collector = ref<CollectorStatus | null>(null);
  const scopes = ref<Scope[]>([]);
  const exclusions = ref<ExclusionRule[]>([]);
  const jobs = ref<Job[]>([]);
  const revision = ref(0);
  const unlisteners: UnlistenFn[] = [];
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let pollTimer: ReturnType<typeof setInterval> | undefined;
  let syncTimer: ReturnType<typeof setInterval> | undefined;
  let disposed = false;
  let connecting = false;
  let sequence = 0;
  const activeJobs = computed((): Job[] => jobs.value.filter((job): boolean => ['pending', 'running'].includes(job.status)));
  function rememberJob(job: Job): void {
    jobs.value = [job, ...jobs.value.filter((item): boolean => item.jobId !== job.jobId)].slice(0, 20);
  }
  async function refresh(): Promise<void> {
    if (!connected.value) return;
    const ticket = ++sequence;
    loading.value = true;
    try {
      const [nextOverview, nextCollector, nextScopes, nextExclusions] = await Promise.all([api.getOverview(thresholdDays.value), api.getCollectorStatus(), api.listScopes(), api.listExclusions(), useBootstrapStore().refresh()]);
      if (disposed || ticket !== sequence) return;
      overview.value = nextOverview; collector.value = nextCollector; scopes.value = nextScopes; exclusions.value = nextExclusions;
      const jobId = nextCollector.catalog.runningJobId;
      if (jobId) rememberJob(await api.getJob(jobId));
      if (disposed || ticket !== sequence) return;
      error.value = ''; revision.value += 1;
    } catch (cause: unknown) {
      if (ticket === sequence && !disposed) error.value = cause instanceof Error ? cause.message : '读取工作区失败，请重试。';
    } finally { if (ticket === sequence) loading.value = false; }
  }
  function scheduleRefresh(): void {
    if (refreshTimer || disposed) return;
    refreshTimer = setTimeout((): void => { refreshTimer = undefined; void refresh(); }, 300);
  }
  async function connect(): Promise<void> {
    disposed = false;
    if (!connected.value || ready.value || connecting) return;
    connecting = true;
    loading.value = true;
    try {
      settings.value = await api.getSettings(); thresholdDays.value = settings.value.thresholdDays;
      const subscriptions = await Promise.allSettled([
        subscribe('objects-updated', scheduleRefresh),
        subscribe<CollectorStatus>('collector-status-changed', (status): void => { collector.value = status; scheduleRefresh(); }),
        subscribe<Job>('job-progress', (job): void => { rememberJob(job); if (['completed', 'failed', 'cancelled'].includes(job.status)) scheduleRefresh(); }),
      ]);
      for (const result of subscriptions) if (result.status === 'fulfilled') { if (disposed) result.value(); else unlisteners.push(result.value); }
      const failed = subscriptions.find((result): boolean => result.status === 'rejected');
      if (failed?.status === 'rejected') { unlisteners.splice(0).forEach((unlisten): void => unlisten()); throw failed.reason; }
      if (disposed) return;
      ready.value = true;
      await refresh();
      if (disposed) return;
      // 任务事件可能早于订阅，定时查询作为补偿。
      pollTimer = setInterval((): void => { void pollJobs(); }, 3000);
      syncTimer = setInterval(scheduleRefresh, 30000);
    } catch (cause: unknown) { error.value = cause instanceof Error ? cause.message : '连接失败，请重试。'; }
    finally { loading.value = false; connecting = false; }
  }
  async function pollJobs(): Promise<void> {
    if (disposed || !connected.value) return;
    try {
      for (const job of activeJobs.value) { const updated = await api.getJob(job.jobId); if (disposed) return; rememberJob(updated); if (!['pending', 'running'].includes(updated.status)) scheduleRefresh(); }
    } catch (cause: unknown) { error.value = cause instanceof Error ? cause.message : '读取任务失败。'; }
  }
  async function saveSettings(patch: Partial<Settings>): Promise<void> {
    const applied = await api.updateSettings(patch);
    settings.value = applied; thresholdDays.value = applied.thresholdDays;
    await refresh();
  }
  async function changeCollection(next: 'running' | 'paused'): Promise<void> { collector.value = await api.setCollectionState(next); await refresh(); }
  async function refreshCatalog(): Promise<void> { const result = await api.refreshCatalog(); rememberJob(await api.getJob(result.jobId)); }
  async function trackJob(jobId: string): Promise<void> { rememberJob(await api.getJob(jobId)); }
  async function cancelJob(jobId: string): Promise<void> { rememberJob(await api.cancelJob(jobId)); }
  function disconnect(): void {
    disposed = true; sequence += 1; ready.value = false;
    unlisteners.splice(0).forEach((unlisten): void => unlisten());
    if (refreshTimer) clearTimeout(refreshTimer);
    if (pollTimer) clearInterval(pollTimer);
    if (syncTimer) clearInterval(syncTimer);
    refreshTimer = undefined; pollTimer = undefined; syncTimer = undefined;
  }
  return { connected, ready, loading, error, thresholdDays, settings, overview, collector, scopes, exclusions, jobs, activeJobs, revision, connect, refresh, scheduleRefresh, saveSettings, changeCollection, refreshCatalog, trackJob, cancelJob, disconnect };
});
