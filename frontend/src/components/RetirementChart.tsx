import { useId, useState } from 'react'

import { money } from '../lib/format'
import { useWidth } from './charts'

export interface DrawdownSeries {
  key: string
  label: string
  /** Index 0 is today; then one closing balance per year. */
  values: number[]
  /** Calendar year it first goes below zero, if it does. */
  runsOutIn: number | null
  /** Shown beside the label in the legend; by default, how long it lasts. */
  note?: string
}

const CHART_HEIGHT = 300
const PAD = { top: 14, right: 64, bottom: 24, left: 8 }
/** Space between the cursor line and the tooltip beside it. */
const TOOLTIP_GAP = 12

/** Categorical slots in fixed order: a scenario keeps its colour by position. */
export const SCENARIO_COLOURS = ['var(--series-1)', 'var(--series-2)', 'var(--series-3)', 'var(--series-7)']

/** Round steps for the value axis, zero always among them. */
function ticks(lo: number, hi: number, count = 4): number[] {
  const span = hi - lo || Math.abs(hi) || 1
  const raw = span / count
  const mag = Math.pow(10, Math.floor(Math.log10(raw)))
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw
  const out: number[] = []
  for (let v = Math.ceil(lo / step) * step; v <= hi + step * 1e-9; v += step) out.push(Math.abs(v) < step * 1e-9 ? 0 : v)
  return out
}

/**
 * The corpus over the plan, one line per withdrawal. Below zero is shaded:
 * that is the money the plan does not have. The selected scenario carries
 * its currency-risk band (10th to 90th percentile).
 */
