import { createRouter, createWebHistory } from 'vue-router'
import { navItems } from '@/android/components/layout/navItems'
import { launchPage } from '@/android/stores/pager'

const PagerRoute = { render: () => null }

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      redirect: launchPage,
    },
    {
      path: '/overview',
      redirect: '/home',
    },
    ...navItems.map((item) => ({
      path: item.path,
      name: item.path.slice(1),
      component: PagerRoute,
    })),
    {
      path: '/:pathMatch(.*)*',
      redirect: launchPage,
    },
  ],
})

export default router
