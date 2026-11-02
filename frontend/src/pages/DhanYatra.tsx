import { useQuery } from '@tanstack/react-query'
import {
  useDeferredValue,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type RefObject,
} from 'react'
import { RotateCcw } from 'lucide-react'

import { api } from '../api/client'
import type { NetWorth } from '../api/types'
import { PageTitle } from '../components/PageTitle'
import {
  RecordedBanner,
  SessionControls,
  YATRA_SESSIONS,
  fileBase,
  utcIso,
  type Recorded,
  type SessionId,
  type SessionPart,
} from '../components/PlanSessions'
import { DrawdownChart, SCENARIO_COLOURS } from '../components/RetirementChart'
import { Button, Card, ErrorBanner, Field, Spinner, StatTile, ToggleGroup, inputClass, useConfirm } from '../components/ui'
import { currencySymbol, money, pct } from '../lib/format'
import { INPUTS_CHANGED, STORAGE_KEY } from '../lib/dhanYatraPage'
import {
  flowAverages,
  flowBand,
  flowYears,
  projectFlow,
  type FlowPlan,
  type PreWithdrawal,
} from '../lib/dhanYatra'
import {
  INFLATION,
  INR_OTHER_TREND,
  INR_PER_UNIT,
  REF_FIRST_YEAR,
  REF_LAST_YEAR,
  firstYearFraction,
  historicalInflation,
  inrHistory,
  pairDrift,
  pairVolatility,
  priceLevel,
  type CurrencyLeg,
  type ExtraAmount,
  type YearShifts,
} from '../lib/retirement'
import {
  CurrencyCard,
  ExtrasCard,
  MoneyInput,
  NumberInput,
  ProjectionTable,
  roundish,
  type ShiftKind,
  selectedBorder,
  shortLabel,
} from '../components/planning'

/** How far, in pixels, the simulation table reaches past the results column into the side column's. */
const WIDEN_KEY = 'cairezeki:dhan-yatra:widen'
/** The side column, 360px, and the gap before it, 20px: widened this far, the table spans the page. */
const WIDEN_MAX = 380
/** Withdrawals from retirement, as a share of the corpus at retirement each year. */
const DEFAULT_RATES = [3, 4, 5, 6]
const DEFAULT_RETURN = 8
const FALLBACK_INFLATION = 5
/** Retirement, when nothing is typed: at this age, else this many years out. */
const DEFAULT_RETIRE_AGE = 60
const DEFAULT_YEARS_TO_RETIRE = 15
/** How far the plan reaches: retirement at most this many years out, at most this age; an age under 100. */
const MAX_YEARS_TO_RETIRE = 50
const MAX_RETIRE_AGE = 80
const MAX_AGE = 99
/** A share of the corpus a year, when withdrawals before retirement are first switched on. */
const DEFAULT_PRE_PCT = 2

/** What the reader has typed. Anything left null follows the portfolio, history or a default. */
interface Inputs {
  /** What the plan is counted in; null follows the portfolio's base. */
  currency: string | null
  corpus: number | null
  /** Age today. */
  age: number | null
  retireYear: number | null
  returnPct: number
  inflationPct: number | null
  /** Before retirement: no withdrawal, a share of the corpus, or an amount. */
  preKind: PreWithdrawal
  prePct: number
  /** A year, today's money; null: a share of today's corpus. */
  preAmount: number | null
  /** From retirement, per Sim, a year in today's money; null: a share of the corpus then. */
  withdrawals: (number | null)[]
  extras: ExtraAmount[]
  fxTrend: boolean
  inrMoves: Record<string, number>
  inrOtherPct: number | null
  /** Years moved off the plan on the table's rope. */
  shifts: YearShifts
}

const EMPTY: Inputs = {
  currency: null,
  corpus: null,
  age: null,
  retireYear: null,
  returnPct: DEFAULT_RETURN,
  inflationPct: null,
  preKind: 'none',
  prePct: DEFAULT_PRE_PCT,
  preAmount: null,
  withdrawals: [null, null, null, null],
  extras: [],
  fxTrend: false,
  inrMoves: {},
  inrOtherPct: null,
  shifts: {},
}

/** What the journey reads of the portfolio. */
type Portfolio = Pick<NetWorth, 'base_currency' | 'as_of' | 'total_base' | 'total_values' | 'display_currencies' | 'currency_exposure'>

/**
 * How far the simulation table reaches past the results column: dragged by its
 * right edge, nudged with the arrow keys, or toggled to the full width; kept
 * for the next visit. Only the table widens - the rest of the page stays put.
 */
