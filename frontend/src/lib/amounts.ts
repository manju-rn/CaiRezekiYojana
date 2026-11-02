import { useCallback, useSyncExternalStore } from 'react'

/**
 * How portfolio-scale amounts read across the app: in full (₹ 1,23,45,678)
 * or short in the largest unit they fill (₹ 1.23 Cr, $ 4.56M). Chosen once
 * in the banner and remembered; unit prices and compact labels keep their
 * own form either way.
 */
export type AmountStyle = 'full' | 'short'

const KEY = 'cairezeki-amounts'

let style: AmountStyle = localStorage.getItem(KEY) === 'short' ? 'short' : 'full'
const listeners = new Set<() => void>()

/** The current style, for formatters outside React. */
export function amountStyle(): AmountStyle {
  return style
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** The style, and a way to change it; a component calling this re-renders when it changes. */
export function useAmountStyle() {
  const current = useSyncExternalStore(subscribe, amountStyle)
  const setStyle = useCallback((next: AmountStyle) => {
    if (next === style) return
    style = next
    localStorage.setItem(KEY, next)
    listeners.forEach((l) => l())
  }, [])
  return { style: current, setStyle }
}
