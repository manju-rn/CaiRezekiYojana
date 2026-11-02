/**
 * Recorded sessions of the planner (RezekiYojana) and of CaiYatra: what
 * the page showed at a moment, kept on the server under a name. The
 * controls pick one to show (read-only), save what is shown as one and
 * upload one from a JSON download; a banner says when a saved session is on
 * show. The profile's cards delete them, and hold what the user allows.
 * `SessionKind` says whose: where they are kept and how they read.
 */
import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Download, History, Save, Trash2, Undo2, Upload } from 'lucide-react'

import { api } from '../api/client'
import { Badge, Button, ErrorBanner, inputClass, useConfirm } from './ui'

export interface PlanSessionListed {
  id: number
  name: string
  /** When it was first saved: UTC, without an offset. */
  created_at: string
  /** When it was uploaded from a downloaded file; absent: saved here. */
  uploaded_at?: string
}

export interface PlanSessionList {
  /** Newest first. */
  sessions: PlanSessionListed[]
  /** Sessions may be recorded at all. */
  enabled: boolean
  /** How many may be kept: the user's setting, 10 unless changed. */
  max: number
  download: boolean
  upload: boolean
}

/** A session as it rides in a JSON download, under `session`. */
export interface SessionPart {
  /** `cairezeki-plan-session` or `cairezeki-dhan-yatra-session`. */
  format: string
  version: 1
  name: string
  /** When it was first saved, ISO with its offset. */
  created_at: string
  /** A planner session's plan; none for CaiYatra. */
  plan?: unknown
  page: unknown
}

/** Which session a download is of: its name and when it was saved. */
export interface SessionId {
  name: string
  /** ISO with its offset. */
  created_at: string
}

/** Whose sessions: the planner's or CaiYatra's - where they are kept, what their files say, how they read. */
export interface SessionKind {
  path: string
  key: string[]
  /** What the page's JSON download says it is; the session rides in it under `session`. */
  pageFormat: string
  sessionFormat: string
  /** Its settings, before `_enabled`, `_max`, ... */
  prefix: string
  /** What an uploaded file must be, for messages. */
  fileNoun: string
  /** The dropdown's first entry. */
  live: string
  /** What saving keeps, in the save dialog. */
  keeps: string
  /** After saving: what downloading gives. */
  downloadOffer: string
  downloadLabel: string
  /** Under the recorded-session banner. */
  banner: string
  /** Who goes with a session: needs its plan. */
  needsPlan: boolean
  /** Sessions other users share are listed beside the user's own: the planner's. */
  sharing: boolean
}

export const PLANNER_SESSIONS: SessionKind = {
  path: '/planner/recorded',
  key: ['plan-sessions'],
  pageFormat: 'cairezeki-plan',
  sessionFormat: 'cairezeki-plan-session',
  prefix: 'plan_sessions',
  fileNoun: 'a plan downloaded from RezekiYojana as JSON',
  live: 'Current plan (live)',
  keeps: 'The plan and the values it is shown against are kept as they are now, read-only.',
  downloadOffer: 'You get the CSV and the JSON; the JSON uploads back as this session and opens in the viewer.',
  downloadLabel: 'Download CSV and JSON',
  banner: 'The plan and every value below are as they were when this session was saved. It cannot be changed; your current plan is untouched.',
  needsPlan: true,
  sharing: true,
}

export const YATRA_SESSIONS: SessionKind = {
  path: '/dhan-yatra/recorded',
  key: ['yatra-sessions'],
  pageFormat: 'cairezeki-dhan-yatra',
  sessionFormat: 'cairezeki-dhan-yatra-session',
  prefix: 'yatra_sessions',
  fileNoun: 'a CaiYatra session downloaded as JSON',
  live: 'Current (live)',
  keeps: 'Every setting and the portfolio it starts from are kept as they are now, read-only.',
  downloadOffer: 'The JSON holds the whole page; it uploads back, here or without signing in.',
  downloadLabel: 'Download JSON',
  banner: 'Every setting and figure below is as it was when this session was saved. It cannot be changed; your current settings are untouched.',
  needsPlan: false,
  sharing: false,
}

export const PLAN_SESSIONS_KEY = PLANNER_SESSIONS.key

/** A server timestamp - UTC, written without an offset or with one - as a Date. */
function utc(iso: string): Date {
  return new Date(/[zZ]$|[+-]\d\d:?\d\d$/.test(iso) ? iso : `${iso}Z`)
}

