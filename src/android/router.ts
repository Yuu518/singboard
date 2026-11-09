import { createRouter, createWebHistory } from 'vue-router'
import { navItems } from '@/android/components/layout/navItems'

const PagerRoute = { render: () => null }

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      redirect: '/home',
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
      redirect: '/home',
    },
  ],
})

export default router
