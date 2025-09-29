import type { Config } from 'tailwindcss';
import plugin from 'tailwindcss/plugin';

const MUI_BREAKPOINTS: Record<string, number> = {
  xs: 0,
  sm: 600,
  md: 900,
  lg: 1200,
  xl: 1536,
};

const muiScreens = Object.fromEntries(
  Object.entries(MUI_BREAKPOINTS).map(([key, value]) => [key, `${value}px`]),
) as Record<string, string>;

const config: Config = {
  content: ['./index.html', './src/**/*.{ts,tsx,js,jsx}'],
  darkMode: 'selector',
  theme: {
    extend: {
      maxHeight: {
        '1/8': 'calc(100vh / 8)',
      },
      zIndex: {
        top: 100000,
      },
      animation: {
        marquee: 'marquee 4s linear infinite',
      },
      keyframes: {
        marquee: {
          '0%': { transform: 'translateX(100%)' },
          '100%': { transform: 'translateX(-100%)' },
        },
      },
      colors: {
        scroller: 'var(--scroller-color)',
        container: 'var(--background-color)',
      },
    },
    screens: muiScreens,
  },
  plugins: [
    plugin(({ addBase }) => {
      addBase({
        '.scrollbar-hidden::-webkit-scrollbar': {
          width: '0px',
        },
      });
    }),
  ],
};

export default config;
