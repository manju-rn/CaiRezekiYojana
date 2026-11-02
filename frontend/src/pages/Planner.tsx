/**
 * RezekiYojana: plan where the money goes next. Today's allocation on the
 * left, the plan on the right. Drag a category (or fresh money) onto the
 * plan, choose an existing category or a new one with its classification,
 * say how much, and the tables underneath show what changed and by how much.
 *
 * A plan is the user's own scratchpad, always worked out against today's
 * values. It is kept on the server, one per view, so the API and the MCP
 * tools change the same plan; the page polls and shows their changes.
 */
import { Fragment, useEffect, useMemo, useState, type DragEvent, type ReactNode } from 'react'
import { useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  ArrowRight,
  ChevronDown,
  ChevronRight,
  ExternalLink,
  GripVertical,
  Plus,
  RotateCcw,
  Share2,
  Trash2,
  Undo2,
  X,
} from 'lucide-react'

import { ApiError, api } from '../api/client'
import type { CashAccount, Deposit, NetWorth, PortfolioView, Position, Segment } from '../api/types'
import { PageTitle } from '../components/PageTitle'
import { MoneyInput } from '../components/planning'
import { ShareDialog } from '../components/PlanShares'
import {
  RecordedBanner,
  SessionControls,
  dateTime,
  fileBase,
  fileStamp,
  usePlanSessions,
  utcIso,
  type Recorded,
  type SessionId,
  type SessionPart,
  type SessionRef,
  type SharedSessionListed,
} from '../components/PlanSessions'
import { Badge, Button, Card, EmptyState, ErrorBanner, Field, Spinner, ToggleGroup, inputClass, useConfirm } from '../components/ui'
import { currencySymbol, date, deltaClass, humanise, money, num, pct, signedMoney } from '../lib/format'

/** The currency the page reports in, and what one unit of the base is worth in it. */
interface Fx {
  code: string
  rate: number
}

const fxMoney = (v: number, fx: Fx) => money(v * fx.rate, fx.code, { whole: true })
const fxSigned = (v: number, fx: Fx) => signedMoney(v * fx.rate, fx.code, { whole: true })
/** A base amount as typed in the display currency, to the cent, so an input reads back what was typed. */
const inDisplay = (v: number, fx: Fx) => Math.round(v * fx.rate * 100) / 100

/** Where plan-viewer.html keeps the plan it shows. */
const VIEWER_STORE = 'cairezeki-plan-viewer'
/** Where it keeps one opened from a shared link: a viewer without downloads or uploads. */
const VIEWER_SHARED_STORE = 'cairezeki-plan-viewer-shared'

/** Which split of the portfolio the plan works on. */
type View = 'segments' | 'classes'

/** The source key for money not yet invested. */
const FRESH = '__fresh__'

/**
 * Where a category sits on the equity-debt line. The first five are the
 * dashboard's own classes (same keys); gold and real estate are for plans
 * that reach past what is held today.
 */
const CLASSES = [
  { key: 'equity_fund', label: 'Equity – ETF/Fund', group: 'Equity' },
  { key: 'equity_stock', label: 'Equity – Stocks', group: 'Equity' },
  { key: 'debt_high', label: 'Debt – High Yield', group: 'Debt – High Yield' },
  { key: 'debt_low', label: 'Debt – Low Yield (Cash)', group: 'Debt – Low Yield' },
  { key: 'gold', label: 'Gold / Commodities', group: 'Gold / Commodities' },
  { key: 'real_estate', label: 'Real estate', group: 'Real estate' },
  { key: 'other', label: 'Other', group: 'Other' },
] as const
type ClassKey = (typeof CLASSES)[number]['key']
const classLabel = (k: string) => CLASSES.find((c) => c.key === k)?.label ?? k

interface Bucket {
  key: string
  label: string
}

interface Created extends Bucket {
  cls: ClassKey
}

interface Move {
  id: string
  from: string
  /** The fund or account within `from` it is taken out of; absent: the category as a whole. */
  from_sub?: string
  to: string
  amount: number
  /** The fund or account within `to` it merges into, or a new one; absent: the category as a whole. */
  to_sub?: string
  /** Why: shown in the decision list and the CSV. */
  note: string
  at: string
}

/** A subcategory made in the plan: a fund or holding not held today. */
interface NewSub {
  key: string
  bucket: string
  label: string
  cls: ClassKey
}

interface Plan {
  /** Categories made on the right that do not exist today. */
  created: Created[]
  /** Subcategories made on the right, in any category. */
  subs: NewSub[]
  moves: Move[]
  /** Money to be invested on top of today's portfolio, in base currency. */
  fresh: number
  /** Today's categories deleted from the plan once emptied. */
  removed: string[]
  /** A classification chosen for one of today's categories, over the one it has. */
  cls: Record<string, ClassKey>
}

const EMPTY_PLAN: Plan = { created: [], subs: [], moves: [], fresh: 0, removed: [], cls: {} }

/** A plan as the server holds it, and the revision it is at. */
interface PlanState {
  view: View
  revision: number
  updated_at: string | null
  plan: Plan
}

/**
 * What a saved session keeps beside the plan: the values the page showed it
 * against, so it reads later exactly as it did when saved.
 */
interface SessionPage {
  version: 1
  data: NetWorth
  segSubs: Record<string, Sub[]>
  clsSubs: Record<string, Sub[]>
}

/** A saved session, as the server hands it back. */
interface RecordedSession extends Recorded {
  plan: Plan
  page: SessionPage
}

/** How often the page looks for a change made elsewhere: the API, an assistant over MCP, another window. */
const POLL_MS = 4000
/** How long after the last edit the plan is saved, so typing an amount is one save, not one a keystroke. */
const SAVE_DELAY_MS = 400

/** A plan as read, with the defaults filled in. Plans this browser kept before had `fromSub` and `toSub`. */
type LegacyMove = Move & { fromSub?: string; toSub?: string }

function normalise(p: Omit<Partial<Plan>, 'moves'> & { moves?: LegacyMove[] }): Plan {
  return {
    created: (p.created ?? []).map((c) => ({ ...c, cls: c.cls ?? 'other' })),
    subs: p.subs ?? [],
    moves: (p.moves ?? []).map(({ fromSub, toSub, ...m }) => ({
      ...m,
      from_sub: m.from_sub ?? fromSub,
      to_sub: m.to_sub ?? toSub,
    })),
    fresh: p.fresh ?? 0,
    removed: p.removed ?? [],
    cls: p.cls ?? {},
  }
}

/** Where this browser kept a plan before plans moved to the server. */
const legacyKey = (view: View) => `cairezeki-plan-${view}`

/** A plan this browser kept before plans moved to the server, if it has anything in it. */
function legacyPlan(view: View): Plan | null {
  try {
    const raw = localStorage.getItem(legacyKey(view))
    if (!raw) return null
    const p = normalise(JSON.parse(raw))
    return p.moves.length || p.created.length || p.subs.length || p.removed.length || p.fresh ? p : null
  } catch {
    return null
  }
}

/**
 * The plan for a view, kept on the server - the same plan the API and the
 * MCP tools read and change. An edit shows at once and is saved a moment
 * later against the revision it was made on; in between, the page polls, so
 * a change made elsewhere shows within seconds. A save the server refuses
 * because the plan moved on meanwhile is dropped, and the plan as it now is
 * shown, with a word to say so.
 */
function useStoredPlan(view: View) {
  const qc = useQueryClient()
  const [draft, setDraft] = useState<{ view: View; base: number; plan: Plan } | null>(null)
  const [saving, setSaving] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)

  const query = useQuery({
    queryKey: ['planner', view],
    queryFn: async () => {
      const next = await api.get<PlanState>(`/planner/${view}`)
      // A poll sent before a save answered must not wind the plan back.
      const held = qc.getQueryData<PlanState>(['planner', view])
      return held && held.revision > next.revision ? held : { ...next, plan: normalise(next.plan) }
    },
    refetchInterval: draft || saving ? false : POLL_MS,
    refetchOnWindowFocus: true,
  })
  const server = query.data

  // A plan from before plans were kept on the server moves there once,
  // unless the server already has one.
  useEffect(() => {
    if (!server || draft) return
    const legacy = legacyPlan(view)
    if (legacy && server.revision === 0) setDraft({ view, base: 0, plan: legacy })
    else localStorage.removeItem(legacyKey(view))
  }, [server, view])

  useEffect(() => {
    if (!draft || saving) return
    const sent = draft
    const timer = setTimeout(() => {
      setSaving(true)
      api
        .put<PlanState>(`/planner/${sent.view}`, { plan: sent.plan, revision: sent.base })
        .then((saved) => {
          qc.setQueryData(['planner', sent.view], { ...saved, plan: normalise(saved.plan) })
          localStorage.removeItem(legacyKey(sent.view))
          // Edits made while this was on its way are saved next, on top of it.
          setDraft((d) => (d === sent ? null : d && d.view === sent.view ? { ...d, base: saved.revision } : d))
        })
        .catch((e: unknown) => {
          setDraft((d) => (d?.view === sent.view ? null : d))
          setNotice(
            e instanceof ApiError && e.status === 409
              ? 'This plan was changed elsewhere - through the API, an assistant or another window - so your last change was not saved. It now shows the plan as it is.'
              : `Your last change could not be saved: ${e instanceof Error ? e.message : String(e)}`,
          )
          qc.invalidateQueries({ queryKey: ['planner', sent.view] })
        })
        .finally(() => setSaving(false))
    }, SAVE_DELAY_MS)
    return () => clearTimeout(timer)
  }, [draft, saving])

  const plan = draft?.view === view ? draft.plan : (server?.plan ?? EMPTY_PLAN)
  const setPlan = (fn: (p: Plan) => Plan) =>
    setDraft((d) =>
      d?.view === view
        ? { ...d, plan: fn(d.plan) }
        : { view, base: server?.revision ?? 0, plan: fn(server?.plan ?? EMPTY_PLAN) },
    )

  return {
    plan,
    setPlan,
    isLoading: query.isLoading,
    error: query.error,
    status: draft || saving ? 'Saving…' : server?.updated_at ? `Saved ${savedAt(server.updated_at)}` : null,
    notice,
    dismiss: () => setNotice(null),
  }
}

/** When a plan was last saved: the time today, else the date. */
function savedAt(iso: string): string {
  const d = new Date(iso)
  return d.toDateString() === new Date().toDateString()
    ? d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
    : date(iso.slice(0, 10))
}

/**
 * A move being set up. `from` null: the source is still to choose (Add money).
 * `to` a key: that category; '' an existing one still to choose; null a new one.
 * `remove`: the source is being deleted, so everything in it moves.
 */
interface Pending {
  /** A source: a category key, `category|fund`, or fresh money. */
  from: string | null
  to: string | null
  remove?: boolean
}

type Status = 'new' | 'added' | 'reduced' | 'exited' | 'unchanged'

const STATUS: Record<Status, { label: string; tone: 'neutral' | 'good' | 'warning' | 'critical' | 'info' }> = {
  new: { label: 'New', tone: 'info' },
  added: { label: 'Added', tone: 'good' },
  reduced: { label: 'Reduced', tone: 'warning' },
  exited: { label: 'Exited', tone: 'critical' },
  unchanged: { label: 'Unchanged', tone: 'neutral' },
}

/** Below this, an amount is rounding noise. */
const EPS = 0.5

/**
 * One fund, stock, deposit or bank account inside a category. `cls` is a
 * class key, or `mixed` for a hybrid fund the dashboard splits by its
 * disclosed portfolio.
 */
interface Sub {
  key: string
  label: string
  detail: string
  cls: string
  value: number
  /** Keys this line used to go by: each deposit's own, from before they were grouped by institution. */
  aliases?: string[]
}

/** A bank's deposits, as one line. `|` separates a category from its line, so it cannot be in a key. */
const bankKey = (institution: string) => `b:${institution.replace(/\|/g, '/')}`

/**
 * Moves planned out of (or into) one deposit, from before deposits were
 * grouped, onto its bank's line. Null when none needs it.
 */
function regroupDeposits(plan: Plan, subs: Sub[] | undefined): Plan | null {
  const to: Record<string, string> = {}
  for (const s of subs ?? []) for (const a of s.aliases ?? []) to[a] = s.key
  const remap = (sub: string | undefined) => (sub && to[sub]) || sub
  if (!plan.moves.some((m) => remap(m.from_sub) !== m.from_sub || remap(m.to_sub) !== m.to_sub)) return null
  const moves = plan.moves
    .map((m) => ({ ...m, from_sub: remap(m.from_sub), to_sub: remap(m.to_sub) }))
    // One deposit into another at the same bank is now a bank into itself: nothing moves.
    .filter((m) => !(m.from === m.to && m.from_sub === m.to_sub))
  return { ...plan, moves }
}

const subClassLabel = (cls: string) => (cls === 'mixed' ? 'Hybrid / mixed' : classLabel(cls))

const isIndia = (p: Position) => (p.country ?? 'IN').toUpperCase() === 'IN'

/** The dashboard's segment for a holding: shares together, funds by domicile. */
function segmentOf(p: Position): string {
  if (p.kind === 'MUTUAL_FUND' && isIndia(p)) return 'india_mf'
  if ((p.kind === 'ETF' || p.kind === 'MUTUAL_FUND') && !isIndia(p)) return 'global_etf'
  if (p.kind === 'EQUITY') return 'stocks'
  return 'other'
}

/**
 * A holding's place on the equity-debt line, as the dashboard decides it
 * where it can from the holding alone. A hybrid Indian fund is `mixed`: the
 * dashboard splits it by a portfolio disclosure this page does not load.
 */
function classOfPosition(p: Position): string {
  switch (p.kind) {
    case 'EQUITY':
      return 'equity_stock'
    case 'BOND':
      return 'debt_high'
    case 'CASH':
      return 'debt_low'
    case 'REIT':
      return 'equity_fund'
    case 'COMMODITY':
      return 'gold'
  }
  // Gold, silver and other commodity funds, by name or category: as the dashboard tells them.
  const commodity = /gold|silver|bullion|commodit|precious metal/i.test(`${p.name} ${p.fund_category ?? ''}`)
  if (p.kind === 'MUTUAL_FUND' && isIndia(p)) {
    if (p.fund_class === 'Equity') return 'equity_fund'
    if (p.fund_class === 'Debt') return 'debt_high'
    return commodity ? 'gold' : 'mixed'
  }
  if (p.kind === 'ETF' || p.kind === 'MUTUAL_FUND') return commodity ? 'gold' : 'equity_fund'
  return 'other'
}

const HOLDING_SCOPES = ['india_mf', 'india_equity', 'global'] as const

