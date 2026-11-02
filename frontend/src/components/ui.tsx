import { CSSProperties, ReactNode, useEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'

import { deltaClass, money, signedMoney, signedPct } from '../lib/format'

export function Card({
  title,
  subtitle,
  actions,
  children,
  className = '',
  style,
}: {
  title?: ReactNode
  subtitle?: ReactNode
  actions?: ReactNode
  children: ReactNode
  className?: string
  style?: CSSProperties
}) {
  return (
    <section className={`card p-4 ${className}`} style={style}>
      {(title || actions) && (
        <header className={`${children ? 'mb-3' : ''} flex items-start justify-between gap-3`}>
          <div>
            {title && <h2 className="text-sm font-semibold text-ink">{title}</h2>}
            {subtitle && <p className="mt-0.5 text-xs text-ink-muted">{subtitle}</p>}
          </div>
          {actions}
        </header>
      )}
      {children}
    </section>
  )
}

/**
 * A single headline figure. No plot, so no hover layer - the number is the
 * whole message.
 */
export function StatTile({
  label,
  value,
  delta,
  deltaPct,
  currency,
  hint,
  whole = false,
  equivalents,
  to,
  corner,
  children,
}: {
  label: string
  value: number | null
  delta?: number | null
  deltaPct?: number | null
  currency: string
  hint?: string
  /** Drop the minor-unit digits: for portfolio totals, where they are noise. */
  whole?: boolean
  /** The same figure in other currencies, listed under it. A null value
   *  is a currency with no rate. */
  equivalents?: { currency: string; value: number | null }[]
  /** The screen the figure comes from. The whole tile opens it; the label
   *  is the real link, for the keyboard and screen readers. */
  to?: string
  /** On the figure's line, at its right - the tile's share of the
   *  portfolio, say. Drops below the figure when the tile is too narrow. */
  corner?: ReactNode
  /** Extra detail under the hint - a mini chart, an expandable breakdown.
   *  Controls in here should stop their clicks reaching a linked tile. */
  children?: ReactNode
}) {
  const navigate = useNavigate()
  const labelClass = 'text-xs font-medium uppercase tracking-wide text-ink-muted'
  return (
    <div
      className={`card min-w-0 p-4 ${to ? 'cursor-pointer transition hover:border-series-1/50' : ''}`}
      onClick={to ? () => navigate(to) : undefined}
    >
      {to ? (
        <Link to={to} onClick={(e) => e.stopPropagation()} className={`${labelClass} hover:text-ink`}>
          {label} <span aria-hidden>→</span>
        </Link>
      ) : (
        <div className={labelClass}>{label}</div>
      )}
      <div className="mt-1.5 flex flex-wrap items-center justify-between gap-x-2 gap-y-1">
        <div className="text-2xl font-semibold text-ink">{money(value, currency, { whole })}</div>
        {corner}
      </div>
      {(delta !== undefined || deltaPct !== undefined) && (
        <div className={`mt-1 text-sm tabular ${deltaClass(delta ?? deltaPct)}`}>
          {delta !== undefined && delta !== null && signedMoney(delta, currency, { whole })}
          {delta !== undefined && deltaPct !== undefined && deltaPct !== null && ' · '}
          {deltaPct !== undefined && deltaPct !== null && signedPct(deltaPct)}
        </div>
      )}
      {hint && <div className="mt-1 text-xs text-ink-muted">{hint}</div>}
      {children && <div className="mt-2">{children}</div>}
      {equivalents && equivalents.length > 0 && (
        <dl className="mt-2 space-y-0.5 border-t border-[color:var(--hairline)] pt-2 text-xs">
          {equivalents.map((e) => (
            <div key={e.currency} className="flex items-baseline justify-between gap-2">
              {/* The code, not just the symbol: US and Singapore dollars are both `$`. */}
              <dt className="font-medium text-ink-muted">{e.currency}</dt>
              <dd
                className="tabular text-ink-secondary"
                title={e.value === null ? `No exchange rate to ${e.currency} yet` : undefined}
              >
                {money(e.value, e.currency, { whole })}
              </dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  )
}

export function Spinner({ label = 'Loading' }: { label?: string }) {
  return (
    <div className="flex items-center gap-2 p-6 text-sm text-ink-muted" role="status">
      <span
        className="inline-block h-3.5 w-3.5 animate-spin rounded-full border-2 border-line border-t-series-1"
        aria-hidden
      />
      {label}…
    </div>
  )
}

export function ErrorBanner({ error }: { error: unknown }) {
  const message = error instanceof Error ? error.message : String(error)
  return (
    <div
      role="alert"
      className="flex items-start gap-2 rounded-lg border border-status-critical/40 bg-status-critical/5 p-3 text-sm"
    >
      {/* Icon plus text: the status colour never carries the meaning alone. */}
      <span aria-hidden className="mt-0.5 text-status-critical">
        ⚠
      </span>
      <div>
        <div className="font-medium text-ink">Something went wrong</div>
        <div className="mt-0.5 text-ink-secondary">{message}</div>
      </div>
    </div>
  )
}

/**
 * What went in, as one line under a market value. Open to start with; a
 * click folds away the note and the figure in the other currencies.
 */
export function InvestedLine({
  value,
  label = 'Invested',
  currency,
  hint,
  equivalents,
}: {
  value: number
  currency: string
  hint?: string
  /** What went in is principal on the deposits page. */
  label?: string
  equivalents: { currency: string; value: number | null }[]
}) {
  const [open, setOpen] = useState(true)
  return (
    <div className="border-t border-[color:var(--hairline)] pt-2">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex w-full items-baseline justify-between gap-2 text-sm text-ink-secondary hover:text-ink"
      >
        <span>
          <span aria-hidden className="mr-1 inline-block w-2 text-[10px] text-ink-muted">
            {open ? '▾' : '▸'}
          </span>
          {label}
        </span>
        <span className="tabular font-medium text-ink">{money(value, currency, { whole: true })}</span>
      </button>
      {open && (
        <div className="mt-1.5 pl-3">
          {hint && <div className="text-xs text-ink-muted">{hint}</div>}
          {equivalents.length > 0 && (
            <dl className={`${hint ? 'mt-1.5 ' : ''}space-y-0.5 text-xs`}>
              {equivalents.map((e) => (
                <div key={e.currency} className="flex items-baseline justify-between gap-2">
                  <dt className="font-medium text-ink-muted">{e.currency}</dt>
                  <dd
                    className="tabular text-ink-secondary"
                    title={e.value === null ? `No exchange rate to ${e.currency} yet` : undefined}
                  >
                    {money(e.value, e.currency, { whole: true })}
                  </dd>
                </div>
              ))}
            </dl>
          )}
        </div>
      )}
    </div>
  )
}

/** One labelled figure under a tile's headline, ruled off like the
 *  invested line. */
export function FigureLine({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-2 border-t border-[color:var(--hairline)] pt-2 text-sm text-ink-secondary">
      <span>{label}</span>
      {children}
    </div>
  )
}

export function WarningList({ warnings }: { warnings: string[] }) {
  if (warnings.length === 0) return null
  return (
    <div
      role="status"
      className="rounded-lg border border-status-warning/40 bg-status-warning/5 p-3 text-sm"
    >
      <div className="flex items-center gap-2 font-medium text-ink">
        <span aria-hidden className="text-status-serious">
          ⚑
        </span>
        Worth knowing
      </div>
      <ul className="mt-1.5 list-disc space-y-1 pl-8 text-ink-secondary">
        {warnings.map((w) => (
          <li key={w}>{w}</li>
        ))}
      </ul>
    </div>
  )
}

export function EmptyState({
  title,
  body,
  action,
}: {
  title: string
  body: string
  action?: ReactNode
}) {
  return (
    <div className="card flex flex-col items-center gap-2 p-10 text-center">
      <div className="text-sm font-semibold text-ink">{title}</div>
      <p className="max-w-md text-sm text-ink-secondary">{body}</p>
      {action && <div className="mt-2">{action}</div>}
    </div>
  )
}

export function Badge({
  children,
  tone = 'neutral',
}: {
  children: ReactNode
  tone?: 'neutral' | 'good' | 'warning' | 'critical' | 'info'
}) {
  const tones: Record<string, string> = {
    neutral: 'bg-ink/5 text-ink-secondary',
    good: 'bg-status-good/10 text-status-good',
    warning: 'bg-status-warning/15 text-ink-secondary',
    critical: 'bg-status-critical/10 text-status-critical',
    info: 'bg-series-1/10 text-series-1',
  }
  return (
    <span
      className={`inline-flex items-center rounded px-1.5 py-0.5 text-[11px] font-medium ${tones[tone]}`}
    >
      {children}
    </span>
  )
}

export function Button({
  children,
  onClick,
  variant = 'secondary',
  disabled,
  type = 'button',
  title,
}: {
  children: ReactNode
  onClick?: () => void
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger'
  disabled?: boolean
  type?: 'button' | 'submit'
  /** A tooltip, and the name read out: for a button that is only an icon. */
  title?: string
}) {
  const variants: Record<string, string> = {
    primary: 'bg-series-1 text-white hover:opacity-90',
    secondary: 'border border-[color:var(--hairline)] bg-surface text-ink hover:bg-ink/5',
    ghost: 'text-ink-secondary hover:bg-ink/5',
    danger: 'bg-status-critical text-white hover:opacity-90',
  }
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={title}
      className={`rounded-md px-3 py-1.5 text-sm font-medium transition disabled:cursor-not-allowed disabled:opacity-50 ${variants[variant]}`}
    >
      {children}
    </button>
  )
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: ReactNode
}) {
  return (
    <label className="block">
      <span className="mb-1 block text-xs font-medium text-ink-secondary">{label}</span>
      {children}
      {hint && <span className="mt-1 block text-xs text-ink-muted">{hint}</span>}
    </label>
  )
}

export const inputClass =
  'w-full rounded-md border border-[color:var(--hairline)] bg-surface px-2.5 py-1.5 text-sm text-ink placeholder:text-ink-muted'

/**
 * A sortable column heading. The whole label is the button, and the active
 * column carries its direction both as an arrow and as `aria-sort`.
 */
export function SortableHeader({
  children,
  onClick,
  active,
  descending,
  align = 'left',
}: {
  children: ReactNode
  onClick: () => void
  active: boolean
  descending: boolean
  align?: 'left' | 'right'
}) {
  return (
    <th
      className={`px-2 py-2 font-medium ${align === 'right' ? 'text-right' : 'text-left'}`}
      aria-sort={active ? (descending ? 'descending' : 'ascending') : undefined}
    >
      <button
        type="button"
        onClick={onClick}
        className={`hover:text-ink ${active ? 'text-ink' : ''}`}
      >
        {children}
        {active && <span aria-hidden> {descending ? '↓' : '↑'}</span>}
      </button>
    </th>
  )
}

/** A row of toggle buttons, one picked at a time; the picked one in blue. */
export function ToggleGroup<T extends string>({
  label,
  options,
  value,
  onChange,
  size = 'md',
  stretch = false,
}: {
  label: string
  options: readonly (readonly [T, ReactNode])[]
  value: T
  onChange: (value: T) => void
  /** `sm` for a control tucked under a chart. */
  size?: 'sm' | 'md'
  /** Fill the width, the buttons sharing it evenly. */
  stretch?: boolean
}) {
  return (
    <div
      role="group"
      aria-label={label}
      className={`flex shrink-0 overflow-hidden rounded-md border border-[color:var(--hairline)] bg-surface ${
        stretch ? 'w-full' : 'w-fit'
      }`}
    >
      {options.map(([v, text]) => (
        <button
          key={v}
          type="button"
          aria-pressed={v === value}
          onClick={() => onChange(v)}
          className={`whitespace-nowrap border-l border-[color:var(--hairline)] font-medium transition first:border-l-0 ${
            size === 'sm' ? 'px-1.5 py-0.5 text-[11px]' : 'px-2.5 py-1 text-xs'
          } ${stretch ? 'flex-1' : ''} ${
            v === value ? 'bg-series-1 text-white' : 'text-ink-secondary hover:bg-ink/5 hover:text-ink'
          }`}
        >
          {text}
        </button>
      ))}
    </div>
  )
}

/** Segmented control for the currency a card reports in. */
export function CurrencySwitch({
  options,
  value,
  onChange,
}: {
  options: string[]
  value: string
  onChange: (currency: string) => void
}) {
  return (
    <ToggleGroup
      label="Report in currency"
      options={options.map((c) => [c, c] as const)}
      value={value}
      onChange={onChange}
    />
  )
}

/** What a confirmation asks, and what to do on yes. */
export interface ConfirmAsk {
  title: string
  body: ReactNode
  /** The yes button, e.g. "Reset". */
  confirmLabel?: string
  onConfirm: () => void
}

/**
 * A modal asking to confirm a change that cannot be undone. Escape, a click
 * outside or Cancel leave everything as it was.
 */
export function ConfirmDialog({ ask, onDone }: { ask: ConfirmAsk | null; onDone: () => void }) {
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const d = ref.current
    if (!d) return
    if (ask && !d.open) d.showModal()
    else if (!ask && d.open) d.close()
  }, [ask])
  return (
    <dialog
      ref={ref}
      onClose={onDone}
      onClick={(e) => e.target === e.currentTarget && onDone()}
      aria-labelledby="confirm-title"
      className="card m-auto w-[min(26rem,calc(100vw-2rem))] p-0 text-ink backdrop:bg-black/40"
    >
      {ask && (
        <div className="p-5">
          <h2 id="confirm-title" className="text-sm font-semibold text-ink">
            {ask.title}
          </h2>
          <div className="mt-2 text-sm text-ink-secondary">{ask.body}</div>
          <div className="mt-5 flex justify-end gap-2">
            <Button onClick={onDone}>Cancel</Button>
            <Button
              variant="danger"
              onClick={() => {
                ask.onConfirm()
                onDone()
              }}
            >
              {ask.confirmLabel ?? 'Reset'}
            </Button>
          </div>
        </div>
      )}
    </dialog>
  )
}

/** `confirm(ask)` opens the dialog; render `dialog` once in the component. */
export function useConfirm() {
  const [ask, setAsk] = useState<ConfirmAsk | null>(null)
  return { confirm: setAsk, dialog: <ConfirmDialog ask={ask} onDone={() => setAsk(null)} /> }
}
