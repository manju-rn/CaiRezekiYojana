/**
 * Charts, built as plain HTML/SVG.
 *
 * Three forms cover what this app needs: a composition bar (parts of a
 * whole), a donut (the same job, where a round shape was asked for) and
 * ranked magnitude bars. All carry a hover layer and a table view, and all
 * draw their colours from the validated palette in its documented slot
 * order - assigned by entity, never by rank, so filtering the data never
 * repaints the survivors.
 */

import { ReactNode, useEffect, useId, useMemo, useRef, useState } from 'react'

import {
  date,
  deltaClass,
  humanise,
  money,
  num,
  pct,
  qty,
  rate as fmtRate,
  signedMoney,
  signedPct,
} from '../lib/format'
import { RANK_OTHER, rankClass, seriesColor } from '../lib/theme'

/** Past this many categories, the tail folds into "Other" rather than
 *  inventing a ninth hue. */
const MAX_SLICES = 7

export interface Slice {
  key: string
  label: string
  value: number
  /** Extra line shown in the tooltip, e.g. the exchange rate used. */
  detail?: string
}

function foldTail(slices: Slice[]): Slice[] {
  const sorted = [...slices].filter((s) => s.value > 0).sort((a, b) => b.value - a.value)
  if (sorted.length <= MAX_SLICES + 1) return sorted

  const head = sorted.slice(0, MAX_SLICES)
  const tail = sorted.slice(MAX_SLICES)
  return [
    ...head,
    {
      key: '__other__',
      label: `Other (${tail.length})`,
      value: tail.reduce((sum, s) => sum + s.value, 0),
    },
  ]
}

function Tooltip({ children }: { children: ReactNode }) {
  return (
    <div
      role="tooltip"
      className="pointer-events-none absolute bottom-full left-1/2 z-20 mb-2 -translate-x-1/2 whitespace-nowrap rounded-md border border-[color:var(--hairline)] bg-surface px-2.5 py-1.5 text-xs shadow-lg"
    >
      {children}
    </div>
  )
}

export function TableToggle({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      onClick={onToggle}
      className="text-xs font-medium text-ink-muted underline-offset-2 hover:text-ink hover:underline"
      aria-expanded={open}
    >
      {open ? 'Hide table' : 'View as table'}
    </button>
  )
}

/**
 * Parts of a whole, as one segmented bar.
 *
 * A bar rather than a pie: comparing segment lengths against a shared
 * baseline is a job people are good at, comparing angles is not.
 */