function useTableWiden() {
  const [widen, setWiden] = useState(() => {
    const v = Number(localStorage.getItem(WIDEN_KEY))
    return v >= 0 && v <= WIDEN_MAX ? v : 0
  })
  useEffect(() => localStorage.setItem(WIDEN_KEY, String(widen)), [widen])
  const clamp = (v: number) => Math.min(WIDEN_MAX, Math.max(0, Math.round(v)))
  const full = widen >= WIDEN_MAX
  const toggle = () => setWiden(full ? 0 : WIDEN_MAX)
  const handle = {
    role: 'separator',
    'aria-orientation': 'vertical' as const,
    'aria-label': 'Drag to widen the simulation; double-click to toggle the full width',
    'aria-valuemin': 0,
    'aria-valuemax': WIDEN_MAX,
    'aria-valuenow': widen,
    tabIndex: 0,
    title: 'Drag to widen the simulation; double-click to toggle the full width',
    onPointerDown: (e: ReactPointerEvent<HTMLDivElement>) => {
      e.preventDefault()
      const x0 = e.clientX
      const w0 = widen
      const move = (ev: PointerEvent) => setWiden(clamp(w0 + (ev.clientX - x0)))
      const up = () => {
        window.removeEventListener('pointermove', move)
        window.removeEventListener('pointerup', up)
        document.body.style.cursor = ''
        document.body.style.userSelect = ''
      }
      document.body.style.cursor = 'col-resize'
      document.body.style.userSelect = 'none'
      window.addEventListener('pointermove', move)
      window.addEventListener('pointerup', up)
    },
    onDoubleClick: toggle,
    onKeyDown: (e: ReactKeyboardEvent<HTMLDivElement>) => {
      const step = e.shiftKey ? 80 : 20
      if (e.key === 'ArrowLeft') setWiden((w) => clamp(w - step))
      else if (e.key === 'ArrowRight') setWiden((w) => clamp(w + step))
      else if (e.key === 'Home') setWiden(0)
      else if (e.key === 'End') setWiden(WIDEN_MAX)
      else return
      e.preventDefault()
    },
  }
  return { widen, full, toggle, set: (v: number) => setWiden(clamp(v)), handle }
}

/** An element's height, kept up to date as it changes. */
function useHeight(ref: RefObject<HTMLElement | null>) {
  const [height, setHeight] = useState<number | null>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const observer = new ResizeObserver(() => setHeight(el.offsetHeight))
    observer.observe(el)
    return () => observer.disconnect()
  }, [ref])
  return height
}

/** Whether a media query matches, kept up to date. */
function useMedia(query: string) {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches)
  useEffect(() => {
    const m = window.matchMedia(query)
    const on = () => setMatches(m.matches)
    on()
    m.addEventListener('change', on)
    return () => m.removeEventListener('change', on)
  }, [query])
  return matches
}

/**
 * How far an out-of-flow `card` reaches past the bottom of `page`, in
 * pixels: room to leave under the page, so a card opened to its full height
 * lengthens the page rather than running off it. 0 while `on` is off.
 */
function useOverhang(card: RefObject<HTMLElement | null>, page: RefObject<HTMLElement | null>, on: boolean) {
  const [overhang, setOverhang] = useState(0)
  useLayoutEffect(() => {
    const c = card.current
    const p = page.current
    if (!on || !c || !p) {
      setOverhang(0)
      return
    }
    const measure = () =>
      setOverhang(Math.max(0, Math.ceil(c.getBoundingClientRect().bottom - p.getBoundingClientRect().bottom)))
    measure()
    // The room left goes under the page, not in it, so neither box grows with it.
    const observer = new ResizeObserver(measure)
    observer.observe(c)
    // Its slot, too: it shrinks as the cards above it grow, moving the card.
    if (c.parentElement) observer.observe(c.parentElement)
    observer.observe(p)
    return () => observer.disconnect()
  }, [card, page, on])
  return overhang
}

function load(key: string, empty: Inputs): Inputs {
  try {
    const raw = localStorage.getItem(key)
    return raw ? { ...empty, ...JSON.parse(raw) } : empty
  } catch {
    return empty
  }
}

/** What a session keeps of the page: every input, the view, and the portfolio it started from. */
interface YatraPage {
  version: 1
  inputs: Partial<Inputs>
  selected: number
  real: boolean
  portfolio: Portfolio
}

/** A saved session, as the server hands it back. */
interface YatraSession extends Recorded {
  page: YatraPage
}

/** Only what the journey reads of the portfolio, to keep a session small. */
const portfolioOf = (p: Portfolio): Portfolio => ({
  base_currency: p.base_currency,
  as_of: p.as_of,
  total_base: p.total_base,
  total_values: p.total_values,
  display_currencies: p.display_currencies,
  currency_exposure: p.currency_exposure,
})

/** The page as a JSON file: the session it is, under `session` - signed in or not, the same file. */
function downloadPage(page: YatraPage, id: SessionId) {
  const doc = {
    format: YATRA_SESSIONS.pageFormat,
    version: 1,
    generated_at: new Date().toISOString(),
    session: {
      format: YATRA_SESSIONS.sessionFormat,
      version: 1,
      name: id.name,
      created_at: id.created_at,
      page,
    } satisfies SessionPart,
  }
  const a = document.createElement('a')
  a.href = URL.createObjectURL(new Blob([JSON.stringify(doc, null, 2)], { type: 'application/json' }))
  a.download = `${fileBase(id)}.json`
  a.click()
  URL.revokeObjectURL(a.href)
}

/**
 * The journey starts from the portfolio, and the page can be saved as a
 * session, kept on the server and shown again read-only.
 */
