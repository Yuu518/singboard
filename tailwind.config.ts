import type { Config } from 'tailwindcss'
import daisyui from 'daisyui'
import daisyThemes from 'daisyui/src/theming/themes'

const shape = {
  '--rounded-box': '1.125rem',
  '--rounded-btn': '9999px',
  '--rounded-badge': '9999px',
  '--tab-radius': '9999px',
  '--animation-btn': '0.15s',
  '--animation-input': '0.2s',
  '--btn-focus-scale': '0.97',
  '--border-btn': '0px',
}

export default {
  content: ['./index.html', './src/**/*.{vue,js,ts,jsx,tsx}'],
  theme: {
    extend: {},
  },
  plugins: [daisyui],
  daisyui: {
    themes: [
      {
        light: {
          ...daisyThemes.light,
          primary: '#007AFF',
          'primary-content': '#FFFFFF',
          secondary: '#5856D6',
          'secondary-content': '#FFFFFF',
          accent: '#AF52DE',
          'accent-content': '#FFFFFF',
          neutral: '#3A3A3C',
          'neutral-content': '#F2F2F7',
          'base-100': '#FFFFFF',
          'base-200': '#F2F2F7',
          'base-300': '#E5E5EA',
          'base-content': '#1C1C1E',
          info: '#5AC8FA',
          'info-content': '#FFFFFF',
          success: '#34C759',
          'success-content': '#FFFFFF',
          warning: '#FF9500',
          'warning-content': '#FFFFFF',
          error: '#FF3B30',
          'error-content': '#FFFFFF',
          ...shape,
        },
      },
      {
        dark: {
          ...daisyThemes.dark,
          primary: '#0A84FF',
          'primary-content': '#FFFFFF',
          secondary: '#5E5CE6',
          'secondary-content': '#FFFFFF',
          accent: '#BF5AF2',
          'accent-content': '#FFFFFF',
          neutral: '#48484A',
          'neutral-content': '#F2F2F7',
          'base-100': '#1C1C1E',
          'base-200': '#2C2C2E',
          'base-300': '#3A3A3C',
          'base-content': '#F2F2F7',
          info: '#64D2FF',
          'info-content': '#0B1A24',
          success: '#30D158',
          'success-content': '#FFFFFF',
          warning: '#FF9F0A',
          'warning-content': '#1C1C1E',
          error: '#FF453A',
          'error-content': '#FFFFFF',
          ...shape,
        },
      },
    ],
  },
} satisfies Config
