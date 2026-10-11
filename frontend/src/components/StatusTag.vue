<script setup lang="ts">
import type { CandidateStatus } from '../types/objects';
import { STATUS_CLASSES, STATUS_LABELS } from '../utils/objects';
defineProps<{ status: CandidateStatus }>();
</script>
<template>
  <span
    class="tag"
    :class="`tag--${STATUS_CLASSES[status]}`"
  >{{ STATUS_LABELS[status] }}</span>
</template>