export default function DhanYatra() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['dashboard'],
    queryFn: () => api.get<NetWorth>('/dashboard'),
  })
  const storageKey = STORAGE_KEY
  const empty = EMPTY
  const [inputs, setInputs] = useState<Inputs>(() => load(storageKey, empty))
  const [selected, setSelected] = useState(1)
  const [real, setReal] = useState(false)
  // A saved session on show instead of the current settings: read-only, with the portfolio it was saved with.
  const [recordedId, setRecordedId] = useState<number | null>(null)
  const recorded = useQuery({
    queryKey: ['yatra-session', recordedId],
    queryFn: () => api.get<YatraSession>(`${YATRA_SESSIONS.path}/${recordedId}`),
    enabled: recordedId !== null,
    // A saved session never changes.
    staleTime: Infinity,
  })
  const session = recordedId !== null ? recorded.data : undefined
  // Which Sim and whether in today's money only change how a saved session reads; they start as it was saved.
  const [viewSelected, setViewSelected] = useState(1)
  const [viewReal, setViewReal] = useState(false)
  useEffect(() => {
    if (!session) return
    setViewSelected(session.page.selected ?? 1)
    setViewReal(!!session.page.real)
  }, [session])

  // A journey put on the page from outside the page: show it, leaving any saved session on show.
  useEffect(() => {
    const reload = (e: Event) => {
      if (e instanceof StorageEvent && e.key !== storageKey) return
      setRecordedId(null)
      setInputs(load(storageKey, empty))
    }
    window.addEventListener(INPUTS_CHANGED, reload)
    window.addEventListener('storage', reload)
    return () => {
      window.removeEventListener(INPUTS_CHANGED, reload)
      window.removeEventListener('storage', reload)
    }
  }, [storageKey, empty])

  useEffect(() => localStorage.setItem(storageKey, JSON.stringify(inputs)), [storageKey, inputs])
  const set = (patch: Partial<Inputs>) => setInputs((i) => ({ ...i, ...patch }))

  const portfolio = data
  if (isLoading) return <Spinner label="Reading your portfolio" />
  if (error) return <ErrorBanner error={error} />
  if (!portfolio) return null
  if (recordedId !== null && recorded.isLoading) return <Spinner label="Opening the saved session" />
  if (recordedId !== null && recorded.error) {
    return (
      <div className="space-y-3">
        <ErrorBanner error={recorded.error} />
        <Button variant="primary" onClick={() => setRecordedId(null)}>
          Back to current settings
        </Button>
      </div>
    )
  }

  /** The page as it is now, to save or download. */
  const current = (): YatraPage => ({ version: 1, inputs, selected, real, portfolio: portfolioOf(portfolio) })

  const tools = (
    <SessionControls
      kind={YATRA_SESSIONS}
      compact
      recorded={session ?? null}
      onOpen={setRecordedId}
      snapshot={() => ({ page: current() })}
      onDownload={(id) => downloadPage(current(), id)}
      onDownloadShown={
        session
          ? () => downloadPage(session.page, { name: session.name, created_at: utcIso(session.created_at) })
          : undefined
      }
    />
  )

  return (
    <Journey
      // A fresh journey for each session, so nothing dragged or opened in one carries into another.
      key={session ? `session-${session.id}` : 'live'}
      data={session ? session.page.portfolio : portfolio}
      empty={empty}
      inputs={session ? { ...empty, ...session.page.inputs } : inputs}
      set={session ? () => {} : set}
      selected={session ? viewSelected : selected}
      setSelected={session ? setViewSelected : setSelected}
      real={session ? viewReal : real}
      setReal={session ? setViewReal : setReal}
      tools={tools}
      readOnly={!!session}
      banner={
        session ? (
          <RecordedBanner kind={YATRA_SESSIONS} recorded={session} onBack={() => setRecordedId(null)} />
        ) : null
      }
    />
  )
}

