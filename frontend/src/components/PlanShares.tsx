/**
 * Sharing saved planner sessions, read-only: with another user, who sees
 * them under "Shared with me" in their session dropdown, or as a link that
 * anyone can open without signing in - a page with the planner alone. Each
 * share has its own set of sessions, changed here at any time; a link keeps
 * its URL. Revoking a share ends it at once. Only saved sessions are ever
 * shared, as they were saved; never the live plan.
 */
import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, Copy, Link2, Pencil, Trash2, User } from 'lucide-react'

import { api } from '../api/client'
import { dateTime, usePlanSessions, type PlanSessionListed } from './PlanSessions'
import { Badge, Button, ErrorBanner, Field, inputClass } from './ui'

export interface PlanShare {
  id: number
  kind: 'user' | 'link'
  recipient?: { name: string; email: string }
  /** A link's page: `/shared/<token>`. */
  path?: string
  label: string
  created_at: string
  expires_at?: string
  expired: boolean
  /** The saved sessions it shows, by id. */
  sessions: number[]
}

export interface PlanShareList {
  shares: PlanShare[]
  max: number
}

const SHARES_KEY = ['plan-shares']

export function usePlanShares(enabled = true) {
  return useQuery({ queryKey: SHARES_KEY, queryFn: () => api.get<PlanShareList>('/planner/shares'), enabled })
}

/** A link's whole address, to copy. */
export const linkUrl = (path: string) => `${window.location.origin}${path}`

/** A date input's value as the end of that day, local time, ISO; empty: no expiry. */
function endOfDay(day: string): string | null {
  if (!day) return null
  const d = new Date(`${day}T23:59:59`)
  return Number.isNaN(d.getTime()) ? null : d.toISOString()
}

/** An expiry as a date input's value. */
function dayOf(iso?: string): string {
  if (!iso) return ''
  const d = new Date(iso)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

const today = () => dayOf(new Date().toISOString())
const iconText = 'inline-flex items-center gap-1.5'

/** Copy text, saying so for a moment. */
function CopyButton({ text, label = 'Copy link' }: { text: string; label?: string }) {
  const [done, setDone] = useState(false)
  return (
    <Button
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(() => {
          setDone(true)
          setTimeout(() => setDone(false), 1500)
        })
      }}
      title={label}
    >
      <span className={iconText}>
        {done ? <Check size={14} aria-hidden /> : <Copy size={14} aria-hidden />}
        {done ? 'Copied' : label}
      </span>
    </Button>
  )
}

/** The user's saved planner sessions, to tick. */
function SessionPicker({
  sessions,
  picked,
  onChange,
}: {
  sessions: PlanSessionListed[]
  picked: Set<number>
  onChange: (next: Set<number>) => void
}) {
  if (sessions.length === 0)
    return <p className="text-sm text-ink-muted">No saved sessions yet: save one with the Save button first.</p>
  const toggle = (id: number) => {
    const next = new Set(picked)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    onChange(next)
  }
  return (
    <ul className="max-h-48 overflow-y-auto rounded-md border border-[color:var(--hairline)] px-2 py-1">
      {sessions.map((s) => (
        <li key={s.id}>
          <label className="flex items-center gap-2 py-1 text-sm">
            <input type="checkbox" checked={picked.has(s.id)} onChange={() => toggle(s.id)} />
            <span className="min-w-0 flex-1 truncate text-ink">{s.name}</span>
            <span className="shrink-0 text-xs text-ink-muted tabular">{dateTime(s.created_at)}</span>
          </label>
        </li>
      ))}
    </ul>
  )
}

