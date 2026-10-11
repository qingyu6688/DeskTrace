<script setup lang="ts">
import type { CoverageStatus } from '../types/objects';
defineProps<{ coverage: CoverageStatus; visible: boolean }>();
const labels: Record<CoverageStatus, string> = { continuous: '连续', partial: '有中断', unknown: '未知' };
</script>
<template>
  <span class="cov"><template v-if="visible"><span
    class="cov-mark"
    :class="`cov-mark--${coverage}`"
    aria-hidden="true"
  />{{ labels[coverage] }}</template><template v-else>—</template></span>
</template>