/**
 * Every fund, stock, deposit and bank account, grouped under the category
 * it sits in for this view; undefined while still loading. Shares the
 * holdings, deposits and cash pages' caches.
 */
function useSubcategories(view: View, excluded: string[], enabled = true): Record<string, Sub[]> | undefined {
  const results = useQueries({
    queries: [
      ...HOLDING_SCOPES.map((scope) => ({
        queryKey: ['holdings', scope, false],
        queryFn: () => api.get<PortfolioView>('/holdings', { scope, include_closed: false }),
        enabled: enabled && !excluded.includes(scope),
      })),
      {
        queryKey: ['deposits'],
        queryFn: () => api.get<{ deposits: Deposit[] }>('/deposits'),
        enabled: enabled && !excluded.includes('deposits'),
      },
      {
        queryKey: ['cash'],
        queryFn: () => api.get<{ accounts: CashAccount[] }>('/cash'),
        enabled: enabled && !excluded.includes('cash'),
      },
    ],
  })
  const loading = results.some((r) => r.isLoading)
  const stamp = results.map((r) => r.dataUpdatedAt).join(',')

  return useMemo(() => {
    if (loading) return undefined
    const [mf, eq, gl, dep, cash] = results.map((r) => r.data) as [
      PortfolioView | undefined,
      PortfolioView | undefined,
      PortfolioView | undefined,
      { deposits: Deposit[] } | undefined,
      { accounts: CashAccount[] } | undefined,
    ]
    const out: Record<string, Sub[]> = {}
    const add = (segment: string, sub: Sub) => {
      if (sub.value <= EPS) return
      // In the class view a holding sits under its class. A hybrid fund is
      // split across several, so it is left to the "not itemised" line.
      const bucket = view === 'segments' ? segment : sub.cls
      if (bucket === 'mixed') return
      const list = (out[bucket] ??= [])
      const same = list.find((s) => s.key === sub.key)
      if (same) {
        same.value += sub.value
        if (!same.detail.includes(sub.detail)) same.detail += `, ${sub.detail}`
      } else list.push({ ...sub })
    }
    for (const view of [mf, eq, gl]) {
      for (const p of view?.positions ?? []) {
        add(segmentOf(p), {
          key: `i:${p.instrument_id}`,
          label: p.name,
          detail: p.fund_category ?? p.sector ?? p.account_name,
          cls: classOfPosition(p),
          value: p.market_value_base ?? 0,
        })
      }
    }
    // Deposits by institution: one line a bank, however many deposits it holds.
    const banks = new Map<string, Deposit[]>()
    for (const d of dep?.deposits ?? []) {
      if (d.current_value_base <= EPS) continue
      const list = banks.get(d.institution)
      if (list) list.push(d)
      else banks.set(d.institution, [d])
    }
    for (const [institution, ds] of banks) {
      const rates = ds.map((d) => d.interest_rate)
      const lo = Math.min(...rates)
      const hi = Math.max(...rates)
      const matures = ds.map((d) => d.maturity_date).sort()
      add('deposits', {
        key: bankKey(institution),
        label: institution,
        detail: [
          ds.length === 1
            ? `${humanise(ds[0].kind)}${ds[0].reference ? ` ${ds[0].reference}` : ''}`
            : `${ds.length} deposits`,
          lo === hi ? `${num(lo, 2)}%` : `${num(lo, 2)}–${num(hi, 2)}%`,
          matures.length === 1 || matures[0] === matures[matures.length - 1]
            ? `matures ${date(matures[0])}`
            : `matures ${date(matures[0])} – ${date(matures[matures.length - 1])}`,
        ].join(' · '),
        cls: 'debt_high',
        value: ds.reduce((s, d) => s + d.current_value_base, 0),
        aliases: ds.map((d) => `d:${d.id}`),
      })
    }
    for (const a of cash?.accounts ?? []) {
      add('cash', {
        key: `c:${a.account_id}`,
        label: a.name,
        detail: [a.institution, a.currency].filter(Boolean).join(' · '),
        cls: 'debt_low',
        value: a.balance_base,
      })
    }
    for (const list of Object.values(out)) list.sort((a, b) => b.value - a.value)
    return out
  }, [loading, stamp, view])
}

export default function Planner() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['dashboard'],
    queryFn: () => api.get<NetWorth>('/dashboard'),
  })
  const [view, setView] = useState<View>(() =>
    localStorage.getItem('cairezeki-plan-view') === 'classes' ? 'classes' : 'segments',
  )
  // One plan, made by segment; the equity-debt view shows the same plan regrouped by class.
  const stored = useStoredPlan('segments')
  const [pending, setPending] = useState<Pending | null>(null)
  // A saved session on show instead of the current plan - one's own, or one shared by
  // another user: read-only, with the values it was saved with.
  const [recordedRef, setRecordedRef] = useState<SessionRef | null>(null)
  const recorded = useQuery({
    queryKey: ['plan-session', recordedRef?.shared ? 'shared' : 'own', recordedRef?.id],
    queryFn: () =>
      api
        .get<RecordedSession>(`/planner/${recordedRef?.shared ? 'shared' : 'recorded'}/${recordedRef?.id}`)
        .then((s) => ({ ...s, plan: normalise(s.plan) })),
    enabled: recordedRef !== null,
    // A saved session never changes.
    staleTime: Infinity,
  })
  const session = recordedRef !== null ? recorded.data : undefined

  useEffect(() => localStorage.setItem('cairezeki-plan-view', view), [view])

  const switchView = (next: View) => {
    if (next === view) return
    setPending(null)
    setView(next)
  }
  const openSession = (id: number | null, shared = false) => {
    setPending(null)
    setRecordedRef(id === null ? null : { id, shared })
  }

  if (isLoading || stored.isLoading) return <Spinner label="Valuing your portfolio" />
  if (error || stored.error) return <ErrorBanner error={error ?? stored.error} />
  if (!data) return null
  if (recordedRef !== null && recorded.isLoading) return <Spinner label="Opening the saved session" />
  if (recordedRef !== null && recorded.error) {
    return (
      <div className="space-y-3">
        <ErrorBanner error={recorded.error} />
        <Button variant="primary" onClick={() => openSession(null)}>
          Back to current plan
        </Button>
      </div>
    )
  }

  const shown = session?.page.data ?? data
  const rows = rowsOf(shown, view)
  if (rows.length === 0) {
    return (
      <>
        <PageTitle title="RezekiYojana - Allocation Planner" />
        <EmptyState
          title="Nothing to plan yet"
          body="Import holdings, deposits or bank balances first; the planner starts from what you hold today."
        />
      </>
    )
  }

  return (
    <PlannerBody
      // A fresh body for each session, so nothing opened or half-made in one carries into another.
      key={session ? `session-${recordedRef?.shared ? 's' : ''}${session.id}` : 'live'}
      data={shown}
      rows={rows}
      view={view}
      onView={switchView}
      plan={session ? session.plan : stored.plan}
      setPlan={session ? () => {} : stored.setPlan}
      saveStatus={session ? null : stored.status}
      notice={session ? null : stored.notice}
      onDismissNotice={stored.dismiss}
      pending={pending}
      setPending={setPending}
      recorded={session ?? null}
      onOpenSession={openSession}
    />
  )
}

/** Today's categories in a view, those that hold anything. */
function rowsOf(data: NetWorth, view: View): Segment[] {
  return (view === 'segments' ? data.segments : data.by_class).filter((s) => Math.abs(s.value_base) > EPS)
}

/** What a shared link shows: whose it is, and the saved sessions in it. */
interface SharedLink {
  owner_name: string
  label: string
  expires_at?: string
  sessions: SharedSessionListed[]
}

/**
 * The planner as a shared link opens it, without signing in: the saved
 * sessions in the link and nothing else - one at a time, read-only, picked
 * from a dropdown of the link's own. No saving, uploading or downloading.
 */
export function SharedLinkPlanner({ token }: { token: string }) {
  const base = `/shared/${encodeURIComponent(token)}`
  const link = useQuery({
    queryKey: ['shared-link', token],
    queryFn: () => api.get<SharedLink>(base),
    retry: false,
  })
  const [picked, setPicked] = useState<number | null>(null)
  const id = picked ?? link.data?.sessions[0]?.id ?? null
  const session = useQuery({
    queryKey: ['shared-link', token, id],
    queryFn: () =>
      api
        .get<RecordedSession>(`${base}/plans/${id}`)
        .then((s) => ({ ...s, plan: normalise(s.plan) })),
    enabled: id !== null,
    staleTime: Infinity,
    retry: false,
  })
  const [view, setView] = useState<View>('segments')
  const [pending, setPending] = useState<Pending | null>(null)

  if (link.isLoading || session.isLoading) return <Spinner label="Opening the shared plan" />
  if (link.error) return <ErrorBanner error={link.error} />
  if (!link.data || link.data.sessions.length === 0)
    return <EmptyState title="Nothing shared" body="This link has no plans in it at the moment." />
  if (session.error) return <ErrorBanner error={session.error} />
  if (!session.data) return null
  const s = session.data
  const rows = rowsOf(s.page.data, view)

  return (
    <PlannerBody
      key={`link-${s.id}`}
      data={s.page.data}
      rows={rows}
      view={view}
      onView={(v) => {
        setPending(null)
        setView(v)
      }}
      plan={s.plan}
      setPlan={() => {}}
      saveStatus={null}
      notice={null}
      onDismissNotice={() => {}}
      pending={pending}
      setPending={setPending}
      recorded={s}
      onOpenSession={(next) => {
        setPending(null)
        if (next !== null) setPicked(next)
      }}
      link={link.data.sessions}
    />
  )
}

/** One line of what a category holds today. */
interface Part {
  key: string
  label: string
  value: number
}

/**
 * What each of today's categories is made of. The dashboard breaks each
 * class down by segment; read the other way, that is each segment's split
 * across the classes.
 */
function composition(data: NetWorth, view: View): Record<string, Part[]> {
  const out: Record<string, Part[]> = {}
  for (const cls of data.by_class) {
    for (const p of cls.parts ?? []) {
      if (Math.abs(p.value_base) <= EPS) continue
      if (view === 'classes') (out[cls.key] ??= []).push({ key: p.key, label: p.label, value: p.value_base })
      else (out[p.key] ??= []).push({ key: cls.key, label: cls.label, value: p.value_base })
    }
  }
  for (const parts of Object.values(out)) parts.sort((a, b) => b.value - a.value)
  return out
}

/**
 * The segment plan regrouped by class, for the equity-debt view. A move out
 * of or into one fund goes to that fund's class. A move on a category as a
 * whole is split across the classes it holds today - or the class chosen
 * for it, on the way in - so each piece keeps the move's number in
 * `origin`. Choosing a class for one of today's categories moves what is
 * left in it to that class. A class the plan reaches that is not held
 * today (gold, real estate) shows as a new category.
 */
function projectToClasses(
  plan: Plan,
  data: NetWorth,
  clsSubs: Record<string, Sub[]> | undefined,
): { plan: Plan; origin: Record<string, number> } {
  type Side = { key: string; sub?: string; share: number }
  const parts = composition(data, 'segments')
  const segValue = Object.fromEntries(data.segments.map((s) => [s.key, s.value_base]))
  const held = new Set(data.by_class.filter((c) => Math.abs(c.value_base) > EPS).map((c) => c.key))
  const created: Created[] = []
  const bucketFor = (cls: string) => {
    if (!held.has(cls) && !created.some((c) => c.key === cls))
      created.push({ key: cls, label: classLabel(cls), cls: (CLASSES.some((c) => c.key === cls) ? cls : 'other') as ClassKey })
    return cls
  }
  /** The class a fund held today sits under in the equity-debt view; none for a hybrid fund. */
  const classOfFund = (sub: string) => Object.entries(clsSubs ?? {}).find(([, l]) => l.some((x) => x.key === sub))?.[0]
  const subs: NewSub[] = plan.subs.map((s) => ({ ...s, bucket: bucketFor(s.cls) }))
  const madeCls = (key: string) => plan.created.find((c) => c.key === key)?.cls
  const heldShares = (seg: string): Side[] => {
    const ps = parts[seg] ?? []
    const total = ps.reduce((s, p) => s + p.value, 0)
    return total > 0 ? ps.map((p) => ({ key: bucketFor(p.key), share: p.value / total })) : [{ key: bucketFor('other'), share: 1 }]
  }
  const fromSides = (cat: string, sub?: string): Side[] => {
    if (cat === FRESH) return [{ key: FRESH, share: 1 }]
    const made = madeCls(cat)
    if (made) return [{ key: bucketFor(made), share: 1 }]
    const fund = sub && classOfFund(sub)
    return fund ? [{ key: bucketFor(fund), sub, share: 1 }] : heldShares(cat)
  }
  const toSides = (cat: string, sub?: string): Side[] => {
    const madeSub = sub && subs.find((s) => s.key === sub)
    if (madeSub) return [{ key: madeSub.bucket, sub, share: 1 }]
    const fund = sub && classOfFund(sub)
    if (fund) return [{ key: bucketFor(fund), sub, share: 1 }]
    const chosen = madeCls(cat) ?? plan.cls[cat]
    return chosen ? [{ key: bucketFor(chosen), share: 1 }] : heldShares(cat)
  }

  const moves: Move[] = []
  const origin: Record<string, number> = {}
  const known = (k: string) => k === FRESH || k in segValue || plan.created.some((c) => c.key === k)
  plan.moves.forEach((m, i) => {
    if (!known(m.from) || !known(m.to)) return
    let n = 0
    for (const f of fromSides(m.from, m.from_sub))
      for (const t of toSides(m.to, m.to_sub)) {
        const amount = m.amount * f.share * t.share
        // Within one class and no fund named on either side, nothing changes.
        if (amount <= EPS || (f.key === t.key && !f.sub && !t.sub)) continue
        const id = `${m.id}#${n++}`
        origin[id] = i
        moves.push({ ...m, id, from: f.key, from_sub: f.sub, to: t.key, to_sub: t.sub, amount })
      }
  })
  // A class chosen for one of today's categories takes what the plan leaves in it.
  let next = plan.moves.length
  Object.entries(plan.cls).forEach(([seg, cls]) => {
    if (!(seg in segValue)) return
    const out = plan.moves.filter((m) => m.from === seg).reduce((s, m) => s + m.amount, 0)
    const left = segValue[seg] - out
    const label = data.segments.find((s) => s.key === seg)?.label ?? seg
    let n = 0
    for (const f of heldShares(seg)) {
      const amount = left * f.share
      if (amount <= EPS || f.key === cls) continue
      const id = `cls-${seg}#${n++}`
      origin[id] = next
      moves.push({ id, from: f.key, to: bucketFor(cls), amount, note: `${label} classed as ${classLabel(cls)}`, at: '' })
    }
    if (n) next++
  })
  return { plan: { created, subs, moves, fresh: plan.fresh, removed: [], cls: {} }, origin }
}

