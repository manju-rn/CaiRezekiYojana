/**
 * A colour token that also takes Tailwind's opacity modifier (`bg-series-1/10`):
 * a bare `var()` cannot, so the class would silently generate nothing.
 */
const v = (name) => `color-mix(in srgb, var(--${name}) calc(<alpha-value> * 100%), transparent)`

/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: ['selector', ':root:not([data-theme="light"])'],
  theme: {
    extend: {
      // Every colour resolves to a CSS variable defined in index.css, so
      // light and dark are two sets of values rather than two sets of rules.
      colors: {
        surface: v('surface-1'),
        plane: v('page-plane'),
        ink: {
          DEFAULT: v('text-primary'),
          secondary: v('text-secondary'),
          muted: v('text-muted'),
        },
        line: v('gridline'),
        baseline: v('baseline'),
        series: {
          1: v('series-1'),
          2: v('series-2'),
          3: v('series-3'),
          4: v('series-4'),
          5: v('series-5'),
          6: v('series-6'),
          7: v('series-7'),
          8: v('series-8'),
        },
        status: {
          good: v('status-good'),
          warning: v('status-warning'),
          serious: v('status-serious'),
          critical: v('status-critical'),
        },
        gain: v('delta-up'),
        loss: v('status-critical'),
      },
      fontFamily: {
        sans: ['system-ui', '-apple-system', 'Segoe UI', 'sans-serif'],
      },
      borderRadius: { chart: '4px' },
    },
  },
  plugins: [],
}