function Journey({
  data,
  empty,
  inputs,
  set,
  selected,
  setSelected,
  real,
  setReal,
  tools,
  readOnly,
  banner,
}: {
  data: Portfolio
  /** What Reset goes back to. */
  empty: Inputs
  inputs: Inputs
  set: (patch: Partial<Inputs>) => void
  selected: number
  setSelected: (i: number) => void
  real: boolean
  setReal: (r: boolean) => void
  /** The session controls, in the Settings card's header. */
  tools: ReactNode
  /** A saved session on show: nothing can be changed. */
  readOnly: boolean
  /** Says a saved session is on show, under the title. */
  banner: ReactNode
}) {
  const table = useTableWiden()
  const { confirm, dialog } = useConfirm()
  const journeyRef = useRef<HTMLDivElement>(null)
  const journeyHeight = useHeight(journeyRef)
  // On wide screens the funds card and a widened simulation share the side
  // column's lower part: the funds open down it while the simulation keeps
  // to the results column, and fold back above it once the simulation widens.
  const wide = useMedia('(min-width: 1280px)')
  const fundsOpen = wide && table.widen === 0
  const gridRef = useRef<HTMLDivElement>(null)
  const fundsRef = useRef<HTMLDivElement>(null)
  const fundsOverhang = useOverhang(fundsRef, gridRef, fundsOpen)
  const portfolioBase = data.base_currency
  /** Units of `c` to one unit of the portfolio's base. */
  const perBase = (c: string): number | null => {
    if (c === portfolioBase) return 1
    const total = data.total_values[c]
    if (total && data.total_base) return total / data.total_base
    const exposure = data.currency_exposure.find((e) => e.currency === c)
    if (exposure?.fx_rate) return 1 / exposure.fx_rate
    const from = INR_PER_UNIT[portfolioBase]
    const to = INR_PER_UNIT[c]
    return from && to ? from[from.length - 1] / to[to.length - 1] : null
  }
  const currencies = [
    ...new Set([portfolioBase, ...data.display_currencies, ...data.currency_exposure.map((c) => c.currency), ...Object.keys(INR_PER_UNIT)]),
  ].filter((c) => perBase(c) !== null)
  const base = inputs.currency !== null && currencies.includes(inputs.currency) ? inputs.currency : portfolioBase
  const toBase = perBase(base) ?? 1
  const portfolioCorpus = Math.round(data.total_base * toBase)

  // Typed amounts move to the new currency at today's rate.
  const switchCurrency = (next: string) => {
    const k = (perBase(next) ?? 1) / toBase
    const scale = (v: number | null) => (v === null ? null : Math.round(v * k))
    set({
      currency: next === portfolioBase ? null : next,
      corpus: scale(inputs.corpus),
      preAmount: scale(inputs.preAmount),
      withdrawals: inputs.withdrawals.map(scale),
      extras: inputs.extras.map((e) => ({ ...e, amount: Math.round(e.amount * k) })),
    })
  }

  const corpus = inputs.corpus ?? portfolioCorpus
  // The journey starts today.
  const startYear = new Date().getFullYear()
  const yearFraction = firstYearFraction(startYear)
  // The latest retirement allowed: 50 years out, and by 80 when the age is known.
  const lastRetireYear = Math.max(
    startYear,
    Math.min(startYear + MAX_YEARS_TO_RETIRE, inputs.age !== null ? startYear + MAX_RETIRE_AGE - inputs.age : Infinity),
  )
  // Clamped as well as checked where typed: inputs kept from before, or put on the page from AI mode, may be out of range.
  const retireYear = Math.min(
    lastRetireYear,
    Math.max(
      startYear,
      inputs.retireYear ??
        startYear + (inputs.age !== null ? Math.max(0, DEFAULT_RETIRE_AGE - inputs.age) : DEFAULT_YEARS_TO_RETIRE),
    ),
  )
  const ageIn = (year: number) => (inputs.age === null ? null : inputs.age + year - startYear)
  const histInflation = historicalInflation(base)
  const inflationPct = inputs.inflationPct ?? histInflation ?? FALLBACK_INFLATION
  const preAmount = inputs.preAmount ?? roundish(corpus * 0.03)

  // The rupee against each currency, and each part's move against the base.
  const inrOther = inputs.inrOtherPct ?? (inputs.fxTrend ? INR_OTHER_TREND : 0)
  const inrMove = (c: string): number => {
    if (c === 'INR') return 0
    const typed = inputs.inrMoves[c]
    if (typed !== undefined) return typed
    const h = inrHistory(c)
    return h ? (inputs.fxTrend ? h.trendPct : 0) : inrOther
  }
  const exposures = data.currency_exposure.filter((c) => c.value_base > 0)
  const exposed = exposures.reduce((s, c) => s + c.value_base, 0)
  const legs: CurrencyLeg[] =
    exposed > 0
      ? exposures.map((c) => {
          const isBase = c.currency === base
          return {
            currency: c.currency,
            weight: c.value_base / exposed,
            driftPct: isBase ? 0 : pairDrift(c.currency, base, inrMove),
            volatilityPct: pairVolatility(c.currency, base),
          }
        })
      : [{ currency: base, weight: 1, driftPct: 0, volatilityPct: 0 }]

  const shared: FlowPlan = {
    corpus,
    startYear,
    firstYearFraction: yearFraction,
    returnPct: inputs.returnPct,
    inflationPct,
    withdrawal: 0,
    extras: inputs.extras,
    legs,
    retireYear,
    preKind: inputs.preKind,
    prePct: inputs.prePct,
    preAmount,
    years: flowYears(startYear, retireYear),
    shifts: inputs.shifts,
  }
  // Up to retirement every Sim is the same journey; it sets what each draws after.
  const buildKey = JSON.stringify(shared)
  const build = useMemo(() => projectFlow(shared), [buildKey])
  const atRetirementReal = Math.max(0, build.atRetirementReal)
  const withdrawals = DEFAULT_RATES.map(
    (r, i) => inputs.withdrawals[i] ?? roundish((atRetirementReal * r) / 100),
  )
  const plans: FlowPlan[] = withdrawals.map((w) => ({ ...shared, withdrawal: w }))

  const key = JSON.stringify(plans)
  const flows = useMemo(() => plans.map((p) => projectFlow(p)), [key])

  const averages = useMemo(() => flows.map((f, i) => flowAverages(plans[i], f)), [flows])

  const pick = Math.min(selected, plans.length - 1)
  const foreign = legs.some((l) => l.currency !== base && l.volatilityPct > 0)
  // Only the selected Sim's band is drawn. It is the slow part, so it is worked
  // out after a keystroke has painted rather than holding up the typing.
  const bandKey = useDeferredValue(foreign ? JSON.stringify(plans[pick]) : '')
  const picked = useMemo(() => (bandKey ? flowBand(JSON.parse(bandKey) as FlowPlan) : null), [bandKey])
  const flow = flows[pick]
  const years = shared.years
  const deflator = (k: number) => (real ? priceLevel(shared, k + 2) : 1)
  /** Prices in the retirement year against today's. */
  const retireInflation = priceLevel(shared, build.retireIndex)
  /** A yearly draw as a share of the corpus at retirement. */
  const rateOf = (w: number) => (atRetirementReal > 0 ? pct((w * 100) / atRetirementReal, 1) : '—')
  const retireLabel = `Retire ${retireYear}${ageIn(retireYear) !== null ? ` · age ${ageIn(retireYear)}` : ''}`

  const series = flows.map((f, i) => ({
    key: `w${i}`,
    label: `Sim ${i + 1} – ${rateOf(withdrawals[i])}`,
    values: [corpus, ...f.rows.map((r, k) => r.closing / deflator(k))],
    runsOutIn: f.runsOutIn,
    note: f.failsBeforeRetiring
      ? `runs out ${f.runsOutIn}, before retiring`
      : f.runsOutIn === null
        ? `lasts ${f.yearsInRetirement}+ yrs retired`
        : `runs out ${f.runsOutIn}`,
  }))
  // A band still catching up with a change in the plan's length is left out until it has.
  const band =
    foreign && picked && picked.p10.length === years
      ? {
          low: [corpus, ...picked.p10.map((v, k) => v / deflator(k))],
          high: [corpus, ...picked.p90.map((v, k) => v / deflator(k))],
        }
      : undefined

  /** A year back on the plan drops out, so a straightened rope leaves nothing behind. */
  const setShift = (kind: ShiftKind, year: number, points: number) => {
    const { [year]: _, ...rest } = inputs.shifts[kind] ?? {}
    set({ shifts: { ...inputs.shifts, [kind]: points === 0 ? rest : { ...rest, [year]: points } } })
  }

  const setWithdrawal = (i: number, v: number) =>
    set({ withdrawals: DEFAULT_RATES.map((_, k) => (k === i ? v : inputs.withdrawals[k] ?? null)) })

  const outcome = (f: (typeof flows)[number]) =>
    f.failsBeforeRetiring
      ? `Runs out in ${f.runsOutIn}, before retiring`
      : f.runsOutIn === null
        ? `Lasts ${f.yearsInRetirement}+ years in retirement`
        : `Runs out in ${f.runsOutIn}, after ${f.yearsInRetirement} years in retirement`

  /** The selected Sim as a CSV: what was chosen, every Sim's outcome, and its year-by-year table. */
  const download = () => {
    const plan = plans[pick]
    const today = new Date().toISOString().slice(0, 10)
    const whole = (v: number) => Math.round(v)
    const two = (v: number | null) => (v === null ? '' : Number(v.toFixed(2)))
    const shiftOf = (kind: ShiftKind, year: number) => inputs.shifts[kind]?.[year] ?? 0
    const pre =
      inputs.preKind === 'pct'
        ? `${inputs.prePct}% of the corpus a year`
        : inputs.preKind === 'amount'
          ? `${whole(preAmount)} a year, today's money`
          : 'None'
    const end = flow.rows.findIndex((r) => r.opening < 0 && r.closing < 0)
    const rows = end === -1 ? flow.rows : flow.rows.slice(0, end + 1)
    const lines: (string | number)[][] = [
      ['CaiYatra - The Journey of Money!'],
      ['Downloaded', today],
      ['Simulation', `Sim ${pick + 1}`],
      [],
      ['Assumptions'],
      ['Currency', base],
      ['Corpus today', whole(corpus)],
      ['Age today', inputs.age ?? ''],
      ['Retire in', retireYear],
      ['Retirement age', ageIn(retireYear) ?? ''],
      ['Return % a year', inputs.returnPct],
      ['Inflation % a year', two(inflationPct)],
      ['Withdrawal before retirement', pre],
      ['Corpus at retirement', whole(build.atRetirement)],
      ["Corpus at retirement, today's money", whole(build.atRetirementReal)],
      [],
      ['Funds Add/Remove', 'Amount', 'From', 'To', 'Rises with inflation'],
      ...inputs.extras.map((e) => [e.label, e.amount, e.fromYear, e.toYear, e.indexed ? 'Yes' : 'No']),
      [],
      ['Currency', 'Share of corpus %', 'Assumed move against base % a year'],
      ...legs.map((l) => [l.currency, two(l.weight * 100), two(l.driftPct)]),
      [],
      [
        'Sim',
        "Yearly withdrawal from retirement, today's money",
        'Withdrawal % of corpus at retirement',
        'Average withdrawal %, with shifts',
        'Average return %, with shifts',
        'Average inflation %, with shifts',
        'Outcome',
      ],
      ...flows.map((f, i) => [
        `Sim ${i + 1}${i === pick ? ' (this one)' : ''}`,
        whole(withdrawals[i]),
        two(atRetirementReal > 0 ? (withdrawals[i] * 100) / atRetirementReal : null),
        two(averages[i].withdrawalPct),
        two(averages[i].returnPct),
        two(averages[i].inflationPct),
        outcome(f),
      ]),
      [],
      [`Year by year, Sim ${pick + 1}`],
      [
        'Yr #',
        'Year',
        'Age',
        'Return %',
        'Inflation %',
        'Withdrawal shift, % of opening',
        'Opening',
        'Withdrawal',
        'Withdrawal % of opening',
        'Additional',
        'Return',
        'Currency impact',
        'Closing',
        "Closing, today's money",
        'Status',
      ],
      ...rows.map((r) => [
        r.index,
        r.year,
        ageIn(r.year) ?? '',
        two(plan.returnPct + shiftOf('returnPct', r.year)),
        two(plan.inflationPct + shiftOf('inflationPct', r.year)),
        shiftOf('withdrawalPct', r.year),
        whole(r.opening),
        whole(r.withdrawal),
        two(r.opening > 0 ? (r.withdrawal * 100) / r.opening : null),
        whole(r.extras),
        whole(r.returns),
        whole(r.fxEffect),
        whole(r.closing),
        whole(r.closingReal),
        [r.year === retireYear ? 'Retires' : '', r.status === 'negative' ? 'Short' : r.status === 'low' ? 'Low' : '']
          .filter(Boolean)
          .join(', '),
      ]),
    ]
    const cell = (v: string | number) => (typeof v === 'string' && /[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : String(v))
    // The byte-order mark tells a spreadsheet the file is UTF-8.
    const csv = '﻿' + lines.map((l) => l.map(cell).join(',')).join('\r\n')
    const a = document.createElement('a')
    a.href = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }))
    a.download = `dhan-yatra-sim-${pick + 1}-${today}.csv`
    a.click()
    URL.revokeObjectURL(a.href)
  }

  return (
    <div className="space-y-5">
      <PageTitle
        title="CaiYatra - The Journey of Money!"
        description="The corpus from today: built up until retirement, then drawn down at different withdrawals."
      />

      {banner}

      {/*
        On wide screens: the results and the assumptions side by side, the
        simulation table in a row of its own under both - so it alone can widen
        across the page - and the currency card under that.
      */}
      <div ref={gridRef} className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
        {/* -- Assumptions ------------------------------------------------- */}
        <div className="order-4 min-w-0 space-y-5 xl:col-start-2 xl:row-start-1 xl:flex xl:flex-col">
          <Card
            title="Settings"
            actions={
              <div className="flex flex-wrap items-center justify-end gap-1.5">
                {tools}
                {!readOnly && (
                  <Button
                    variant="ghost"
                    title="Reset: every assumption back to its default"
                    onClick={() =>
                      confirm({
                        title: 'Reset CaiYatra?',
                        body: 'This removes all your changes and puts every assumption back to its default value. The currency and any extra money you added stay as they are.',
                        onConfirm: () => set({ ...empty, currency: inputs.currency, extras: inputs.extras }),
                      })
                    }
                  >
                    <RotateCcw size={14} aria-hidden />
                  </Button>
                )}
              </div>
            }
          >
            {dialog}
            {/* A saved session is read-only: its settings show, and cannot be changed. */}
            <fieldset disabled={readOnly} className="m-0 min-w-0 border-0 p-0">
            <div className="grid grid-cols-3 gap-x-3 gap-y-2.5">
              <label className="col-span-3 flex items-center justify-between gap-2 text-xs font-medium text-ink-secondary">
                Currency
                <select
                  className={`${inputClass} w-auto py-1`}
                  value={base}
                  onChange={(e) => switchCurrency(e.target.value)}
                  title="Every amount on this page is shown in this currency"
                >
                  {currencies.map((c) => (
                    <option key={c} value={c}>
                      {c}
                      {c === portfolioBase ? ' (portfolio base)' : ''}
                    </option>
                  ))}
                </select>
              </label>
              <div className="col-span-2">
                <Field
                  label={shortLabel(`Corpus today (${base})`, corpus, base)}
                  hint={
                    inputs.corpus === null
                      ? `Your portfolio, ${data.as_of}`
                      : `Portfolio: ${money(portfolioCorpus, base, { whole: true })}`
                  }
                >
                  <MoneyInput
                    value={corpus}
                    currency={base}
                    onChange={(v) => set({ corpus: v === portfolioCorpus ? null : v })}
                  />
                </Field>
              </div>
              <div title="Age today. Optional.">
                <Field label="Age">
                  <NumberInput
                    value={inputs.age}
                    onChange={(v) => set({ age: v === null ? null : Math.round(v) })}
                    step={1}
                    min={0}
                    max={MAX_AGE}
                    allowEmpty
                  />
                </Field>
              </div>
              <div title="Yearly return, before currency moves">
                <Field label="Return %">
                  <NumberInput value={inputs.returnPct} onChange={(v) => set({ returnPct: v ?? 0 })} step={0.25} />
                </Field>
              </div>
              <Field label="Inflation %">
                <NumberInput
                  value={Number(inflationPct.toFixed(2))}
                  onChange={(v) => set({ inflationPct: v })}
                  step={0.1}
                />
              </Field>
              <p className="col-span-3 text-[11px] text-ink-muted">
                Return is before currency moves.{' '}
                {histInflation !== null
                  ? `Inflation in ${INFLATION[base].country}, ${REF_FIRST_YEAR + 1}–${REF_LAST_YEAR}: ${pct(histInflation)} a year.`
                  : `No reference inflation series for ${base}.`}
              </p>
            </div>
            </fieldset>
          </Card>

          <Card title="Retirement" subtitle="The tipping point: the Sims' withdrawals start this year">
            <fieldset disabled={readOnly} className="m-0 min-w-0 border-0 p-0">
            <div className="space-y-3">
              <div className="grid grid-cols-2 gap-x-3">
                <Field label="Retire in">
                  <NumberInput
                    value={retireYear}
                    onChange={(v) => set({ retireYear: v === null ? null : Math.round(v) })}
                    step={1}
                    min={startYear}
                    max={lastRetireYear}
                  />
                </Field>
                <div title={inputs.age === null ? 'Enter your age to plan by age' : undefined}>
                  <Field label="At age">
                    {inputs.age === null ? (
                      <input className={`${inputClass} tabular`} disabled value="—" aria-label="Retirement age" />
                    ) : (
                      <NumberInput
                        value={ageIn(retireYear)}
                        onChange={(v) => v !== null && set({ retireYear: startYear + Math.round(v) - inputs.age! })}
                        step={1}
                        min={inputs.age}
                        max={ageIn(lastRetireYear) ?? undefined}
                        ariaLabel="Retirement age"
                      />
                    )}
                  </Field>
                </div>
              </div>

              <div>
                <p className="mb-1.5 text-xs font-medium text-ink-secondary">Withdrawal until then</p>
                <ToggleGroup
                  label="Withdrawal before retirement"
                  stretch
                  options={[
                    ['none', 'None'],
                    ['pct', '% of corpus'],
                    ['amount', 'Amount'],
                  ]}
                  value={inputs.preKind}
                  onChange={(k) => set({ preKind: k })}
                />
                {inputs.preKind === 'pct' && (
                  <div className="mt-2 flex items-center gap-2">
                    <div className="w-20">
                      <NumberInput
                        value={inputs.prePct}
                        onChange={(v) => set({ prePct: Math.max(0, v ?? 0) })}
                        step={0.25}
                        ariaLabel="Withdrawal before retirement, % of the corpus a year"
                      />
                    </div>
                    <span className="text-xs text-ink-secondary">% of the corpus at the start of each year</span>
                  </div>
                )}
                {inputs.preKind === 'amount' && (
                  <div className="mt-2 space-y-1.5">
                    {[false, true].map((m) => (
                      <label key={String(m)} className="flex min-w-0 items-center gap-2">
                        <span className="w-14 shrink-0 text-xs text-ink-secondary">{m ? 'Monthly' : 'Yearly'}</span>
                        <MoneyInput
                          value={Math.round(m ? preAmount / 12 : preAmount)}
                          currency={base}
                          prefix={currencySymbol(base)}
                          onChange={(v) => set({ preAmount: m ? v * 12 : v })}
                          ariaLabel={`${m ? 'Monthly' : 'Yearly'} withdrawal before retirement in ${base}`}
                        />
                      </label>
                    ))}
                    <p className="text-[11px] text-ink-muted">Today's money; it rises with inflation.</p>
                  </div>
                )}
                {inputs.preKind === 'none' && (
                  <p className="mt-1.5 text-[11px] text-ink-muted">
                    The corpus only grows until {retireYear}: its return and the funds added or removed below.
                  </p>
                )}
              </div>
            </div>
            </fieldset>
          </Card>

          {/*
            On wide screens, open, it runs on down the side column beside the
            simulation at its full height, never scrolling: past the end of
            the page, the page grows to hold it.
            Folded - the simulation widened - it takes what is left of the
            column beside the results, never more than the Corpus Journey's
            height, and scrolls past that: the table's row starts under the
            results, not under it.
          */}
          <div className="xl:relative xl:min-h-[240px] xl:flex-1">
            <div
              className={`xl:absolute xl:inset-x-0 xl:top-0 xl:z-10 xl:flex xl:flex-col ${fundsOpen ? '' : 'xl:max-h-full'}`}
              ref={fundsRef}
            >
              <ExtrasCard
                expand={
                  wide
                    ? {
                        open: fundsOpen,
                        // Together with the simulation: one widens as the other folds.
                        onToggle: () => table.set(fundsOpen ? WIDEN_MAX : 0),
                      }
                    : undefined
                }
                extras={inputs.extras}
                onChange={(extras) => set({ extras })}
                currency={base}
                startYear={startYear}
                title="Funds Add/Remove"
                empty={`Savings from salary until ${retireYear}, a bonus, rent; a pension after. Negative for a spend going out - a wedding, a car.`}
                defaultUntil={retireYear - 1}
                maxHeight={fundsOpen ? undefined : (journeyHeight ?? undefined)}
              />
            </div>
          </div>
        </div>

        {/* -- Results ----------------------------------------------------- */}
        <div className="order-1 min-w-0 space-y-5 xl:col-start-1 xl:row-start-1">
          <div className="grid gap-3 sm:grid-cols-3">
            <StatTile label="Today" value={corpus} currency={base} whole hint={inputs.age !== null ? `Age ${inputs.age}` : `${startYear}`} />
            <StatTile
              label={`At retirement · ${retireYear}`}
              value={build.atRetirement}
              currency={base}
              whole
              hint={`${money(build.atRetirementReal, base, { whole: true })} in today's money${
                ageIn(retireYear) !== null ? ` · age ${ageIn(retireYear)}` : ''
              }`}
            />
            <StatTile
              label={`Peak · Sim ${pick + 1}`}
              value={flow.peak.value}
              currency={base}
              whole
              hint={
                flow.peak.year === startYear && flow.peak.value === corpus
                  ? 'Today: it only falls from here'
                  : `End of ${flow.peak.year}${ageIn(flow.peak.year) !== null ? ` · age ${ageIn(flow.peak.year)}` : ''}`
              }
            />
          </div>

          <div className="space-y-2">
            <p className="text-xs text-ink-muted">
              Withdrawals from {retireYear} in {base}, today's money; each rises with inflation. The rate is of the corpus
              at retirement.
            </p>
            <div className="grid gap-3 sm:grid-cols-2 2xl:grid-cols-4">
              {flows.map((f, i) => {
                const fails = f.runsOutIn !== null
                return (
                  // A div, not a button: it holds the withdrawal's input.
                  <div
                    key={i}
                    role="button"
                    tabIndex={0}
                    onClick={() => setSelected(i)}
                    onKeyDown={(e) => {
                      if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) {
                        e.preventDefault()
                        setSelected(i)
                      }
                    }}
                    aria-pressed={i === pick}
                    className="card min-w-0 cursor-pointer p-4 text-left transition hover:border-ink/20"
                    style={i === pick ? selectedBorder(SCENARIO_COLOURS[i]) : undefined}
                  >
                    <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-wide text-ink-muted">
                      <span aria-hidden className="h-0.5 w-4 rounded" style={{ background: SCENARIO_COLOURS[i] }} />
                      <span className="text-ink">Sim {i + 1}</span>
                      <span className={`ml-auto normal-case tracking-normal ${fails ? 'text-loss' : ''}`}>
                        {rateOf(withdrawals[i])} withdrawal
                      </span>
                    </div>
                    <div className={`mt-1.5 text-2xl font-semibold ${fails ? 'text-loss' : 'text-ink'}`}>
                      {f.failsBeforeRetiring
                        ? 'Not to retirement'
                        : `${f.yearsInRetirement}${fails ? '' : '+'} years`}
                    </div>
                    <div className="mt-1 text-xs text-ink-muted">
                      {!fails
                        ? `In retirement; ${money(f.rows[years - 1].closingReal, base, { whole: true })} left in today's money${
                            ageIn(startYear + years - 1) !== null ? ` at age ${ageIn(startYear + years - 1)}` : ''
                          }`
                        : `Runs out in ${f.runsOutIn}${ageIn(f.runsOutIn!) !== null ? `, at age ${ageIn(f.runsOutIn!)}` : ''}`}
                    </div>
                    <div className="mt-2 space-y-1.5 border-t border-[color:var(--hairline)] pt-2">
                      {[false, true].map((m) => (
                        <label key={String(m)} className="flex min-w-0 items-center gap-2">
                          <span className="w-14 shrink-0 text-xs text-ink-secondary">{m ? 'Monthly' : 'Yearly'}</span>
                          <MoneyInput
                            value={Math.round(m ? withdrawals[i] / 12 : withdrawals[i])}
                            currency={base}
                            prefix={currencySymbol(base)}
                            onChange={(v) => setWithdrawal(i, m ? v * 12 : v)}
                            ariaLabel={`${m ? 'Monthly' : 'Yearly'} withdrawal in ${base} from retirement, Sim ${i + 1}`}
                          />
                        </label>
                      ))}
                      {retireYear > startYear && (
                        <p className="text-[11px] text-ink-muted">
                          {money(withdrawals[i] * retireInflation, base, { whole: true })} a year in {retireYear}
                        </p>
                      )}
                    </div>
                  </div>
                )
              })}
            </div>
          </div>

          <div ref={journeyRef}>
            <Card
              title="Corpus Journey"
              subtitle={
                foreign
                  ? `Shaded band: 10th–90th percentile of ${series[pick].label} across 500 currency paths`
                  : 'All of the corpus is in the base currency, so there is no currency band'
              }
              actions={
                <ToggleGroup
                  label="Money terms"
                  options={[
                    ['nominal', 'Nominal'],
                    ['real', "Today's money"],
                  ]}
                  value={real ? 'real' : 'nominal'}
                  onChange={(v) => setReal(v === 'real')}
                />
              }
            >
              <DrawdownChart
                series={series}
                selected={pick}
                onSelect={setSelected}
                band={band}
                currency={base}
                startYear={startYear}
                age={inputs.age}
                marker={{ at: build.retireIndex - 1, label: retireLabel }}
                title={`Corpus in ${base} over ${years} years: built up to ${retireYear}, then drawn down, one line per withdrawal`}
              />
            </Card>
          </div>
        </div>

        {/* -- The year-by-year table: the results column's width, widened at will - */}
        <div className="order-2 min-w-0 xl:col-span-2 xl:row-start-2">
          <div
            className="relative min-w-0 xl:w-[calc(100%_-_var(--rest))]"
            style={{ '--rest': `${WIDEN_MAX - table.widen}px` } as CSSProperties}
          >
            {/* Its right edge, on wide screens: drag it to widen the table. */}
            <div
              {...table.handle}
              className="group absolute -right-2.5 bottom-0 top-0 z-20 hidden w-5 cursor-col-resize justify-center focus:outline-none xl:flex"
            >
              <span className="my-3 w-1 rounded-full bg-transparent transition group-hover:bg-ink/30 group-focus-visible:bg-ink/50" />
            </div>
            <ProjectionTable
              rows={flow.rows}
              label={`Sim ${pick + 1}`}
              rate={rateOf(withdrawals[pick])}
              colour={SCENARIO_COLOURS[pick]}
              currency={base}
              age={inputs.age}
              startYear={startYear}
              yearFraction={yearFraction}
              returnPct={inputs.returnPct}
              inflationPct={inflationPct}
              hasFx={legs.some((l) => l.driftPct !== 0)}
              mark={{ year: retireYear, label: 'Retires' }}
              averages={{
                rate:
                  averages[pick].withdrawalPct === null ? rateOf(withdrawals[pick]) : pct(averages[pick].withdrawalPct!, 1),
                returnPct: averages[pick].returnPct,
                inflationPct: averages[pick].inflationPct,
              }}
              onDownload={download}
              expand={{ full: table.full, onToggle: table.toggle }}
              rope={{
                shifts: inputs.shifts,
                onShift: setShift,
                onClear: (kind) => set({ shifts: { ...inputs.shifts, [kind]: {} } }),
              }}
            />
          </div>
        </div>

        <div className="order-3 min-w-0 xl:col-start-1 xl:row-start-3">
          <CurrencyCard
            base={base}
            legs={legs}
            inrMove={inrMove}
            inrOther={inrOther}
            fxTrend={inputs.fxTrend}
            edited={Object.keys(inputs.inrMoves).length > 0 || inputs.inrOtherPct !== null}
            onTrend={(fxTrend) => set({ fxTrend })}
            onMove={(c, v) => set({ inrMoves: { ...inputs.inrMoves, [c]: v } })}
            onOther={(inrOtherPct) => set({ inrOtherPct })}
            onReset={() => set({ inrMoves: {}, inrOtherPct: null })}
          />
        </div>
      </div>
      {/* Room under the page for an opened funds card that runs past it. */}
      {fundsOverhang > 0 && <div aria-hidden style={{ height: fundsOverhang, marginTop: 0 }} />}
    </div>
  )
}