export function DrawdownChart({
  series,
  selected,
  onSelect,
  band,
  currency,
  startYear,
  age,
  title,
  marker,
}: {
  series: DrawdownSeries[]
  selected: number
  onSelect: (index: number) => void
  band?: { low: number[]; high: number[] }
  currency: string
  startYear: number
  /** Age today, to label the years with. */
  age: number | null
  title: string
  /** A moment to mark with a dashed line: `at` is a point index (0 is today). */
  marker?: { at: number; label: string }
}) {
  const { ref, width } = useWidth<HTMLDivElement>()
  const [cursor, setCursor] = useState<number | null>(null)
  const titleId = useId()

  const points = series[0]?.values.length ?? 0
  if (points < 2) return null

  const plotWidth = Math.max(width, 280) - PAD.left - PAD.right
  const plotHeight = CHART_HEIGHT - PAD.top - PAD.bottom
  const bottom = PAD.top + plotHeight

  const all = [...series.flatMap((s) => s.values), ...(band ? [...band.low, ...band.high] : []), 0]
  const lo = Math.min(...all)
  const hi = Math.max(...all)
  const headroom = (hi - lo || 1) * 0.06
  const yTicks = ticks(lo, hi)
  const yMin = Math.min(lo - (lo < 0 ? headroom : 0), yTicks[0])
  const yMax = Math.max(hi + headroom, yTicks[yTicks.length - 1])

  const x = (i: number) => PAD.left + (i / (points - 1)) * plotWidth
  const y = (v: number) => PAD.top + plotHeight - ((v - yMin) / (yMax - yMin)) * plotHeight
  const path = (values: number[]) =>
    values.map((v, i) => `${i === 0 ? 'M' : 'L'}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(' ')

  const bandPath = band
    ? `${path(band.high)} ${band.low
        .map((v, i) => [i, v] as const)
        .reverse()
        .map(([i, v]) => `L${x(i).toFixed(1)},${y(v).toFixed(1)}`)
        .join(' ')} Z`
    : null

  const yearLabel = (i: number) => `${startYear + i}${age !== null ? ` · age ${age + i}` : ''}`
  const xTicks = Array.from({ length: Math.floor((points - 1) / 10) + 1 }, (_, k) => k * 10)
  const zeroY = y(0)
  // Draw the selected scenario last, so it sits on top.
  const order = series.map((_, i) => i).sort((a, b) => (a === selected ? 1 : b === selected ? -1 : a - b))

  const indexAt = (clientX: number, rect: DOMRect) =>
    Math.max(0, Math.min(points - 1, Math.round(((clientX - rect.left - PAD.left) / plotWidth) * (points - 1))))

  return (
    <div>
      {/* Legend, and the scenario picker: the table and band follow it. */}
      <div className="flex flex-wrap gap-1.5" role="group" aria-label="Withdrawal scenario">
        {series.map((s, i) => (
          <button
            key={s.key}
            type="button"
            aria-pressed={i === selected}
            onClick={() => onSelect(i)}
            className={`flex items-center gap-2 rounded-md border px-2.5 py-1 text-xs transition ${
              i === selected
                ? 'border-[color:var(--edge)] bg-ink/5 text-ink'
                : 'border-transparent text-ink-secondary hover:bg-ink/5 hover:text-ink'
            }`}
          >
            <span aria-hidden className="h-0.5 w-4 rounded" style={{ background: SCENARIO_COLOURS[i] }} />
            <span className="font-medium">{s.label}</span>
            <span className="text-ink-muted">
              {s.note ?? (s.runsOutIn === null ? `lasts ${points - 1}+ yrs` : `runs out ${s.runsOutIn}`)}
            </span>
          </button>
        ))}
      </div>

      <div ref={ref} className="relative mt-3">
        <span className="sr-only" id={titleId}>
          {title}
        </span>
        <svg
          width="100%"
          height={CHART_HEIGHT}
          role="img"
          aria-labelledby={titleId}
          tabIndex={0}
          className="touch-none select-none"
          onPointerMove={(e) => setCursor(indexAt(e.clientX, e.currentTarget.getBoundingClientRect()))}
          onPointerLeave={() => setCursor(null)}
          onFocus={() => setCursor((c) => c ?? 0)}
          onBlur={() => setCursor(null)}
          onKeyDown={(e) => {
            if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
            e.preventDefault()
            setCursor((c) => Math.max(0, Math.min(points - 1, (c ?? 0) + (e.key === 'ArrowRight' ? 1 : -1))))
          }}
        >
          {/* The shortfall: everything under zero. */}
          {zeroY < bottom && (
            <rect
              x={PAD.left}
              y={zeroY}
              width={plotWidth}
              height={bottom - zeroY}
              fill="var(--status-critical)"
              fillOpacity={0.07}
            />
          )}

          {yTicks.map((v) => (
            <g key={`grid-${v}`}>
              <line
                x1={PAD.left}
                x2={PAD.left + plotWidth}
                y1={y(v)}
                y2={y(v)}
                stroke={v === 0 ? 'var(--baseline)' : 'var(--gridline)'}
                strokeWidth={1}
              />
              <text x={PAD.left + plotWidth + 6} y={y(v) + 3.5} className="tabular fill-[color:var(--text-muted)] text-[10px]">
                {money(v, currency, { compact: true })}
              </text>
            </g>
          ))}

          {xTicks.map((i, k) => (
            <text
              key={`tick-${i}`}
              x={x(i)}
              y={CHART_HEIGHT - 6}
              textAnchor={k === 0 ? 'start' : i === points - 1 ? 'end' : 'middle'}
              className="fill-[color:var(--text-muted)] text-[10px]"
            >
              {startYear + i}
            </text>
          ))}

          {marker && marker.at > 0 && marker.at < points && (
            <g>
              <line
                x1={x(marker.at)}
                x2={x(marker.at)}
                y1={PAD.top}
                y2={bottom}
                stroke="var(--text-muted)"
                strokeWidth={1}
                strokeDasharray="4 3"
              />
              <text
                x={x(marker.at) + (x(marker.at) > PAD.left + plotWidth * 0.8 ? -4 : 4)}
                y={PAD.top + 8}
                textAnchor={x(marker.at) > PAD.left + plotWidth * 0.8 ? 'end' : 'start'}
                className="fill-[color:var(--text-muted)] text-[10px] font-medium"
              >
                {marker.label}
              </text>
            </g>
          )}

          {bandPath && <path d={bandPath} fill={SCENARIO_COLOURS[selected]} fillOpacity={0.14} />}

          {order.map((i) => (
            <path
              key={series[i].key}
              d={path(series[i].values)}
              fill="none"
              stroke={SCENARIO_COLOURS[i]}
              strokeWidth={2}
              strokeOpacity={i === selected ? 1 : 0.55}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
          ))}

          {/* Where each plan runs dry: the point worth labelling directly. */}
          {series.map((s, i) => {
            if (s.runsOutIn === null) return null
            const at = s.runsOutIn - startYear + 1
            return (
              <circle
                key={`out-${s.key}`}
                cx={x(at)}
                cy={y(s.values[at])}
                r={4}
                fill={SCENARIO_COLOURS[i]}
                stroke="var(--surface-1)"
                strokeWidth={2}
              >
                <title>{`${s.label}: runs out in ${s.runsOutIn}`}</title>
              </circle>
            )
          })}

          {cursor !== null && (
            <g>
              <line x1={x(cursor)} x2={x(cursor)} y1={PAD.top} y2={bottom} stroke="var(--baseline)" strokeWidth={1} />
              {series.map((s, i) => (
                <circle
                  key={`dot-${s.key}`}
                  cx={x(cursor)}
                  cy={y(s.values[cursor])}
                  r={4}
                  fill={SCENARIO_COLOURS[i]}
                  stroke="var(--surface-1)"
                  strokeWidth={2}
                />
              ))}
            </g>
          )}
        </svg>

        {cursor !== null && (
          <div
            role="tooltip"
            className="pointer-events-none absolute top-0 z-20 whitespace-nowrap rounded-md border border-[color:var(--hairline)] bg-surface px-2.5 py-1.5 text-xs shadow-lg"
            // Beside the cursor line, never over it: the dots sit on that line.
            style={
              x(cursor) < width / 2
                ? { left: x(cursor) + TOOLTIP_GAP }
                : { right: width - x(cursor) + TOOLTIP_GAP }
            }
          >
            <div className="mb-1 text-ink-muted">{cursor === 0 ? `Today · ${yearLabel(0)}` : `End of ${yearLabel(cursor - 1)}`}</div>
            {series.map((s, i) => (
              <div key={s.key} className="flex items-baseline justify-between gap-4">
                <span className="flex items-center gap-1.5 text-ink-secondary">
                  <span aria-hidden className="h-0.5 w-3 rounded" style={{ background: SCENARIO_COLOURS[i] }} />
                  {s.label}
                </span>
                <span className={`tabular font-semibold ${s.values[cursor] < 0 ? 'text-loss' : 'text-ink'}`}>
                  {money(s.values[cursor], currency, { whole: true })}
                </span>
              </div>
            ))}
            {band && cursor > 0 && (
              <div className="mt-1 border-t border-[color:var(--hairline)] pt-1 text-ink-muted">
                Currency risk, {series[selected].label}: {money(band.low[cursor], currency, { compact: true })} to{' '}
                {money(band.high[cursor], currency, { compact: true })}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
