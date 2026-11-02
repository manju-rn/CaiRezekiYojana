import type { ReactNode } from 'react'
import { TriangleAlert } from 'lucide-react'

import type { Purged } from '../api/types'
import { inputClass } from './ui'

/** What has to be typed, exactly, before a bulk delete goes ahead. */
export const CONFIRM_PHRASE = 'DELETE'

/**
 * A boxed-off area for deletes that cannot be undone. The warning is in
 * words as well as colour, and always says the data will not come back.
 */
export function DangerZone({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="rounded-lg border border-status-critical/50 bg-status-critical/[0.03] p-4">
      <header className="mb-3 flex items-start gap-2">
        <TriangleAlert size={18} className="mt-0.5 shrink-0 text-status-critical" aria-hidden />
        <div>
          <h2 className="text-sm font-semibold text-status-critical">Danger zone · {title}</h2>
          <p className="mt-0.5 text-xs text-ink-secondary">
            All data deleted here is gone for good and <strong>cannot be restored</strong>. There is no undo
            and no recycle bin — export anything you want to keep first.
          </p>
        </div>
      </header>
      {children}
    </section>
  )
}

/** A box the person must type `expected` into, exactly. */
export function TypeToConfirm({
  label,
  expected,
  value,
  onChange,
}: {
  label: ReactNode
  expected: string
  value: string
  onChange: (v: string) => void
}) {
  const ok = value === expected
  return (
    <label className="block">
      <span className="mb-1 block text-xs font-medium text-ink-secondary">{label}</span>
      <input
        className={`${inputClass} max-w-xs font-mono`}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={expected}
        autoComplete="off"
        spellCheck={false}
        aria-invalid={value !== '' && !ok}
      />
    </label>
  )
}

/** "Deleted 3 deposits and 1 account." */
export function purgedSummary(p: Purged): string {
  const parts = (
    [
      [p.transactions, 'transaction'],
      [p.deposits, 'deposit'],
      [p.cash_balances, 'bank balance'],
      [p.accounts, 'account'],
      [p.import_batches, 'import'],
    ] as const
  )
    .filter(([n]) => n > 0)
    .map(([n, what]) => `${n} ${what}${n === 1 ? '' : 's'}`)
  if (parts.length === 0) return 'Nothing matched, so nothing was deleted.'
  const list = parts.length === 1 ? parts[0] : `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}`
  return `Deleted ${list}.`
}
