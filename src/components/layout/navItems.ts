import { navIcons } from './navIcons'

export interface NavItem {
  path: string
  label: string
  icon: string[]
}

export const navItems: NavItem[] = [
  {
    path: '/overview',
    label: '概览',
    icon: navIcons.overview,
  },
  {
    path: '/proxies',
    label: '代理',
    icon: navIcons.proxies,
  },
  {
    path: '/connections',
    label: '连接',
    icon: navIcons.connections,
  },
  {
    path: '/logs',
    label: '日志',
    icon: navIcons.logs,
  },
  {
    path: '/rules',
    label: '规则',
    icon: navIcons.rules,
  },
  {
    path: '/config',
    label: '配置',
    icon: navIcons.config,
  },
  {
    path: '/settings',
    label: '设置',
    icon: navIcons.settings,
  },
]

export const DEFAULT_START_PAGE = '/proxies'

export function resolveStartPage(path: string): string {
  return navItems.some((item) => item.path === path) ? path : DEFAULT_START_PAGE
}
