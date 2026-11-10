import { createRouter, createWebHistory } from 'vue-router'
import { useConfigStore } from '@/stores/config'
import { resolveStartPage } from '@/components/layout/navItems'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      redirect: () => resolveStartPage(useConfigStore().config.value.startPage),
    },
    {
      path: '/overview',
      name: 'overview',
      component: () => import('@/views/OverviewPage.vue'),
    },
    {
      path: '/proxies',
      name: 'proxies',
      component: () => import('@/views/ProxiesPage.vue'),
    },
    {
      path: '/connections',
      name: 'connections',
      component: () => import('@/views/ConnectionsPage.vue'),
    },
    {
      path: '/logs',
      name: 'logs',
      component: () => import('@/views/LogsPage.vue'),
    },
    {
      path: '/rules',
      name: 'rules',
      component: () => import('@/views/RulesPage.vue'),
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('@/views/SettingsPage.vue'),
    },
    {
      path: '/config',
      name: 'config',
      component: () => import('@/views/ConfigPage.vue'),
    },
    {
      path: '/config-editor',
      redirect: '/config',
    },
  ],
})

export default router