export function CompositionBar({
  slices,
  currency,
  title,
  emptyMessage = 'Nothing to show yet.',
  whole = false,
}: {
  slices: Slice[]
  currency: string
  title: string
  emptyMessage?: string
  /** Amounts in whole units, for portfolio-scale figures. */
  whole?: boolean
}) {
  const [hovered, setHovered] = useState<string | null>(null)
  const [showTable, setShowTable] = useState(false)
  const id = useId()

  const data = foldTail(slices)
  const total = data.reduce((sum, s) => sum + s.value, 0)

  if (total <= 0) {
    return <p className="py-6 text-sm text-ink-muted">{emptyMessage}</p>
  }

  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <span className="sr-only" id={id}>
          {title}
        </span>
        <TableToggle open={showTable} onToggle={() => setShowTable((v) => !v)} />
      </div>

      {/* 2px surface gaps keep adjacent fills from reading as one block. */}
      <div className="flex h-9 w-full gap-[2px]" role="img" aria-labelledby={id}>
        {data.map((s, i) => {
          const width = (s.value / total) * 100
          return (
            <div
              key={s.key}
              className="relative h-full transition-opacity first:rounded-l-chart last:rounded-r-chart"
              style={{
                width: `${width}%`,
                minWidth: 3,
                background: seriesColor(i),
                opacity: hovered && hovered !== s.key ? 0.45 : 1,
              }}
              onMouseEnter={() => setHovered(s.key)}
              onMouseLeave={() => setHovered(null)}
              onFocus={() => setHovered(s.key)}
              onBlur={() => setHovered(null)}
              tabIndex={0}
              aria-label={`${s.label}: ${money(s.value, currency, { whole })}, ${pct((s.value / total) * 100)}`}
            >
              {hovered === s.key && (
                <Tooltip>
                  <div className="font-medium text-ink">{s.label}</div>
                  <div className="tabular text-ink-secondary">
                    {money(s.value, currency, { whole })} · {pct((s.value / total) * 100)}
                  </div>
                  {s.detail && <div className="text-ink-muted">{s.detail}</div>}
                </Tooltip>
              )}
            </div>
          )
        })}
      </div>

      {/* Legend doubles as the direct labels - identity is never colour alone. */}
      <ul className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1.5 sm:grid-cols-3">
        {data.map((s, i) => (
          <li
            key={s.key}
            className="flex items-baseline gap-1.5 text-[13px]"
            onMouseEnter={() => setHovered(s.key)}
            onMouseLeave={() => setHovered(null)}
          >
            <span
              aria-hidden
              className="mt-0.5 inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
              style={{ background: seriesColor(i) }}
            />
            <span className="truncate text-ink-secondary">{s.label}</span>
            <span className="tabular ml-auto shrink-0 font-medium text-ink">
              {pct((s.value / total) * 100, 1)}
            </span>
          </li>
        ))}
      </ul>

      {showTable && (
        <table className="mt-4 w-full text-sm">
          <thead>
            <tr className="border-b border-[color:var(--hairline)] text-left text-ink-muted">
              <th className="py-1.5 font-medium">Category</th>
              <th className="py-1.5 text-right font-medium">Value</th>
              <th className="py-1.5 text-right font-medium">Share</th>
            </tr>
          </thead>
          <tbody>
            {data.map((s) => (
              <tr key={s.key} className="border-b border-[color:var(--hairline)] last:border-0">
                <td className="py-1.5 text-ink-secondary">{s.label}</td>
                <td className="tabular py-1.5 text-right text-ink">{money(s.value, currency, { whole })}</td>
                <td className="tabular py-1.5 text-right text-ink-secondary">
                  {pct((s.value / total) * 100, 1)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}

/* ------------------------------------------------------------------ *
 * Donut
 * ------------------------------------------------------------------ */

/** Default diameter. The radii are fixed fractions of whatever diameter is
 *  asked for, so a larger ring keeps the same proportions. */
const DONUT_SIZE = 168
const DONUT_OUTER_RATIO = 78 / 168
const DONUT_INNER_RATIO = 50 / 168

/** The diameter every ring in the app is drawn at - half again the default,
 *  so the dashboard and the funds tab read as one set rather than as charts
 *  that happen to look alike. */
export const RING_SIZE = 252

/** Centre text in a roomy ring, as fractions of the hole's diameter. The
 *  figure is what the ring is read for, so it is sized to fill the hole:
 *  a compact one like "₹ 5Cr" runs to about 134px of the 150px a 252px
 *  ring offers. The label takes a little under half that, which keeps it
 *  inside the ring at its own height, where the hole is narrower than on
 *  the centre line. */
const CENTRE_VALUE_RATIO = 0.36
const CENTRE_LABEL_RATIO = 0.16

/** With two or more lines under it the figure gives up some size, so the
 *  block still clears the ring where the hole narrows above and below the
 *  centre line. */
const CENTRE_STACKED_VALUE_RATIO = 0.28
const CENTRE_STACKED_LINE_RATIO = 0.12

/** Between two small lines, as a fraction of their size. */
const CENTRE_SMALL_GAP_RATIO = 0.45

/** How much of the hole's diameter the figure may run to. Less than all of
 *  it: the figure sits a little off the centre line, where the hole is
 *  narrower. */
const CENTRE_MAX_WIDTH_RATIO = 0.8

/** Rough advance widths, in ems, of semibold UI type. Enough to tell
 *  "₹ 5Cr" from "₹ 5.2Cr" and shrink the latter before it touches the ring;
 *  this is an estimate, not a measurement, so it errs wide. */
function textEms(text: string): number {
  let ems = 0
  for (const ch of text) {
    if (ch === '.' || ch === ',') ems += 0.3
    else if (ch === ' ' || ch === ' ') ems += 0.2
    else if (ch === ' ' || ch === ' ') ems += 0.3
    else if (/[a-z]/.test(ch)) ems += 0.55
    else ems += 0.62
  }
  return ems
}

/** Cap height as a fraction of font size, and the gap between the two
 *  centre lines - enough to read as two, not enough to break the block. */
const CAP_HEIGHT_RATIO = 0.7
const CENTRE_LINE_GAP_RATIO = 0.15

interface RingGeometry {
  size: number
  outer: number
  inner: number
}

function ringGeometry(size: number): RingGeometry {
  const round = (v: number) => Math.round(v * 100) / 100
  return { size, outer: round(size * DONUT_OUTER_RATIO), inner: round(size * DONUT_INNER_RATIO) }
}

/** SVG path for one ring segment from `a0` to `a1` radians (12 o'clock = 0). */
function ringSegment(g: RingGeometry, a0: number, a1: number): string {
  const cx = g.size / 2
  const cy = g.size / 2
  const at = (r: number, a: number) => {
    const t = a - Math.PI / 2
    return `${(cx + r * Math.cos(t)).toFixed(2)},${(cy + r * Math.sin(t)).toFixed(2)}`
  }
  const large = a1 - a0 > Math.PI ? 1 : 0
  return [
    `M${at(g.outer, a0)}`,
    `A${g.outer},${g.outer} 0 ${large} 1 ${at(g.outer, a1)}`,
    `L${at(g.inner, a1)}`,
    `A${g.inner},${g.inner} 0 ${large} 0 ${at(g.inner, a0)}`,
    'Z',
  ].join(' ')
}

/**
 * How big the lines in the hole are, and where their baselines sit: the
 * figure, then `lines` smaller ones under it.
 *
 * A roomy ring is read for the figure in its centre, so that figure is
 * sized to the hole rather than to a type step. The lines are then
 * centred as one block: at this size the fixed baselines the smaller ring
 * uses would have the figure and its label overlapping.
 */
function centreText(
  g: RingGeometry,
  roomy: boolean,
  lines: number,
  stacked: boolean,
  /** The widest figure the hole will have to hold, in ems. */
  figureEms: number,
) {
  const scale = g.size / DONUT_SIZE
  if (!roomy) {
    const lift = (lines - 1) * 6 * scale
    return {
      valueSize: 14,
      labelSize: 10,
      valueY: g.size / 2 - (lines > 0 ? 2 * scale + lift : -4 * scale),
      lineY: (i: number) => g.size / 2 + (14 + i * 12) * scale - lift,
    }
  }

  const hole = g.inner * 2
  const valueSize = Math.min(
    hole * (stacked ? CENTRE_STACKED_VALUE_RATIO : CENTRE_VALUE_RATIO),
    (hole * CENTRE_MAX_WIDTH_RATIO) / Math.max(figureEms, 1),
  )
  const labelSize = hole * (stacked ? CENTRE_STACKED_LINE_RATIO : CENTRE_LABEL_RATIO)
  const cap = (s: number) => s * CAP_HEIGHT_RATIO

  if (lines === 0) {
    return { valueSize, labelSize, valueY: g.size / 2 + cap(valueSize) / 2, lineY: () => 0 }
  }

  const gap = valueSize * CENTRE_LINE_GAP_RATIO
  const smallGap = labelSize * CENTRE_SMALL_GAP_RATIO
  const block = cap(valueSize) + gap + lines * cap(labelSize) + (lines - 1) * smallGap
  const valueY = g.size / 2 - block / 2 + cap(valueSize)
  return {
    valueSize,
    labelSize,
    valueY,
    lineY: (i: number) => valueY + gap + cap(labelSize) + i * (cap(labelSize) + smallGap),
  }
}

/**
 * Parts of a whole, as a ring.
 *
 * The legend carries the numbers - a ring is read for the rough split, and
 * the percentages beside each label do the precise work. A 2px surface
 * stroke separates adjacent fills, the same spacer the bar uses. The
 * diameter is a prop, so a ring carrying eight categories can take more
 * room than one splitting two.
 */
export function DonutChart({
  slices,
  currency,
  title,
  centreLabel,
  centreLines,
  tableToggle = true,
  emptyMessage = 'Nothing to show yet.',
  size = DONUT_SIZE,
  whole = false,
  legendBeside = false,
}: {
  slices: Slice[]
  currency: string
  title: string
  /** Word under the total in the middle, e.g. "invested". */
  centreLabel?: string
  /** Lines under the total instead of a label - the total again in other
   *  currencies, say. Hovering a slice swaps them for its share. */
  centreLines?: string[]
  /** Off where the page sets a fuller table under the ring. */
  tableToggle?: boolean
  emptyMessage?: string
  /** Diameter in px. A ring with many slices can ask for more room than
   *  one with two; the centre text follows. */
  size?: number
  /** Amounts in whole units, for portfolio-scale figures. */
  whole?: boolean
  /** The legend to the right of the ring, where the card is wide enough,
   *  rather than under it. */
  legendBeside?: boolean
}) {
  const [hovered, setHovered] = useState<string | null>(null)
  const [showTable, setShowTable] = useState(false)
  const id = useId()

  const g = ringGeometry(size)
  const scale = size / DONUT_SIZE
  // Half again the default or more, and the hole has room for larger text.
  const roomy = scale >= 1.4

  const data = foldTail(slices)
  const total = data.reduce((sum, s) => sum + s.value, 0)

  if (total <= 0) {
    return <p className="py-6 text-sm text-ink-muted">{emptyMessage}</p>
  }

  let angle = 0
  const segments = data.map((s, i) => {
    const sweep = (s.value / total) * Math.PI * 2
    const seg = { slice: s, index: i, a0: angle, a1: angle + sweep }
    angle += sweep
    return seg
  })
  const active = hovered ? data.find((s) => s.key === hovered) : undefined
  const idle = centreLines ?? (centreLabel ? [centreLabel] : [])
  const lines = active ? [pct((active.value / total) * 100, 1)] : idle
  const figure = (v: number) => money(v, currency, { compact: true })
  // Sized for the idle block and the widest figure hovering can show, so
  // hovering does not resize the figure.
  const figureEms = Math.max(textEms(figure(total)), ...data.map((s) => textEms(figure(s.value))))
  const centre = centreText(g, roomy, lines.length, idle.length > 1, figureEms)

  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <span className="sr-only" id={id}>
          {title}
        </span>
        {tableToggle && <TableToggle open={showTable} onToggle={() => setShowTable((v) => !v)} />}
      </div>

      <div
        className={`flex flex-col items-center gap-4 ${legendBeside ? 'sm:flex-row sm:gap-8' : ''}`}
      >
        <div className="relative shrink-0">
          <svg
            width={size}
            height={size}
            viewBox={`0 0 ${size} ${size}`}
            role="img"
            aria-labelledby={id}
          >
            {segments.map(({ slice: s, index, a0, a1 }) =>
              data.length === 1 ? (
                <circle
                  key={s.key}
                  cx={size / 2}
                  cy={size / 2}
                  r={(g.outer + g.inner) / 2}
                  fill="none"
                  stroke={seriesColor(index)}
                  strokeWidth={g.outer - g.inner}
                />
              ) : (
                <path
                  key={s.key}
                  d={ringSegment(g, a0, a1)}
                  fill={seriesColor(index)}
                  stroke="var(--surface-1)"
                  strokeWidth={2}
                  strokeLinejoin="round"
                  className="transition-opacity"
                  style={{ opacity: hovered && hovered !== s.key ? 0.45 : 1 }}
                  onMouseEnter={() => setHovered(s.key)}
                  onMouseLeave={() => setHovered(null)}
                  onFocus={() => setHovered(s.key)}
                  onBlur={() => setHovered(null)}
                  tabIndex={0}
                  aria-label={`${s.label}: ${money(s.value, currency, { whole })}, ${pct((s.value / total) * 100)}`}
                />
              ),
            )}
            {/* The hole answers "how much in total" without a second glance. */}
            <text
              x={size / 2}
              y={centre.valueY}
              fontSize={centre.valueSize}
              textAnchor="middle"
              className="tabular fill-[color:var(--text-primary)] font-semibold"
            >
              {figure(active ? active.value : total)}
            </text>
            {lines.map((line, i) => (
              <text
                key={i}
                x={size / 2}
                y={centre.lineY(i)}
                fontSize={centre.labelSize}
                textAnchor="middle"
                className="fill-[color:var(--text-muted)]"
              >
                {line}
              </text>
            ))}
          </svg>
          {active && (
            <Tooltip>
              <div className="font-medium text-ink">{active.label}</div>
              <div className="tabular text-ink-secondary">
                {money(active.value, currency, { whole })} · {pct((active.value / total) * 100)}
              </div>
              {active.detail && <div className="text-ink-muted">{active.detail}</div>}
            </Tooltip>
          )}
        </div>

        {/* Legend doubles as the direct labels - identity is never colour alone.
            It sits under the ring, or beside it where asked, name and share
            only: the amounts are in the centre on hover and in the table.
            Long names wrap rather than truncate. */}
        <ul className={`w-full space-y-1.5 ${legendBeside ? 'sm:w-auto sm:min-w-0 sm:max-w-sm sm:flex-1' : ''}`}>
          {data.map((s, i) => (
            <li
              key={s.key}
              className="flex items-baseline gap-2 text-[13px] leading-5"
              onMouseEnter={() => setHovered(s.key)}
              onMouseLeave={() => setHovered(null)}
            >
              <span
                aria-hidden
                className="mt-0.5 inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
                style={{ background: seriesColor(i) }}
              />
              <span className="min-w-0 flex-1 text-ink-secondary">{s.label}</span>
              <span className="tabular shrink-0 font-medium text-ink">
                {pct((s.value / total) * 100, 1)}
              </span>
            </li>
          ))}
        </ul>
      </div>

      {tableToggle && showTable && (
        <table className="mt-4 w-full text-sm">
          <thead>
            <tr className="border-b border-[color:var(--hairline)] text-left text-ink-muted">
              <th className="py-1.5 font-medium">Category</th>
              <th className="py-1.5 text-right font-medium">Value</th>
              <th className="py-1.5 text-right font-medium">Share</th>
            </tr>
          </thead>
          <tbody>
            {data.map((s) => (
              <tr key={s.key} className="border-b border-[color:var(--hairline)] last:border-0">
                <td className="py-1.5 text-ink-secondary">{s.label}</td>
                <td className="tabular py-1.5 text-right text-ink">{money(s.value, currency, { whole })}</td>
                <td className="tabular py-1.5 text-right text-ink-secondary">
                  {pct((s.value / total) * 100, 1)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}

/** Where each slice's colour sits in a ring given these slices: the ring
 *  sorts by value and folds its tail, so a table set beside it asks here
 *  rather than guessing. Folded or empty slices have no swatch. */
export function donutSwatches(slices: Slice[]): Map<string, string> {
  const data = foldTail(slices)
  return new Map(data.filter((s) => s.key !== '__other__').map((s, i) => [s.key, seriesColor(i)]))
}

export interface MagnitudeRow {
  key: string
  label: string
  value: number
  /** Secondary line under the label, e.g. "USD · 88.12 INR". */
  sublabel?: string
  /** Extra tooltip lines, e.g. the funds a security is held through. */
  details?: string[]
}

/**
 * Ranked magnitude. One hue, because there is one measure - length carries
 * the comparison and colour would only add noise.
 */
export function MagnitudeBars({
  rows,
  currency,
  emptyMessage = 'Nothing to show yet.',
  maxRows = 10,
}: {
  rows: MagnitudeRow[]
  currency: string
  emptyMessage?: string
  maxRows?: number
}) {
  const [hovered, setHovered] = useState<string | null>(null)

  const data = [...rows]
    .filter((r) => Number.isFinite(r.value))
    .sort((a, b) => Math.abs(b.value) - Math.abs(a.value))
    .slice(0, maxRows)

  if (data.length === 0) {
    return <p className="py-6 text-sm text-ink-muted">{emptyMessage}</p>
  }

  const max = Math.max(...data.map((r) => Math.abs(r.value)), 1)

  return (
    <ul className="space-y-2.5">
      {data.map((r) => (
        <li
          key={r.key}
          className="relative grid grid-cols-[minmax(0,10rem)_1fr_auto] items-center gap-3"
          onMouseEnter={() => setHovered(r.key)}
          onMouseLeave={() => setHovered(null)}
        >
          <div className="min-w-0">
            <div className="truncate text-xs font-medium text-ink">{r.label}</div>
            {r.sublabel && (
              <div className="truncate text-[11px] text-ink-muted">{r.sublabel}</div>
            )}
          </div>

          {/* Bars grow from a shared baseline; ends are rounded 4px. */}
          <div className="relative h-4 w-full">
            <div
              className="h-full rounded-r-chart transition-opacity"
              style={{
                width: `${(Math.abs(r.value) / max) * 100}%`,
                minWidth: 2,
                background: 'var(--seq-450)',
                opacity: hovered && hovered !== r.key ? 0.5 : 1,
              }}
            />
            {hovered === r.key && (
              <Tooltip>
                <div className="font-medium text-ink">{r.label}</div>
                <div className="tabular text-ink-secondary">{money(r.value, currency)}</div>
                {r.details?.map((line) => (
                  <div key={line} className="text-ink-muted">
                    {line}
                  </div>
                ))}
              </Tooltip>
            )}
          </div>

          <div className="tabular shrink-0 text-xs text-ink">{money(r.value, currency, { compact: true })}</div>
        </li>
      ))}
    </ul>
  )
}

/**
 * Term progress for one deposit: elapsed against the full term.
 *
 * A meter, not a chart - it shows one value against one known maximum, so a
 * plot would be overkill.
 */
export function ProgressMeter({
  pctComplete,
  label,
}: {
  pctComplete: number
  label: string
}) {
  const clamped = Math.max(0, Math.min(100, pctComplete))
  return (
    <div className="flex items-center gap-2">
      <div
        className="h-1.5 w-20 shrink-0 overflow-hidden rounded-full"
        style={{ background: 'var(--gridline)' }}
        role="meter"
        aria-valuenow={Math.round(clamped)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={label}
      >
        <div className="h-full rounded-full" style={{ width: `${clamped}%`, background: 'var(--seq-450)' }} />
      </div>
      <span className="tabular text-[11px] text-ink-muted">{num(clamped, 0)}%</span>
    </div>
  )
}

/**
 * The exchange number, shown beside a converted figure.
 *
 * Every non-base amount in this app renders with this next to it, so a
 * converted number is never presented without the rate that produced it.
 */
export function FxChip({
  currency,
  baseCurrency,
  rate,
  source,
  adjusted,
  feePct,
}: {
  currency: string
  baseCurrency: string
  rate: number
  source?: string
  adjusted?: boolean
  /** The transfer fee inside `rate`, as a % of the amount converted. */
  feePct?: number | null
}) {
  if (currency === baseCurrency) return null

  const unavailable = source === 'unavailable'
  return (
    <span
      className="tabular inline-flex items-center gap-1 rounded bg-ink/5 px-1.5 py-0.5 text-[11px] text-ink-secondary"
      title={
        unavailable
          ? `No rate available for ${currency} → ${baseCurrency}; counted at 1:1`
          : `1 ${currency} = ${fmtRate(rate)} ${baseCurrency}${
              feePct != null
                ? ` after a ${num(feePct, 2)}% Wise fee on this amount`
                : adjusted
                  ? ' (adjusted)'
                  : ''
            }${source ? ` · ${source}` : ''}`
      }
    >
      {unavailable ? (
        <>
          <span aria-hidden className="text-status-serious">
            ⚠
          </span>
          no rate
        </>
      ) : (
        <>
          1 {currency} = {fmtRate(rate)} {baseCurrency}
          {adjusted && (
            <span
              aria-hidden
              title={feePct != null ? 'Wise fee included' : 'A margin or manual override is applied'}
            >
              ✎
            </span>
          )}
        </>
      )}
    </span>
  )
}

/* ------------------------------------------------------------------ *
 * Price history
 * ------------------------------------------------------------------ */

export interface SeriesPoint {
  /** ISO date, `YYYY-MM-DD`. */
  date: string
  value: number
}

export interface TradeMark {
  date: string
  kind: string
  quantity?: number
  price?: number
  /** Replaces the default "n units @ price" wording. A portfolio-level mark
   *  stands for a day's dealing across several holdings, where a unit count
   *  and a unit price mean nothing. */
  label?: string
}

/** What a mark says on hover: its own wording, or the single-trade default. */
function markText(t: TradeMark, currency: string): string {
  if (t.label) return t.label
  return `${humanise(t.kind)} ${qty(t.quantity ?? 0)} units @ ${money(t.price ?? 0, currency)}`
}

/** Ranges offered above the plot. `days: null` means "everything stored". */
const RANGES: { key: string; label: string; days: number | null }[] = [
  { key: '1d', label: '1D', days: 1 },
  { key: '5d', label: '5D', days: 5 },
  { key: '15d', label: '15D', days: 15 },
  { key: '1m', label: '1M', days: 30 },
  { key: '3m', label: '3M', days: 90 },
  { key: '6m', label: '6M', days: 182 },
  { key: '1y', label: '1Y', days: 365 },
  { key: '3y', label: '3Y', days: 1096 },
  { key: '5y', label: '5Y', days: 1826 },
  { key: '7y', label: '7Y', days: 2557 },
  { key: '10y', label: '10Y', days: 3653 },
  { key: 'max', label: 'Max', days: null },
]

/** The window the chart opens on: the last day's move, the question asked
 *  most often. The longer ranges are one click away. */
const DEFAULT_RANGE = '1d'

/** Past this many trades in view, the markers stop helping and start hiding
 *  the line. A monthly SIP over ten years is 120 dots, which still reads. */
const MAX_TRADE_MARKS = 200

/** Past this many marks, the dots shrink so the line stays visible. */
const DENSE_TRADE_MARKS = 60

/** Purchases and redemptions on the price line: yellow in, red out. */
const BUY_MARK = 'var(--status-warning)'
const SELL_MARK = 'var(--series-8)'

const DAY_MS = 86_400_000
const CHART_HEIGHT = 224
/** Beside the stat tiles on a holdings page: short enough that the card is
 *  about as tall as a dashboard tile with its sparkline. */
const COMPACT_CHART_HEIGHT = 96
const PAD = { top: 14, right: 60, bottom: 24, left: 8 }

/** Bare `YYYY-MM-DD` parses as UTC midnight, which reads as the day before in
 *  any negative-offset zone. Anchor it to local midnight instead. */
function parseDay(iso: string): number {
  return Date.parse(/^\d{4}-\d{2}-\d{2}$/.test(iso) ? `${iso}T00:00:00` : iso)
}

/** Width of the element, tracked live - the plot is drawn in real pixels so
 *  the 2px stroke and the tick text are never scaled by a viewBox. */
export function useWidth<T extends HTMLElement>() {
  const ref = useRef<T | null>(null)
  const [width, setWidth] = useState(0)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    setWidth(el.getBoundingClientRect().width)
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width))
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  return { ref, width }
}

type TimedPoint = SeriesPoint & { ms: number }

/** Finite points with their times, oldest first. */
function timed(points: SeriesPoint[]): TimedPoint[] {
  return points
    .filter((p) => Number.isFinite(p.value))
    .map((p) => ({ ...p, ms: parseDay(p.date) }))
    .sort((a, b) => a.ms - b.ms)
}

/**
 * The last `days` of a sorted series; everything when `days` is null.
 *
 * "Today" is the last price we hold, not the wall clock - a stale feed
 * should shorten the window, not empty it.
 */
function lastDays(series: TimedPoint[], days: number | null): TimedPoint[] {
  if (days === null || series.length === 0) return series
  const cutoff = series[series.length - 1].ms - days * DAY_MS
  // Start from the last close on or before the cutoff, so the change is
  // measured against the right base. Prices are daily and skip weekends
  // and holidays: without this anchor, 1D on a Monday would hold only
  // Monday's close, and a short window would miss its opening price.
  let start = series.findIndex((p) => p.ms > cutoff)
  if (start === -1) start = series.length
  const slice = series.slice(Math.max(0, start - 1))
  // Too little in the window to draw a line: show everything rather than
  // an empty plot, and say so in the header.
  return slice.length >= 2 ? slice : series
}

function axisDate(ms: number, spanDays: number): string {
  const d = new Date(ms)
  if (spanDays <= 120) return d.toLocaleDateString('en-GB', { day: '2-digit', month: 'short' })
  if (spanDays <= 900) return d.toLocaleDateString('en-GB', { month: 'short', year: '2-digit' })
  return d.toLocaleDateString('en-GB', { year: 'numeric' })
}

/** Axis labels have ~56px: keep the cents on small numbers, compact the rest. */
function axisMoney(value: number, currency: string, whole = false): string {
  return money(value, currency, { compact: Math.abs(value) >= 10_000, whole })
}

/**
 * One instrument's price over time.
 *
 * A single series, so no legend - the header names what is plotted. The line
 * takes its colour from the direction of travel over the *selected* range,
 * which is the one thing the reader is asking the chart, and the same signed
 * change is printed in the header so the direction is never colour alone.
 */
export function PriceHistoryChart({
  points,
  currency,
  title,
  trades = [],
  maxMarks = MAX_TRADE_MARKS,
  emptyMessage = 'No stored prices for this holding yet.',
  compact = false,
  valueSuffix,
  wholeUnits = false,
}: {
  points: SeriesPoint[]
  currency: string
  title: string
  trades?: TradeMark[]
  /** Marks in view past which they are hidden. A portfolio line carries a
   *  mark per dealing day rather than per trade, so it can show more of
   *  them before the dots start hiding the line. */
  maxMarks?: number
  emptyMessage?: string
  /** A short plot, with the figures and range buttons on one line. */
  compact?: boolean
  /** Muted text after the latest value - what the figure leaves out. */
  valueSuffix?: string
  /** Amounts in whole units, for portfolio-scale figures. Percentages keep
   *  their decimals. */
  wholeUnits?: boolean
}) {
  const [rangeKey, setRangeKey] = useState(DEFAULT_RANGE)
  const [cursor, setCursor] = useState<number | null>(null)
  const [showTable, setShowTable] = useState(false)
  const { ref, width } = useWidth<HTMLDivElement>()
  const titleId = useId()

  const series = useMemo(() => timed(points), [points])

  const newest = series.length > 0 ? series[series.length - 1].ms : 0
  const oldest = series.length > 0 ? series[0].ms : 0
  const storedDays = (newest - oldest) / DAY_MS

  // A range is offered only once at least half of it is actually stored -
  // a "5Y" button on eight months of history promises depth that isn't
  // there. The default falls back to Max rather than leaving a disabled
  // button looking selected.
  const covered = (r: (typeof RANGES)[number]) => r.days === null || storedDays >= r.days * 0.5
  const selected = RANGES.find((r) => r.key === rangeKey) ?? RANGES[RANGES.length - 1]

  const active = covered(selected) ? selected : RANGES[RANGES.length - 1]
  const data = useMemo(() => lastDays(series, active.days), [series, active])

  // A decade of daily closes is thousands of points behind a ~700px plot.
  // Thin them for drawing - the reader cannot resolve more than one point
  // per pixel, and the crosshair redraws on every pointer move. The table
  // view below still lists every stored close.
  const plotted = useMemo(() => {
    const step = Math.ceil(data.length / 900)
    if (step <= 1) return data
    return data.filter((_, i) => i % step === 0 || i === data.length - 1)
  }, [data])

  if (series.length === 0) {
    return <p className="py-6 text-sm text-ink-muted">{emptyMessage}</p>
  }

  const whole = data.length === series.length
  const rangeLabel = active.days === null || whole ? 'all stored history' : active.label
  const first = plotted[0]
  const last = plotted[plotted.length - 1]
  const change = last.value - first.value
  const changePct = first.value !== 0 ? (change / Math.abs(first.value)) * 100 : null
  const stroke = change >= 0 ? 'var(--delta-up)' : 'var(--status-critical)'

  const plotWidth = Math.max(width, 280) - PAD.left - PAD.right
  const chartHeight = compact ? COMPACT_CHART_HEIGHT : CHART_HEIGHT
  const plotHeight = chartHeight - PAD.top - PAD.bottom
  const baseline = PAD.top + plotHeight

  const values = plotted.map((p) => p.value)
  const lo = Math.min(...values)
  const hi = Math.max(...values)
  const headroom = (hi - lo || Math.abs(hi) || 1) * 0.08
  const yMin = lo - headroom
  const yMax = hi + headroom

  const x = (ms: number) =>
    PAD.left + (last.ms === first.ms ? plotWidth : ((ms - first.ms) / (last.ms - first.ms)) * plotWidth)
  const y = (value: number) => PAD.top + plotHeight - ((value - yMin) / (yMax - yMin)) * plotHeight

  const spanDays = Math.max((last.ms - first.ms) / DAY_MS, 1)
  const linePath = plotted
    .map((p, i) => `${i === 0 ? 'M' : 'L'}${x(p.ms).toFixed(1)},${y(p.value).toFixed(1)}`)
    .join(' ')
  const areaPath = `${linePath} L${x(last.ms).toFixed(1)},${baseline} L${x(first.ms).toFixed(1)},${baseline} Z`

  // A short plot has room for the extremes only.
  const yTicks = compact ? [hi, lo] : [hi, (hi + lo) / 2, lo]
  // A span of a few days has too few dates for five ticks without repeats.
  const xTicks = (spanDays < 4 ? [0, 1] : [0, 0.25, 0.5, 0.75, 1]).map((f) => first.ms + (last.ms - first.ms) * f)

  const inRange = trades.filter((t) => {
    const ms = parseDay(t.date)
    return ms >= first.ms && ms <= last.ms
  })
  const marks = inRange.length > maxMarks ? [] : inRange
  const markRadius = marks.length > DENSE_TRADE_MARKS ? 3 : 4

  /** Price on the plotted line at a trade's date, so the mark sits on the
   *  line rather than floating at a price the axis may not even cover. */
  const lineValueAt = (ms: number) => {
    let nearest = plotted[0]
    for (const p of plotted) {
      if (p.ms <= ms) nearest = p
      else break
    }
    return nearest.value
  }

  /** A trade sits at the price it was actually done at when the axis covers
   *  it - with daily history that is on the line; with sparse history it is
   *  the line - and falls back to the line otherwise. A mark with no price
   *  of its own, such as a day of portfolio dealing, always rides the line. */
  const markValue = (t: TradeMark, ms: number) =>
    t.price !== undefined && t.price > yMin && t.price < yMax ? t.price : lineValueAt(ms)

  const nearestIndex = (clientX: number, rect: DOMRect) => {
    const px = clientX - rect.left
    let best = 0
    let bestGap = Infinity
    plotted.forEach((p, i) => {
      const gap = Math.abs(x(p.ms) - px)
      if (gap < bestGap) {
        bestGap = gap
        best = i
      }
    })
    return best
  }

  const hovered = cursor === null ? null : plotted[Math.min(cursor, plotted.length - 1)]
  const hoveredTrades = hovered ? trades.filter((t) => t.date === hovered.date) : []

  return (
    <div>
      <div
        className={`flex flex-wrap justify-between ${compact ? 'items-center gap-x-3 gap-y-1' : 'items-end gap-3'}`}
      >
        <div>
          <div className={`tabular font-semibold text-ink ${compact ? 'text-base' : 'text-xl'}`}>
            {money(last.value, currency, { whole: wholeUnits })}
            {valueSuffix && (
              <span className={`font-normal text-ink-muted ${compact ? 'text-xs' : 'text-sm'}`}> {valueSuffix}</span>
            )}
          </div>
          <div className={`tabular ${compact ? 'text-xs' : 'text-sm'} ${deltaClass(change)}`}>
            {signedMoney(change, currency, { whole: wholeUnits })}
            {changePct !== null && <> · {signedPct(changePct)}</>}
            <span className="text-ink-muted"> over {rangeLabel}</span>
          </div>
        </div>

        {/* Range presets sit above the plot they scope, in one row. */}
        <div className={`flex flex-wrap ${compact ? 'gap-0.5' : 'gap-1'}`} role="group" aria-label="Chart range">
          {RANGES.map((r) => {
            const unavailable = !covered(r)
            return (
              <button
                key={r.key}
                type="button"
                disabled={unavailable}
                onClick={() => {
                  setRangeKey(r.key)
                  setCursor(null)
                }}
                aria-pressed={r.key === active.key}
                className={`rounded font-medium transition disabled:cursor-not-allowed disabled:opacity-35 ${
                  compact ? 'px-1 py-px text-[10px]' : 'px-2 py-0.5 text-xs'
                } ${
                  r.key === active.key ? 'bg-series-1 text-white' : 'text-ink-muted hover:bg-ink/5 hover:text-ink'
                }`}
              >
                {r.label}
              </button>
            )
          })}
        </div>
      </div>

      <div ref={ref} className="relative mt-2">
        <span className="sr-only" id={titleId}>
          {title}
        </span>

        {plotted.length < 2 ? (
          <p className="py-6 text-sm text-ink-muted">
            Only one stored price for this holding, so there is nothing to plot yet. Turn on
            "Backfill price history" in Settings and refresh — that fetches the full daily series.
          </p>
        ) : (
          <>
            <svg
              width="100%"
              height={chartHeight}
              role="img"
              aria-labelledby={titleId}
              tabIndex={0}
              className="touch-none select-none"
              onPointerMove={(e) => setCursor(nearestIndex(e.clientX, e.currentTarget.getBoundingClientRect()))}
              onPointerLeave={() => setCursor(null)}
              onFocus={() => setCursor((c) => c ?? plotted.length - 1)}
              onBlur={() => setCursor(null)}
              onKeyDown={(e) => {
                if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
                e.preventDefault()
                setCursor((c) => {
                  const at = c ?? plotted.length - 1
                  return Math.max(0, Math.min(plotted.length - 1, at + (e.key === 'ArrowRight' ? 1 : -1)))
                })
              }}
            >
              {/* Recessive chrome: hairline, solid, one step off the surface. */}
              {yTicks.map((v, i) => (
                <g key={`grid-${i}`}>
                  <line
                    x1={PAD.left}
                    x2={PAD.left + plotWidth}
                    y1={y(v)}
                    y2={y(v)}
                    stroke="var(--gridline)"
                    strokeWidth={1}
                  />
                  <text
                    x={PAD.left + plotWidth + 6}
                    y={y(v) + 3.5}
                    className="tabular fill-[color:var(--text-muted)] text-[10px]"
                  >
                    {axisMoney(v, currency, wholeUnits)}
                  </text>
                </g>
              ))}

              {xTicks.map((ms, i) => (
                <text
                  key={`tick-${i}`}
                  x={x(ms)}
                  y={chartHeight - 6}
                  textAnchor={i === 0 ? 'start' : i === xTicks.length - 1 ? 'end' : 'middle'}
                  className="fill-[color:var(--text-muted)] text-[10px]"
                >
                  {axisDate(ms, spanDays)}
                </text>
              ))}

              <path d={areaPath} fill={stroke} fillOpacity={0.1} />
              <path
                d={linePath}
                fill="none"
                stroke={stroke}
                strokeWidth={2}
                strokeLinejoin="round"
                strokeLinecap="round"
              />

              {/* Trades are dots at the price they were done at - the
                  statement's NAV, not the line's. Yellow is a purchase, red
                  a redemption; the tooltip names the kind too, so hue is
                  never the only cue. */}
              {marks.map((t, i) => {
                const ms = parseDay(t.date)
                const ty = y(markValue(t, ms))
                const tx = x(ms)
                const sell = /SELL|TRANSFER_OUT/i.test(t.kind)
                return (
                  <circle
                    key={`${t.date}-${i}`}
                    cx={tx}
                    cy={ty}
                    r={markRadius}
                    fill={sell ? SELL_MARK : BUY_MARK}
                    stroke="var(--surface-1)"
                    strokeWidth={1.5}
                  >
                    <title>{`${markText(t, currency)} on ${date(t.date)}`}</title>
                  </circle>
                )
              })}

              {/* The endpoint is the one point worth labelling directly. */}
              <circle
                cx={x(last.ms)}
                cy={y(last.value)}
                r={4}
                fill={stroke}
                stroke="var(--surface-1)"
                strokeWidth={2}
              />

              {hovered && (
                <g>
                  <line
                    x1={x(hovered.ms)}
                    x2={x(hovered.ms)}
                    y1={PAD.top}
                    y2={baseline}
                    stroke="var(--baseline)"
                    strokeWidth={1}
                  />
                  <circle
                    cx={x(hovered.ms)}
                    cy={y(hovered.value)}
                    r={4}
                    fill={stroke}
                    stroke="var(--surface-1)"
                    strokeWidth={2}
                  />
                </g>
              )}
            </svg>

            {hovered && (
              <div
                role="tooltip"
                className="pointer-events-none absolute top-0 z-20 -translate-x-1/2 whitespace-nowrap rounded-md border border-[color:var(--hairline)] bg-surface px-2.5 py-1.5 text-xs shadow-lg"
                style={{
                  left: Math.min(Math.max(x(hovered.ms), 60), Math.max(width - 60, 60)),
                }}
              >
                {/* Value leads, label follows - the reader already knows the series. */}
                <div className="tabular font-semibold text-ink">
                  {money(hovered.value, currency, { whole: wholeUnits })}
                </div>
                <div className="text-ink-muted">{date(hovered.date)}</div>
                {hoveredTrades.map((t, i) => (
                  <div key={i} className="tabular mt-0.5 text-ink-secondary">
                    <span
                      aria-hidden
                      style={{ color: /SELL|TRANSFER_OUT/i.test(t.kind) ? SELL_MARK : BUY_MARK }}
                    >
                      ●
                    </span>{' '}
                    {markText(t, currency)}
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </div>

      <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
        <p className="text-[11px] text-ink-muted">
          {marks.length > 0 ? (
            <>
              <span aria-hidden style={{ color: BUY_MARK }}>
                ●
              </span>{' '}
              bought · <span aria-hidden style={{ color: SELL_MARK }}>●</span> sold, each at the
              price on your statement — hover the line for the feed price on any day
            </>
          ) : inRange.length > 0 ? (
            `${inRange.length} trades in this window — markers hidden to keep the line readable`
          ) : (
            ''
          )}
        </p>
        <TableToggle open={showTable} onToggle={() => setShowTable((v) => !v)} />
      </div>

      {showTable && (
        <div className="mt-2 max-h-56 overflow-y-auto">
          <table className="w-full text-xs">
            <thead className="sticky top-0 bg-surface">
              <tr className="border-b border-[color:var(--hairline)] text-left text-ink-muted">
                <th className="py-1.5 font-medium">Date</th>
                <th className="py-1.5 text-right font-medium">Price</th>
              </tr>
            </thead>
            <tbody>
              {[...data].reverse().map((p) => (
                <tr key={p.date} className="border-b border-[color:var(--hairline)] last:border-0">
                  <td className="py-1 text-ink-secondary">{date(p.date)}</td>
                  <td className="tabular py-1 text-right text-ink">
                    {money(p.value, currency, { whole: wholeUnits })}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}

/** The short windows a mini chart offers - the recent move, not the story. */
const MINI_RANGES = RANGES.filter((r) => ['1d', '5d', '15d', '1m', '3m'].includes(r.key))

const MINI_HEIGHT = 44

/**
 * A sparkline over a short window, for a summary tile.
 *
 * No axes: the change over the window is printed beside the range buttons,
 * signed, so the line's colour is never the only cue. Hover reads a day's
 * value off the line.
 */
export function MiniHistoryChart({
  points,
  currency,
  title,
  defaultRange = '1d',
}: {
  points: SeriesPoint[]
  currency: string
  title: string
  /** One of `1d`, `5d`, `15d`, `1m`, `3m`. */
  defaultRange?: string
}) {
  const [rangeKey, setRangeKey] = useState(defaultRange)
  const [cursor, setCursor] = useState<number | null>(null)
  const { ref, width } = useWidth<HTMLDivElement>()

  const series = useMemo(() => timed(points), [points])
  const active = MINI_RANGES.find((r) => r.key === rangeKey) ?? MINI_RANGES[0]
  const data = useMemo(() => lastDays(series, active.days), [series, active])

  if (data.length < 2) {
    return (
      <div ref={ref} className="text-[11px] text-ink-muted">
        Not enough history to plot yet.
      </div>
    )
  }

  const first = data[0]
  const last = data[data.length - 1]
  const change = last.value - first.value
  const changePct = first.value !== 0 ? (change / Math.abs(first.value)) * 100 : null
  const stroke = change >= 0 ? 'var(--delta-up)' : 'var(--status-critical)'

  const w = Math.max(width, 120)
  const lo = Math.min(...data.map((p) => p.value))
  const hi = Math.max(...data.map((p) => p.value))
  const pad = 3
  const x = (ms: number) => pad + ((ms - first.ms) / (last.ms - first.ms || 1)) * (w - 2 * pad)
  const y = (v: number) =>
    hi === lo ? MINI_HEIGHT / 2 : pad + (1 - (v - lo) / (hi - lo)) * (MINI_HEIGHT - 2 * pad)
  const line = data
    .map((p, i) => `${i === 0 ? 'M' : 'L'}${x(p.ms).toFixed(1)},${y(p.value).toFixed(1)}`)
    .join(' ')
  const area = `${line} L${x(last.ms).toFixed(1)},${MINI_HEIGHT} L${x(first.ms).toFixed(1)},${MINI_HEIGHT} Z`
  const hovered = cursor === null ? null : data[Math.min(cursor, data.length - 1)]

  const nearest = (clientX: number, rect: DOMRect) => {
    const px = clientX - rect.left
    let best = 0
    data.forEach((p, i) => {
      if (Math.abs(x(p.ms) - px) < Math.abs(x(data[best].ms) - px)) best = i
    })
    return best
  }

  return (
    <div>
      <div className="flex flex-wrap items-center justify-between gap-x-2 gap-y-1">
        <div className={`tabular text-xs ${deltaClass(change)}`}>
          {hovered ? (
            <span className="text-ink-secondary">
              {money(hovered.value, currency, { whole: true })} · {date(hovered.date)}
            </span>
          ) : (
            <>
              {signedMoney(change, currency, { whole: true })}
              {changePct !== null && <> · {signedPct(changePct)}</>}
            </>
          )}
        </div>
        <div className="flex gap-0.5" role="group" aria-label="Mini chart range">
          {MINI_RANGES.map((r) => (
            <button
              key={r.key}
              type="button"
              onClick={(e) => {
                // The tile around the chart is itself a link.
                e.stopPropagation()
                setRangeKey(r.key)
              }}
              aria-pressed={r.key === active.key}
              className={`rounded px-1 py-px text-[10px] font-medium transition ${
                r.key === active.key ? 'bg-series-1 text-white' : 'text-ink-muted hover:bg-ink/5 hover:text-ink'
              }`}
            >
              {r.label}
            </button>
          ))}
        </div>
      </div>
      <div ref={ref} className="mt-1">
        <svg
          width="100%"
          height={MINI_HEIGHT}
          role="img"
          aria-label={`${title}: ${signedMoney(change, currency, { whole: true })} over ${active.label}`}
          className="touch-none select-none"
          onPointerMove={(e) => setCursor(nearest(e.clientX, e.currentTarget.getBoundingClientRect()))}
          onPointerLeave={() => setCursor(null)}
        >
          <path d={area} fill={stroke} fillOpacity={0.1} />
          <path d={line} fill="none" stroke={stroke} strokeWidth={1.5} strokeLinejoin="round" />
          {hovered ? (
            <>
              <line
                x1={x(hovered.ms)}
                x2={x(hovered.ms)}
                y1={0}
                y2={MINI_HEIGHT}
                stroke="var(--baseline)"
                strokeWidth={1}
              />
              <circle cx={x(hovered.ms)} cy={y(hovered.value)} r={3} fill={stroke} />
            </>
          ) : (
            <circle cx={x(last.ms)} cy={y(last.value)} r={2.5} fill={stroke} />
          )}
        </svg>
      </div>
    </div>
  )
}

/* ------------------------------------------------------------------ *
 * Stacked bars
 * ------------------------------------------------------------------ */

export interface StackSegment {
  key: string
  label: string
  value: number
  /** The folded tail of the stack. Takes the neutral rather than a step on
   *  the rank ramp, so the reader sees where the named pieces stop. */
  tail?: boolean
}

export interface StackRow {
  key: string
  label: string
  segments: StackSegment[]
}

/** Segments narrower than this cannot hold a label without clipping it, so
 *  theirs moves to the tooltip and the table. */
const LABEL_MIN_PCT = 11

/**
 * Part-to-whole within each of several groups: one horizontal bar per group,
 * split into the pieces that make it up.
 *
 * Horizontal because the group names are long and the pieces inside them
 * longer still. Bar length is the group's total, so the groups compare
 * against each other as well as within themselves.
 *
 * Colour encodes rank within the stack, and every bar is coloured the same
 * way: the largest piece takes the strongest step of the single-hue ramp,
 * the next the step below it, and so on. That makes the bars comparable to
 * each other - the darkest block is the sector's biggest holding wherever
 * you look - which ten categorical hues per bar could not do, being five
 * times the palette and indistinguishable under colour-blindness besides.
 *
 * Rank is an ordered dimension, so a ramp reads it correctly; the pieces
 * are nominal, and they are named in the label, the tooltip and the table
 * rather than by hue. Pieces are separated by 2px surface gaps.
 */
export function StackedBars({
  rows,
  currency,
  title,
  emptyMessage = 'Nothing to show yet.',
}: {
  rows: StackRow[]
  currency: string
  title: string
  emptyMessage?: string
}) {
  const [hovered, setHovered] = useState<string | null>(null)
  const [showTable, setShowTable] = useState(false)
  const id = useId()

  const data = rows.filter((r) => r.segments.some((s) => s.value > 0))
  if (data.length === 0) {
    return <p className="py-6 text-sm text-ink-muted">{emptyMessage}</p>
  }

  // Every bar runs the full width, so the question each answers is "what is
  // this group made of". Sizing bars against the largest instead would
  // squeeze a small group's ten pieces into a tenth of the row, where the
  // slimmest are a few pixels wide - too small to read, label or hover. The
  // group's own size is printed beside its name instead.
  const totals = data.map((r) => r.segments.reduce((sum, s) => sum + s.value, 0))
  const grand = totals.reduce((sum, t) => sum + t, 0)

  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <span className="sr-only" id={id}>
          {title}
        </span>
        <TableToggle open={showTable} onToggle={() => setShowTable((v) => !v)} />
      </div>

      <ul className="space-y-3" aria-labelledby={id}>
        {data.map((row, rowIndex) => {
          const total = totals[rowIndex]
          return (
            <li key={row.key}>
              <div className="mb-1 flex items-baseline justify-between gap-3 text-xs">
                <span className="truncate font-medium text-ink">{row.label}</span>
                <span className="tabular shrink-0 font-semibold text-ink">
                  {money(total, currency, { compact: true })} · {pct((total / grand) * 100, 1)} of
                  the stocks
                </span>
              </div>
              {/* 2px surface gaps separate the pieces - a border round each
                  would read as chrome rather than as a spacer. */}
              <div
                className="flex h-8 w-full gap-[2px]"
                role="img"
                aria-label={`${row.label}: ${row.segments
                  .map((s) => `${s.label} ${money(s.value, currency)}`)
                  .join(', ')}`}
              >
                {row.segments
                  .filter((s) => s.value > 0)
                  .map((s, i) => {
                    const share = (s.value / total) * 100
                    const key = `${row.key}-${s.key}`
                    const active = hovered === key
                    return (
                      <div
                        key={key}
                        className={`relative first:rounded-l-[4px] last:rounded-r-[4px] ${
                          s.tail ? RANK_OTHER : rankClass(i)
                        }`}
                        style={{
                          width: `${share}%`,
                          opacity: hovered && !active ? 0.4 : 1,
                        }}
                        onMouseEnter={() => setHovered(key)}
                        onMouseLeave={() => setHovered(null)}
                        onFocus={() => setHovered(key)}
                        onBlur={() => setHovered(null)}
                        tabIndex={0}
                        aria-label={`${s.label}: ${money(s.value, currency)}, ${pct(share)} of ${row.label}`}
                      >
                        {/* Only a segment wide enough to hold its name gets
                            one; the rest are read from the tooltip or the
                            table, never clipped mid-word. */}
                        {share >= LABEL_MIN_PCT && (
                          <span className="pointer-events-none absolute inset-0 flex items-center overflow-hidden px-1.5 text-[10px] font-medium">
                            <span className="truncate">{s.label}</span>
                          </span>
                        )}
                        {active && (
                          <Tooltip>
                            <div className="font-medium text-ink">{s.label}</div>
                            <div className="tabular text-ink-secondary">
                              {money(s.value, currency)} · {pct(share, 1)} of {row.label}
                            </div>
                          </Tooltip>
                        )}
                      </div>
                    )
                  })}
              </div>
            </li>
          )
        })}
      </ul>

      {showTable && (
        <table className="mt-4 w-full text-sm">
          <thead>
            <tr className="border-b border-[color:var(--hairline)] text-left text-ink-muted">
              <th className="py-1.5 font-medium">Group</th>
              <th className="py-1.5 font-medium">Holding</th>
              <th className="py-1.5 text-right font-medium">Value</th>
              <th className="py-1.5 text-right font-medium">Share</th>
            </tr>
          </thead>
          <tbody>
            {data.flatMap((row, rowIndex) =>
              row.segments.map((s, i) => (
                <tr
                  key={`${row.key}-${s.key}`}
                  className="border-b border-[color:var(--hairline)] last:border-0"
                >
                  <td className="py-1.5 text-ink-secondary">{i === 0 ? row.label : ''}</td>
                  <td className="py-1.5 text-ink-secondary">{s.label}</td>
                  <td className="tabular py-1.5 text-right text-ink">
                    {money(s.value, currency)}
                  </td>
                  <td className="tabular py-1.5 text-right text-ink-secondary">
                    {pct((s.value / totals[rowIndex]) * 100, 1)}
                  </td>
                </tr>
              )),
            )}
          </tbody>
        </table>
      )}
    </div>
  )
}

/* ------------------------------------------------------------------ *
 * Grouped donut
 * ------------------------------------------------------------------ */

/** A thin band just inside the ring's edge carries the groups; the parts
 *  keep the thick ring, where there is room for small slices. The hole is
 *  the plain ring's, so the centre text sits the same. */
const BAND_OUTER_RATIO = 125 / 252
const BAND_INNER_RATIO = 117 / 252
const PARTS_OUTER_RATIO = 114 / 252

export interface GroupedSlice extends Slice {
  color: string
  /** Lines under the amount in the tooltip - what the slice is made of. */
  details?: string[]
}

export interface RingGroup {
  key: string
  label: string
  color: string
  slices: GroupedSlice[]
}

/**
 * Parts of a whole, grouped: a thin outer band for the groups - equity and
 * debt, say - over a thick ring of the parts, each part directly under its
 * group. Groups and parts keep the order given rather than sorting by
 * size, so a part never drifts away from its group; the colours come from
 * the caller, one hue family per group.
 */
export function GroupedDonutChart({
  groups,
  currency,
  title,
  centreLines = [],
  emptyMessage = 'Nothing to show yet.',
  size = RING_SIZE,
  whole = false,
}: {
  groups: RingGroup[]
  currency: string
  title: string
  /** Lines under the total - the total in other currencies, say. Hovering
   *  swaps them for the share of whatever is hovered. */
  centreLines?: string[]
  emptyMessage?: string
  size?: number
  /** Amounts in whole units, for portfolio-scale figures. */
  whole?: boolean
}) {
  const [hovered, setHovered] = useState<string | null>(null)
  const id = useId()

  const live = groups
    .map((grp) => ({ ...grp, slices: grp.slices.filter((s) => s.value > 0) }))
    .filter((grp) => grp.slices.length > 0)
  const groupValue = (grp: RingGroup) => grp.slices.reduce((sum, s) => sum + s.value, 0)
  const total = live.reduce((sum, grp) => sum + groupValue(grp), 0)

  if (total <= 0) {
    return <p className="py-6 text-sm text-ink-muted">{emptyMessage}</p>
  }

  const round = (v: number) => Math.round(v * 100) / 100
  const band: RingGeometry = {
    size,
    outer: round(size * BAND_OUTER_RATIO),
    inner: round(size * BAND_INNER_RATIO),
  }
  const parts: RingGeometry = {
    size,
    outer: round(size * PARTS_OUTER_RATIO),
    inner: round(size * DONUT_INNER_RATIO),
  }
  // A lone arc of a full turn has no length to draw; stop just short.
  const FULL = Math.PI * 2 - 1e-4

  let angle = 0
  const arcs = live.map((grp) => {
    const a0 = angle
    const slices = grp.slices.map((s) => {
      const sweep = Math.min((s.value / total) * Math.PI * 2, FULL)
      const arc = { slice: s, a0: angle, a1: angle + sweep }
      angle += sweep
      return arc
    })
    return { group: grp, a0, a1: Math.min(angle, a0 + FULL), slices }
  })

  // A hovered group lights its own parts; a hovered part lights its group.
  const hoveredGroup = live.find((grp) => grp.key === hovered)
  const hoveredSlice = live.flatMap((grp) => grp.slices).find((s) => s.key === hovered)
  const ownerOf = (key: string) => live.find((grp) => grp.slices.some((s) => s.key === key))
  const lit = (groupKey: string, sliceKey?: string) => {
    if (!hovered) return true
    if (hoveredGroup) return groupKey === hoveredGroup.key
    return sliceKey === undefined ? ownerOf(hovered)?.key === groupKey : sliceKey === hovered
  }
  const active = hoveredGroup
    ? { label: hoveredGroup.label, value: groupValue(hoveredGroup), details: [] as string[] }
    : hoveredSlice
      ? {
          label: hoveredSlice.label,
          value: hoveredSlice.value,
          details: hoveredSlice.details ?? [],
        }
      : undefined

  const lines = active ? [pct((active.value / total) * 100, 1)] : centreLines
  const figure = (v: number) => money(v, currency, { compact: true })
  const figureEms = Math.max(
    textEms(figure(total)),
    ...live.map((grp) => textEms(figure(groupValue(grp)))),
    ...live.flatMap((grp) => grp.slices.map((s) => textEms(figure(s.value)))),
  )
  const centre = centreText(
    parts,
    size / DONUT_SIZE >= 1.4,
    lines.length,
    centreLines.length > 1,
    figureEms,
  )

  const hover = (key: string) => ({
    onMouseEnter: () => setHovered(key),
    onMouseLeave: () => setHovered(null),
    onFocus: () => setHovered(key),
    onBlur: () => setHovered(null),
    tabIndex: 0,
  })

  return (
    <div>
      <span className="sr-only" id={id}>
        {title}
      </span>

      <div className="flex flex-col items-center gap-4">
        <div className="relative shrink-0">
          <svg
            width={size}
            height={size}
            viewBox={`0 0 ${size} ${size}`}
            role="img"
            aria-labelledby={id}
          >
            {arcs.map(({ group: grp, a0, a1, slices }) => (
              <g key={grp.key}>
                <path
                  d={ringSegment(band, a0, a1)}
                  fill={grp.color}
                  stroke="var(--surface-1)"
                  strokeWidth={2}
                  strokeLinejoin="round"
                  className="transition-opacity"
                  style={{ opacity: lit(grp.key) ? 1 : 0.45 }}
                  aria-label={`${grp.label}: ${money(groupValue(grp), currency, { whole })}, ${pct((groupValue(grp) / total) * 100)}`}
                  {...hover(grp.key)}
                />
                {slices.map(({ slice: s, a0: s0, a1: s1 }) => (
                  <path
                    key={s.key}
                    d={ringSegment(parts, s0, s1)}
                    fill={s.color}
                    stroke="var(--surface-1)"
                    strokeWidth={2}
                    strokeLinejoin="round"
                    className="transition-opacity"
                    style={{ opacity: lit(grp.key, s.key) ? 1 : 0.45 }}
                    aria-label={`${grp.label}, ${s.label}: ${money(s.value, currency, { whole })}, ${pct((s.value / total) * 100)}`}
                    {...hover(s.key)}
                  />
                ))}
              </g>
            ))}
            <text
              x={size / 2}
              y={centre.valueY}
              fontSize={centre.valueSize}
              textAnchor="middle"
              className="tabular fill-[color:var(--text-primary)] font-semibold"
            >
              {figure(active ? active.value : total)}
            </text>
            {lines.map((line, i) => (
              <text
                key={i}
                x={size / 2}
                y={centre.lineY(i)}
                fontSize={centre.labelSize}
                textAnchor="middle"
                className="fill-[color:var(--text-muted)]"
              >
                {line}
              </text>
            ))}
          </svg>
          {active && (
            <Tooltip>
              <div className="font-medium text-ink">{active.label}</div>
              <div className="tabular text-ink-secondary">
                {money(active.value, currency, { whole })} · {pct((active.value / total) * 100)}
              </div>
              {active.details.map((d) => (
                <div key={d} className="tabular text-ink-muted">
                  {d}
                </div>
              ))}
            </Tooltip>
          )}
        </div>

        {/* The legend is the direct labels: each group with its share, its
            parts indented under it, so identity is never colour alone. */}
        <ul className="w-full space-y-2">
          {live.map((grp) => (
            <li key={grp.key}>
              <div
                className="flex items-baseline gap-2 text-[13px] leading-5"
                onMouseEnter={() => setHovered(grp.key)}
                onMouseLeave={() => setHovered(null)}
              >
                <span
                  aria-hidden
                  className="mt-0.5 inline-block h-1.5 w-4 shrink-0 self-center rounded-sm"
                  style={{ background: grp.color }}
                />
                <span className="min-w-0 flex-1 font-medium text-ink">{grp.label}</span>
                <span className="tabular shrink-0 font-semibold text-ink">
                  {pct((groupValue(grp) / total) * 100, 1)}
                </span>
              </div>
              {grp.slices.length > 1 && (
                <ul className="mt-1 space-y-1 pl-6">
                  {grp.slices.map((s) => (
                    <li
                      key={s.key}
                      className="flex items-baseline gap-2 text-[13px] leading-5"
                      onMouseEnter={() => setHovered(s.key)}
                      onMouseLeave={() => setHovered(null)}
                    >
                      <span
                        aria-hidden
                        className="mt-0.5 inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
                        style={{ background: s.color }}
                      />
                      <span className="min-w-0 flex-1 text-ink-secondary">{s.label}</span>
                      <span className="tabular shrink-0 text-ink">
                        {pct((s.value / total) * 100, 1)}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </li>
          ))}
        </ul>
      </div>
    </div>
  )
}
