import { createRouter, createWebHashHistory } from 'vue-router';
import type { Component } from 'vue';

type PageModule = { default: Component };

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', component: (): Promise<PageModule> => import('../pages/OverviewPage.vue') },
    { path: '/folders', component: (): Promise<PageModule> => import('../pages/ObjectsPage.vue'), props: { objectType: 'folder' } },
    { path: '/applications', component: (): Promise<PageModule> => import('../pages/ObjectsPage.vue'), props: { objectType: 'application' } },
    { path: '/objects/:id', component: (): Promise<PageModule> => import('../pages/ObjectDetailPage.vue') },
    { path: '/settings', component: (): Promise<PageModule> => import('../pages/SettingsPage.vue') },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
});
