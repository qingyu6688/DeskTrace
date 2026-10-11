<script setup lang="ts">
import { NConfigProvider, NDialogProvider, NMessageProvider, dateZhCN, zhCN } from 'naive-ui';
import type { GlobalThemeOverrides } from 'naive-ui';

import AppShell from './components/AppShell.vue';
const themeOverrides: GlobalThemeOverrides = { common: { primaryColor: '#2457c5', primaryColorHover: '#1a3f94', primaryColorPressed: '#1a3f94', borderRadius: '6px', fontFamily: '"Segoe UI Variable Text", "Microsoft YaHei UI", sans-serif' } };
</script>

<template>
  <NConfigProvider
    :locale="zhCN"
    :date-locale="dateZhCN"
    :theme-overrides="themeOverrides"
  >
    <NMessageProvider><NDialogProvider><AppShell><RouterView /></AppShell></NDialogProvider></NMessageProvider>
  </NConfigProvider>
</template>