/** Who may see a set of the user's saved sessions: people and links, made, changed and revoked. */
export function ShareDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const d = ref.current
    if (!d) return
    if (open && !d.open) d.showModal()
    else if (!open && d.open) d.close()
  }, [open])
  const { data } = usePlanShares(open)
  const sessions = usePlanSessions().data?.sessions ?? []
  const shares = data?.shares ?? []

  return (
    <dialog
      ref={ref}
      onClose={onClose}
      onClick={(e) => e.target === e.currentTarget && onClose()}
      aria-labelledby="share-plans-title"
      className="card m-auto w-[min(40rem,calc(100vw-2rem))] p-0 text-ink backdrop:bg-black/40"
    >
      {open && (
        <div className="max-h-[85vh] space-y-5 overflow-y-auto p-5">
          <div>
            <h2 id="share-plans-title" className="text-sm font-semibold text-ink">
              Share saved plans
            </h2>
            <p className="mt-1 text-xs text-ink-muted">
              Share saved sessions, read-only, with another user (they see them under “Shared with me”) or as a link
              anyone can open without signing in: a page with the planner alone. Only what a session saved is shared -
              never your current plan. Revoke a share to end it.
            </p>
          </div>
          <NewShare sessions={sessions} full={data ? shares.length >= data.max : false} />
          <div>
            <div className="text-xs font-medium text-ink-secondary">
              Your shares{data ? ` · ${shares.length} of ${data.max}` : ''}
            </div>
            {shares.length === 0 ? (
              <p className="mt-2 text-sm text-ink-muted">Nothing shared yet.</p>
            ) : (
              <ul className="mt-2 space-y-2">
                {shares.map((s) => (
                  <ShareRow key={s.id} share={s} sessions={sessions} />
                ))}
              </ul>
            )}
          </div>
          <div className="flex justify-end">
            <Button onClick={onClose}>Done</Button>
          </div>
        </div>
      )}
    </dialog>
  )
}

function NewShare({ sessions, full }: { sessions: PlanSessionListed[]; full: boolean }) {
  const qc = useQueryClient()
  const [kind] = useState<'user' | 'link'>('link')
  const [email, setEmail] = useState('')
  const [label, setLabel] = useState('')
  const [expires, setExpires] = useState('')
  const [picked, setPicked] = useState<Set<number>>(new Set())
  const [made, setMade] = useState<PlanShare | null>(null)

  const create = useMutation({
    mutationFn: () =>
      api.post<PlanShare>('/planner/shares', {
        kind,
        ...(kind === 'user' ? { email: email.trim() } : {}),
        sessions: [...picked],
        label: label.trim() || undefined,
        expires_at: endOfDay(expires) ?? undefined,
      }),
    onSuccess: (share) => {
      void qc.invalidateQueries({ queryKey: SHARES_KEY })
      setMade(share)
      setEmail('')
      setLabel('')
      setExpires('')
      setPicked(new Set())
    },
  })

  return (
    <form
      className="space-y-3 rounded-lg border border-[color:var(--hairline)] p-3"
      onSubmit={(e) => {
        e.preventDefault()
        setMade(null)
        create.mutate()
      }}
    >
      {(
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Label" hint="For you, to tell your links apart.">
            <input className={inputClass} value={label} maxLength={120} onChange={(e) => setLabel(e.target.value)} />
          </Field>
          <Field label="Expires" hint="Optional: it stops opening after this day.">
            <input className={inputClass} type="date" min={today()} value={expires} onChange={(e) => setExpires(e.target.value)} />
          </Field>
        </div>
      )}
      <div>
        <div className="mb-1 text-xs font-medium text-ink-secondary">Saved sessions to share</div>
        <SessionPicker sessions={sessions} picked={picked} onChange={setPicked} />
      </div>
      {create.isError && <ErrorBanner error={create.error} />}
      {made?.path && (
        <div className="flex flex-wrap items-center gap-2 rounded-md bg-status-good/10 p-2 text-sm">
          <span className="min-w-0 flex-1 break-all font-mono text-xs">{linkUrl(made.path)}</span>
          <CopyButton text={linkUrl(made.path)} />
        </div>
      )}
      {made && !made.path && (
        <p className="text-sm text-status-good">Shared with {made.recipient?.name || made.recipient?.email}.</p>
      )}
      <div className="flex justify-end">
        <Button
          type="submit"
          variant="primary"
          disabled={full || picked.size === 0 || (kind === 'user' && !email.trim()) || create.isPending}
          title={full ? 'As many shares as allowed: revoke one first' : undefined}
        >
          {create.isPending ? 'Sharing…' : kind === 'link' ? 'Make link' : 'Share'}
        </Button>
      </div>
    </form>
  )
}

