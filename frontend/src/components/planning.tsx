/**
 * The pieces of CaiYatra's planner: number and amount fields, the extra
 * amounts and currency cards, and the year-by-year table with its rope.
 */
import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { ChevronDown, ChevronRight, ChevronsDownUp, ChevronsLeftRight, ChevronsRightLeft, ChevronsUpDown, Download, Plus, RotateCcw, Trash2 } from 'lucide-react'

import { TableToggle } from './charts'
import { Badge, Button, Card, Field, ToggleGroup, inputClass, useConfirm } from './ui'
import { bigMoney, currencyLocale, currencySymbol, money, pct, signedPct } from '../lib/format'
import {
  INR_PER_UNIT,
  REF_FIRST_YEAR,
  REF_LAST_YEAR,
  inrHistory,
  type CurrencyLeg,
  type ExtraAmount,
  type YearRow,
  type YearShifts,
} from '../lib/retirement'

/** Two significant figures: a default amount should look typed, not computed. */
export function roundish(v: number): number {
  if (v <= 0) return 0
  const mag = Math.pow(10, Math.floor(Math.log10(v)) - 1)
  return Math.round(v / mag) * mag
}

/**
 * A number field that lets the reader type freely and reports parsed values.
 * A value outside `min`-`max` is not reported: the field shows it as invalid
 * and goes back to the last good value when left.
 */
export function NumberInput({
  value,
  onChange,
  step,
  min,
  max,
  allowEmpty = false,
  ariaLabel,
}: {
  value: number | null
  onChange: (v: number | null) => void
  step?: number
  min?: number
  max?: number
  allowEmpty?: boolean
  ariaLabel?: string
}) {
  const [text, setText] = useState(value === null ? '' : String(value))
  useEffect(() => {
    const parsed = text.trim() === '' ? null : Number(text)
    if (parsed !== value) setText(value === null ? '' : String(value))
  }, [value])
  const inRange = (v: number) => (min === undefined || v >= min) && (max === undefined || v <= max)
  const typed = text.trim() === '' ? null : Number(text)
  const invalid = typed !== null && Number.isFinite(typed) && !inRange(typed)
  return (
    <input
      type="number"
      inputMode="decimal"
      step={step}
      min={min}
      max={max}
      aria-label={ariaLabel}
      aria-invalid={invalid || undefined}
      title={invalid ? `Between ${min ?? '…'} and ${max ?? '…'}` : undefined}
      className={`${inputClass} tabular ${invalid ? 'border-status-critical/50 text-status-critical' : ''}`}
      value={text}
      onChange={(e) => {
        setText(e.target.value)
        if (e.target.value.trim() === '') {
          if (allowEmpty) onChange(null)
          return
        }
        const v = Number(e.target.value)
        if (Number.isFinite(v) && inRange(v)) onChange(v)
      }}
      onBlur={() => {
        if (invalid) setText(value === null ? '' : String(value))
      }}
    />
  )
}

/** The digits, the decimal point and the sign of what was typed, as JS reads a number. */
function plainNumber(text: string, decimal: string): string {
  let out = ''
  for (const ch of text) {
    if (ch >= '0' && ch <= '9') out += ch
    else if (ch === decimal && !out.includes('.')) out += '.'
    else if (ch === '-' && out === '') out += '-'
  }
  return out
}

/**
 * An amount field grouped the way the currency reads (1,23,456 for INR) as
 * the reader types, keeping the caret beside the digit it was next to.
 */
