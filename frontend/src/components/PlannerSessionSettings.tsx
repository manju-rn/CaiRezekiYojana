import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { api } from '../api/client'
import type { SettingsResponse } from '../api/types'
import { PLANNER_SESSIONS, SavedSessionsList, YATRA_SESSIONS, type SessionKind } from './PlanSessions'
import { Button, Card, ErrorBanner, Field, Spinner, inputClass } from './ui'

/** A page's saved sessions: the user's own `<prefix>_*` settings. */
interface Prefs {
  enabled: boolean
  max: string
  download: boolean
  upload: boolean
}

/** On unless turned off. */
const on = (v: string | undefined) => v !== 'false'

/** RezekiYojana's saved sessions, in Settings. */
export const PlannerSessionSettings = () => (
  <SessionSettings
    kind={PLANNER_SESSIONS}
    title="Planner"
    subtitle="Saved sessions in RezekiYojana: read-only snapshots of a plan and its values."
  />
)

/** CaiYatra's saved sessions, in Settings. */
export const YatraSessionSettings = () => (
  <SessionSettings
    kind={YATRA_SESSIONS}
    title="CaiYatra"
    subtitle="Saved sessions in CaiYatra: read-only snapshots of every setting and the portfolio it starts from."
  />
)

/**
 * A page's saved sessions, as the user wants them: whether they are
 * recorded and how many are kept, and whether they may be downloaded as
 * JSON and uploaded again; and the sessions kept, to delete.
 */
function SessionSettings({ kind, title, subtitle }: { kind: SessionKind; title: string; subtitle: string }) {
  const p = kind.prefix
  const qc = useQueryClient()
  const { data, isLoading, error } = useQuery({
    queryKey: ['settings'],
    queryFn: () => api.get<SettingsResponse>('/settings'),
  })
  const [prefs, setPrefs] = useState<Prefs | null>(null)

  useEffect(() => {
    if (!data) return
    const s = data.settings
    setPrefs({
      enabled: on(s[`${p}_enabled`]),
      max: s[`${p}_max`] ?? '10',
      download: on(s[`${p}_download`]),
      upload: on(s[`${p}_upload`]),
    })
  }, [data])

  const save = useMutation({
    mutationFn: (v: Prefs) =>
      api.put<SettingsResponse>('/settings', {
        settings: {
          [`${p}_enabled`]: String(v.enabled),
          [`${p}_max`]: v.max.trim(),
          [`${p}_download`]: String(v.download),
          [`${p}_upload`]: String(v.upload),
        },
      }),
    onSuccess: (res) => {
      qc.setQueryData(['settings'], res)
      qc.invalidateQueries({ queryKey: kind.key })
    },
  })

  const set = (patch: Partial<Prefs>) => {
    save.reset()
    setPrefs((p) => (p ? { ...p, ...patch } : p))
  }

  return (
    <Card title={title} subtitle={subtitle}>
      {isLoading || !prefs ? (
        error ? (
          <ErrorBanner error={error} />
        ) : (
          <Spinner />
        )
      ) : (
        <div className="space-y-3">
          <label className="flex items-center gap-2 text-sm text-ink">
            <input type="checkbox" checked={prefs.enabled} onChange={(e) => set({ enabled: e.target.checked })} />
            Allow sessions to be recorded
          </label>
          <div className="max-w-xs pl-6">
            <Field
              label="Number of sessions to keep"
              hint="1 to 100. Once this many are saved, delete one to save another. Lowering it deletes none."
            >
              <input
                className={inputClass}
                type="number"
                min={1}
                max={100}
                step={1}
                value={prefs.max}
                disabled={!prefs.enabled}
                onChange={(e) => set({ max: e.target.value })}
              />
            </Field>
          </div>
          <label className="flex items-center gap-2 text-sm text-ink">
            <input type="checkbox" checked={prefs.download} onChange={(e) => set({ download: e.target.checked })} />
            Allow recorded sessions to be downloaded (JSON)
          </label>
          <label className="flex items-center gap-2 text-sm text-ink">
            <input type="checkbox" checked={prefs.upload} onChange={(e) => set({ upload: e.target.checked })} />
            Allow recorded sessions to be uploaded
          </label>
          <p className="text-xs text-ink-muted">
            Turning recording off keeps the sessions already saved, to look at and delete; uploading also needs
            recording on.
          </p>
          <div className="flex items-center gap-3">
            <Button variant="primary" onClick={() => save.mutate(prefs)} disabled={save.isPending}>
              {save.isPending ? 'Saving…' : `Save ${title} settings`}
            </Button>
            {save.isSuccess && <span className="text-sm text-gain">Saved.</span>}
          </div>
          {save.isError && <ErrorBanner error={save.error} />}
          <div className="border-t border-[color:var(--hairline)] pt-4">
            <SavedSessionsList kind={kind} />
          </div>
        </div>
      )}
    </Card>
  )
}
