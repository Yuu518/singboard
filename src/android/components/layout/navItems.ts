import type { AsyncComponentLoader } from 'vue'
import { navIcons } from '@/components/layout/navIcons'

export interface NavItem {
  path: string
  label: string
  icon: string[]
  fill?: boolean
  gate?: string
  load: AsyncComponentLoader
}

export const navItems: NavItem[] = [
  {
    path: '/home',
    load: () => import('@/android/views/HomePage.vue'),
    label: '首页',
    icon: navIcons.home,
  },
  {
    path: '/proxies',
    fill: true,
    load: () => import('@/views/ProxiesPage.vue'),
    gate: '代理信息',
    label: '代理',
    icon: navIcons.proxies,
  },
  {
    path: '/connections',
    fill: true,
    load: () => import('@/views/ConnectionsPage.vue'),
    gate: '连接信息',
    label: '连接',
    icon: navIcons.connections,
  },
  {
    path: '/logs',
    fill: true,
    load: () => import('@/views/LogsPage.vue'),
    gate: '日志',
    label: '日志',
    icon: navIcons.logs,
  },
  {
    path: '/rules',
    fill: true,
    load: () => import('@/views/RulesPage.vue'),
    gate: '规则信息',
    label: '规则',
    icon: navIcons.rules,
  },
  {
    path: '/settings',
    load: () => import('@/android/views/SettingsPage.vue'),
    label: '设置',
    icon: navIcons.settings,
  },
]

export function resolveStartPage(path: string): string {
  return navItems.some((item) => item.path === path) ? path : navItems[0].path
}

