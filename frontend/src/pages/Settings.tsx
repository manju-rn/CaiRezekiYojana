import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { PageTitle } from '../components/PageTitle'
import { api } from '../api/client'
import type { RefreshReport, SettingsResponse } from '../api/types'
import { Button, Card, ErrorBanner, Field, Spinner, inputClass } from '../components/ui'
import { CURRENCIES, DEFAULT_CURRENCY } from '../lib/currencies'
import { PlannerSessionSettings, YatraSessionSettings } from '../components/PlannerSessionSettings'

// Only these preferences are saved from here; the server rejects anything
// else, so send only these.
const OWN_KEYS = ['base_currency', 'cost_basis_method', 'financial_year_start_month']

export default function Settings() {
  const queryClient = useQueryClient()
  const { data, isLoading, error } = useQuery({
    queryKey: ['settings'],
    queryFn: () => api.get<SettingsResponse>('/settings'),
  })

  const [draft, setDraft] = useState<Record<string, string>>({})

  // Seed the form once the server values arrive, without clobbering edits.
  useEffect(() => {
    if (data) setDraft(data.settings)
  }, [data])

  const save = useMutation({
    mutationFn: (settings: Record<string, string>) =>
      api.put<SettingsResponse>('/settings', {
        settings: Object.fromEntries(OWN_KEYS.filter((k) => k in settings).map((k) => [k, settings[k]])),
      }),
    onSuccess: () => queryClient.invalidateQueries(),
  })

  const refreshAll = useMutation({
    mutationFn: () => api.post<RefreshReport[]>('/dashboard/refresh'),
    onSuccess: () => queryClient.invalidateQueries(),
  })

  if (isLoading) return <Spinner />
  if (error) return <ErrorBanner error={error} />

  const set = (key: string, value: string) => setDraft((d) => ({ ...d, [key]: value }))

  return (
    <div className="space-y-5">
      <PageTitle
        title="General"
        description="How your figures are calculated and reported."
      />

      <Card title="Reporting">
        <div className="grid gap-3 sm:grid-cols-3">
          <Field
            label="Main currency"
            hint="Every total is reported in this currency first, then in the others."
          >
            <select
              className={inputClass}
              value={draft.base_currency ?? DEFAULT_CURRENCY}
              onChange={(e) => set('base_currency', e.target.value)}
            >
              {CURRENCIES.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </Field>

          <Field
            label="Cost basis method"
            hint="FIFO: the first units bought are the first sold."
          >
            <select
              className={inputClass}
              value={draft.cost_basis_method ?? 'FIFO'}
              onChange={(e) => set('cost_basis_method', e.target.value)}
            >
              <option value="FIFO">FIFO (first in, first out)</option>
              <option value="WEIGHTED_AVERAGE">Weighted average</option>
            </select>
          </Field>

          <Field label="Financial year starts" hint="Month number. 1 = January.">
            <input
              className={inputClass}
              type="number"
              min={1}
              max={12}
              value={draft.financial_year_start_month ?? '1'}
              onChange={(e) => set('financial_year_start_month', e.target.value)}
            />
          </Field>
        </div>
      </Card>

      <PlannerSessionSettings />
      <YatraSessionSettings />

      <div className="flex flex-wrap items-center gap-3">
        <Button variant="primary" onClick={() => save.mutate(draft)} disabled={save.isPending}>
          {save.isPending ? 'Saving…' : 'Save settings'}
        </Button>
        <Button onClick={() => refreshAll.mutate()} disabled={refreshAll.isPending}>
          {refreshAll.isPending ? 'Refreshing…' : 'Refresh all data now'}
        </Button>
        {save.isSuccess && <span className="text-sm text-gain">Saved.</span>}
      </div>

      {save.isError && <ErrorBanner error={save.error} />}

      <p className="text-xs text-ink-muted">
        The API is described at <a className="text-series-1 hover:underline" href="/api/docs/">/api/docs</a>.
      </p>

      {refreshAll.data && (
        <Card title="Last refresh">
          <ul className="space-y-2 text-sm">
            {refreshAll.data.map((r) => (
              <li key={r.source} className="border-b border-[color:var(--hairline)] pb-2 last:border-0">
                <span className="font-medium text-ink">{r.source}</span>
                <span className="text-ink-secondary">
                  {' '}
                  — {r.updated} updated of {r.requested} requested
                  {r.missing > 0 && `, ${r.missing} missing`}
                </span>
                {r.warnings.length > 0 && (
                  <ul className="mt-1 list-disc pl-5 text-xs text-ink-muted">
                    {r.warnings.slice(0, 8).map((w) => (
                      <li key={w}>{w}</li>
                    ))}
                    {r.warnings.length > 8 && <li>and {r.warnings.length - 8} more</li>}
                  </ul>
                )}
              </li>
            ))}
          </ul>
        </Card>
      )}
    </div>
  )
}