export function MoneyInput({
  value,
  currency,
  onChange,
  ariaLabel,
  prefix,
}: {
  value: number
  currency: string
  onChange: (v: number) => void
  ariaLabel?: string
  /** Set inside the field, before the figure: the currency symbol. */
  prefix?: string
}) {
  const locale = currencyLocale(currency)
  const decimal = new Intl.NumberFormat(locale).formatToParts(1.5).find((p) => p.type === 'decimal')?.value ?? '.'
  /** Groups the whole part; a decimal part is kept as typed, trailing zeros and all. */
  const group = (plain: string) => {
    const [whole, frac] = plain.split('.')
    const digits = whole.replace('-', '')
    const grouped = digits === '' ? '' : new Intl.NumberFormat(locale).format(Number(digits))
    return `${whole.startsWith('-') ? '-' : ''}${grouped}${frac !== undefined ? decimal + frac : ''}`
  }
  const show = (v: number) => group(String(Math.round(v * 100) / 100))

  const ref = useRef<HTMLInputElement>(null)
  const caret = useRef<number | null>(null)
  const [text, setText] = useState(show(value))
  useEffect(() => {
    const plain = plainNumber(text, decimal)
    if (Number(plain) !== value || group(plain) !== text) setText(show(value))
  }, [value, locale])
  useLayoutEffect(() => {
    if (caret.current === null || !ref.current) return
    // Put the caret after as many digits as were before it.
    let seen = 0
    let at = 0
    while (at < text.length && seen < caret.current) {
      if ((text[at] >= '0' && text[at] <= '9') || text[at] === decimal || text[at] === '-') seen++
      at++
    }
    ref.current.setSelectionRange(at, at)
    caret.current = null
  }, [text])

  const input = (
    <input
      ref={ref}
      type="text"
      inputMode="decimal"
      aria-label={ariaLabel}
      className={`${inputClass} tabular ${prefix ? (prefix.length > 2 ? 'pl-10' : prefix.length > 1 ? 'pl-8' : 'pl-6') : ''}`}
      value={text}
      onChange={(e) => {
        const before = e.target.value.slice(0, e.target.selectionStart ?? e.target.value.length)
        caret.current = plainNumber(before, decimal).length
        const plain = plainNumber(e.target.value, decimal)
        setText(group(plain))
        // An empty field or a lone sign is mid-typing: leave the figure as it was.
        if (plain !== '' && plain !== '-' && Number.isFinite(Number(plain))) onChange(Number(plain))
      }}
    />
  )
  // Always the same wrapper, so the input is never remounted - and never loses focus - as the figure changes.
  return (
    <div className="relative min-w-0 flex-1">
      {prefix && (
        <span
          aria-hidden
          className="pointer-events-none absolute inset-y-0 left-2 flex items-center text-[11px] font-medium text-ink-muted"
        >
          {prefix}
        </span>
      )}
      {input}
    </div>
  )
}

/** A field's label with the amount in short units after it (1.00 Cr, 1.00M). Under a thousand that would only repeat the figure. */
export function shortLabel(label: string, value: number, currency: string): string {
  return Math.abs(value) >= 1000 ? `${label} · ${bigMoney(value, currency)}` : label
}

/** The selected scenario's outline: its own colour, a medium weight. */
export function selectedBorder(colour: string) {
  return { borderColor: colour, boxShadow: `0 0 0 2px ${colour}` }
}

/** Income and one-off spends on top of the withdrawal. */
export function ExtrasCard({
  extras,
  onChange,
  currency,
  startYear,
  title = 'Additional amounts',
  empty = "A pension, rent, a property sale coming in; a child's education or a car going out.",
  defaultUntil = startYear,
  maxHeight,
  expand,
}: {
  extras: ExtraAmount[]
  onChange: (extras: ExtraAmount[]) => void
  currency: string
  startYear: number
  title?: string
  /** Shown while there are none: what they might be. */
  empty?: string
  /** Last year of a newly added amount. */
  defaultUntil?: number
  /** The card's tallest, in pixels: past it, the amounts scroll. */
  maxHeight?: number
  /** A button to open the card down the page, and fold it back. */
  expand?: { open: boolean; onToggle: () => void }
}) {
  const update = (id: string, patch: Partial<ExtraAmount>) =>
    onChange(extras.map((e) => (e.id === id ? { ...e, ...patch } : e)))
  return (
    <Card
      title={title}
      subtitle={`In ${currency}, today's money: positive comes in, negative goes out`}
      className={maxHeight ? 'flex min-h-0 flex-col' : ''}
      style={maxHeight ? { maxHeight } : undefined}
      actions={
        expand && (
          <Button variant={expand.open ? 'primary' : 'secondary'} onClick={expand.onToggle}>
            <span
              className="flex items-center"
              title={expand.open ? 'Fold up, and widen the simulation' : 'Open down the page, beside the simulation'}
              aria-label={expand.open ? 'Fold the funds' : 'Open the funds'}
            >
              {expand.open ? <ChevronsDownUp size={14} aria-hidden /> : <ChevronsUpDown size={14} aria-hidden />}
            </span>
          </Button>
        )
      }
    >
      <div className={`space-y-3 ${maxHeight ? '-mr-2 min-h-0 flex-1 overflow-y-auto pr-2' : ''}`}>
        {extras.length === 0 && (
          <p className="text-xs text-ink-muted">{empty}</p>
        )}
        {extras.map((e) => (
          <div key={e.id} className="space-y-2 rounded-md border border-[color:var(--hairline)] p-2.5">
            <div className="flex items-center gap-2">
              <input
                className={inputClass}
                value={e.label}
                placeholder="What it is"
                aria-label="Description"
                onChange={(ev) => update(e.id, { label: ev.target.value })}
              />
              <button
                type="button"
                onClick={() => onChange(extras.filter((x) => x.id !== e.id))}
                aria-label={`Remove ${e.label || 'amount'}`}
                className="text-ink-muted hover:text-ink"
              >
                <Trash2 size={14} />
              </button>
            </div>
            <div className="grid grid-cols-3 gap-2">
              <Field label="Per year">
                <MoneyInput value={e.amount} currency={currency} onChange={(v) => update(e.id, { amount: v })} />
              </Field>
              <Field label="From">
                <NumberInput
                  value={e.fromYear}
                  onChange={(v) => update(e.id, { fromYear: v ?? startYear, toYear: Math.max(e.toYear, v ?? startYear) })}
                  step={1}
                />
              </Field>
              <Field label="Until">
                <NumberInput value={e.toYear} onChange={(v) => update(e.id, { toYear: v ?? e.fromYear })} step={1} />
              </Field>
            </div>
            <label className="flex items-center gap-2 text-xs text-ink-secondary">
              <input type="checkbox" checked={e.indexed} onChange={(ev) => update(e.id, { indexed: ev.target.checked })} />
              Rises with inflation
              <span className="ml-auto">
                <Badge tone={e.amount >= 0 ? 'good' : 'critical'}>{e.amount >= 0 ? 'In' : 'Out'}</Badge>
              </span>
            </label>
          </div>
        ))}
      </div>
      {/* Under the list, not in it: in view however far the amounts scroll. */}
      <div className={extras.length > 0 ? 'mt-3' : 'mt-2'}>
        <Button
          variant="ghost"
          onClick={() =>
            onChange([
              ...extras,
              {
                id: `${Date.now()}`,
                label: '',
                amount: 0,
                fromYear: startYear,
                toYear: Math.max(startYear, defaultUntil),
                indexed: true,
              },
            ])
          }
        >
          <span className="flex items-center gap-1.5">
            <Plus size={14} aria-hidden /> Add an amount
          </span>
        </Button>
      </div>
    </Card>
  )
}

