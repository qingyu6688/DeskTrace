<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import type { ObservationTrack } from '../types/objects';
import { formatTime } from '../utils/objects';
import { trackGeometry } from '../utils/track';
const props = defineProps<{ track: ObservationTrack | null; threshold: number; excluded?: boolean; unknown?: boolean }>();
const plot = ref<HTMLElement | null>(null);
const width = ref(280);
let observer: ResizeObserver | null = null;
const geometry = computed(() => props.track ? trackGeometry(props.track, props.threshold, width.value) : null);
const description = computed((): string => {
  const track = props.track;
  const shape = geometry.value;
  if (!track || !shape) return '尚无有效观察窗口，轨迹不可用。';
  const thresholdNote = shape.threshold === null ? `当前展示窗口不足 ${props.threshold} 天` : `最近 ${props.threshold} 天的边界位于轨迹 ${Math.round(shape.threshold)}% 处`;
  const gapsNote = track.gaps.map((gap): string => `${formatTime(gap.startedAt)} 至 ${gap.endedAt ? formatTime(gap.endedAt) : '进行中'}，${gap.reasonLabel}`).join('；');
  return `自 ${formatTime(track.windowStart, true)} 至 ${formatTime(track.windowEnd, true)}，${shape.marks.length} 个${shape.weekly ? '周' : '日期'}有记录，最近一次 ${formatTime(track.lastUsedAt)}；${thresholdNote}；${track.gaps.length} 段记录缺口${gapsNote ? `：${gapsNote}` : ''}。刻度不表示次数或时长。`;
});
onMounted((): void => { if (plot.value && typeof ResizeObserver !== 'undefined') { observer = new ResizeObserver((entries): void => { width.value = entries[0]?.contentRect.width ?? 280; }); observer.observe(plot.value); } });
onBeforeUnmount((): void => { observer?.disconnect(); });
</script>
<template>
  <section>
    <div class="track-head">
      <h3 class="section-label">
        观察轨迹
      </h3><span class="small muted">{{ geometry?.weekly ? '按周汇总' : '按天汇总' }}</span>
    </div>
    <div class="track-labels">
      <span
        v-if="geometry?.threshold !== null && geometry"
        :style="{ left: `${Math.min(geometry.threshold ?? 0, 55)}%` }"
      >{{ threshold }} 天阈值</span><span v-else>当前窗口不足 {{ threshold }} 天</span>
    </div>
    <div
      ref="plot"
      class="track-plot"
      :class="{ 'track-excluded': excluded, 'track-unresolved': unknown }"
      role="img"
      :aria-label="description"
    >
      <template v-if="geometry">
        <div
          class="track-recent"
          :style="{ left: `${geometry.threshold ?? 0}%` }"
        />
        <div
          v-if="geometry.threshold !== null"
          class="track-threshold"
          :style="{ left: `${geometry.threshold}%` }"
        />
        <div
          class="track-line"
          style="left: 0"
        />
        <div
          v-for="(gap, index) in geometry.gaps"
          :key="index"
          :class="gap.state === 'gap' ? 'track-gap' : 'track-unknown'"
          :style="{ left: `${gap.left}%`, width: `${gap.width}%` }"
          :title="gap.label"
        />
        <span
          v-for="point in geometry.marks"
          :key="point"
          class="track-mark"
          :style="{ left: `${point}%` }"
        />
        <span
          v-if="geometry.last !== null"
          class="track-last"
          :style="{ left: `${geometry.last}%` }"
        />
      </template>
      <p
        v-else
        class="small muted"
      >
        {{ track ? '观察窗口刚开始，暂无轨迹。' : '尚无有效观察起点。' }}
      </p>
    </div>
    <div
      v-if="track"
      class="track-axis"
      aria-hidden="true"
    >
      <span style="left: 0">{{ formatTime(track.windowStart, true).slice(5) }}</span><span style="left: 100%">{{ formatTime(track.windowEnd, true).slice(5) }}</span>
    </div>
    <p
      v-if="geometry?.earlier"
      class="small muted"
    >
      更早：{{ formatTime(track?.lastUsedAt ?? null) }}
    </p>
    <ul class="track-legend">
      <li><span class="key key--mark" />当天有记录</li><li><span class="key key--last" />最近一次</li><li><span class="key key--gap" />记录中断</li><li><span class="key key--recent" />最近 {{ threshold }} 天</li>
    </ul>
    <details
      v-if="track?.gaps.length"
      class="gap-details"
    >
      <summary>{{ track.gaps.length }} 段缺口</summary><ul class="track-gaps">
        <li
          v-for="gap in track.gaps"
          :key="gap.startedAt"
        >
          <span class="mono">{{ formatTime(gap.startedAt).slice(5) }} → {{ gap.endedAt ? formatTime(gap.endedAt).slice(5) : '进行中' }}</span><span>{{ gap.reasonLabel }}</span>
        </li>
      </ul>
    </details>
  </section>
</template>
