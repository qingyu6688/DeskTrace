<script setup lang="ts">
import { NAlert, NButton, NCheckbox, NCheckboxGroup, NDataTable, NDrawer, NDrawerContent, NEmpty, NInput, NModal, NPagination, NSelect, useMessage } from 'naive-ui';
import type { DataTableColumns } from 'naive-ui';
import { computed, h, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import * as api from '../api/workspace';
import CoverageMark from '../components/CoverageMark.vue';
import JobPanel from '../components/JobPanel.vue';
import ObjectDetail from '../components/ObjectDetail.vue';
import ScopeDialog from '../components/ScopeDialog.vue';
import StatusTag from '../components/StatusTag.vue';
import ThresholdSelect from '../components/ThresholdSelect.vue';
import { useResource } from '../hooks/useResource';
import { useWorkspaceStore } from '../stores/workspace';
import type { ListKind, ObjectQuery } from '../types/ipc';
import type { CandidateStatus, UsageObject } from '../types/objects';
import { summaryView } from '../utils/adapters';
import { STATUS_LABELS, STATUS_ORDER, formatTime, timeNote } from '../utils/objects';
const props = defineProps<{ objectType: 'application' | 'folder' }>();
const workspace = useWorkspaceStore();
const route = useRoute();
const router = useRouter();
const message = useMessage();
const keyword = ref('');
const searched = ref('');
const continuousOnly = ref(false);
const sortBy = ref<ObjectQuery['sortBy']>('lastUsedAt');
const descending = ref(false);
const page = ref(1);
const PAGE_SIZE = 50;
const WIDE_LAYOUT_MIN_WIDTH = 1260;
const selectedId = ref<string | null>(null);
const detailClosed = ref(false);
const scopeDialog = ref(false);
const exportDialog = ref(false);
const exporting = ref(false);
const refreshing = ref(false);
const wide = ref(window.innerWidth >= WIDE_LAYOUT_MIN_WIDTH);
let searchTimer: ReturnType<typeof setTimeout> | undefined;
const fields = ref(['objectType', 'displayName', 'path', 'lastUsedAt', 'installationDate', 'installationMeaning', 'observationStartedAt', 'candidateStatus', 'coverageStatus']);
const fieldOptions = [{ value: 'objectType', label: '对象类型' }, { value: 'displayName', label: '名称' }, { value: 'path', label: '路径或入口' }, { value: 'lastUsedAt', label: '最近观察时间' }, { value: 'installationDate', label: '系统日期' }, { value: 'installationMeaning', label: '日期含义' }, { value: 'observationStartedAt', label: '观察起点' }, { value: 'candidateStatus', label: '候选状态' }, { value: 'coverageStatus', label: '记录覆盖' }];
const title = computed((): string => props.objectType === 'application' ? '应用' : '桌面与文件夹');
const kind = computed((): ListKind => props.objectType === 'application' ? 'application' : route.query.kind === 'file' ? 'file' : route.query.kind === 'folder' ? 'folder' : 'location');
const filterStatus = computed((): CandidateStatus | 'all' => STATUS_ORDER.includes(route.query.status as CandidateStatus) ? route.query.status as CandidateStatus : 'all');
const query = computed((): ObjectQuery => ({ objectType: kind.value, keyword: searched.value, candidateStatuses: filterStatus.value === 'all' ? [] : [filterStatus.value], coverageStatus: continuousOnly.value ? 'continuous' : null, thresholdDays: workspace.thresholdDays, sortBy: sortBy.value, sortDirection: descending.value ? 'desc' : 'asc', page: page.value, pageSize: PAGE_SIZE }));
const { data, loading, error, reload } = useResource((): string => JSON.stringify(query.value), (): boolean => workspace.ready, (): ReturnType<typeof api.listObjects> => api.listObjects(query.value), (): number => workspace.revision);
const rows = computed((): UsageObject[] => (data.value?.items ?? []).map(summaryView));
const counts = computed(() => {
  const overview = workspace.overview;
  if (kind.value === 'application') return overview?.applications;
  if (kind.value === 'folder') return overview?.folders;
  if (kind.value === 'file') return overview?.files;
  if (!overview) return undefined;
  return { total: overview.files.total + overview.folders.total, counts: Object.fromEntries(STATUS_ORDER.map((status): [CandidateStatus, number] => [status, overview.files.counts[status] + overview.folders.counts[status]])) as Record<CandidateStatus, number> };
});
const active = computed(() => rows.value.find((item): boolean => item.id === selectedId.value) ?? (!detailClosed.value && wide.value ? rows.value[0] : null) ?? null);
const drawer = computed({ get: (): boolean => !wide.value && active.value !== null, set: (value: boolean): void => { if (!value) closeDetail(); } });
function resize(): void { wide.value = window.innerWidth >= WIDE_LAYOUT_MIN_WIDTH; }
onMounted((): void => { window.addEventListener('resize', resize); window.addEventListener('keydown', drawerKeyboard); });
onBeforeUnmount((): void => { window.removeEventListener('resize', resize); window.removeEventListener('keydown', drawerKeyboard); if (searchTimer) clearTimeout(searchTimer); });
watch(keyword, (): void => { if (searchTimer) clearTimeout(searchTimer); searchTimer = setTimeout((): void => { searched.value = keyword.value; }, 200); });
watch([searched, continuousOnly, filterStatus, kind, sortBy, descending, (): number => workspace.thresholdDays], (): void => { page.value = 1; selectedId.value = null; detailClosed.value = false; });
watch(page, (): void => { selectedId.value = null; detailClosed.value = false; });
watch(() => data.value?.total, (total): void => { if (total !== undefined) page.value = Math.min(page.value, Math.max(1, Math.ceil(total / PAGE_SIZE))); });
watch((): 'application' | 'folder' => props.objectType, (): void => { keyword.value = ''; searched.value = ''; continuousOnly.value = false; sortBy.value = 'lastUsedAt'; descending.value = false; });
function closeDetail(): void { selectedId.value = null; detailClosed.value = true; }
function select(item: UsageObject): void { selectedId.value = item.id; detailClosed.value = false; }
async function changeQuery(values: Record<string, string | undefined>): Promise<void> { try { await router.replace({ query: { ...route.query, ...values } }); } catch { message.error('筛选失败，请重试。'); } }
async function keyboard(event: KeyboardEvent, item: UsageObject): Promise<void> {
  if (event.key === 'Escape') { closeDetail(); return; }
  if (event.key === 'Enter') { event.preventDefault(); await router.push('/objects/' + item.id); return; }
  if (!['ArrowUp', 'ArrowDown'].includes(event.key)) return;
  event.preventDefault();
  const index = rows.value.findIndex((row): boolean => row.id === item.id);
  const next = rows.value[index + (event.key === 'ArrowDown' ? 1 : -1)];
  if (next) { select(next); await nextTick(); if (wide.value) document.getElementById('row-' + next.id)?.focus(); }
}
function drawerKeyboard(event: KeyboardEvent): void {
  const target = event.target;
  if (wide.value || !active.value || !(target instanceof Element) || !target.closest('.n-drawer') || target.closest('input, textarea, select')) return;
  if (['ArrowUp', 'ArrowDown'].includes(event.key)) void keyboard(event, active.value).catch((): void => { message.error('切换详情失败。'); });
}
function rowProps(item: UsageObject): Record<string, unknown> { return { id: 'row-' + item.id, tabindex: 0, 'aria-selected': item.id === active.value?.id, onClick: (): void => select(item), onKeydown: (event: KeyboardEvent): void => { void keyboard(event, item).catch((): void => { message.error('打开详情失败。'); }); } }; }
function renderName(item: UsageObject): ReturnType<typeof h> { return h('div', { class: 'name-cell' }, [h('span', { class: 'avatar', 'aria-hidden': true }, item.objectType === 'file' ? '文' : item.displayName.slice(0, 1)), h('div', {}, [h('span', { class: 'primary-text' }, item.displayName), h('span', { class: 'sub' }, item.subtitle)])]); }
function renderLast(item: UsageObject): ReturnType<typeof h> { return h('div', {}, [h('span', { class: item.lastUsedAt ? 'mono' : 'value-missing' }, formatTime(item.lastUsedAt)), h('span', { class: 'sub' }, timeNote(item, new Date().toISOString()))]); }
const columns = computed((): DataTableColumns<UsageObject> => [
  { title: '名称', key: 'displayName', width: 182, render: renderName },
  { title: (): ReturnType<typeof h> => h('button', { class: 'sort-button', 'aria-label': '切换最近时间排序方向', onClick: (): void => { sortBy.value = 'lastUsedAt'; descending.value = !descending.value; } }, '最近观察 ' + (descending.value ? '↓' : '↑')), key: 'lastUsedAt', width: 140, render: renderLast },
  { title: props.objectType === 'application' ? '安装/更新日期' : '开始观察', key: 'installationDate', width: 134, render: (item): ReturnType<typeof h> => h('div', {}, [h('span', { class: 'mono' }, props.objectType === 'folder' ? formatTime(item.observationStartedAt, true) : item.installationDate ? formatTime(item.installationDate, true) : '未知'), h('span', { class: 'sub wrap' }, item.objectType === 'application' ? item.installationLabel : item.objectType === 'file' ? '文件' : '文件夹')]) },
  { title: '状态', key: 'status', width: 120, render: (item): ReturnType<typeof h> => h('div', {}, [h(StatusTag, { status: item.candidateStatus ?? 'unknown' }), h('span', { class: 'sub wrap' }, item.unknownReason ?? '')]) },
  { title: '记录覆盖', key: 'coverage', width: 74, render: (item): ReturnType<typeof h> => h(CoverageMark, { coverage: item.coverage, visible: ['stale_candidate', 'never_observed'].includes(item.candidateStatus ?? 'unknown') }) },
]);
async function refresh(): Promise<void> { refreshing.value = true; try { if (props.objectType === 'application') await workspace.refreshCatalog(); else await workspace.refresh(); await reload(); } catch (cause: unknown) { message.error(cause instanceof Error ? cause.message : '刷新失败。'); } finally { refreshing.value = false; } }
async function exportResults(): Promise<boolean> {
  exporting.value = true;
  try { const result = await api.exportObjects(query.value, fields.value); if (result.jobId) { await workspace.trackJob(result.jobId); message.success('已启动导出，可在后台任务中查看进度。'); } return true; }
  catch (cause: unknown) { message.error(cause instanceof Error ? cause.message : '导出失败。'); return false; }
  finally { exporting.value = false; }
}
</script>
<template>
  <div class="page-head">
    <div>
      <h1 class="page-title">
        {{ title }}
      </h1><p class="page-desc">
        {{ counts?.total ?? 0 }} 个实际对象 · 未知和观察不足不会当作长期未用
      </p>
    </div>
    <NButton
      v-if="objectType === 'folder'"
      :disabled="!workspace.ready"
      @click="scopeDialog = true"
    >
      添加观察范围
    </NButton>
    <NButton
      :disabled="!workspace.ready"
      :loading="refreshing"
      @click="refresh"
    >
      {{ objectType === 'application' ? '刷新应用清单' : '刷新列表' }}
    </NButton>
    <NButton
      type="primary"
      :disabled="!workspace.ready || loading || !data?.total"
      @click="exportDialog = true"
    >
      导出当前结果
    </NButton>
  </div>
  <div class="toolbar">
    <NInput
      v-model:value="keyword"
      aria-label="搜索名称或路径"
      placeholder="搜索名称或路径"
      clearable
      :maxlength="200"
      style="max-width: 290px"
    /><ThresholdSelect />
    <NSelect
      v-if="objectType === 'folder'"
      aria-label="对象类型筛选"
      :value="kind"
      :options="[{ label: '文件与文件夹', value: 'location' }, { label: '仅文件', value: 'file' }, { label: '仅文件夹', value: 'folder' }]"
      style="width: 150px"
      @update:value="(value: string) => changeQuery({ kind: value === 'location' ? undefined : value })"
    />
    <NSelect
      v-model:value="sortBy"
      aria-label="排序字段"
      :options="[{ label: '最近观察时间', value: 'lastUsedAt' }, { label: '名称', value: 'displayName' }, { label: '首次发现', value: 'firstSeenAt' }, ...(objectType === 'application' ? [{ label: '安装/更新日期', value: 'installationDate' }] : [])]"
      style="width: 150px"
    />
    <NButton
      size="small"
      @click="descending = !descending"
    >
      {{ descending ? '降序 ↓' : '升序 ↑' }}
    </NButton><NCheckbox v-model:checked="continuousOnly">
      仅看记录连续的
    </NCheckbox>
  </div>
  <div
    class="chips"
    role="group"
    aria-label="按状态筛选"
  >
    <button
      class="chip"
      type="button"
      :aria-pressed="filterStatus === 'all'"
      @click="changeQuery({ status: undefined })"
    >
      全部 <span class="chip-count">{{ counts?.total ?? 0 }}</span>
    </button><button
      v-for="status in STATUS_ORDER"
      :key="status"
      class="chip"
      type="button"
      :aria-pressed="filterStatus === status"
      @click="changeQuery({ status })"
    >
      {{ STATUS_LABELS[status] }} <span class="chip-count">{{ counts?.counts[status] ?? 0 }}</span>
    </button>
  </div>
  <NAlert
    v-if="error"
    title="列表读取失败"
    type="error"
  >
    {{ error }} <NButton
      size="small"
      @click="reload"
    >
      重试
    </NButton>
  </NAlert>
  <div
    class="workspace"
    :class="{ 'has-detail': wide && active }"
  >
    <section
      class="panel list-panel"
      :aria-label="title + '列表'"
    >
      <NDataTable
        :columns="columns"
        :data="rows"
        :loading="loading"
        :row-key="(item: UsageObject): string => item.id"
        :row-props="rowProps"
        :bordered="false"
        :single-line="false"
        :scroll-x="650"
      >
        <template #empty>
          <NEmpty :description="!workspace.connected ? '连接桌面程序后查看本机清单。' : error ? '查询失败，请重试。' : '没有符合条件的项目。'">
            <template #extra>
              <NButton
                :disabled="!workspace.ready"
                @click="keyword = ''; continuousOnly = false; changeQuery({ status: undefined, kind: undefined })"
              >
                清除筛选
              </NButton>
            </template>
          </NEmpty>
        </template>
      </NDataTable><div class="pager">
        <span class="pager-info">共 {{ data?.total ?? 0 }} 项 · 每页 50 · 未知时间始终置后</span><NPagination
          v-model:page="page"
          :item-count="data?.total ?? 0"
          :page-size="PAGE_SIZE"
          :show-size-picker="false"
        />
      </div>
    </section><ObjectDetail
      v-if="wide && active"
      :object-id="active.id"
      @close="closeDetail"
    />
  </div>
  <NDrawer
    v-model:show="drawer"
    :width="360"
    :trap-focus="true"
    :auto-focus="true"
    :close-on-esc="true"
  >
    <NDrawerContent :title="title + '详情'">
      <ObjectDetail
        v-if="active"
        :object-id="active.id"
        @close="closeDetail"
      />
    </NDrawerContent>
  </NDrawer>
  <ScopeDialog v-model:show="scopeDialog" />
  <NModal
    v-model:show="exportDialog"
    preset="dialog"
    title="导出当前筛选结果"
    :closable="false"
    positive-text="选择保存位置"
    negative-text="取消"
    :positive-button-props="{ loading: exporting, disabled: fields.length === 0 }"
    @positive-click="exportResults"
  >
    <p>将导出全部 {{ data?.total ?? 0 }} 个匹配对象，包含其他分页。请选择导出字段：</p><NCheckboxGroup v-model:value="fields">
      <div class="export-fields">
        <NCheckbox
          v-for="field in fieldOptions"
          :key="field.value"
          :value="field.value"
        >
          {{ field.label }}
        </NCheckbox>
      </div>
    </NCheckboxGroup>
  </NModal>
  <JobPanel v-if="workspace.jobs.length" />
</template>
