<script setup lang="ts">
withDefaults(defineProps<{ name: 'brand' | 'overview' | 'folder' | 'application' | 'settings'; size?: number }>(), { size: 16 });
</script>
<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <template v-if="name === 'brand'"><rect
      x="3"
      y="4"
      width="18"
      height="16"
      rx="3"
    /><path d="M7 14l3-3 3 2 4-5" /></template>
    <template v-else-if="name === 'overview'"><rect
      x="3"
      y="3"
      width="7"
      height="9"
      rx="1.5"
    /><rect
      x="14"
      y="3"
      width="7"
      height="5"
      rx="1.5"
    /><rect
      x="14"
      y="12"
      width="7"
      height="9"
      rx="1.5"
    /><rect
      x="3"
      y="16"
      width="7"
      height="5"
      rx="1.5"
    /></template>
    <path
      v-else-if="name === 'folder'"
      d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"
    />
    <template v-else-if="name === 'application'"><path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z" /><path d="M4 7.5l8 4.5 8-4.5M12 12v9" /></template>
    <template v-else><circle
      cx="12"
      cy="12"
      r="3"
    /><path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1" /></template>
  </svg>
</template>