function ShareRow({ share, sessions }: { share: PlanShare; sessions: PlanSessionListed[] }) {
  const qc = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const [picked, setPicked] = useState<Set<number>>(new Set(share.sessions))
  const [label, setLabel] = useState(share.label)
  const [expires, setExpires] = useState(dayOf(share.expires_at))

  const update = useMutation({
    mutationFn: () =>
      api.put<PlanShare>(`/planner/shares/${share.id}`, {
        sessions: [...picked],
        label: label.trim() || undefined,
        expires_at: endOfDay(expires),
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: SHARES_KEY })
      setEditing(false)
    },
  })
  const revoke = useMutation({
    mutationFn: () => api.del<PlanShareList>(`/planner/shares/${share.id}`),
    onSuccess: (list) => qc.setQueryData(SHARES_KEY, list),
  })

  const names = share.sessions.map((id) => sessions.find((s) => s.id === id)?.name).filter(Boolean)
  const title =
    share.kind === 'user'
      ? `${share.recipient?.name ?? ''}${share.recipient ? ` (${share.recipient.email})` : ''}`
      : share.label || 'Link'

  return (
    <li className="rounded-lg border border-[color:var(--hairline)] p-3">
      <div className="flex flex-wrap items-center gap-2">
        {share.kind === 'user' ? (
          <User size={14} className="text-ink-secondary" aria-hidden />
        ) : (
          <Link2 size={14} className="text-ink-secondary" aria-hidden />
        )}
        <span className="min-w-0 flex-1 truncate text-sm font-medium text-ink">{title}</span>
        {share.expired ? (
          <Badge tone="critical">expired</Badge>
        ) : (
          share.expires_at && <Badge>until {dateTime(share.expires_at)}</Badge>
        )}
        <Badge tone="info">
          {share.sessions.length} plan{share.sessions.length === 1 ? '' : 's'}
        </Badge>
      </div>
      <p className="mt-1 truncate text-xs text-ink-muted">{names.join(' · ') || 'No sessions left in it.'}</p>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        {share.path && <CopyButton text={linkUrl(share.path)} />}
        <Button onClick={() => setEditing((e) => !e)} title="Change which plans it shows">
          <span className={iconText}>
            <Pencil size={14} aria-hidden /> Change
          </span>
        </Button>
        <Button
          variant="danger"
          disabled={revoke.isPending}
          onClick={() => (confirming ? revoke.mutate() : setConfirming(true))}
        >
          <span className={iconText}>
            <Trash2 size={14} aria-hidden /> {confirming ? 'Yes, revoke' : 'Revoke'}
          </span>
        </Button>
        {confirming && (
          <>
            <Button onClick={() => setConfirming(false)}>Cancel</Button>
            <span className="text-xs text-status-critical">
              {share.kind === 'link' ? 'The link stops working at once.' : 'They stop seeing these plans at once.'}
            </span>
          </>
        )}
      </div>
      {revoke.isError && (
        <div className="mt-2">
          <ErrorBanner error={revoke.error} />
        </div>
      )}
      {editing && (
        <form
          className="mt-3 space-y-3 border-t border-[color:var(--hairline)] pt-3"
          onSubmit={(e) => {
            e.preventDefault()
            update.mutate()
          }}
        >
          {share.kind === 'link' && (
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Label">
                <input className={inputClass} value={label} maxLength={120} onChange={(e) => setLabel(e.target.value)} />
              </Field>
              <Field label="Expires" hint="Empty: until revoked.">
                <input className={inputClass} type="date" min={today()} value={expires} onChange={(e) => setExpires(e.target.value)} />
              </Field>
            </div>
          )}
          <SessionPicker sessions={sessions} picked={picked} onChange={setPicked} />
          {share.kind === 'link' && <p className="text-xs text-ink-muted">The link stays the same.</p>}
          {update.isError && <ErrorBanner error={update.error} />}
          <div className="flex justify-end gap-2">
            <Button onClick={() => setEditing(false)}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={picked.size === 0 || update.isPending}>
              {update.isPending ? 'Saving…' : 'Save'}
            </Button>
          </div>
        </form>
      )}
    </li>
  )
}