/**
 * The rupee against each currency the plan touches - what it did over ten
 * years and what is assumed - and what that makes of each part of the corpus.
 */
export function CurrencyCard({
  base,
  legs,
  inrMove,
  inrOther,
  fxTrend,
  edited,
  onTrend,
  onMove,
  onOther,
  onReset,
}: {
  base: string
  legs: CurrencyLeg[]
  inrMove: (currency: string) => number
  inrOther: number
  fxTrend: boolean
  edited: boolean
  onTrend: (on: boolean) => void
  onMove: (currency: string, pct: number) => void
  onOther: (pct: number) => void
  onReset: () => void
}) {
  const [showRates, setShowRates] = useState(false)
  const [open, setOpen] = useState(false)
  const foreign = legs.filter((l) => l.currency !== base)
  const years = Array.from({ length: REF_LAST_YEAR - REF_FIRST_YEAR + 1 }, (_, i) => REF_FIRST_YEAR + i)
  // Every currency in play but the rupee: the base and each part.
  const involved = [...new Set([base, ...legs.map((l) => l.currency)])].filter((c) => c !== 'INR')
  const paired = involved.filter((c) => inrHistory(c) !== null)
  const others = involved.filter((c) => inrHistory(c) === null)
  const known = Object.keys(INR_PER_UNIT).filter((c) => c !== 'INR')
  const rupees = (v: number) => `${currencySymbol('INR')}${v.toFixed(1)}`
  const { confirm, dialog } = useConfirm()

  return (
    <Card
      title={
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
          className="flex items-center gap-1.5 hover:text-ink-secondary"
        >
          {open ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
          Currency risk
        </button>
      }
      subtitle={
        open
          ? `How the rupee moves against each currency, a year. Defaults follow ${REF_FIRST_YEAR}–${REF_LAST_YEAR}.`
          : undefined
      }
      actions={
        open && (
          <label className="flex shrink-0 items-center gap-2 text-xs text-ink-secondary">
            <input type="checkbox" checked={fxTrend} onChange={(e) => onTrend(e.target.checked)} />
            Apply the 10-year trend
          </label>
        )
      }
    >
      {!open ? null : foreign.length === 0 ? (
        <p className="text-sm text-ink-muted">The whole corpus is in {base}: no currency risk to model.</p>
      ) : (
        <>
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-[color:var(--hairline)] text-left text-ink-muted">
                  <th className="py-1.5 pr-2 font-medium">Pair</th>
                  <th className="px-2 py-1.5 font-medium">Last 10 years</th>
                  <th className="py-1.5 pl-2 text-right font-medium">Assumed, a year</th>
                </tr>
              </thead>
              <tbody>
                {paired.map((c) => {
                  const h = inrHistory(c)!
                  const r = INR_PER_UNIT[c]
                  return (
                    <tr key={c} className="border-b border-[color:var(--hairline)] last:border-0">
                      <td className="py-1.5 pr-2 font-medium text-ink">INR / {c}</td>
                      <td className="px-2 py-1.5 text-ink-secondary">
                        {moveWords(h.trendPct)} {pct(Math.abs(h.trendPct), 1)} a year
                        <span className="block text-[11px] text-ink-muted">
                          {rupees(r[0])} → {rupees(r[r.length - 1])} per {currencySymbol(c)}
                        </span>
                      </td>
                      <td className="py-1 pl-2">
                        <MoveInput value={inrMove(c)} onChange={(v) => onMove(c, v)} label={`INR against ${c}`} />
                      </td>
                    </tr>
                  )
                })}
                {others.length > 0 && (
                  <tr className="border-b border-[color:var(--hairline)] last:border-0">
                    <td className="py-1.5 pr-2 font-medium text-ink">
                      INR / {others.join(', ')}
                    </td>
                    <td className="px-2 py-1.5 text-ink-secondary">
                      No history here
                      <span className="block text-[11px] text-ink-muted">
                        Default: the rupee's average against {known.join(', ')}
                      </span>
                    </td>
                    <td className="py-1 pl-2">
                      <MoveInput value={inrOther} onChange={onOther} label="INR against other currencies" />
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>

          <p className="mb-1.5 mt-4 text-xs font-medium text-ink-secondary">What it does to each part, counted in {base}</p>
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-[color:var(--hairline)] text-left text-ink-muted">
                  <th className="py-1.5 pr-2 font-medium">Part</th>
                  <th className="px-2 py-1.5 text-right font-medium">Share</th>
                  <th className="px-2 py-1.5 text-right font-medium">Gain / loss a year</th>
                  <th className="py-1.5 pl-2 text-right font-medium" title="Typical yearly swing, for the shaded band">
                    Swing
                  </th>
                </tr>
              </thead>
              <tbody>
                {foreign.map((l) => (
                  <tr key={l.currency} className="border-b border-[color:var(--hairline)] last:border-0">
                    <td className="py-1.5 pr-2 font-medium text-ink">{l.currency}</td>
                    <td className="tabular px-2 py-1.5 text-right text-ink-secondary">{pct(l.weight * 100, 1)}</td>
                    <td className={`tabular px-2 py-1.5 text-right ${l.driftPct < 0 ? 'text-loss' : 'text-ink-secondary'}`}>
                      {signedPct(l.driftPct, 1)}
                    </td>
                    <td className="tabular py-1.5 pl-2 text-right text-ink-secondary">±{pct(l.volatilityPct, 1)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
            <p className="text-[11px] text-ink-muted">
              Each part earns the return in its own currency, then converts to {base}. "INR depreciates 3%" means a rupee
              buys 3% less of the other currency each year.
              {edited && (
                <>
                  {' '}
                  <button
                    type="button"
                    className="underline hover:text-ink"
                    onClick={() =>
                      confirm({
                        title: 'Reset the currency moves?',
                        body: `This removes all your changes to the currency moves and puts them back to the default ${REF_FIRST_YEAR}–${REF_LAST_YEAR} trend.`,
                        onConfirm: onReset,
                      })
                    }
                  >
                    Back to the 10-year trend
                  </button>
                </>
              )}
            </p>
            {paired.length > 0 && <TableToggle open={showRates} onToggle={() => setShowRates((v) => !v)} />}
          </div>
          {showRates && paired.length > 0 && (
            <div className="mt-2 overflow-x-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="border-b border-[color:var(--hairline)] text-ink-muted">
                    <th className="py-1.5 pr-2 text-left font-medium">INR per unit, year end</th>
                    {years.map((y) => (
                      <th key={y} className="px-1.5 py-1.5 text-right font-medium">
                        {y}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {paired.map((c) => (
                    <tr key={c} className="border-b border-[color:var(--hairline)] last:border-0">
                      <td className="py-1 pr-2 font-medium text-ink">{c}</td>
                      {INR_PER_UNIT[c].map((v, i) => (
                        <td key={years[i]} className="tabular px-1.5 py-1 text-right text-ink-secondary">
                          {v.toFixed(1)}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
              <p className="mt-1 text-[11px] text-ink-muted">
                Rounded reference rates; inflation from the World Bank, Eurostat and the ONS. {REF_LAST_YEAR} figures are
                provisional.
              </p>
            </div>
          )}
        </>
      )}
      {dialog}
    </Card>
  )
}

/** "Depreciated" or "appreciated", for the rupee's change. */
const moveWords = (pctChange: number) => (pctChange < 0 ? 'Depreciated' : 'Appreciated')

/**
 * The rupee's yearly change as the reader thinks of it: a direction and a
 * size. The direction is kept while the size is zero, so it can be picked first.
 */
function MoveInput({ value, onChange, label }: { value: number; onChange: (v: number) => void; label: string }) {
  const [down, setDown] = useState(value < 0)
  useEffect(() => {
    if (value !== 0) setDown(value < 0)
  }, [value])
  const size = Number(Math.abs(value).toFixed(2))
  return (
    <div className="ml-auto flex w-52 items-center gap-1.5">
      <select
        className={`${inputClass} w-auto py-1`}
        value={down ? 'down' : 'up'}
        aria-label={`Direction of ${label}`}
        onChange={(e) => {
          const d = e.target.value === 'down'
          setDown(d)
          onChange(d ? -size : size)
        }}
      >
        <option value="down">Depreciates</option>
        <option value="up">Appreciates</option>
      </select>
      <div className="w-16">
        <NumberInput
          value={size}
          onChange={(v) => onChange((down ? -1 : 1) * Math.abs(v ?? 0))}
          step={0.1}
          ariaLabel={`Yearly change of ${label}, %`}
        />
      </div>
      <span className="text-ink-muted">%</span>
    </div>
  )
}

const STATUS_ROW: Record<YearRow['status'], string> = {
  ok: '',
  low: 'bg-status-warning/10',
  negative: 'bg-status-critical/10',
}

/** A figure as the table shows it, red below zero. */
function Figure({ value, show }: { value: number; show: (v: number) => string }) {
  return <span className={value < 0 ? 'font-medium text-loss' : ''}>{show(value)}</span>
}

/** A figure in the table's title; when the rope moves it, the average it ran at instead. */
function TitleFigure({ value, average, name }: { value: string; average?: string; name: string }) {
  const moved = average !== undefined && average !== value
  return (
    <span
      className="font-normal text-ink-secondary"
      title={moved ? `${value} as planned; ${average} on average with the rope's shifts, over the years the corpus lasts` : undefined}
    >
      · {moved ? <s className="text-ink-muted">{value}</s> : value}
      {moved && <span className="font-medium text-ink"> avg {average}</span>} {name}
    </span>
  )
}

export type ShiftKind = keyof YearShifts

/** A rope knot moves this many steps either side of the plan's value, each this many percentage points. */
export const ROPE_STEPS = 20
export const ROPE_STEP = 1

const ROPE_KINDS: [ShiftKind, string][] = [
  ['returnPct', 'Return'],
  ['inflationPct', 'Inflation'],
  ['withdrawalPct', 'Withdrawal'],
]

/** What the table's rope column needs: the shifts so far and a way to change them. */
export interface RopeProps {
  shifts: YearShifts
  onShift: (kind: ShiftKind, year: number, points: number) => void
  onClear: (kind: ShiftKind) => void
}

/** Which assumption the rope shows, offered in the table's header and footer. */
function RopePicker({
  kind,
  setKind,
  shifts,
  onClear,
}: {
  kind: ShiftKind
  setKind: (k: ShiftKind) => void
  shifts: YearShifts
  onClear: (kind: ShiftKind) => void
}) {
  const count = (k: ShiftKind) => Object.keys(shifts[k] ?? {}).length
  const name = ROPE_KINDS.find(([k]) => k === kind)![1].toLowerCase()
  const { confirm, dialog } = useConfirm()
  return (
    <div className="flex items-center justify-center gap-1.5">
      <ToggleGroup
        label="Rope shows"
        size="sm"
        options={ROPE_KINDS.map(([k, label]) => [
          k,
          <>
            {label}
            {count(k) > 0 && <span className="ml-1 opacity-70">· {count(k)}</span>}
          </>,
        ])}
        value={kind}
        onChange={setKind}
      />
      <button
        type="button"
        onClick={() =>
          confirm({
            title: `Reset the ${name} rope?`,
            body: `This removes all ${count(kind)} ${name} shift${count(kind) === 1 ? '' : 's'} and puts every year back to the default value.`,
            onConfirm: () => onClear(kind),
          })
        }
        disabled={count(kind) === 0}
        aria-label={`Clear the ${name} shifts`}
        title="Straighten the rope"
        className="text-ink-muted hover:text-ink disabled:opacity-30"
      >
        <RotateCcw size={12} aria-hidden />
      </button>
      {dialog}
    </div>
  )
}

/** Where a knot `steps` from the middle sits across the rope's cell, %. */
const ropeX = (steps: number) => ((steps + ROPE_STEPS) / (2 * ROPE_STEPS)) * 100

/**
 * One row's stretch of the rope: a knot dragged right raises that year's
 * value, left lowers it. The line runs from midway to the row above's knot,
 * through this one, to midway to the row below's, so the rows join up.
 */
function RopeKnot({
  steps,
  prev,
  next,
  colour,
  label,
  onChange,
}: {
  steps: number
  prev: number | null
  next: number | null
  colour: string
  label: string
  onChange: (steps: number) => void
}) {
  const x = ropeX(steps)
  const top = prev === null ? x : (ropeX(prev) + x) / 2
  const bottom = next === null ? x : (ropeX(next) + x) / 2
  const set = (s: number) => {
    const clamped = Math.max(-ROPE_STEPS, Math.min(ROPE_STEPS, s))
    if (clamped !== steps) onChange(clamped)
  }
  const fromPointer = (e: ReactPointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect()
    set(Math.round(((e.clientX - rect.left) / rect.width) * 2 * ROPE_STEPS - ROPE_STEPS))
  }
  return (
    <div
      className="absolute inset-y-0 left-2 right-2 cursor-ew-resize touch-none select-none"
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId)
        fromPointer(e)
      }}
      onPointerMove={(e) => {
        if (e.currentTarget.hasPointerCapture(e.pointerId)) fromPointer(e)
      }}
    >
      <svg aria-hidden className="absolute inset-0 h-full w-full overflow-visible" viewBox="0 0 100 100" preserveAspectRatio="none">
        <line x1="50" y1="0" x2="50" y2="100" stroke="currentColor" strokeDasharray="2 3" vectorEffect="non-scaling-stroke" className="text-ink/20" />
        {/* Faint guides halfway out each side. */}
        {[-ROPE_STEPS / 2, ROPE_STEPS / 2].map((s) => (
          <line key={s} x1={ropeX(s)} y1="0" x2={ropeX(s)} y2="100" stroke="currentColor" strokeDasharray="1 4" vectorEffect="non-scaling-stroke" className="text-ink/10" />
        ))}
        <polyline
          points={`${top},0 ${x},50 ${bottom},100`}
          fill="none"
          stroke={colour}
          strokeWidth={1.5}
          strokeLinejoin="round"
          vectorEffect="non-scaling-stroke"
        />
      </svg>
      <span
        role="slider"
        tabIndex={0}
        aria-label={label}
        aria-valuemin={-ROPE_STEPS * ROPE_STEP}
        aria-valuemax={ROPE_STEPS * ROPE_STEP}
        aria-valuenow={steps * ROPE_STEP}
        aria-valuetext={steps === 0 ? 'As planned' : `${steps > 0 ? '+' : ''}${steps * ROPE_STEP} points`}
        onKeyDown={(e) => {
          const to =
            e.key === 'ArrowRight' || e.key === 'ArrowUp'
              ? steps + 1
              : e.key === 'ArrowLeft' || e.key === 'ArrowDown'
                ? steps - 1
                : e.key === 'Home' || e.key === 'Delete' || e.key === 'Backspace' || e.key === '0'
                  ? 0
                  : null
          if (to === null) return
          e.preventDefault()
          set(to)
        }}
        className="absolute top-1/2 h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 bg-surface"
        style={{ left: `${x}%`, borderColor: steps === 0 ? 'var(--edge-low)' : colour, background: steps === 0 ? undefined : colour }}
      />
    </div>
  )
}

/** Year by year, for the selected withdrawal. */
export function ProjectionTable({
  rows,
  label,
  rate,
  colour,
  currency,
  age,
  startYear,
  yearFraction,
  returnPct,
  inflationPct,
  hasFx,
  mark,
  rope,
  averages,
  onDownload,
  expand,
}: {
  rows: YearRow[]
  label: string
  /** The withdrawal as a share of the corpus, formatted. */
  rate: string
  colour: string
  currency: string
  age: number | null
  startYear: number
  /** Share of the first calendar year the plan runs. */
  yearFraction: number
  returnPct: number
  inflationPct: number
  hasFx: boolean
  /** A year to badge and rule off above: where the plan changes phase. */
  mark?: { year: number; label: string }
  /** A rope down the side, to shift a year's return, inflation or withdrawal. */
  rope?: RopeProps
  /** What the plan averaged with the rope's shifts; shown beside each figure the rope moves. */
  averages?: { rate: string; returnPct: number; inflationPct: number }
  /** Offers the simulation as a download. */
  onDownload?: () => void
  /** A button to widen the table to the page's full width, and back; on wide screens only. */
  expand?: { full: boolean; onToggle: () => void }
}) {
  const [ropeKind, setRopeKind] = useState<ShiftKind>('returnPct')
  const stepsAt = (year: number) => Math.round((rope?.shifts[ropeKind]?.[year] ?? 0) / ROPE_STEP)
  /** The row's value for the rope's assumption: the year's rate, or what the withdrawal takes of its opening balance. */
  const ropeValue = (r: YearRow) => {
    const shift = rope?.shifts[ropeKind]?.[r.year] ?? 0
    if (ropeKind === 'returnPct') return pct(returnPct + shift, 1)
    if (ropeKind === 'inflationPct') return pct(inflationPct + shift, 1)
    return r.opening > 0 ? pct((r.withdrawal * 100) / r.opening, 1) : '—'
  }
  const ropeName = ROPE_KINDS.find(([k]) => k === ropeKind)![1]
  const badges = (r: YearRow) => (
    <>
      {mark?.year === r.year && <Badge tone="info">{mark.label}</Badge>}
      {r.status === 'negative' && <Badge tone="critical">Short</Badge>}
      {r.status === 'low' && <Badge tone="warning">Low</Badge>}
    </>
  )
  const show = (v: number) => money(v, currency, { whole: true })
  const daysInYear = (Date.UTC(startYear + 1, 0, 1) - Date.UTC(startYear, 0, 1)) / 86_400_000
  const firstDays = Math.round(yearFraction * daysInYear)
  // Once a year opens and closes below zero the plan has failed; stop there.
  const end = rows.findIndex((r) => r.opening < 0 && r.closing < 0)
  const shown = end === -1 ? rows : rows.slice(0, end + 1)
  return (
    <Card
      style={selectedBorder(colour)}
      title={
        <span className="flex flex-wrap items-center gap-x-2 gap-y-0.5">
          <span aria-hidden className="h-0.5 w-4 rounded" style={{ background: colour }} />
          <span>Simulation: {label}</span>
          <TitleFigure value={rate} average={averages?.rate} name="withdrawal" />
          <TitleFigure value={pct(inflationPct, 1)} average={averages && pct(averages.inflationPct, 1)} name="Inflation" />
          <TitleFigure value={pct(returnPct, 1)} average={averages && pct(averages.returnPct, 1)} name="Return" />
        </span>
      }
      actions={
        <div className="flex shrink-0 flex-col items-end gap-1.5 text-[11px] text-ink-muted">
          <div className="flex items-center gap-1.5">
            {expand && (
              <span className="hidden xl:block">
                <Button variant={expand.full ? 'primary' : 'secondary'} onClick={expand.onToggle}>
                  <span
                    className="flex items-center"
                    title={expand.full ? "Back to the results column's width" : "Widen to the page's full width"}
                    aria-label={expand.full ? 'Narrow the simulation' : 'Widen the simulation'}
                  >
                    {expand.full ? <ChevronsRightLeft size={14} aria-hidden /> : <ChevronsLeftRight size={14} aria-hidden />}
                  </span>
                </Button>
              </span>
            )}
            {onDownload && (
              <Button variant="secondary" onClick={onDownload}>
                <span
                  className="flex items-center gap-1.5"
                  title="This simulation as a CSV for a spreadsheet: the assumptions, every Sim, and the year-by-year table"
                >
                  <Download size={14} aria-hidden /> Download
                </span>
              </Button>
            )}
          </div>
          <div className="flex flex-wrap justify-end gap-2">
            <span className="flex items-center gap-1">
              <span aria-hidden className="h-2.5 w-2.5 rounded-sm bg-status-warning/40" /> Under 2 years of withdrawals left
            </span>
            <span className="flex items-center gap-1">
              <span aria-hidden className="h-2.5 w-2.5 rounded-sm bg-status-critical/40" /> Below zero
            </span>
          </div>
        </div>
      }
    >
      <div className="max-h-[520px] overflow-auto">
        <table className="w-full whitespace-nowrap text-xs">
          <thead className="sticky top-0 z-10 bg-surface">
            <tr className="border-b border-[color:var(--hairline)] text-ink-muted">
              <th className="py-1.5 pr-2 text-left font-medium" title="Year of the plan">
                Yr #
              </th>
              <th className="px-2 py-1.5 text-left font-medium">Year</th>
              {age !== null && <th className="px-2 py-1.5 text-right font-medium">Age</th>}
              <th className="px-2 py-1.5 text-right font-medium">Opening</th>
              <th className="px-2 py-1.5 text-right font-medium">Withdrawal</th>
              <th className="px-2 py-1.5 text-right font-medium">Additional</th>
              <th className="px-2 py-1.5 text-right font-medium">Return</th>
              {hasFx && (
                <th
                  className="px-2 py-1.5 text-right font-medium"
                  title="What the assumed exchange-rate trend adds (or takes) on the corpus held in other currencies, on top of the return"
                >
                  Currency Impact
                </th>
              )}
              <th className="px-2 py-1.5 text-right font-medium">Closing</th>
              <th className="whitespace-normal px-2 py-1.5 text-right font-medium leading-tight">
                Closing,
                <br />
                today's money
              </th>
              {!rope && (
                <th className="py-1.5 pl-2 text-left font-medium">
                  <span className="sr-only">Status</span>
                </th>
              )}
              {rope && (
                <th colSpan={2} className="w-full border-l border-[color:var(--hairline)] py-1 pl-2 font-medium">
                  <RopePicker kind={ropeKind} setKind={setRopeKind} shifts={rope.shifts} onClear={rope.onClear} />
                </th>
              )}
            </tr>
          </thead>
          <tbody>
            {shown.map((r, k) => (
              <tr
                key={r.year}
                className={`border-b border-[color:var(--hairline)] last:border-0 ${STATUS_ROW[r.status]} ${
                  mark?.year === r.year && r.index > 1 ? 'border-t-2 border-t-[color:var(--edge)]' : ''
                }`}
              >
                <td className="tabular py-1 pr-2 text-ink-secondary">{r.index}</td>
                <td className="tabular px-2 py-1 text-ink">
                  {r.year}
                  {/* With the rope taking the right-hand side, the badges ride on the year. */}
                  {rope && <span className="ml-1.5 space-x-1">{badges(r)}</span>}
                </td>
                {age !== null && <td className="tabular px-2 py-1 text-right text-ink-secondary">{age + r.year - startYear}</td>}
                <td className="tabular px-2 py-1 text-right text-ink-secondary">
                  <Figure value={r.opening} show={show} />
                </td>
                <td className="tabular px-2 py-1 text-right text-ink-secondary">{show(r.withdrawal)}</td>
                <td className="tabular px-2 py-1 text-right text-ink-secondary">
                  {r.extras === 0 ? '—' : <Figure value={r.extras} show={show} />}
                </td>
                <td className="tabular px-2 py-1 text-right text-ink-secondary">
                  {r.returns === 0 ? '—' : show(r.returns)}
                </td>
                {hasFx && (
                  <td className="tabular px-2 py-1 text-right text-ink-secondary">
                    {r.fxEffect === 0 ? '—' : <Figure value={r.fxEffect} show={show} />}
                  </td>
                )}
                <td className="tabular px-2 py-1 text-right text-ink">
                  <Figure value={r.closing} show={show} />
                </td>
                <td className="tabular px-2 py-1 text-right text-ink-secondary">
                  <Figure value={r.closingReal} show={show} />
                </td>
                {!rope && <td className="space-x-1 whitespace-nowrap py-1 pl-2">{badges(r)}</td>}
                {rope && (
                  <>
                    <td
                      className={`tabular border-l border-[color:var(--hairline)] py-1 pl-2 text-right ${
                        stepsAt(r.year) === 0 ? 'text-ink-muted' : 'font-medium text-ink'
                      }`}
                    >
                      {ropeValue(r)}
                    </td>
                    <td className="relative w-full min-w-56 p-0">
                      <RopeKnot
                        steps={stepsAt(r.year)}
                        prev={k > 0 ? stepsAt(shown[k - 1].year) : null}
                        next={k < shown.length - 1 ? stepsAt(shown[k + 1].year) : null}
                        colour={colour}
                        label={`${ropeName} in ${r.year}, against the plan`}
                        onChange={(s) => rope.onShift(ropeKind, r.year, s * ROPE_STEP)}
                      />
                    </td>
                  </>
                )}
              </tr>
            ))}
          </tbody>
          {rope && (
            <tfoot className="sticky bottom-0 z-10 bg-surface">
              <tr className="border-t border-[color:var(--hairline)]">
                {/* Under the rope's two columns, as in the header. */}
                <td colSpan={8 + (age !== null ? 1 : 0) + (hasFx ? 1 : 0)} />
                <td colSpan={2} className="border-l border-[color:var(--hairline)] py-1 pl-2">
                  <RopePicker kind={ropeKind} setKind={setRopeKind} shifts={rope.shifts} onClear={rope.onClear} />
                </td>
              </tr>
            </tfoot>
          )}
        </table>
      </div>
      {rope && (
        <p className="mt-2 text-[11px] text-ink-muted">
          Drag a row's knot on the rope right to raise that year's {ropeName.toLowerCase()}, left to lower it:{' '}
          {ROPE_STEP} point{ROPE_STEP === 1 ? '' : 's'} a step, up to {ROPE_STEPS * ROPE_STEP} either way.
          {ropeKind === 'withdrawalPct'
            ? ' Each withdrawal step takes that share of the opening corpus more, or less; the column shows what comes out against the opening.'
            : ropeKind === 'inflationPct'
              ? ' Inflation in one year carries into the prices of every year after it.'
              : ''}{' '}
          Shifts apply to every Sim.
        </p>
      )}
      <p className="mt-2 text-[11px] text-ink-muted">
        Withdrawal and additional amounts come out at the start of each year; the rest earns that year's return.
        {yearFraction < 1 &&
          ` ${startYear} is the ${firstDays} days left of it, so its withdrawal, recurring amounts and return are for that part only.`}{' '}
        Assumptions, not a forecast.
      </p>
    </Card>
  )
}
