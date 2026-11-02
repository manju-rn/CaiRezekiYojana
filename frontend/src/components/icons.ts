import { createLucideIcon } from 'lucide-react'

/**
 * CaiYatra: a wavy-edged banknote carrying ₹, $ and €, on wheels, speed
 * lines streaming behind it. Drawn on Lucide's 24-unit grid so it sits with
 * the rest; the symbols take a finer stroke so they stay apart when small.
 */
export const MoneyOnWheels = createLucideIcon('money-on-wheels', [
  ['path', { d: 'M6 4.5q2.125-1.2 4.25 0t4.25 0t4.25 0t4.25 0V13.5q-2.125 1.2-4.25 0t-4.25 0t-4.25 0t-4.25 0Z', key: 'note' }],
  ['path', { d: 'M8 6.8h3.2M8 8.4h3.2M8 6.8h1.1a1.6 1.6 0 0 1 0 3.2H8l2.6 2.2', strokeWidth: 1.25, key: 'rupee' }],
  [
    'path',
    {
      d: 'M15.6 7.8a1.2 1.1 0 0 0-1.1-.7h-.2a1.1 1.1 0 0 0 0 2.2h.4a1.1 1.1 0 0 1 0 2.2h-.2a1.2 1.1 0 0 1-1.1-.7M14.5 6.2v.9M14.5 11.5v.8',
      strokeWidth: 1.25,
      key: 'dollar',
    },
  ],
  ['path', { d: 'M21 7.3A2.2 2.2 0 1 0 21 10.9M17 8.5h2.6M17 9.7h2.6', strokeWidth: 1.25, key: 'euro' }],
  ['circle', { cx: '10', cy: '18.5', r: '2', key: 'wheel-back' }],
  ['circle', { cx: '19', cy: '18.5', r: '2', key: 'wheel-front' }],
  ['path', { d: 'M2 6.5h2.5', key: 'speed-1' }],
  ['path', { d: 'M1 9.5h3.5', key: 'speed-2' }],
  ['path', { d: 'M2.5 12.5h2', key: 'speed-3' }],
])