/** A server timestamp as ISO with its offset. */
export const utcIso = (iso: string) => utc(iso).toISOString()

/** Two timestamps to the same second: the same session. */
const sameMoment = (a: string, b: string) => Math.floor(utc(a).getTime() / 1000) === Math.floor(utc(b).getTime() / 1000)

/** A server timestamp in local time, to the minute. */
export function dateTime(iso: string): string {
  const d = utc(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString('en-GB', {
    day: '2-digit',
    month: 'short',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

/** A moment in local time, for a file name: `2026-10-18_14-32-05`. Now, when none is given. */
export function fileStamp(iso?: string): string {
  const d = iso ? utc(iso) : new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}_${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}`
}

/** A session's file name, without the extension: its name, then when it was saved. */
export function fileBase(s: SessionId): string {
  const safe = s.name.trim().replace(/[^\p{L}\p{N}_-]/gu, '_') || 'plan-session'
  return `${safe}_${fileStamp(s.created_at)}`
}

export function usePlanSessions(kind: SessionKind = PLANNER_SESSIONS, enabled = true) {
  return useQuery({
    queryKey: kind.key,
    queryFn: () => api.get<PlanSessionList>(kind.path),
    enabled,
  })
}

/** A saved planner session another user shares with this one. */
export interface SharedSessionListed {
  id: number
  name: string
  created_at: string
  /** Whose it is: their name. */
  owner_name: string
}

export const SHARED_WITH_ME_KEY = ['plan-shared-with-me']

/** The saved planner sessions other users share with this one, newest first. */
export function useSharedWithMe(enabled = true) {
  return useQuery({
    queryKey: SHARED_WITH_ME_KEY,
    queryFn: () => api.get<{ sessions: SharedSessionListed[] }>('/planner/shared'),
    enabled,
  })
}

/** The saved session on show, if one is. */
export interface Recorded {
  id: number
  name: string
  created_at: string
  uploaded_at?: string
  /** Someone else's, shared with whoever is looking: its owner's name. */
  owner_name?: string
}

/** Which saved session to show: one's own, or one shared by someone else. */
export interface SessionRef {
  id: number
  shared: boolean
}

/** The session in an uploaded file: the page's JSON download, or one from the API. */
export function sessionIn(kind: SessionKind, doc: unknown): SessionPart | null {
  const d = doc as { format?: string; session?: SessionPart } | null
  const s = d?.format === kind.pageFormat ? d.session : (d as SessionPart | null)
  return s?.format === kind.sessionFormat && s.version === 1 && s.created_at && s.page && (s.plan || !kind.needsPlan) ? s : null
}

const iconText = 'inline-flex items-center gap-1.5'

/**
 * The session controls of a page - the planner's toolbar, CaiYatra's
 * Settings: which session to show, save what is shown as one, upload one;
 * with `onDownloadShown`, download the saved session on show.
 */
export function SessionControls({
  kind = PLANNER_SESSIONS,
  compact = false,
  recorded,
  onOpen,
  snapshot,
  onDownload,
  onDownloadShown,
}: {
  kind?: SessionKind
  /** For a narrow header: no "Session" label, a narrower dropdown. */
  compact?: boolean
  recorded: Recorded | null
  /** Show a saved session - with `shared`, one another user shares - or `null` for the current plan. */
  onOpen: (id: number | null, shared?: boolean) => void
  /** What to save: the page's values (and the planner's plan); `null` while those are still loading. */
  snapshot: () => { plan?: unknown; page: unknown } | null
  /** Download the session just saved. */
  onDownload: (s: SessionId) => void
  /** Download the saved session on show, if the page offers that here. */
  onDownloadShown?: () => void
}) {
  const qc = useQueryClient()
  const { data } = usePlanSessions(kind)
  const sessions = data?.sessions ?? []
  const shared = useSharedWithMe(kind.sharing).data?.sessions ?? []
  const enabled = data?.enabled ?? true
  const max = data?.max ?? 10
  const canDownload = data?.download ?? true
  const canUpload = enabled && (data?.upload ?? true)
  const full = sessions.length >= max
  const [saving, setSaving] = useState(false)
  const [uploadError, setUploadError] = useState<unknown>(null)
  const fileInput = useRef<HTMLInputElement>(null)
  const { confirm, dialog: confirmDialog } = useConfirm()

  const upload = useMutation({
    mutationFn: ({ doc, overwrite }: { doc: unknown; part: SessionPart; overwrite: boolean }) =>
      api.post<PlanSessionList>(`${kind.path}/import${overwrite ? '?overwrite=true' : ''}`, doc),
    onSuccess: (list, { part }) => {
      qc.setQueryData(kind.key, list)
      // Show what was uploaded, as a saved session is shown.
      const made = list.sessions.find((s) => sameMoment(s.created_at, part.created_at))
      if (made) onOpen(made.id)
    },
    onError: setUploadError,
  })

  const readFile = async (file: File) => {
    setUploadError(null)
    let doc: unknown
    try {
      doc = JSON.parse(await file.text())
    } catch {
      setUploadError(`${file.name} is not JSON.`)
      return
    }
    const part = sessionIn(kind, doc)
    if (!part) {
      setUploadError(`${file.name} is not ${kind.fileNoun}.`)
      return
    }
    const kept = sessions.find((s) => sameMoment(s.created_at, part.created_at))
    if (kept) {
      confirm({
        title: 'Overwrite the saved session?',
        body: `A session saved at ${dateTime(part.created_at)} is kept already: “${kept.name}”. Replace it with the one in ${file.name}?`,
        confirmLabel: 'Overwrite',
        onConfirm: () => upload.mutate({ doc, part, overwrite: true }),
      })
    } else if (full) {
      setUploadError(`You keep ${sessions.length} of ${max} saved sessions: delete one, or allow more, in your profile.`)
    } else upload.mutate({ doc, part, overwrite: false })
  }

  if (!enabled && sessions.length === 0 && shared.length === 0 && !recorded) return null

  return (
    <div className="flex flex-wrap items-center gap-2">
      <label className="flex items-center gap-2 text-sm">
        {!compact && <span className="text-xs font-medium text-ink-secondary">Session</span>}
        <select
          className={`${inputClass} w-auto py-1 ${compact ? 'max-w-[10rem] text-xs' : 'max-w-[20rem]'}`}
          title="Session to show: the current one, or a saved one (read-only)"
          value={recorded ? `${recorded.owner_name !== undefined ? 's' : ''}${recorded.id}` : ''}
          onChange={(e) => {
            const v = e.target.value
            if (!v) onOpen(null)
            else if (v.startsWith('s')) onOpen(Number(v.slice(1)), true)
            else onOpen(Number(v))
          }}
          aria-label="Plan session to show"
        >
          <option value="">{kind.live}</option>
          {sessions.length > 0 && (
            <optgroup label="Saved sessions - read-only">
              {sessions.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name} · {dateTime(s.created_at)}
                  {s.uploaded_at ? ' · uploaded' : ''}
                </option>
              ))}
            </optgroup>
          )}
          {shared.length > 0 && (
            <optgroup label="Shared with me - read-only">
              {shared.map((s) => (
                <option key={`s${s.id}`} value={`s${s.id}`}>
                  {s.name} · {s.owner_name} · {dateTime(s.created_at)}
                </option>
              ))}
            </optgroup>
          )}
        </select>
      </label>
      {enabled && !recorded && (
        <Button
          onClick={() => setSaving(true)}
          disabled={full}
          title={full ? `Save session - ${max} of ${max} kept: delete one in your profile first` : `Save session (${sessions.length} of ${max} kept)`}
        >
          <Save size={14} aria-hidden />
        </Button>
      )}
      {recorded && onDownloadShown && canDownload && (
        <Button onClick={onDownloadShown} title="Download this saved session as JSON">
          <Download size={14} aria-hidden />
        </Button>
      )}
      {canUpload && (
        <>
          <Button
            onClick={() => fileInput.current?.click()}
            disabled={upload.isPending}
            title={`Upload a session: ${kind.fileNoun}`}
          >
            <Upload size={14} aria-hidden />
          </Button>
          <input
            ref={fileInput}
            type="file"
            accept="application/json,.json"
            className="hidden"
            onChange={(e) => {
              const f = e.target.files?.[0]
              e.target.value = ''
              if (f) void readFile(f)
            }}
          />
        </>
      )}
      {uploadError != null && (
        <div className="w-full">
          <ErrorBanner error={uploadError} />
        </div>
      )}
      {confirmDialog}
      <SaveDialog
        kind={kind}
        open={saving}
        snapshot={snapshot}
        canDownload={canDownload}
        onDownload={onDownload}
        onClose={() => setSaving(false)}
      />
    </div>
  )
}

/** Says, unmistakably, that a saved session is on show, with the way back. */
export function RecordedBanner({
  kind = PLANNER_SESSIONS,
  recorded,
  onBack,
}: {
  kind?: SessionKind
  recorded: Recorded
  /** The way back to the current plan; none where there is no plan of one's own (a shared link). */
  onBack?: () => void
}) {
  const shared = recorded.owner_name !== undefined
  return (
    <div
      role="status"
      className="flex flex-wrap items-center justify-center gap-x-4 gap-y-2 rounded-lg border-2 border-status-warning/60 bg-status-warning/10 px-4 py-3"
    >
      <div className="flex flex-wrap items-center gap-2 text-sm text-ink">
        <History size={16} className="text-status-warning" aria-hidden />
        <Badge tone="warning">{shared ? 'SHARED PLAN · READ-ONLY' : 'RECORDED SESSION · READ-ONLY'}</Badge>
        <span className="font-semibold">{recorded.name}</span>
        <span className="text-ink-secondary">
          {shared && `shared by ${recorded.owner_name} · `}
          saved {dateTime(recorded.created_at)}
          {recorded.uploaded_at && ` · uploaded ${dateTime(recorded.uploaded_at)}`}
        </span>
      </div>
      {onBack && (
        <Button variant="primary" onClick={onBack}>
          <span className={iconText}>
            <Undo2 size={14} aria-hidden /> Back to current plan
          </span>
        </Button>
      )}
      <p className="w-full text-center text-xs text-ink-secondary">
        {shared
          ? `The plan and every value below are as ${recorded.owner_name} saved them. It cannot be changed here.`
          : kind.banner}
      </p>
    </div>
  )
}

/** A modal dialog, open while `open`; Escape or a click outside closes it. */
function useModal(open: boolean, onOpen: () => void) {
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const d = ref.current
    if (!d) return
    if (open && !d.open) {
      onOpen()
      d.showModal()
    } else if (!open && d.open) d.close()
  }, [open])
  return ref
}

/** Name the session and save it; then offer it for download. */
function SaveDialog({
  kind,
  open,
  snapshot,
  canDownload,
  onDownload,
  onClose,
}: {
  kind: SessionKind
  open: boolean
  snapshot: () => { plan?: unknown; page: unknown } | null
  canDownload: boolean
  onDownload: (s: SessionId) => void
  onClose: () => void
}) {
  const qc = useQueryClient()
  const [name, setName] = useState('')
  const [saved, setSaved] = useState<SessionId | null>(null)
  const ref = useModal(open, () => {
    setName(`Plan ${dateTime(new Date().toISOString())}`)
    setSaved(null)
    save.reset()
  })

  const save = useMutation({
    mutationFn: (n: string) => {
      const snap = snapshot()
      if (!snap) throw new Error('The page is still loading; try again in a moment.')
      return api.post<PlanSessionList>(kind.path, { name: n, ...snap })
    },
    onSuccess: (list, n) => {
      qc.setQueryData(kind.key, list)
      const made = list.sessions.find((s) => s.name === n)
      setSaved({ name: n, created_at: made ? utcIso(made.created_at) : new Date().toISOString() })
    },
  })

  return (
    <dialog
      ref={ref}
      onClose={onClose}
      onClick={(e) => e.target === e.currentTarget && onClose()}
      aria-labelledby="save-session-title"
      className="card m-auto w-[min(28rem,calc(100vw-2rem))] p-0 text-ink backdrop:bg-black/40"
    >
      <div className="p-5">
        <h2 id="save-session-title" className="text-sm font-semibold text-ink">
          {saved ? 'Session saved' : 'Save session'}
        </h2>
        {saved ? (
          <>
            <p className="mt-2 text-sm text-ink-secondary">
              “{saved.name}” is saved, {dateTime(saved.created_at)}.
              {canDownload && ` Download it to disk as well? ${kind.downloadOffer}`}
            </p>
            <div className="mt-5 flex justify-end gap-2">
              <Button onClick={onClose}>{canDownload ? 'No, thanks' : 'Done'}</Button>
              {canDownload && (
                <Button
                  variant="primary"
                  onClick={() => {
                    onDownload(saved)
                    onClose()
                  }}
                >
                  <span className={iconText}>
                    <Download size={14} aria-hidden /> {kind.downloadLabel}
                  </span>
                </Button>
              )}
            </div>
          </>
        ) : (
          <form
            onSubmit={(e) => {
              e.preventDefault()
              if (name.trim()) save.mutate(name.trim())
            }}
          >
            <p className="mt-1 text-xs text-ink-muted">
              {kind.keeps}
            </p>
            <input
              className={`${inputClass} mt-3`}
              value={name}
              maxLength={120}
              autoFocus
              onFocus={(e) => e.target.select()}
              onChange={(e) => setName(e.target.value)}
              aria-label="Name for the saved session"
            />
            {save.isError && (
              <div className="mt-3">
                <ErrorBanner error={save.error} />
              </div>
            )}
            <div className="mt-5 flex justify-end gap-2">
              <Button onClick={onClose}>Cancel</Button>
              <Button type="submit" variant="primary" disabled={!name.trim() || save.isPending}>
                {save.isPending ? 'Saving…' : 'Save'}
              </Button>
            </div>
          </form>
        )}
      </div>
    </dialog>
  )
}

/** A page's saved sessions, to tick and delete: one, several or all. In the profile's cards. */
export function SavedSessionsList({ kind = PLANNER_SESSIONS }: { kind?: SessionKind }) {
  const qc = useQueryClient()
  const { data, isLoading } = usePlanSessions(kind)
  const sessions = data?.sessions ?? []
  const [ticked, setTicked] = useState<Set<number>>(new Set())
  const [confirming, setConfirming] = useState(false)

  const remove = useMutation({
    mutationFn: async (ids: number[]) => {
      // One at a time; what is gone stays gone if a later one fails.
      for (const id of ids) await api.del<PlanSessionList>(`${kind.path}/${id}`)
      return ids
    },
    onSettled: () => qc.invalidateQueries({ queryKey: kind.key }),
    onSuccess: () => {
      setTicked(new Set())
      setConfirming(false)
    },
  })

  const toggle = (id: number) => {
    setConfirming(false)
    setTicked((s) => {
      const next = new Set(s)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }
  const all = sessions.length > 0 && sessions.every((s) => ticked.has(s.id))

  if (isLoading) return null
  return (
    <div>
      <div className="text-xs font-medium text-ink-secondary">
        Saved sessions · {sessions.length} of {data?.max ?? 10} kept
      </div>
      {sessions.length === 0 ? (
        <p className="mt-2 text-sm text-ink-muted">None saved.</p>
      ) : (
        <>
          <label className="mt-2 flex items-center gap-2 border-b border-[color:var(--hairline)] pb-2 text-xs font-medium text-ink-secondary">
            <input
              type="checkbox"
              checked={all}
              onChange={() => {
                setConfirming(false)
                setTicked(all ? new Set() : new Set(sessions.map((s) => s.id)))
              }}
            />
            Select all
          </label>
          <ul className="max-h-72 overflow-y-auto">
            {sessions.map((s) => (
              <li key={s.id}>
                <label className="flex items-center gap-2 py-1.5 text-sm">
                  <input type="checkbox" checked={ticked.has(s.id)} onChange={() => toggle(s.id)} />
                  <span className="min-w-0 flex-1 truncate font-medium text-ink">{s.name}</span>
                  <span className="shrink-0 text-xs text-ink-muted tabular">
                    {dateTime(s.created_at)}
                    {s.uploaded_at && ' · uploaded'}
                  </span>
                </label>
              </li>
            ))}
          </ul>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <Button
              variant="danger"
              disabled={ticked.size === 0 || remove.isPending}
              onClick={() => (confirming ? remove.mutate([...ticked]) : setConfirming(true))}
            >
              <span className={iconText}>
                <Trash2 size={14} aria-hidden />
                {remove.isPending
                  ? 'Deleting…'
                  : confirming
                    ? `Yes, delete ${ticked.size}`
                    : `Delete selected${ticked.size ? ` (${ticked.size})` : ''}`}
              </span>
            </Button>
            {confirming && (
              <>
                <Button onClick={() => setConfirming(false)}>Cancel</Button>
                <span className="text-xs text-status-critical">This cannot be undone.</span>
              </>
            )}
          </div>
          {remove.isError && (
            <div className="mt-3">
              <ErrorBanner error={remove.error} />
            </div>
          )}
        </>
      )}
    </div>
  )
}
