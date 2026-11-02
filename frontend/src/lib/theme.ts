import { useCallback, useEffect, useState } from 'react'

type Theme = 'light' | 'dark'

const KEY = 'cairezeki-theme'

/** Dark unless the viewer has switched to light; the OS setting is not consulted. */
export function useTheme() {
  const [theme, setTheme] = useState<Theme>(() =>
    localStorage.getItem(KEY) === 'light' ? 'light' : 'dark',
  )

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
    localStorage.setItem(KEY, theme)
  }, [theme])

  const toggle = useCallback(() => setTheme((t) => (t === 'dark' ? 'light' : 'dark')), [])

  return { theme, setTheme, toggle }
}

/**
 * Categorical series colours, in the palette's documented slot order.
 *
 * The order is the colour-blind-safety mechanism: adjacent pairs were
 * validated for separation under CVD simulation. Assign by index and never
 * cycle - a ninth series folds into "Other" instead.
 */
export const SERIES = [
  'var(--series-1)',
  'var(--series-2)',
  'var(--series-3)',
  'var(--series-4)',
  'var(--series-5)',
  'var(--series-6)',
  'var(--series-7)',
  'var(--series-8)',
] as const

export const SERIES_MAX = SERIES.length

export function seriesColor(index: number): string {
  return SERIES[Math.min(index, SERIES_MAX - 1)]
}

/** Single-hue ramp for magnitude encoding, weakest step first. */
export const SEQUENTIAL = [
  'var(--seq-250)',
  'var(--seq-350)',
  'var(--seq-450)',
  'var(--seq-550)',
  'var(--seq-650)',
] as const

/**
 * Rank within a group, as classes that carry the fill *and* the text colour
 * that is readable on it - the one place in the app where a label sits on a
 * data colour, so the ink has to flip with the step and with the theme.
 *
 * Strongest step first: the largest share in a stack is the most prominent
 * piece of it, in either theme. The ramp is five steps deep because that is
 * what stays monotone and separable; a rank past the fifth takes the last
 * step rather than an invented sixth, which is honest - by then the pieces
 * are slivers and their order on the bar, not their colour, identifies them.
 */
export const RANK_STEPS = ['heat-5', 'heat-4', 'heat-3', 'heat-2', 'heat-1'] as const

/** The neutral for a stack's folded tail - not a step on the ramp. */
export const RANK_OTHER = 'heat-other'

export function rankClass(index: number): string {
  return RANK_STEPS[Math.min(index, RANK_STEPS.length - 1)]
}
