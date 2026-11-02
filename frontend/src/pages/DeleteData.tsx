import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Trash2 } from 'lucide-react'

import { PageTitle } from '../components/PageTitle'
import { api } from '../api/client'
import type { DeletableCategory, Purged } from '../api/types'
import { CONFIRM_PHRASE, DangerZone, TypeToConfirm, purgedSummary } from '../components/DangerZone'
import { Button, ErrorBanner, Field, Spinner, inputClass } from '../components/ui'

/** `''` stands for "All" in both pickers. */
const ALL = ''

const plural = (n: number, unit: string) => `${n} ${n === 1 ? unit.replace(/s$/, '') : unit}`

/**
 * Deleting records in bulk: pick a page, then an institution within it,
 * either of them "All", and type the confirmation.
 */
export default function DeleteData() {
  const queryClient = useQueryClient()
  const inventory = useQuery({
    queryKey: ['deletable'],
    queryFn: () => api.get<DeletableCategory[]>('/data'),
  })
  const [category, setCategory] = useState(ALL)
  const [institution, setInstitution] = useState(ALL)
  const [confirm, setConfirm] = useState('')
  const [done, setDone] = useState<Purged | null>(null)

  const categories = inventory.data ?? []
  const chosen = categories.filter((c) => category === ALL || c.key === category)

  // Every institution in the chosen pages, with how much each holds there.
  const totals = new Map<string, number>()
  for (const c of chosen) for (const i of c.institutions) totals.set(i.name, (totals.get(i.name) ?? 0) + i.count)
  const institutions = [...totals.entries()].sort(([a], [b]) => a.localeCompare(b))

  // What would go, page by page.
  const doomed = chosen
    .map((c) => ({
      ...c,
      n: institution === ALL ? c.count : (c.institutions.find((i) => i.name === institution)?.count ?? 0),
    }))
    .filter((c) => c.n > 0)
  const everything = category === ALL && institution === ALL

  const purge = useMutation({
    mutationFn: () =>
      api.post<Purged>('/data/delete', {
        category: category || null,
        institution: institution || null,
        confirm,
      }),
    onSuccess: (p) => {
      setDone(p)
      setConfirm('')
      setInstitution(ALL)
      // Every page's figures may have changed.
      queryClient.invalidateQueries()
    },
  })

  if (inventory.isLoading) return <Spinner />
  if (inventory.error) return <ErrorBanner error={inventory.error} />

  const where = [
    category === ALL ? 'every page' : categories.find((c) => c.key === category)?.label,
    institution === ALL ? 'every institution' : institution,
  ].join(' · ')

  return (
    <div className="space-y-5">
      <PageTitle
        title="Delete data"
        description="Remove records in bulk - one page's, one bank or broker's, or everything. Your account, settings and upload history stay."
      />

      <DangerZone title="Delete records">
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Category">
            <select
              className={inputClass}
              value={category}
              onChange={(e) => {
                setCategory(e.target.value)
                setInstitution(ALL)
                setDone(null)
              }}
            >
              <option value={ALL}>All categories</option>
              {categories.map((c) => (
                <option key={c.key} value={c.key} disabled={c.count === 0}>
                  {c.label} ({plural(c.count, c.unit)})
                </option>
              ))}
            </select>
          </Field>
          <Field label="Institution" hint="Banks for deposits and cash; brokers and platforms for investments.">
            <select
              className={inputClass}
              value={institution}
              onChange={(e) => {
                setInstitution(e.target.value)
                setDone(null)
              }}
            >
              <option value={ALL}>All institutions</option>
              {institutions.map(([name, n]) => (
                <option key={name} value={name}>
                  {name} ({n})
                </option>
              ))}
            </select>
          </Field>
        </div>

        <div className="mt-4 rounded-md border border-[color:var(--hairline)] bg-surface p-3 text-sm">
          <div className="text-xs font-medium text-ink-secondary">This will permanently delete — {where}</div>
          {doomed.length === 0 ? (
            <p className="mt-1 text-ink-muted">Nothing matches.</p>
          ) : (
            <ul className="mt-1 list-inside list-disc text-ink">
              {doomed.map((c) => (
                <li key={c.key}>
                  {c.label}: {plural(c.n, c.unit)}
                </li>
              ))}
              {everything && <li>Every account, and the import history</li>}
            </ul>
          )}
          {doomed.length > 0 && !everything && (
            <p className="mt-1 text-xs text-ink-muted">Accounts left with nothing in them are removed too.</p>
          )}
        </div>

        <div className="mt-4 flex flex-wrap items-end gap-3">
          <TypeToConfirm
            label={
              <>
                Type <code className="font-mono font-semibold text-ink">{CONFIRM_PHRASE}</code> to confirm
              </>
            }
            expected={CONFIRM_PHRASE}
            value={confirm}
            onChange={setConfirm}
          />
          <Button
            variant="danger"
            disabled={confirm !== CONFIRM_PHRASE || doomed.length === 0 || purge.isPending}
            onClick={() => purge.mutate()}
          >
            <span className="flex items-center gap-1.5">
              <Trash2 size={14} aria-hidden />
              {purge.isPending ? 'Deleting…' : 'Delete permanently'}
            </span>
          </Button>
        </div>

        {done && (
          <p role="status" className="mt-3 text-sm text-ink">
            {purgedSummary(done)}
          </p>
        )}
        {purge.isError && (
          <div className="mt-3">
            <ErrorBanner error={purge.error} />
          </div>
        )}
      </DangerZone>
    </div>
  )
}