function PlannerBody({
  data,
  rows,
  view,
  onView,
  plan: segPlan,
  setPlan,
  saveStatus,
  notice,
  onDismissNotice,
  pending,
  setPending,
  recorded,
  onOpenSession,
  link,
}: {
  data: NetWorth
  rows: Segment[]
  view: View
  onView: (v: View) => void
  /** The plan, as made by segment. */
  plan: Plan
  setPlan: (fn: (p: Plan) => Plan) => void
  /** "Saving…", or when the plan was last saved. */
  saveStatus: string | null
  /** Why the last edit did not stick, if it did not. */
  notice: string | null
  onDismissNotice: () => void
  pending: Pending | null
  setPending: (p: Pending | null) => void
  /** The saved session on show, read-only; null for the current plan. */
  recorded: RecordedSession | null
  onOpenSession: (id: number | null, shared?: boolean) => void
  /** Opened from a shared link, without signing in: the sessions in the link, and nothing of anyone's live. */
  link?: SharedSessionListed[]
}) {
  // Amounts are kept in the base currency; the switch only changes how they read.
  const base = data.base_currency
  const currencies = data.display_currencies?.length ? data.display_currencies : [base]
  const [currencyPick, setCurrencyPick] = useState(() => localStorage.getItem('cairezeki-plan-currency') ?? base)
  const code = currencies.includes(currencyPick) ? currencyPick : base
  const converted = data.total_values?.[code]
  const fx: Fx =
    code !== base && converted && data.total_base ? { code, rate: converted / data.total_base } : { code: base, rate: 1 }
  const pickCurrency = (c: string) => {
    setCurrencyPick(c)
    localStorage.setItem('cairezeki-plan-currency', c)
  }
  const [chartKind, setChartKind] = useState<ChartKind>(() =>
    localStorage.getItem('cairezeki-plan-chart') === 'pie' ? 'pie' : 'bars',
  )
  const pickChart = (k: ChartKind) => {
    setChartKind(k)
    localStorage.setItem('cairezeki-plan-chart', k)
  }
  const [dragging, setDragging] = useState<string | null>(null)
  const [over, setOver] = useState<string | null>(null)
  const [open, setOpen] = useState<Set<string>>(new Set())
  const [openToday, setOpenToday] = useState<Set<string>>(new Set())
  const [adding, setAdding] = useState(false)

  const parts = useMemo(() => composition(data, view), [data, view])
  const liveSegSubs = useSubcategories('segments', data.excluded ?? [], !link)
  const liveClsSubs = useSubcategories('classes', data.excluded ?? [], !link)
  // A saved session shows the funds and accounts as they were when it was saved.
  const segSubs = recorded ? recorded.page.segSubs : liveSegSubs
  const clsSubs = recorded ? recorded.page.clsSubs : liveClsSubs
  // A plan made when each deposit had a line of its own moves onto the banks' lines, once.
  useEffect(() => {
    if (recorded) return
    if (regroupDeposits(segPlan, segSubs?.deposits)) setPlan((p) => regroupDeposits(p, segSubs?.deposits) ?? p)
  }, [segPlan, segSubs])
  /** By equity / debt shows the segment plan regrouped by class: to look at, not to change. */
  const byClass = view === 'classes'
  /** Nor can a saved session be changed. */
  const readOnly = byClass || recorded !== null
  const editable = !readOnly
  const subcategories = byClass ? clsSubs : segSubs
  const projected = useMemo(
    () => (byClass ? projectToClasses(segPlan, data, clsSubs) : null),
    [byClass, segPlan, data, clsSubs],
  )
  /** Whether a saved session may be downloaded: the user's choice, in their profile. */
  const downloadAllowed = usePlanSessions(undefined, !link).data?.download ?? true
  /** Someone else's plan - shared with this user, or through a link: it is not theirs to download. */
  const foreign = link !== undefined || recorded?.owner_name !== undefined
  const [sharing, setSharing] = useState(false)
  /** The plan and the values it is shown against, to keep as a saved session. */
  const snapshot = () =>
    liveSegSubs && liveClsSubs
      ? { plan: segPlan, page: { version: 1, data, segSubs: liveSegSubs, clsSubs: liveClsSubs } satisfies SessionPage }
      : null
  const plan = projected?.plan ?? segPlan
  /** A move's number in the plan; a whole-category move split across classes keeps its one number. */
  const moveNo = (m: Move) => (projected ? projected.origin[m.id] : plan.moves.indexOf(m)) + 1
  const model = useMemo(() => buildModel(rows, plan), [rows, plan])
  const { buckets, planned, currentTotal, plannedTotal, freshLeft, colour } = model
  const removed = new Set(plan.removed)
  const createdKeys = new Set(plan.created.map((c) => c.key))
  const shown = buckets.filter((b) => !removed.has(b.key))
  const labelOf = (key: string) => (key === FRESH ? 'Fresh money' : buckets.find((b) => b.key === key)?.label ?? key)
  /**
   * What can still leave a category: what it holds today less what is
   * already planned out of it. Money the plan moves in stays where it was
   * put - every move starts from today.
   */
  const available = (key: string) =>
    key === FRESH ? freshLeft : Math.max(0, (model.current[key] ?? 0) - (model.outflow[key] ?? 0))
  const decode = (src: string) => {
    const [cat, sub] = src.split('|')
    return { cat, sub: sub as string | undefined }
  }
  const outOfSub = (cat: string, sub: string) =>
    plan.moves.filter((m) => m.from === cat && m.from_sub === sub).reduce((t, m) => t + m.amount, 0)
  /** What can still leave a source: a category, one fund in it, or fresh money. */
  const srcAvailable = (src: string) => {
    const { cat, sub } = decode(src)
    if (!sub) return available(cat)
    const held = subcategories?.[cat]?.find((x) => x.key === sub)?.value ?? 0
    return Math.max(0, Math.min(held - outOfSub(cat, sub), available(cat)))
  }
  /** What a source holds before any move: the base a move's percentage is of. */
  const srcHeld = (src: string) => {
    if (src === FRESH) return plan.fresh
    const { cat, sub } = decode(src)
    return sub ? (subcategories?.[cat]?.find((x) => x.key === sub)?.value ?? 0) : (model.current[cat] ?? 0)
  }
  /** The share of a source still free to move, as a percentage of what it holds. */
  const leftPct = (src: string) => {
    const held = srcHeld(src)
    return held > 0 ? (srcAvailable(src) / held) * 100 : 0
  }

  /**
   * A category's classification as shares of its value: the one chosen for
   * it, else what it holds today. In the class view a category is its class.
   */
  const split = (key: string): { cls: string; share: number }[] => {
    const chosen = plan.cls[key] ?? plan.created.find((c) => c.key === key)?.cls
    return chosen ? [{ cls: chosen, share: 1 }] : heldSplit(key)
  }
  /** A category's classification as it is held today, before any class the plan chose for it. */
  const heldSplit = (key: string): { cls: string; share: number }[] => {
    if (view === 'classes') return [{ cls: key, share: 1 }]
    const ps = parts[key] ?? []
    const total = ps.reduce((s, p) => s + p.value, 0)
    return total > 0 ? ps.map((p) => ({ cls: p.key, share: p.value / total })) : [{ cls: 'other', share: 1 }]
  }
  /** The classification to show: one class, or "Mixed" where today's holdings span several. */
  const classOf = (key: string) => {
    const s = split(key).filter((x) => x.share > 0.05)
    return s.length === 1 ? classLabel(s[0].cls) : 'Mixed'
  }

  /** The funds and accounts in a category: today's, then the plan's new ones. */
  const subsOf = (bucket: string): (Sub & { isNew: boolean })[] => [
    ...(createdKeys.has(bucket) ? [] : (subcategories?.[bucket] ?? [])).map((s) => ({ ...s, isNew: false })),
    ...plan.subs
      .filter((s) => s.bucket === bucket)
      .map((s) => ({ key: s.key, label: s.label, detail: 'New in the plan', cls: s.cls, value: 0, isNew: true })),
  ]
  const subLabel = (bucket: string, sub: string) => subsOf(bucket).find((s) => s.key === sub)?.label ?? 'Removed subcategory'
  const srcLabel = (src: string) => {
    const { cat, sub } = decode(src)
    return sub ? `${labelOf(cat)} › ${subLabel(cat, sub)}` : labelOf(cat)
  }
  const sourceLabel = (m: Move) => srcLabel(m.from_sub ? `${m.from}|${m.from_sub}` : m.from)
  /** "Global ETFs › Vanguard Total World", or the category alone. */
  const targetLabel = (m: Move) => (m.to_sub ? `${labelOf(m.to)} › ${subLabel(m.to, m.to_sub)}` : labelOf(m.to))

  const known = (k: string) => k === FRESH || k in planned
  /** A category's funds with their changes; undefined while today's funds load. */
  const knownMoves = plan.moves.filter((m) => known(m.from) && known(m.to))
  const linesOf = (key: string) =>
    createdKeys.has(key) || subcategories
      ? subLines(
          subsOf(key),
          knownMoves,
          key,
          model.current[key] ?? 0,
          view === 'classes' ? 'Not itemised (funds the dashboard splits by their portfolio)' : 'Not itemised',
        )
      : undefined

  /**
   * Today and planned by class, for the equity-debt picture in the JSON
   * download. Today is what the categories hold; each move then shifts its
   * amount from the class of the fund (or category) it leaves to the class
   * of the one it joins.
   */
  const classTotals = () => {
    const today: Record<string, number> = {}
    const planned: Record<string, number> = {}
    const add = (into: Record<string, number>, s: { cls: string; share: number }[], v: number) => {
      for (const x of s) into[x.cls] = (into[x.cls] ?? 0) + v * x.share
    }
    const sideSplit = (cat: string, sub?: string) => {
      const s = sub ? subsOf(cat).find((x) => x.key === sub) : undefined
      if (!s || s.cls === 'mixed') return split(cat)
      return [{ cls: s.cls, share: 1 }]
    }
    for (const r of model.table) {
      add(today, heldSplit(r.key), r.current)
      add(planned, split(r.key), r.current)
    }
    for (const m of knownMoves) {
      if (m.from !== FRESH) add(planned, sideSplit(m.from, m.from_sub), -m.amount)
      add(planned, sideSplit(m.to, m.to_sub), m.amount)
    }
    return CLASSES.map((c) => ({ ...c, today: today[c.key] ?? 0, planned: planned[c.key] ?? 0 })).filter(
      (c) => Math.abs(c.today) > EPS || Math.abs(c.planned) > EPS,
    )
  }

  const addMove = (from: string, to: string, amount: number, note: string, toSub?: string, fromSub?: string) =>
    setPlan((p) => ({
      ...p,
      moves: [
        ...p.moves,
        { id: crypto.randomUUID(), from, from_sub: fromSub, to, to_sub: toSub, amount, note, at: new Date().toISOString() },
      ],
    }))

  const createBucket = (label: string, cls: ClassKey) => {
    const key = `custom-${crypto.randomUUID().slice(0, 8)}`
    setPlan((p) => ({ ...p, created: [...p.created, { key, label, cls }] }))
    return key
  }

  const createSub = (bucket: string, label: string, cls: ClassKey) => {
    const key = `sub-${crypto.randomUUID().slice(0, 8)}`
    setPlan((p) => ({ ...p, subs: [...p.subs, { key, bucket, label, cls }] }))
    return key
  }

  /** A new subcategory goes, and the moves into it with it. */
  const deleteSub = (bucket: string, key: string) => {
    const n = plan.moves.filter((m) => m.to === bucket && m.to_sub === key).length
    if (n && !window.confirm(`Delete ${subLabel(bucket, key)} and undo its ${n} move${n === 1 ? '' : 's'}?`)) return
    setPlan((p) => ({
      ...p,
      subs: p.subs.filter((s) => s.key !== key),
      moves: p.moves.filter((m) => !(m.to === bucket && m.to_sub === key)),
    }))
  }

  const removeMove = (id: string) => setPlan((p) => ({ ...p, moves: p.moves.filter((m) => m.id !== id) }))

  /**
   * Deleting a new category undoes every move into or out of it. One of
   * today's categories has to be emptied first: if anything is in it, the
   * move form opens for all of it.
   */
  const deleteBucket = (key: string) => {
    if (createdKeys.has(key)) {
      const n = plan.moves.filter((m) => m.from === key || m.to === key).length
      if (n && !window.confirm(`Delete ${labelOf(key)} and undo its ${n} move${n === 1 ? '' : 's'}?`)) return
      setPlan((p) => ({
        ...p,
        created: p.created.filter((b) => b.key !== key),
        subs: p.subs.filter((s) => s.bucket !== key),
        moves: p.moves.filter((m) => m.from !== key && m.to !== key),
      }))
      return
    }
    const inbound = plan.moves.filter((m) => m.to === key)
    if (
      inbound.length &&
      !window.confirm(`Undo the ${inbound.length} move${inbound.length === 1 ? '' : 's'} into ${labelOf(key)} and delete it?`)
    )
      return
    if (inbound.length) setPlan((p) => ({ ...p, moves: p.moves.filter((m) => m.to !== key) }))
    if (available(key) > EPS) setPending({ from: key, to: '', remove: true })
    else setPlan((p) => ({ ...p, removed: [...p.removed, key] }))
  }

  /** Deleting a fund held today: undo what the plan moved into it, then move out all of it. */
  const deleteHeldSub = (bucket: string, key: string) => {
    const inbound = plan.moves.filter((m) => m.to === bucket && m.to_sub === key)
    if (
      inbound.length &&
      !window.confirm(`Undo the ${inbound.length} move${inbound.length === 1 ? '' : 's'} into ${subLabel(bucket, key)}?`)
    )
      return
    if (inbound.length) setPlan((p) => ({ ...p, moves: p.moves.filter((m) => !(m.to === bucket && m.to_sub === key)) }))
    if (srcAvailable(`${bucket}|${key}`) > EPS) setPending({ from: `${bucket}|${key}`, to: '', remove: true })
  }

  const restore = (key: string) => setPlan((p) => ({ ...p, removed: p.removed.filter((k) => k !== key) }))

  const setClass = (key: string, cls: ClassKey | '') =>
    setPlan((p) => {
      if (createdKeys.has(key) && cls) return { ...p, created: p.created.map((c) => (c.key === key ? { ...c, cls } : c)) }
      const next = { ...p.cls }
      if (cls) next[key] = cls
      else delete next[key]
      return { ...p, cls: next }
    })

  const toggleToday = (key: string) =>
    setOpenToday((s) => {
      const next = new Set(s)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })

  const toggle = (key: string) =>
    setOpen((s) => {
      const next = new Set(s)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })

  const { confirm, dialog } = useConfirm()
  const reset = () =>
    confirm({
      title: 'Reset the plan?',
      body: 'This removes all your changes - every move, fresh money, and every category added or deleted - and puts the plan back to its default: what you hold today.',
      onConfirm: () => {
        setPending(null)
        setPlan(() => EMPTY_PLAN)
      },
    })

  // Native drag and drop. The source key rides in dataTransfer and in state,
  // as some browsers hide dataTransfer until the drop.
  const dragProps = (key: string) => ({
    draggable: true,
    onDragStart: (e: DragEvent) => {
      e.dataTransfer.setData('text/plain', key)
      e.dataTransfer.effectAllowed = 'move'
      setDragging(key)
    },
    onDragEnd: () => {
      setDragging(null)
      setOver(null)
    },
  })
  const dropProps = (target: string | null) => {
    const id = target ?? '__new__'
    return {
      onDragOver: (e: DragEvent) => {
        // A whole category onto itself moves nothing; one fund into another in the same category does.
        if (!dragging || dragging === target) return
        e.preventDefault()
        e.dataTransfer.dropEffect = 'move'
        if (over !== id) setOver(id)
      },
      onDragLeave: () => over === id && setOver(null),
      onDrop: (e: DragEvent) => {
        e.preventDefault()
        const from = e.dataTransfer.getData('text/plain') || dragging
        setDragging(null)
        setOver(null)
        if (from && from !== target) setPending({ from, to: target })
      },
    }
  }
  const dropHighlight = (id: string) =>
    over === id ? 'ring-2 ring-series-1 bg-series-1/5' : dragging ? 'border-dashed' : ''

  /**
   * Every category with each of its funds, as the change table shows them
   * expanded. Amounts are in the currency shown; a fund's shares, like the
   * table's, are of the whole portfolio.
   */
  const exportRows = () => {
    const amt = (v: number) => Math.round(v * fx.rate * 100) / 100
    const share = (v: number, total: number) => (total ? Math.round((v / total) * 10000) / 100 : 0)
    return model.table.map((r) => ({
      key: r.key,
      label: r.label,
      classification: classOf(r.key),
      status: STATUS[r.status].label,
      today: amt(r.current),
      today_pct: share(r.current, currentTotal),
      change: amt(r.change),
      planned: amt(r.planned),
      planned_pct: share(r.planned, plannedTotal),
      subcategories: (linesOf(r.key) ?? []).map((l) => ({
        key: l.key,
        label: l.label,
        detail: l.detail ?? '',
        kind: l.kind,
        classification: l.cls,
        status: STATUS[lineStatus(l)].label,
        today: amt(l.today),
        today_pct: share(l.today, currentTotal),
        change: amt(l.change),
        planned: amt(l.today + l.change),
        planned_pct: share(l.today + l.change, plannedTotal),
      })),
    }))
  }
  const exportMoves = () =>
    plan.moves.map((m) => ({
      from: sourceLabel(m),
      to: targetLabel(m),
      amount: Math.round(m.amount * fx.rate * 100) / 100,
      note: m.note,
      planned_on: m.at.slice(0, 10),
    }))
  /** The session a download is of: the saved one on show; for the current plan, none. */
  const shownSession: SessionId | null = recorded ? { name: recorded.name, created_at: utcIso(recorded.created_at) } : null
  /** A file name without its extension: the session's name and when it was saved, else the plan and now. */
  const baseName = (s: SessionId | null) => (s ? fileBase(s) : `allocation-plan-${fileStamp()}`)
  const save = (body: string, type: string, ext: string, s: SessionId | null = shownSession) => {
    const a = document.createElement('a')
    a.href = URL.createObjectURL(new Blob([body], { type }))
    a.download = `${baseName(s)}.${ext}`
    a.click()
    URL.revokeObjectURL(a.href)
  }

  const downloadCsv = (s: SessionId | null = shownSession) => {
    const c = fx.code
    const lines = [
      ['Category', 'Subcategory', 'Detail', 'Classification', 'Status', `Today (${c})`, 'Today %', `Change (${c})`, `Planned (${c})`, 'Planned %'],
      ...exportRows().flatMap((r) => [
        [r.label, '', '', r.classification, r.status, r.today, r.today_pct, r.change, r.planned, r.planned_pct],
        ...r.subcategories.map((s) => [
          r.label,
          s.label,
          s.detail,
          s.classification,
          s.status,
          s.today,
          s.today_pct,
          s.change,
          s.planned,
          s.planned_pct,
        ]),
      ]),
      [],
      ['From', 'To', `Amount (${c})`, 'Note', 'Planned on'],
      ...exportMoves().map((m) => [m.from, m.to, m.amount, m.note, m.planned_on]),
    ]
    save(lines.map((l) => l.map((v) => `"${String(v).replace(/"/g, '""')}"`).join(',')).join('\n'), 'text/csv', 'csv', s)
  }

  /**
   * The plan as JSON, two ways in one file: what plan-viewer.html turns back
   * into tables and charts, and under `session` the plan with the values it
   * is shown against, which uploads back into the planner as a saved
   * session. For the current plan the session is named for now; for a saved
   * one it is that session.
   */
  const planDoc = (s: SessionId | null = shownSession, withSession = true) => {
    const amt = (v: number) => Math.round(v * fx.rate * 100) / 100
    const now = new Date().toISOString()
    const page = recorded ? recorded.page : snapshot()?.page
    const id = s ?? { name: `Plan ${dateTime(now)}`, created_at: now }
    return {
      format: 'cairezeki-plan',
      version: 1,
      generated_at: now,
      as_of: data.as_of,
      view,
      currency: fx.code,
      base_currency: base,
      fx_rate: fx.rate,
      fresh_money: amt(plan.fresh),
      totals: { today: amt(currentTotal), change: amt(plannedTotal - currentTotal), planned: amt(plannedTotal) },
      categories: exportRows(),
      by_class: classTotals().map((x) => ({
        key: x.key,
        label: x.label,
        group: x.group,
        today: amt(x.today),
        planned: amt(x.planned),
      })),
      moves: exportMoves(),
      // The viewer offers its downloads only where the user allows them.
      downloads: !foreign && !(recorded && !downloadAllowed),
      ...(withSession && page
        ? {
            session: {
              format: 'cairezeki-plan-session',
              version: 1,
              name: id.name,
              created_at: id.created_at,
              plan: segPlan,
              page,
            } satisfies SessionPart,
          }
        : {}),
    }
  }
  const downloadJson = (s: SessionId | null = shownSession) =>
    save(JSON.stringify(planDoc(s), null, 2), 'application/json', 'json', s)
  /** A session just saved, to disk: the CSV and the JSON. */
  const downloadSaved = (s: SessionId) => {
    downloadCsv(s)
    // Two downloads at once; a moment apart, so the browser lets both through.
    setTimeout(() => downloadJson(s), 300)
  }

  /**
   * The viewer on this plan as it stands: the plan rides in the browser's
   * storage, which the viewer reads on opening (and when it changes).
   * Someone else's plan goes without its session, and from a shared link to
   * a viewer of its own that offers nothing to download or upload.
   */
  const openViewer = () => {
    const store = link ? VIEWER_SHARED_STORE : VIEWER_STORE
    try {
      localStorage.setItem(store, JSON.stringify(planDoc(shownSession, !foreign)))
    } catch {
      try {
        // Too big with the session's values: the viewer needs only its own part.
        localStorage.setItem(store, JSON.stringify(planDoc(shownSession, false)))
      } catch {
        // Still too big to hand over: the viewer opens empty, ready for a file.
      }
    }
    window.open(link ? '/plan-viewer.html?shared' : '/plan-viewer.html', '_blank', 'noopener')
  }

  /** Sources a move can come from: today's categories and the funds in them, and fresh money. */
  const sources: Source[] = [
    ...rows.flatMap((r) =>
      available(r.key) > EPS
        ? [
            { key: r.key, label: `All of ${r.label}`, group: r.label },
            ...(subcategories?.[r.key] ?? [])
              .filter((x) => srcAvailable(`${r.key}|${x.key}`) > EPS)
              .map((x) => ({ key: `${r.key}|${x.key}`, label: x.label, group: r.label })),
          ]
        : [],
    ),
    ...(freshLeft > EPS ? [{ key: FRESH, label: 'Fresh money', group: 'New money' }] : []),
  ]

  return (
    <div className="space-y-5">
      <PageTitle
        title="RezekiYojana - Allocation Planner"
        description="Drag money from today into the plan - a whole category or one fund - and see what changes."
      />

      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <ToggleGroup
            label="Plan by"
            options={[
              ['segments', 'By segment'],
              ['classes', 'By equity / debt'],
            ]}
            value={view}
            onChange={onView}
          />
          {currencies.length > 1 && (
            <ToggleGroup
              label="Report in currency"
              options={currencies.map((c) => [c, c] as const)}
              value={fx.code}
              onChange={pickCurrency}
            />
          )}
          {saveStatus && (
            <span className="text-xs text-ink-muted" aria-live="polite">
              {saveStatus}
            </span>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {link ? (
            <label className="flex items-center gap-2 text-sm">
              <span className="text-xs font-medium text-ink-secondary">Plan</span>
              <select
                className={`${inputClass} w-auto max-w-[20rem] py-1`}
                value={recorded ? String(recorded.id) : ''}
                onChange={(e) => onOpenSession(Number(e.target.value))}
                aria-label="Shared plan to show"
              >
                {link.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} · {dateTime(s.created_at)}
                  </option>
                ))}
              </select>
            </label>
          ) : (
            <>
              <SessionControls recorded={recorded} onOpen={onOpenSession} snapshot={snapshot} onDownload={downloadSaved} />
              <Button onClick={() => setSharing(true)} title="Share saved plans with a user, or as a link">
                <Share2 size={14} aria-hidden />
              </Button>
              <ShareDialog open={sharing} onClose={() => setSharing(false)} />
            </>
          )}
          <span className="flex items-center gap-2">
            <span className="text-xs font-medium text-ink-secondary">Viewer</span>
            <Button
              onClick={openViewer}
              disabled={subcategories === undefined}
              title="Open the viewer in a new window: it downloads the plan as HTML or JSON"
            >
              <ExternalLink size={14} aria-hidden />
            </Button>
          </span>
          {!readOnly && (
            <Button
              variant="ghost"
              onClick={reset}
              disabled={plan.moves.length === 0 && plan.fresh === 0 && plan.created.length === 0 && plan.removed.length === 0}
            >
              <span className="inline-flex items-center gap-1.5">
                <RotateCcw size={14} aria-hidden /> Reset
              </span>
            </Button>
          )}
          {dialog}
        </div>
      </div>

      {recorded && <RecordedBanner recorded={recorded} onBack={link ? undefined : () => onOpenSession(null)} />}

      {byClass && !recorded && (
        <div role="status" className="rounded-lg border border-series-1/30 bg-series-1/5 p-3 text-sm text-ink">
          By equity / debt shows your segment plan regrouped by class, to look at. A move on one fund lands in that
          fund's class; a move on a whole category is split by what the category holds today. Switch to{' '}
          <button type="button" onClick={() => onView('segments')} className="font-medium text-series-1 hover:underline">
            By segment
          </button>{' '}
          to change the plan.
        </div>
      )}

      {notice && (
        <div
          role="status"
          className="flex items-start justify-between gap-3 rounded-lg border border-status-warning/40 bg-status-warning/5 p-3 text-sm text-ink"
        >
          <span>{notice}</span>
          <button type="button" onClick={onDismissNotice} aria-label="Dismiss" className="text-ink-muted hover:text-ink">
            <X size={14} aria-hidden />
          </button>
        </div>
      )}

      <div className="grid items-start gap-4 lg:grid-cols-2">
        {/* Left: today, as it is. Each row is a source. */}
        <Card
          title="Today"
          subtitle={`${fxMoney(currentTotal, fx)}${readOnly ? '' : ' · drag a category, or open it and drag one fund, to the plan'}`}
        >
          <ul className="space-y-2">
            {rows.map((r) => {
              const out = model.outflow[r.key] ?? 0
              const left = available(r.key)
              const expanded = openToday.has(r.key)
              const held = subcategories?.[r.key] ?? []
              return (
                <li key={r.key} className="rounded-md border border-[color:var(--hairline)] bg-surface">
                  <div
                    {...(editable && left > EPS ? dragProps(r.key) : {})}
                    className={`group flex items-center gap-2 p-2.5 ${
                      !editable ? '' : left > EPS ? 'cursor-grab active:cursor-grabbing' : 'opacity-60'
                    } ${dragging === r.key ? 'opacity-50' : ''}`}
                  >
                    {editable && <GripVertical size={16} className="shrink-0 text-ink-muted" aria-hidden />}
                    <button
                      type="button"
                      onClick={() => toggleToday(r.key)}
                      aria-expanded={expanded}
                      aria-label={`${expanded ? 'Collapse' : 'Expand'} ${r.label}`}
                      className="rounded p-0.5 text-ink-muted hover:bg-ink/5 hover:text-ink"
                    >
                      {expanded ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
                    </button>
                    <Swatch colour={colour[r.key]} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm font-medium text-ink">{r.label}</div>
                      <div className="text-xs text-ink-muted">
                        {pct((r.value_base / currentTotal) * 100, 1)} of today · {classOf(r.key)}
                        {out > EPS && (
                          <span className="text-loss">
                            {' '}
                            · {fxSigned(-out, fx)} planned out · {pct(leftPct(r.key), 0)} left
                          </span>
                        )}
                      </div>
                    </div>
                    <div className="tabular text-sm text-ink">{fxMoney(r.value_base, fx)}</div>
                    {editable && left > EPS && <MoveButton onClick={() => setPending({ from: r.key, to: '' })} label={r.label} />}
                  </div>

                  {expanded && (
                    <ul className="space-y-1 border-t border-[color:var(--hairline)] p-2 text-xs">
                      {subcategories === undefined ? (
                        <li className="px-2 text-ink-muted">Loading funds…</li>
                      ) : held.length === 0 ? (
                        <li className="px-2 text-ink-muted">No funds itemised; move the category as a whole.</li>
                      ) : (
                        held.map((x) => {
                          const src = `${r.key}|${x.key}`
                          const xLeft = srcAvailable(src)
                          const xOut = outOfSub(r.key, x.key)
                          return (
                            <li
                              key={x.key}
                              {...(editable && xLeft > EPS ? dragProps(src) : {})}
                              className={`flex items-center gap-2 rounded px-2 py-1.5 hover:bg-ink/[0.03] ${
                                !editable ? '' : xLeft > EPS ? 'cursor-grab active:cursor-grabbing' : 'opacity-60'
                              } ${dragging === src ? 'opacity-50' : ''}`}
                            >
                              {editable && <GripVertical size={14} className="shrink-0 text-ink-muted" aria-hidden />}
                              <div className="min-w-0 flex-1">
                                <div className="truncate text-ink">{x.label}</div>
                                <div className="truncate text-[11px] text-ink-muted">
                                  {pct((x.value / currentTotal) * 100, 1)} of today · {subClassLabel(x.cls)}
                                  {x.detail && ` · ${x.detail}`}
                                  {xOut > EPS && (
                                    <span className="text-loss">
                                      {' '}
                                      · {fxSigned(-xOut, fx)} planned out · {pct(leftPct(src), 0)} left
                                    </span>
                                  )}
                                </div>
                              </div>
                              <div className="tabular text-ink-secondary">{fxMoney(x.value, fx)}</div>
                              {editable && xLeft > EPS && <MoveButton onClick={() => setPending({ from: src, to: '' })} label={x.label} />}
                            </li>
                          )
                        })
                      )}
                    </ul>
                  )}
                </li>
              )
            })}

            {/* Fresh money: savings still to be invested, a source like any other. */}
            <li
              {...(editable && freshLeft > EPS ? dragProps(FRESH) : {})}
              className={`flex items-center gap-2 rounded-md border border-dashed border-[color:var(--hairline)] p-2.5 ${
                editable && freshLeft > EPS ? 'cursor-grab active:cursor-grabbing' : ''
              } ${dragging === FRESH ? 'opacity-50' : ''}`}
            >
              <GripVertical size={16} className={`shrink-0 ${editable && freshLeft > EPS ? 'text-ink-muted' : 'text-transparent'}`} aria-hidden />
              <Plus size={14} className="shrink-0 text-series-1" aria-hidden />
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium text-ink">Fresh money</div>
                <div className="text-xs text-ink-muted">
                  {plan.fresh > 0
                    ? `${fxMoney(freshLeft, fx)} still to place · ${pct(leftPct(FRESH), 0)} left`
                    : 'Savings or a bonus to invest on top'}
                </div>
              </div>
              {editable ? (
                <div className="w-36" draggable={false} onDragStart={(e) => e.preventDefault()}>
                  <MoneyInput
                    value={inDisplay(plan.fresh, fx)}
                    currency={fx.code}
                    prefix={currencySymbol(fx.code)}
                    ariaLabel="Fresh money to invest"
                    onChange={(v) => setPlan((p) => ({ ...p, fresh: Math.max(0, v / fx.rate) }))}
                  />
                </div>
              ) : (
                <div className="tabular text-sm text-ink">{fxMoney(plan.fresh, fx)}</div>
              )}
              {editable && freshLeft > EPS && <MoveButton onClick={() => setPending({ from: FRESH, to: '' })} label="Fresh money" />}
            </li>
          </ul>
        </Card>

        {/* Right: the plan. Each row takes drops from Today; nothing is dragged out of it. */}
        <Card
          title="Plan"
          subtitle={`${fxMoney(plannedTotal, fx)}${
            plannedTotal - currentTotal > EPS ? ` · ${fxSigned(plannedTotal - currentTotal, fx)} fresh money` : ''
          }${readOnly ? '' : ' · drop here'}`}
        >
          {pending && editable && (
            <PendingMove
              key={`${pending.from}>${pending.to}>${pending.remove ? 'x' : ''}`}
              pending={pending}
              fx={fx}
              targets={shown}
              sources={sources}
              labelOf={srcLabel}
              available={srcAvailable}
              held={srcHeld}
              subsOf={subsOf}
              subsLoading={subcategories === undefined}
              defaultClass={(key) => {
                const s = split(key)
                const top = [...s].sort((a, b) => b.share - a.share)[0]?.cls
                return (CLASSES.some((c) => c.key === top) ? top : 'equity_fund') as ClassKey
              }}
              onCancel={() => setPending(null)}
              onConfirm={({ from: src, to, amount, note, newBucket, toSub, newSub }) => {
                const { cat: from, sub: fromSub } = decode(src)
                const target = newBucket ? createBucket(newBucket.label, newBucket.cls) : to!
                const sub = newSub ? createSub(target, newSub.label, newSub.cls) : toSub
                addMove(from, target, amount, note, sub, fromSub)
                if (pending.remove && !fromSub && !createdKeys.has(from))
                  setPlan((p) => ({ ...p, removed: [...p.removed, from] }))
                setPending(null)
              }}
            />
          )}

          <ul className="space-y-2">
            {shown.map((b) => {
              const row = model.table.find((r) => r.key === b.key)!
              const isCreated = createdKeys.has(b.key)
              const changed = row.status !== 'unchanged'
              const expanded = open.has(b.key)
              const moves = plan.moves.filter((m) => m.to === b.key || m.from === b.key)
              return (
                <li
                  key={b.key}
                  {...(editable ? dropProps(b.key) : {})}
                  className={`rounded-md border transition ${
                    changed ? 'border-series-1/40 bg-series-1/[0.03]' : 'border-[color:var(--hairline)]'
                  } ${dropHighlight(b.key)}`}
                >
                  <div className="flex items-center gap-2 p-2.5">
                    <button
                      type="button"
                      onClick={() => toggle(b.key)}
                      aria-expanded={expanded}
                      aria-label={`${expanded ? 'Collapse' : 'Expand'} ${b.label}`}
                      className="rounded p-0.5 text-ink-muted hover:bg-ink/5 hover:text-ink"
                    >
                      {expanded ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
                    </button>
                    <Swatch colour={colour[b.key]} />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="truncate text-sm font-medium text-ink">{b.label}</span>
                        {changed && <Badge tone={STATUS[row.status].tone}>{STATUS[row.status].label}</Badge>}
                      </div>
                      <div className="text-xs text-ink-muted">
                        {classOf(b.key)} · {pct(row.plannedPct, 1)} planned
                        {changed && (
                          <>
                            {' '}
                            · was {pct(row.currentPct, 1)}{' '}
                            <span className={deltaClass(row.change)}>({fxSigned(row.change, fx)})</span>
                          </>
                        )}
                      </div>
                    </div>
                    <div className="tabular text-sm text-ink">{fxMoney(row.planned, fx)}</div>
                    {editable && (
                      <>
                        <IconButton label={`Add money to ${b.label}`} onClick={() => setPending({ from: null, to: b.key })}>
                          <Plus size={14} aria-hidden />
                        </IconButton>
                        <IconButton label={`Delete ${b.label}`} danger onClick={() => deleteBucket(b.key)}>
                          <Trash2 size={14} aria-hidden />
                        </IconButton>
                      </>
                    )}
                  </div>

                  {expanded && (
                    <div className="space-y-3 border-t border-[color:var(--hairline)] px-3 pb-3 pt-2 text-xs">
                      <label className="flex items-center gap-2">
                        <span className="w-24 shrink-0 text-ink-muted">Classification</span>
                        {readOnly ? (
                          <span className="text-ink-secondary">{classOf(b.key)}</span>
                        ) : (
                          <select
                            className={`${inputClass} w-auto py-1 text-xs`}
                            value={plan.cls[b.key] ?? plan.created.find((c) => c.key === b.key)?.cls ?? ''}
                            onChange={(e) => setClass(b.key, e.target.value as ClassKey | '')}
                          >
                            {!isCreated && <option value="">As held today ({classOf(b.key) === 'Mixed' ? 'mixed' : classOf(b.key)})</option>}
                            {CLASSES.map((c) => (
                              <option key={c.key} value={c.key}>
                                {c.label}
                              </option>
                            ))}
                          </select>
                        )}
                      </label>

                      {!isCreated && view === 'segments' && (parts[b.key]?.length ?? 0) > 1 && (
                        <p className="text-ink-muted">
                          Held today as{' '}
                          {parts[b.key]
                            .map((p) => `${p.label} ${pct((p.value / (model.current[b.key] || 1)) * 100, 0)}`)
                            .join(' · ')}
                        </p>
                      )}

                      <SubTable
                        fx={fx}
                        lines={linesOf(b.key)}
                        currentTotal={currentTotal}
                        plannedTotal={plannedTotal}
                        readOnly={readOnly}
                        onAddSub={() => setPending({ from: null, to: b.key })}
                        onDeleteSub={(key) => deleteSub(b.key, key)}
                        onDeleteHeld={(key) => deleteHeldSub(b.key, key)}
                      />

                      <div>
                        <div className="mb-1 font-medium text-ink-secondary">Planned moves</div>
                        {moves.length === 0 ? (
                          <p className="pl-2 text-ink-muted">{readOnly ? 'None.' : 'None yet. Drop money here, or use +.'}</p>
                        ) : (
                          <ul className="space-y-0.5">
                            {moves.map((m) => {
                              const into = m.to === b.key
                              return (
                                <li key={m.id} className="flex items-center gap-2 pl-2">
                                  <span className="min-w-0 flex-1 truncate text-ink-secondary">
                                    {into ? `from ${sourceLabel(m)}${m.to_sub ? ` into ${subLabel(m.to, m.to_sub)}` : ''}` : `${sourceLabel(m)} to ${targetLabel(m)}`}
                                    {m.note && <span className="text-ink-muted"> — {m.note}</span>}
                                  </span>
                                  <span className={`tabular ${into ? 'text-gain' : 'text-loss'}`}>
                                    {fxSigned(into ? m.amount : -m.amount, fx)}
                                  </span>
                                  {editable && (
                                    <IconButton label="Undo this move" danger onClick={() => removeMove(m.id)}>
                                      <Trash2 size={12} aria-hidden />
                                    </IconButton>
                                  )}
                                </li>
                              )
                            })}
                          </ul>
                        )}
                      </div>
                    </div>
                  )}
                </li>
              )
            })}

            {editable && (
              <li
                {...dropProps(null)}
                className={`flex items-center justify-center gap-2 rounded-md border-2 border-dashed border-[color:var(--hairline)] p-3 text-sm text-ink-muted transition ${dropHighlight('__new__')}`}
              >
                <Plus size={16} aria-hidden />
                Drop here for a new category
              </li>
            )}
          </ul>

          <div className="mt-3">
            {!editable ? null : adding ? (
              <NewCategoryForm
                existing={buckets}
                onCancel={() => setAdding(false)}
                onAdd={(label, cls) => {
                  createBucket(label, cls)
                  setAdding(false)
                }}
              />
            ) : (
              <Button variant="ghost" onClick={() => setAdding(true)}>
                <span className="inline-flex items-center gap-1.5">
                  <Plus size={14} aria-hidden /> Add an empty category
                </span>
              </Button>
            )}
          </div>

          {editable && plan.removed.length > 0 && (
            <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-[color:var(--hairline)] pt-3 text-xs text-ink-muted">
              Deleted:
              {plan.removed.map((k) => (
                <button
                  key={k}
                  type="button"
                  onClick={() => restore(k)}
                  title="Show it in the plan again (its moves stay)"
                  className="inline-flex items-center gap-1 rounded bg-ink/5 px-2 py-0.5 text-ink-secondary hover:text-ink"
                >
                  {labelOf(k)} <Undo2 size={12} aria-hidden />
                </button>
              ))}
            </div>
          )}
        </Card>
      </div>

      <Card
        title="Allocation, today and planned"
        subtitle={byClass ? 'Share of the total, by equity / debt class' : 'Share of the total, by segment'}
        actions={
          <ToggleGroup
            label="Show allocation as"
            options={[
              ['bars', 'Bar chart'],
              ['pie', 'Pie chart'],
            ]}
            value={chartKind}
            onChange={pickChart}
          />
        }
      >
        <AllocationBars
          chart={chartKind}
          model={model}
          fx={fx}
          moves={plan.moves}
          sourceLabel={sourceLabel}
          targetLabel={targetLabel}
        />
      </Card>

      <Card
        title="What changes"
        subtitle="Changed rows are marked; Change is what goes in (+) or comes out (−) of each category"
      >
        <ChangeTable model={model} fx={fx} linesOf={linesOf} />
      </Card>

      <Card
        title="Decisions"
        subtitle={
          byClass
            ? 'Each move, in the order planned, regrouped by class: a whole-category move shows once per class it touches.'
            : recorded
              ? 'Each move, in the order planned.'
              : 'Each move, in the order planned. Remove one to undo it.'
        }
      >
        {plan.moves.length === 0 ? (
          <p className="text-sm text-ink-muted">
            {recorded
              ? 'No moves in this saved session.'
              : byClass
                ? 'No moves yet. Plan by segment to add some.'
                : 'No moves yet. Drag a category from Today onto the Plan.'}
          </p>
        ) : (
          <ol className="divide-y divide-[color:var(--hairline)]">
            {plan.moves.map((m) => (
              <li key={m.id} className="flex flex-wrap items-center gap-2 py-2 text-sm">
                <span className="w-6 text-xs text-ink-muted tabular">{moveNo(m)}.</span>
                <span className="font-medium text-ink">{sourceLabel(m)}</span>
                <ArrowRight size={14} className="text-ink-muted" aria-label="to" />
                <span className="font-medium text-ink">{targetLabel(m)}</span>
                <span className="tabular text-ink-secondary">{fxMoney(m.amount, fx)}</span>
                {m.note && <span className="text-xs text-ink-muted">— {m.note}</span>}
                <span className="ml-auto text-xs text-ink-muted">{m.at && date(m.at.slice(0, 10))}</span>
                {editable && (
                  <button
                    type="button"
                    onClick={() => removeMove(m.id)}
                    className="rounded p-1 text-ink-muted hover:bg-ink/5 hover:text-status-critical"
                    aria-label={`Undo move ${moveNo(m)}`}
                    title="Undo this move"
                  >
                    <Trash2 size={14} aria-hidden />
                  </button>
                )}
              </li>
            ))}
          </ol>
        )}
      </Card>

      <p className="text-xs text-ink-muted">
        Values as of {date(data.as_of)}
        {recorded && `, as kept in the saved session “${recorded.name}”`}.
      </p>
    </div>
  )
}

/** The keyboard and touch route to a move: drag and drop needs a mouse. */
function MoveButton({ onClick, label }: { onClick: () => void; label: string }) {
  return (
    <IconButton label={`Move money out of ${label}`} onClick={onClick}>
      <ArrowRight size={14} aria-hidden />
    </IconButton>
  )
}

function IconButton({
  label,
  onClick,
  danger = false,
  children,
}: {
  label: string
  onClick: () => void
  danger?: boolean
  children: ReactNode
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className={`rounded p-1 text-ink-muted hover:bg-ink/5 ${danger ? 'hover:text-status-critical' : 'hover:text-ink'}`}
    >
      {children}
    </button>
  )
}

function Swatch({ colour }: { colour: string }) {
  return <span className="h-3 w-3 shrink-0 rounded-sm" style={{ background: colour }} aria-hidden />
}

/**
 * The funds and accounts in one category, today and planned. Money moved
 * into a fund lands on its row; money moved into the category as a whole,
 * and everything moved out, sits on a row of its own, so the column adds up
 * to the category's planned value.
 */
/** One line inside a category: a fund, what is not itemised, or money placed in the category as a whole. */
interface SubLine {
  key: string
  label: string
  detail?: string
  /** `rest` and `whole` are not funds; they are shown in italics and cannot be deleted. */
  kind: 'fund' | 'rest' | 'whole'
  cls: string
  today: number
  change: number
  isNew?: boolean
}

/** A category's funds today, with the moves into and out of each. */
function subLines(subs: (Sub & { isNew: boolean })[], moves: Move[], bucket: string, current: number, unitemised: string): SubLine[] {
  const into = (sub?: string) =>
    moves.filter((m) => m.to === bucket && m.to_sub === sub).reduce((s, m) => s + m.amount, 0)
  const outOf = (sub?: string) =>
    moves.filter((m) => m.from === bucket && m.from_sub === sub).reduce((s, m) => s + m.amount, 0)
  const whole = into(undefined) - outOf(undefined)
  const residual = current - subs.reduce((s, x) => s + x.value, 0)
  return [
    ...subs.map((s) => ({
      key: s.key,
      label: s.label,
      detail: s.detail,
      kind: 'fund' as const,
      cls: subClassLabel(s.cls),
      today: s.value,
      change: into(s.key) - outOf(s.key),
      isNew: s.isNew,
    })),
    ...(Math.abs(residual) > EPS
      ? [{ key: '__rest__', label: unitemised, kind: 'rest' as const, cls: '', today: residual, change: 0 }]
      : []),
    ...(Math.abs(whole) > EPS
      ? [{ key: '__whole__', label: 'Category as a whole (no fund chosen)', kind: 'whole' as const, cls: '', today: 0, change: whole }]
      : []),
  ]
}

/** A line's status, as for a category. */
function lineStatus(l: SubLine): Status {
  const planned = l.today + l.change
  return l.isNew
    ? 'new'
    : Math.abs(l.change) <= EPS
      ? 'unchanged'
      : planned <= EPS
        ? 'exited'
        : l.change > 0
          ? 'added'
          : 'reduced'
}

function SubTable({
  fx,
  lines,
  currentTotal,
  plannedTotal,
  readOnly = false,
  onAddSub,
  onDeleteSub,
  onDeleteHeld,
}: {
  fx: Fx
  /** Undefined while the holdings are still loading. */
  lines: SubLine[] | undefined
  /** The overall totals, for each fund's share of the whole portfolio. */
  currentTotal: number
  plannedTotal: number
  /** Shown, not changed: no adding or deleting. */
  readOnly?: boolean
  onAddSub: () => void
  onDeleteSub: (key: string) => void
  /** Sells a fund held today: everything in it moves elsewhere. */
  onDeleteHeld: (key: string) => void
}) {
  if (!lines) return <p className="text-ink-muted">Loading funds…</p>
  const share = (v: number, total: number) => (
    <span className="block text-[11px] text-ink-muted">{pct(total ? (v / total) * 100 : 0, 1)}</span>
  )

  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <span className="font-medium text-ink-secondary">
          Subcategories <span className="font-normal text-ink-muted">· % of the whole portfolio</span>
        </span>
        {!readOnly && (
          <button type="button" onClick={onAddSub} className="inline-flex items-center gap-1 text-series-1 hover:underline">
            <Plus size={12} aria-hidden /> Add money / new subcategory
          </button>
        )}
      </div>
      {lines.length === 0 ? (
        <p className="pl-2 text-ink-muted">{readOnly ? 'Nothing in it.' : 'Nothing in it yet. Drop money here to add a fund.'}</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full">
            <thead className="text-[11px] text-ink-muted">
              <tr className="border-b border-[color:var(--hairline)]">
                <th className="py-1 pr-2 text-left font-medium">Fund / account</th>
                <th className="px-2 py-1 text-left font-medium">Classification</th>
                <th className="px-2 py-1 text-right font-medium">Today</th>
                <th className="px-2 py-1 text-right font-medium">Change</th>
                <th className="py-1 pl-2 text-right font-medium">Planned</th>
                <th className="w-6" />
              </tr>
            </thead>
            <tbody>
              {lines.map((l) => {
                const changed = Math.abs(l.change) > EPS
                return (
                  <tr
                    key={l.key}
                    className={`border-b border-[color:var(--hairline)] last:border-0 ${changed ? 'bg-series-1/[0.05]' : ''}`}
                  >
                    <td className="max-w-[14rem] py-1 pr-2">
                      <span className="flex items-start gap-1.5">
                        <span className="min-w-0">
                          {l.kind === 'fund' ? (
                            <>
                              <span className="text-ink">{l.label}</span>
                              {l.detail && <span className="block truncate text-[11px] text-ink-muted">{l.detail}</span>}
                            </>
                          ) : (
                            <span className="italic text-ink-muted">{l.label}</span>
                          )}
                        </span>
                        {l.isNew && <Badge tone="info">New</Badge>}
                        {!l.isNew && l.today > EPS && l.today + l.change <= EPS && <Badge tone="critical">Exited</Badge>}
                      </span>
                    </td>
                    <td className="px-2 py-1 text-ink-secondary">{l.cls}</td>
                    <td className="px-2 py-1 text-right tabular text-ink-secondary">
                      {l.today ? (
                        <>
                          {fxMoney(l.today, fx)}
                          {share(l.today, currentTotal)}
                        </>
                      ) : (
                        '—'
                      )}
                    </td>
                    <td className={`px-2 py-1 text-right tabular ${deltaClass(changed ? l.change : 0)}`}>
                      {changed ? fxSigned(l.change, fx) : '—'}
                    </td>
                    <td className="py-1 pl-2 text-right tabular text-ink">
                      {fxMoney(l.today + l.change, fx)}
                      {share(l.today + l.change, plannedTotal)}
                    </td>
                    <td className="pl-1 text-right">
                      {readOnly ? null : l.isNew ? (
                        <IconButton label="Delete this subcategory and its moves" danger onClick={() => onDeleteSub(l.key)}>
                          <Trash2 size={12} aria-hidden />
                        </IconButton>
                      ) : (
                        l.kind === 'fund' &&
                        l.today + l.change > EPS && (
                          <IconButton label="Delete: move all of this fund elsewhere" danger onClick={() => onDeleteHeld(l.key)}>
                            <Trash2 size={12} aria-hidden />
                          </IconButton>
                        )
                      )}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}

/** A name and a classification, for a category made without a move into it. */
function NewCategoryForm({
  existing,
  onAdd,
  onCancel,
}: {
  existing: Bucket[]
  onAdd: (label: string, cls: ClassKey) => void
  onCancel: () => void
}) {
  const [name, setName] = useState('')
  const [cls, setCls] = useState<ClassKey>('equity_fund')
  const clash = existing.some((b) => b.label.toLowerCase() === name.trim().toLowerCase())
  const ok = name.trim() !== '' && !clash
  return (
    <div
      className="space-y-2 rounded-md border border-[color:var(--hairline)] p-3"
      onKeyDown={(e) => {
        if (e.key === 'Enter' && ok) onAdd(name.trim(), cls)
        if (e.key === 'Escape') onCancel()
      }}
    >
      <NewCategoryFields name={name} setName={setName} cls={cls} setCls={setCls} />
      {clash && <p className="text-xs text-ink-muted">A category with that name exists.</p>}
      <div className="flex gap-2">
        <Button variant="primary" disabled={!ok} onClick={() => onAdd(name.trim(), cls)}>
          Add category
        </Button>
        <Button variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </div>
  )
}

function NewCategoryFields({
  name,
  setName,
  cls,
  setCls,
}: {
  name: string
  setName: (v: string) => void
  cls: ClassKey
  setCls: (v: ClassKey) => void
}) {
  return (
    <div className="grid gap-2 sm:grid-cols-2">
      <Field label="Category name">
        <input
          autoFocus
          className={inputClass}
          value={name}
          placeholder="e.g. Gold ETF, REITs, Small cap"
          onChange={(e) => setName(e.target.value)}
        />
      </Field>
      <Field label="Classification">
        <select className={inputClass} value={cls} onChange={(e) => setCls(e.target.value as ClassKey)}>
          {CLASSES.map((c) => (
            <option key={c.key} value={c.key}>
              {c.label}
            </option>
          ))}
        </select>
      </Field>
    </div>
  )
}

/** Somewhere a move can start: a category, `category|fund`, or fresh money. */
interface Source {
  key: string
  label: string
  group: string
}

/** Choices in the fund picker besides the funds themselves. */
const WHOLE = '__whole__'
const NEW_SUB = '__new_sub__'
const isNewDest = (dest: 'existing' | 'new') => dest === 'new'

/** Asks from where, to where and how much: after a drop, a → or a + button, or a delete. */
function PendingMove({
  pending,
  fx,
  targets,
  sources,
  labelOf,
  available,
  held,
  subsOf,
  subsLoading,
  defaultClass,
  onCancel,
  onConfirm,
}: {
  pending: Pending
  fx: Fx
  /** Categories money can go to. */
  targets: Bucket[]
  /** Where money can come from, for a move started on its target. */
  sources: Source[]
  labelOf: (key: string) => string
  available: (key: string) => number
  /** What a source holds before any move: percentages are of this, not of what is left. */
  held: (key: string) => number
  /** The funds and accounts inside a category, to merge into. */
  subsOf: (key: string) => (Sub & { isNew: boolean })[]
  subsLoading: boolean
  /** The classification a new subcategory starts with in a category. */
  defaultClass: (key: string) => ClassKey
  onCancel: () => void
  onConfirm: (m: {
    from: string
    to: string | null
    amount: number
    note: string
    newBucket?: { label: string; cls: ClassKey }
    /** An existing fund or account in `to` to merge into. */
    toSub?: string
    /** A new subcategory to make in the target and move into. */
    newSub?: { label: string; cls: ClassKey }
  }) => void
}) {
  // Percentages are of what the source holds today, so two 25% moves out of
  // one source take half of it, not 25% and then 25% of the rest.
  const pctLeft = (src: string) => {
    const total = src ? held(src) : 0
    return total > 0 ? Math.floor((available(src) / total) * 1000) / 10 : 0
  }
  const [from, setFromState] = useState<string>(pending.from ?? '')
  const [dest, setDest] = useState<'existing' | 'new'>(pending.to === null ? 'new' : 'existing')
  const [to, setTo] = useState<string>(pending.to ?? '')
  const [name, setName] = useState('')
  const [cls, setCls] = useState<ClassKey>('equity_fund')
  const [by, setBy] = useState<'amount' | 'pct'>('pct')
  // Start at everything still free to move.
  const [value, setValue] = useState<number>(() => (pending.from ? pctLeft(pending.from) : 100))
  const setFrom = (src: string) => {
    setFromState(src)
    if (by === 'pct') setValue(src ? pctLeft(src) : 100)
  }
  const [note, setNote] = useState('')
  // Inside an existing category: '' still to choose, WHOLE the category as a
  // whole, NEW_SUB a new subcategory, else the key of a fund to merge into.
  const [sub, setSub] = useState('')
  const [subName, setSubName] = useState('')
  const [subCls, setSubCls] = useState<ClassKey>(() => (pending.to ? defaultClass(pending.to) : 'equity_fund'))
  const chooseTo = (key: string) => {
    setTo(key)
    setSub('')
    if (key) setSubCls(defaultClass(key))
  }

  const subs = !isNewDest(dest) && to ? subsOf(to) : []
  const left = from ? available(from) : 0
  const total = from ? held(from) : 0
  const maxPct = from ? pctLeft(from) : 0
  // At the shown maximum, take exactly what is left, so rounding leaves no crumbs behind.
  const amount = pending.remove
    ? left
    : by === 'pct'
      ? value >= maxPct && value <= maxPct + 0.1
        ? left
        : (total * value) / 100
      : value
  const pctAfter = total > 0 ? Math.max(0, ((left - amount) / total) * 100) : 0
  const isNew = dest === 'new'
  const problem = !from
    ? 'Choose where the money comes from.'
    : amount <= EPS
      ? 'Enter an amount above zero.'
      : amount > left + EPS
        ? by === 'pct'
          ? `Only ${pct(maxPct, 1)} of ${labelOf(from)} is left to move.`
          : `Only ${fxMoney(left, fx)} is left to move from ${labelOf(from)}.`
        : !isNew && !to
          ? 'Choose the category it goes to.'
          : !isNew && to === from
            ? 'Choose a different category to move it to.'
            : !isNew && sub && from === `${to}|${sub}`
              ? 'That is the fund it comes from; choose another.'
            : isNew && !name.trim()
              ? 'Name the new category.'
              : isNew && targets.some((b) => b.label.toLowerCase() === name.trim().toLowerCase())
                ? 'A category with that name exists; choose it under Existing.'
                : !isNew && !sub
                  ? 'Choose a fund to merge into, or a new subcategory.'
                  : !isNew && sub === NEW_SUB && !subName.trim()
                    ? 'Name the new subcategory.'
                    : !isNew && sub === NEW_SUB && subs.some((x) => x.label.toLowerCase() === subName.trim().toLowerCase())
                      ? 'That fund is already in this category; merge into it instead.'
                      : null

  const confirm = () => {
    if (problem) return
    onConfirm({
      from,
      to: isNew ? null : to,
      amount: Math.min(amount, left),
      note: note.trim(),
      newBucket: isNew ? { label: name.trim(), cls } : undefined,
      toSub: !isNew && sub !== WHOLE && sub !== NEW_SUB ? sub : undefined,
      newSub: isNew
        ? subName.trim()
          ? { label: subName.trim(), cls }
          : undefined
        : sub === NEW_SUB
          ? { label: subName.trim(), cls: subCls }
          : undefined,
    })
  }

  const segmented = <T extends string>(value: T, set: (v: T) => void, options: readonly (readonly [T, string])[], label: string) => (
    <ToggleGroup label={label} options={options} value={value} onChange={set} />
  )

  return (
    <div
      role="group"
      aria-label="New move"
      className="mb-3 space-y-3 rounded-md border border-series-1/50 bg-series-1/5 p-3"
      onKeyDown={(e) => {
        if (e.key === 'Enter' && (e.target as HTMLElement).tagName !== 'SELECT') confirm()
        if (e.key === 'Escape') onCancel()
      }}
    >
      <div className="flex flex-wrap items-center gap-2 text-sm">
        {pending.remove ? (
          <span className="text-ink">
            Delete <span className="font-medium">{labelOf(from)}</span>: move all{' '}
            <span className="tabular font-medium">{fxMoney(left, fx)}</span> to
          </span>
        ) : pending.from === null ? (
          <select className={`${inputClass} w-auto`} value={from} onChange={(e) => setFrom(e.target.value)} aria-label="Move from">
            <option value="">From…</option>
            {[...new Set(sources.map((s) => s.group))].map((g) => (
              <optgroup key={g} label={g}>
                {sources
                  .filter((s) => s.group === g)
                  .map((s) => (
                    <option key={s.key} value={s.key}>
                      {s.label} ({fxMoney(available(s.key), fx)})
                    </option>
                  ))}
              </optgroup>
            ))}
          </select>
        ) : (
          <span className="font-medium text-ink">{labelOf(from)}</span>
        )}
        {!pending.remove && <ArrowRight size={14} className="text-ink-muted" aria-label="to" />}
        {!pending.remove && pending.from !== null && (
          <span className="ml-auto text-xs text-ink-muted">
            {fxMoney(left, fx)} available
            {total > 0 && ` · ${pct(maxPct, 1)} of ${fxMoney(total, fx)}`}
          </span>
        )}
      </div>

      <div className="space-y-2">
        {segmented(
          dest,
          setDest,
          [
            ['existing', 'Existing category'],
            ['new', 'New category'],
          ] as const,
          'Move to',
        )}
        {isNew ? (
          <>
            <NewCategoryFields name={name} setName={setName} cls={cls} setCls={setCls} />
            <Field label="First subcategory (optional)" hint="A fund or holding inside the new category, with the same classification">
              <input
                className={inputClass}
                value={subName}
                placeholder="e.g. Nippon Gold BeES"
                onChange={(e) => setSubName(e.target.value)}
              />
            </Field>
          </>
        ) : (
          <select className={inputClass} value={to} onChange={(e) => chooseTo(e.target.value)} aria-label="Move to">
            <option value="">Choose…</option>
            {targets
              .filter((b) => b.key !== from)
              .map((b) => (
                <option key={b.key} value={b.key}>
                  {b.label}
                </option>
              ))}
          </select>
        )}

        {!isNew && to && (
          <div className="space-y-2 rounded-md border border-[color:var(--hairline)] bg-surface p-2.5">
            <Field label={`Into which fund in ${labelOf(to)}?`}>
              <select className={inputClass} value={sub} onChange={(e) => setSub(e.target.value)} aria-label="Into which fund">
                <option value="">{subsLoading ? 'Loading funds…' : 'Choose…'}</option>
                {subs.length > 0 && (
                  <optgroup label="Merge into">
                    {subs.map((x) => (
                      <option key={x.key} value={x.key}>
                        {x.label} · {subClassLabel(x.cls)}
                        {x.isNew ? ' (new in plan)' : ` · ${fxMoney(x.value, fx)}`}
                      </option>
                    ))}
                  </optgroup>
                )}
                <optgroup label="Or">
                  <option value={NEW_SUB}>+ New subcategory / fund…</option>
                  <option value={WHOLE}>The category as a whole (decide the fund later)</option>
                </optgroup>
              </select>
            </Field>
            {sub === NEW_SUB && (
              <div className="grid gap-2 sm:grid-cols-2">
                <Field label="Subcategory / fund name">
                  <input
                    autoFocus
                    className={inputClass}
                    value={subName}
                    placeholder="e.g. Vanguard Total World ETF"
                    onChange={(e) => setSubName(e.target.value)}
                  />
                </Field>
                <Field label="Classification">
                  <select className={inputClass} value={subCls} onChange={(e) => setSubCls(e.target.value as ClassKey)}>
                    {CLASSES.map((c) => (
                      <option key={c.key} value={c.key}>
                        {c.label}
                      </option>
                    ))}
                  </select>
                </Field>
              </div>
            )}
          </div>
        )}
      </div>

      {!pending.remove && (
        <div className="flex flex-wrap items-end gap-3">
          {segmented(
            by,
            (v) => {
              if (v === by) return
              // Carry the figure across, so switching does not lose it.
              setValue(
                v === 'pct' ? (total > 0 ? Math.min(maxPct, Math.round((amount / total) * 1000) / 10) : 0) : Math.round(amount),
              )
              setBy(v)
            },
            [
              ['pct', '%'],
              ['amount', currencySymbol(fx.code)],
            ] as const,
            'Move by',
          )}
          <div className="w-40">
            {by === 'pct' ? (
              <div className="flex items-center gap-2">
                <input
                  type="range"
                  min={0}
                  max={maxPct}
                  step="any"
                  value={Math.min(maxPct, value)}
                  onChange={(e) => setValue(Math.round(Number(e.target.value) * 10) / 10)}
                  className="w-full accent-[var(--series-1)]"
                  aria-label="Percentage to move"
                />
                <input
                  type="number"
                  min={0}
                  max={maxPct}
                  className={`${inputClass} w-16 tabular`}
                  value={value}
                  onChange={(e) => setValue(Number(e.target.value) || 0)}
                  aria-label="Percentage to move"
                />
              </div>
            ) : (
              <MoneyInput
                value={inDisplay(value, fx)}
                currency={fx.code}
                prefix={currencySymbol(fx.code)}
                onChange={(v) => setValue(v / fx.rate)}
                ariaLabel="Amount to move"
              />
            )}
          </div>
          <div className="text-sm text-ink-secondary">
            = <span className="tabular font-medium text-ink">{fxMoney(amount, fx)}</span>
            {by === 'pct' && from && (
              <span className="text-xs text-ink-muted">
                {' '}
                of {fxMoney(total, fx)} · {pct(maxPct, 1)} available → {pct(pctAfter, 1)} after
              </span>
            )}
          </div>
        </div>
      )}

      <Field label="Note (optional)">
        <input
          className={inputClass}
          value={note}
          placeholder="Why - e.g. rebalance after rally, FD maturing in March"
          onChange={(e) => setNote(e.target.value)}
        />
      </Field>

      {problem && <p className="text-xs text-ink-muted">{problem}</p>}
      <div className="flex gap-2">
        <Button variant="primary" onClick={confirm} disabled={!!problem}>
          {pending.remove ? 'Move and delete' : 'Add move'}
        </Button>
        <Button variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </div>
  )
}

interface TableRow {
  key: string
  label: string
  status: Status
  current: number
  currentPct: number
  change: number
  planned: number
  plannedPct: number
}

type Model = ReturnType<typeof buildModel>

/** Today's values, the moves applied to them, and the comparison table. */
function buildModel(rows: Segment[], plan: Plan) {
  const createdKeys = new Set(plan.created.map((b) => b.key))
  const buckets: Bucket[] = [
    ...rows.map((r) => ({ key: r.key, label: r.label })),
    ...plan.created.map(({ key, label }) => ({ key, label })),
  ]
  const current: Record<string, number> = Object.fromEntries(rows.map((r) => [r.key, r.value_base]))
  const planned: Record<string, number> = { ...current }
  const inflow: Record<string, number> = {}
  const outflow: Record<string, number> = {}
  for (const b of plan.created) planned[b.key] = 0

  let freshUsed = 0
  for (const m of plan.moves) {
    // A move out of a category no longer in this view is skipped, not guessed at.
    const known = (k: string) => k === FRESH || k in planned
    if (!known(m.from) || !known(m.to)) continue
    if (m.from === FRESH) freshUsed += m.amount
    else {
      planned[m.from] -= m.amount
      outflow[m.from] = (outflow[m.from] ?? 0) + m.amount
    }
    planned[m.to] += m.amount
    inflow[m.to] = (inflow[m.to] ?? 0) + m.amount
  }

  const currentTotal = rows.reduce((s, r) => s + r.value_base, 0)
  const plannedTotal = currentTotal + freshUsed
  const table: TableRow[] = buckets.map((b) => {
    const cur = current[b.key] ?? 0
    const pl = planned[b.key] ?? 0
    const change = pl - cur
    const status: Status = createdKeys.has(b.key)
      ? 'new'
      : Math.abs(change) <= EPS
        ? 'unchanged'
        : pl <= EPS
          ? 'exited'
          : change > 0
            ? 'added'
            : 'reduced'
    return {
      key: b.key,
      label: b.label,
      status,
      current: cur,
      currentPct: currentTotal ? (cur / currentTotal) * 100 : 0,
      change,
      planned: pl,
      plannedPct: plannedTotal ? (pl / plannedTotal) * 100 : 0,
    }
  })

  // Colour follows the category, in a fixed order: today's first, then new ones.
  const colour: Record<string, string> = Object.fromEntries(
    buckets.map((b, i) => [b.key, i < 8 ? `var(--series-${i + 1})` : 'color-mix(in srgb, var(--series-8) 50%, transparent)']),
  )

  return {
    buckets,
    current,
    planned,
    inflow,
    outflow,
    currentTotal,
    plannedTotal,
    freshLeft: Math.max(0, plan.fresh - freshUsed),
    table,
    colour,
  }
}

type ChartKind = 'bars' | 'pie'

/**
 * Two stacked bars on one 0-100% scale, today then planned, or the same as
 * two rings. A changed
 * category, in either bar or the legend, opens what changed in it and the
 * moves behind that.
 */
function AllocationBars({
  chart,
  model,
  fx,
  moves,
  sourceLabel,
  targetLabel,
}: {
  /** Two stacked bars, or two rings. */
  chart: ChartKind
  model: Model
  fx: Fx
  moves: Move[]
  sourceLabel: (m: Move) => string
  targetLabel: (m: Move) => string
}) {
  const [picked, setPicked] = useState<string | null>(null)
  const rowOf = (key: string) => model.table.find((r) => r.key === key)
  const changed = (key: string) => (rowOf(key)?.status ?? 'unchanged') !== 'unchanged'
  // A pick whose moves have all been undone closes by itself.
  const selected = picked && changed(picked) ? rowOf(picked) : undefined
  const toggle = (key: string) => setPicked((k) => (k === key ? null : key))

  const bars: { label: string; total: number; parts: { key: string; label: string; value: number; share: number }[] }[] = [
    {
      label: 'Today',
      total: model.currentTotal,
      parts: model.table.map((r) => ({ key: r.key, label: r.label, value: r.current, share: r.currentPct })),
    },
    {
      label: 'Planned',
      total: model.plannedTotal,
      parts: model.table.map((r) => ({ key: r.key, label: r.label, value: r.planned, share: r.plannedPct })),
    },
  ]
  const hint = model.table.some((r) => r.status !== 'unchanged') && !selected && (
    <p className="text-xs text-ink-muted">
      {chart === 'pie'
        ? 'Click a changed category, in a ring or its legend, to see what changes in it.'
        : 'Click a changed category, in a bar or underlined above, to see what changes in it.'}
    </p>
  )
  const detail = selected && (
    <ChangeDetail
      row={selected}
      fx={fx}
      colour={model.colour[selected.key]}
      moves={moves.filter((m) => m.from === selected.key || m.to === selected.key)}
      sourceLabel={sourceLabel}
      targetLabel={targetLabel}
      onClose={() => setPicked(null)}
    />
  )
  if (chart === 'pie') {
    return (
      <div className="space-y-3">
        <div className="grid gap-6 lg:grid-cols-2">
          {bars.map((bar) => (
            <PlanDonut
              key={bar.label}
              title={bar.label}
              total={bar.total}
              parts={bar.parts}
              colour={model.colour}
              fx={fx}
              changed={changed}
              selected={selected?.key}
              onPick={toggle}
            />
          ))}
        </div>
        {hint}
        {detail}
      </div>
    )
  }
  return (
    <div className="space-y-3">
      {bars.map((bar) => (
        <div key={bar.label} className="flex items-center gap-3">
          <div className="w-16 shrink-0 text-xs font-medium text-ink-secondary">{bar.label}</div>
          <div
            className="flex h-7 flex-1 gap-[2px] overflow-hidden rounded"
            role="group"
            aria-label={`${bar.label}: ${bar.parts
              .filter((p) => p.share > 0.05)
              .map((p) => `${p.label} ${pct(p.share, 1)}`)
              .join(', ')}`}
          >
            {bar.parts
              .filter((p) => p.share > 0.05)
              .map((p) => {
                const tip = `${p.label}: ${fxMoney(p.value, fx)} (${pct(p.share, 1)})`
                const style = {
                  width: `${p.share}%`,
                  background: model.colour[p.key],
                  opacity: selected && selected.key !== p.key ? 0.3 : undefined,
                }
                const cls =
                  'flex h-full min-w-0 items-center justify-center overflow-hidden text-[11px] font-semibold text-white transition-[width,opacity] first:rounded-l last:rounded-r [text-shadow:0_1px_1px_rgb(0_0_0/0.35)]'
                // The share sits inside the segment where it fits.
                const label = p.share >= 5 ? <span className="tabular pointer-events-none" aria-hidden>{pct(p.share, 0)}</span> : null
                return changed(p.key) ? (
                  <button
                    key={p.key}
                    type="button"
                    onClick={() => toggle(p.key)}
                    aria-pressed={selected?.key === p.key}
                    aria-label={`${tip}, changed: show what changes`}
                    title={`${tip} · click for what changes`}
                    className={`${cls} cursor-pointer hover:brightness-110 focus-visible:outline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ink`}
                    style={style}
                  >
                    {label}
                  </button>
                ) : (
                  <div key={p.key} className={cls} style={style} title={tip}>
                    {label}
                  </div>
                )
              })}
          </div>
          <div className="w-28 shrink-0 text-right text-xs tabular text-ink-secondary">
            {fxMoney(bar.total, fx)}
          </div>
        </div>
      ))}
      <ul className="flex flex-wrap gap-x-4 gap-y-1 pt-1 text-xs text-ink-secondary">
        {model.buckets.map((b) => (
          <li key={b.key}>
            {changed(b.key) ? (
              <button
                type="button"
                onClick={() => toggle(b.key)}
                aria-pressed={selected?.key === b.key}
                className={`flex items-center gap-1.5 rounded px-1 -mx-1 underline decoration-dotted underline-offset-2 hover:bg-ink/5 hover:text-ink ${selected?.key === b.key ? 'bg-ink/5 font-medium text-ink' : ''}`}
              >
                <Swatch colour={model.colour[b.key]} />
                {b.label}
              </button>
            ) : (
              <span className="flex items-center gap-1.5">
                <Swatch colour={model.colour[b.key]} />
                {b.label}
              </span>
            )}
          </li>
        ))}
      </ul>
      {hint}
      {detail}
    </div>
  )
}

/** SVG path for a ring segment from `a0` to `a1` radians, 12 o'clock = 0. */
function arcPath(c: number, r0: number, r1: number, a0: number, a1: number): string {
  if (a1 - a0 >= Math.PI * 2 - 1e-6) a1 = a0 + Math.PI * 2 - 1e-4 // a full ring still draws
  const large = a1 - a0 > Math.PI ? 1 : 0
  const p = (r: number, a: number) => `${c + r * Math.sin(a)},${c - r * Math.cos(a)}`
  return `M${p(r1, a0)} A${r1},${r1} 0 ${large} 1 ${p(r1, a1)} L${p(r0, a1)} A${r0},${r0} 0 ${large} 0 ${p(r0, a0)} Z`
}

/**
 * One of the two rings, today or planned, as the plan viewer draws them:
 * each category in the colour it has in the bars, the total in the hole,
 * and a legend with share and amount. A changed category opens what
 * changes in it, from its slice or its legend line.
 */
function PlanDonut({
  title,
  total,
  parts,
  colour,
  fx,
  changed,
  selected,
  onPick,
}: {
  title: string
  total: number
  parts: { key: string; label: string; value: number; share: number }[]
  colour: Record<string, string>
  fx: Fx
  changed: (key: string) => boolean
  selected: string | undefined
  onPick: (key: string) => void
}) {
  const [hovered, setHovered] = useState<string | null>(null)
  const shown = parts.filter((p) => p.value > EPS)
  const SIZE = 220
  let angle = 0
  const slices = shown.map((p) => {
    const a0 = angle
    angle += (p.share / 100) * Math.PI * 2
    return { ...p, a0, a1: angle }
  })
  const active = shown.find((p) => p.key === hovered)
  const dim = (key: string) => (hovered ?? selected) !== undefined && (hovered ?? selected) !== key
  return (
    <div>
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-secondary">{title}</h3>
      <div className="flex flex-wrap items-center gap-5">
        <svg
          width={SIZE}
          height={SIZE}
          viewBox={`0 0 ${SIZE} ${SIZE}`}
          role="img"
          aria-label={`${title}: ${shown.map((p) => `${p.label} ${pct(p.share, 1)}`).join(', ')}`}
          className="shrink-0 overflow-visible"
        >
          {slices.map((s) => (
            <path
              key={s.key}
              d={arcPath(SIZE / 2, 62, 104, s.a0, Math.max(s.a1, s.a0 + 0.012))}
              fill={colour[s.key]}
              stroke="var(--surface-1)"
              strokeWidth={2}
              className={`transition-opacity ${changed(s.key) ? 'cursor-pointer' : ''}`}
              style={{ opacity: dim(s.key) ? 0.35 : 1 }}
              onMouseEnter={() => setHovered(s.key)}
              onMouseLeave={() => setHovered(null)}
              onClick={changed(s.key) ? () => onPick(s.key) : undefined}
            >
              <title>{`${s.label}: ${fxMoney(s.value, fx)} (${pct(s.share, 1)})`}</title>
            </path>
          ))}
          <text x={SIZE / 2} y={SIZE / 2 - 6} textAnchor="middle" fontSize={12} className="fill-[color:var(--text-muted)]">
            {active ? pct(active.share, 1) : 'Total'}
          </text>
          <text
            x={SIZE / 2}
            y={SIZE / 2 + 16}
            textAnchor="middle"
            fontSize={18}
            className="tabular fill-[color:var(--text-primary)] font-semibold"
          >
            {fxMoney(active ? active.value : total, fx)}
          </text>
        </svg>
        <ul className="min-w-[12rem] flex-1 space-y-1 text-xs">
          {shown.map((p) => {
            const line = (
              <>
                <Swatch colour={colour[p.key]} />
                <span className="min-w-0 flex-1 text-left">{p.label}</span>
                <span className="tabular font-semibold text-ink">{pct(p.share, 1)}</span>
                <span className="tabular w-20 text-right text-ink-muted">{fxMoney(p.value, fx)}</span>
              </>
            )
            return (
              <li key={p.key} onMouseEnter={() => setHovered(p.key)} onMouseLeave={() => setHovered(null)}>
                {changed(p.key) ? (
                  <button
                    type="button"
                    onClick={() => onPick(p.key)}
                    aria-pressed={selected === p.key}
                    className={`-mx-1 flex w-full items-center gap-2 rounded px-1 py-0.5 text-ink-secondary hover:bg-ink/5 hover:text-ink ${
                      selected === p.key ? 'bg-ink/5 font-medium text-ink' : ''
                    }`}
                  >
                    {line}
                  </button>
                ) : (
                  <span className="flex items-center gap-2 py-0.5 text-ink-secondary">{line}</span>
                )}
              </li>
            )
          })}
        </ul>
      </div>
    </div>
  )
}

/** One changed category: today against planned, and each move in or out of it. */
function ChangeDetail({
  row,
  fx,
  colour,
  moves,
  sourceLabel,
  targetLabel,
  onClose,
}: {
  row: TableRow
  fx: Fx
  colour: string
  moves: Move[]
  sourceLabel: (m: Move) => string
  targetLabel: (m: Move) => string
  onClose: () => void
}) {
  const shift = row.plannedPct - row.currentPct
  const stat = (label: string, value: ReactNode, cls = 'text-ink') => (
    <div>
      <dt className="text-ink-muted">{label}</dt>
      <dd className={`tabular font-medium ${cls}`}>{value}</dd>
    </div>
  )
  return (
    <div className="rounded-md border border-[color:var(--hairline)] bg-ink/[0.03] p-3 text-sm">
      <div className="mb-2 flex items-center gap-2">
        <Swatch colour={colour} />
        <span className="font-medium text-ink">{row.label}</span>
        <Badge tone={STATUS[row.status].tone}>{STATUS[row.status].label}</Badge>
        <span className="ml-auto">
          <IconButton label="Close" onClick={onClose}>
            <X size={14} aria-hidden />
          </IconButton>
        </span>
      </div>
      <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs sm:grid-cols-4">
        {stat('Today', `${fxMoney(row.current, fx)} · ${pct(row.currentPct, 1)}`, 'text-ink-secondary')}
        {stat('Change', fxSigned(row.change, fx), deltaClass(row.change))}
        {stat('Planned', `${fxMoney(row.planned, fx)} · ${pct(row.plannedPct, 1)}`)}
        {stat(
          'Shift',
          Math.abs(shift) >= 0.05 ? `${shift > 0 ? '+' : '−'}${num(Math.abs(shift), 1)} pp` : '—',
          deltaClass(Math.abs(shift) >= 0.05 ? shift : 0),
        )}
      </dl>
      {moves.length === 0 ? (
        <p className="mt-2 text-xs text-ink-muted">No moves into or out of it.</p>
      ) : (
        <ul className="mt-2 divide-y divide-[color:var(--hairline)] border-t border-[color:var(--hairline)]">
          {moves.map((m) => {
            const into = m.to === row.key
            return (
              <li key={m.id} className="flex flex-wrap items-center gap-2 py-1.5 text-xs">
                <span className={`w-24 shrink-0 tabular font-medium ${deltaClass(into ? 1 : -1)}`}>
                  {fxSigned(into ? m.amount : -m.amount, fx)}
                </span>
                <span className="text-ink-secondary">{sourceLabel(m)}</span>
                <ArrowRight size={12} className="text-ink-muted" aria-label="to" />
                <span className="text-ink-secondary">{targetLabel(m)}</span>
                {m.note && <span className="text-ink-muted">— {m.note}</span>}
                <span className="ml-auto text-ink-muted">{date(m.at.slice(0, 10))}</span>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

function ChangeTable({
  model,
  fx,
  linesOf,
}: {
  model: Model
  fx: Fx
  /** A category's funds and their changes; undefined while they load. */
  linesOf: (key: string) => SubLine[] | undefined
}) {
  const [open, setOpen] = useState<Set<string>>(new Set())
  const toggle = (key: string) =>
    setOpen((s) => {
      const next = new Set(s)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  const allOpen = model.table.length > 0 && model.table.every((r) => open.has(r.key))
  const totalChange = model.plannedTotal - model.currentTotal
  const th = (children: ReactNode, right = true) => (
    <th className={`px-2 py-2 font-medium ${right ? 'text-right' : 'text-left'}`}>{children}</th>
  )
  const shiftCell = (shift: number) => (
    <td className={`px-2 py-2 text-right tabular ${deltaClass(Math.abs(shift) >= 0.05 ? shift : 0)}`}>
      {Math.abs(shift) >= 0.05 ? `${shift > 0 ? '+' : '−'}${num(Math.abs(shift), 1)} pp` : '—'}
    </td>
  )
  /** A category's funds, indented under it; shares are of the whole portfolio. */
  const subRows = (key: string) => {
    const lines = linesOf(key)
    if (!lines)
      return (
        <tr className="border-b border-[color:var(--hairline)] text-xs">
          <td colSpan={8} className="py-1.5 pl-10 text-ink-muted">
            Loading funds…
          </td>
        </tr>
      )
    if (lines.length === 0)
      return (
        <tr className="border-b border-[color:var(--hairline)] text-xs">
          <td colSpan={8} className="py-1.5 pl-10 text-ink-muted">
            No funds itemised.
          </td>
        </tr>
      )
    return lines.map((l) => {
      const planned = l.today + l.change
      const status = lineStatus(l)
      const changed = Math.abs(l.change) > EPS
      const curPct = model.currentTotal ? (l.today / model.currentTotal) * 100 : 0
      const plPct = model.plannedTotal ? (planned / model.plannedTotal) * 100 : 0
      return (
        <tr key={`${key}|${l.key}`} className="border-b border-[color:var(--hairline)] text-xs">
          <td className="py-1.5 pl-10 pr-2">
            <span className={l.kind === 'fund' ? (changed ? 'text-ink' : 'text-ink-secondary') : 'italic text-ink-muted'}>
              {l.label}
            </span>
          </td>
          <td className="px-2 py-1.5">{status !== 'unchanged' && <Badge tone={STATUS[status].tone}>{STATUS[status].label}</Badge>}</td>
          <td className="px-2 py-1.5 text-right tabular text-ink-secondary">{l.today ? fxMoney(l.today, fx) : '—'}</td>
          <td className="px-2 py-1.5 text-right tabular text-ink-secondary">{l.today ? pct(curPct, 1) : '—'}</td>
          <td className={`px-2 py-1.5 text-right tabular ${deltaClass(changed ? l.change : 0)}`}>
            {changed ? fxSigned(l.change, fx) : '—'}
          </td>
          <td className="px-2 py-1.5 text-right tabular text-ink">{fxMoney(planned, fx)}</td>
          <td className="px-2 py-1.5 text-right tabular text-ink">{pct(plPct, 1)}</td>
          {shiftCell(plPct - curPct)}
        </tr>
      )
    })
  }
  return (
    <div className="overflow-x-auto">
      <div className="mb-2 flex justify-end">
        <button
          type="button"
          onClick={() => setOpen(allOpen ? new Set() : new Set(model.table.map((r) => r.key)))}
          className="inline-flex items-center gap-1 text-xs text-series-1 hover:underline"
        >
          {allOpen ? <ChevronDown size={12} aria-hidden /> : <ChevronRight size={12} aria-hidden />}
          {allOpen ? 'Collapse all' : 'Show subcategories'}
        </button>
      </div>
      <table className="w-full text-sm">
        <thead className="border-b border-[color:var(--hairline)] text-xs text-ink-muted">
          <tr>
            {th('Category', false)}
            {th('Status', false)}
            {th('Today')}
            {th('Today %')}
            {th('Change (+ / −)')}
            {th('Planned')}
            {th('Planned %')}
            {th('Shift')}
          </tr>
        </thead>
        <tbody>
          {model.table.map((r) => {
            const changed = r.status !== 'unchanged'
            const expanded = open.has(r.key)
            return (
              <Fragment key={r.key}>
                <tr className={`border-b border-[color:var(--hairline)] ${changed ? 'bg-series-1/[0.04]' : ''}`}>
                  <td className={`px-2 py-2 ${changed ? 'border-l-2 border-l-series-1' : 'border-l-2 border-l-transparent'}`}>
                    <span className="flex items-center gap-2">
                      <button
                        type="button"
                        onClick={() => toggle(r.key)}
                        aria-expanded={expanded}
                        aria-label={`${expanded ? 'Collapse' : 'Expand'} ${r.label}`}
                        className="rounded p-0.5 text-ink-muted hover:bg-ink/5 hover:text-ink"
                      >
                        {expanded ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
                      </button>
                      <Swatch colour={model.colour[r.key]} />
                      <span className={changed ? 'font-medium text-ink' : 'text-ink-secondary'}>{r.label}</span>
                    </span>
                  </td>
                  <td className="px-2 py-2">
                    <Badge tone={STATUS[r.status].tone}>{STATUS[r.status].label}</Badge>
                  </td>
                  <td className="px-2 py-2 text-right tabular text-ink-secondary">{fxMoney(r.current, fx)}</td>
                  <td className="px-2 py-2 text-right tabular text-ink-secondary">{pct(r.currentPct, 1)}</td>
                  <td className={`px-2 py-2 text-right tabular font-medium ${deltaClass(Math.abs(r.change) > EPS ? r.change : 0)}`}>
                    {Math.abs(r.change) > EPS ? fxSigned(r.change, fx) : '—'}
                  </td>
                  <td className="px-2 py-2 text-right tabular text-ink">{fxMoney(r.planned, fx)}</td>
                  <td className="px-2 py-2 text-right tabular text-ink">{pct(r.plannedPct, 1)}</td>
                  {shiftCell(r.plannedPct - r.currentPct)}
                </tr>
                {expanded && subRows(r.key)}
              </Fragment>
            )
          })}
        </tbody>
        <tfoot className="text-ink">
          <tr className="font-medium">
            <td className="px-2 py-2">Total</td>
            <td />
            <td className="px-2 py-2 text-right tabular">{fxMoney(model.currentTotal, fx)}</td>
            <td className="px-2 py-2 text-right tabular">100%</td>
            <td className={`px-2 py-2 text-right tabular ${deltaClass(Math.abs(totalChange) > EPS ? totalChange : 0)}`}>
              {Math.abs(totalChange) > EPS ? fxSigned(totalChange, fx) : '—'}
            </td>
            <td className="px-2 py-2 text-right tabular">{fxMoney(model.plannedTotal, fx)}</td>
            <td className="px-2 py-2 text-right tabular">100%</td>
            <td />
          </tr>
        </tfoot>
      </table>
    </div>
  )
}
