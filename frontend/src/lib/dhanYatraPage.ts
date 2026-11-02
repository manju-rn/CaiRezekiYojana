/**
 * The CaiYatra page's inputs, as kept in this browser - and putting a
 * journey from AI mode on the page. The assistant's `show_dhan_yatra` takes
 * the API's inputs (`retire_year`, `withdrawals`, ...); here they become the
 * page's own (`retireYear`, ...), replacing what the page showed. Anything
 * left out goes back to the page's default, as Reset would.
 * `set_dhan_yatra_rope` changes only the rope, on top of the page as it
 * stands (or of the journey shown in the same answer).
 */

import type { AiPageAction, AiRopeChange } from '../api/types'

/** Where the signed-in page keeps its inputs. */
export const STORAGE_KEY = 'cairezeki:dhan-yatra'

/** Sent when the inputs are changed from outside the page, for an open page to show them. */
export const INPUTS_CHANGED = 'cairezeki:dhan-yatra-inputs'

/** The page always compares four withdrawals. */
const SIMS = 4

type Api = Record<string, unknown>

const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null)
const str = (v: unknown): string | null => (typeof v === 'string' && v ? v : null)

/** The page's inputs for a journey given as the API takes it; only what it says. */
export function toPageInputs(a: Api): Record<string, unknown> {
  const monthly = a.monthly === true
  const yearly = (v: unknown) => {
    const n = num(v)
    return n === null ? null : monthly ? n * 12 : n
  }
  const out: Record<string, unknown> = {}
  if (str(a.currency)) out.currency = a.currency
  if (num(a.corpus) !== null) out.corpus = a.corpus
  if (num(a.age) !== null) out.age = a.age
  const retireAge = num(a.retire_age)
  const age = num(a.age)
  if (num(a.retire_year) !== null) out.retireYear = a.retire_year
  else if (retireAge !== null && age !== null) out.retireYear = new Date().getFullYear() + (retireAge - age)
  if (num(a.return_pct) !== null) out.returnPct = a.return_pct
  if (num(a.inflation_pct) !== null) out.inflationPct = a.inflation_pct
  if (a.pre_retirement === 'none' || a.pre_retirement === 'pct' || a.pre_retirement === 'amount') out.preKind = a.pre_retirement
  if (num(a.pre_retirement_pct) !== null) out.prePct = a.pre_retirement_pct
  if (num(a.pre_retirement_amount) !== null) out.preAmount = yearly(a.pre_retirement_amount)
  if (Array.isArray(a.withdrawals)) {
    out.withdrawals = Array.from({ length: SIMS }, (_, i) => yearly(a.withdrawals && (a.withdrawals as unknown[])[i]))
  }
  if (Array.isArray(a.extras)) {
    out.extras = (a.extras as Api[]).flatMap((e, i) => {
      const amount = num(e.amount)
      const from = num(e.from_year)
      if (amount === null || from === null) return []
      return [
        {
          id: `ai-${Date.now().toString(36)}-${i}`,
          label: str(e.label) ?? '',
          amount,
          fromYear: from,
          toYear: num(e.to_year) ?? from,
          indexed: e.indexed !== false,
        },
      ]
    })
  }
  if (Array.isArray(a.adjustments)) out.shifts = spread(a.adjustments as Api[])
  if (typeof a.fx_trend === 'boolean') out.fxTrend = a.fx_trend
  if (a.inr_moves && typeof a.inr_moves === 'object') out.inrMoves = a.inr_moves
  if (num(a.inr_other_pct) !== null) out.inrOtherPct = a.inr_other_pct
  return out
}

type Shifts = Record<string, Record<number, number>>

/** The rope's shift keys on the page, by the API's assumption. */
const SHIFT_KEYS: Record<string, string> = { return: 'returnPct', inflation: 'inflationPct', withdrawal: 'withdrawalPct' }

/** The API's adjustments as the page's shifts: ranges spread out, overlaps added up. */
function spread(adjustments: Api[]): Shifts {
  const shifts: Shifts = {}
  for (const adj of adjustments) {
    const key = SHIFT_KEYS[String(adj.assumption)]
    const from = num(adj.from_year)
    const points = num(adj.points)
    if (!key || from === null || points === null) continue
    const to = num(adj.to_year) ?? from
    for (let y = from; y <= to; y++) {
      const years = (shifts[key] ??= {})
      years[y] = (years[y] ?? 0) + points
    }
  }
  return shifts
}

/** The page's shifts with a rope change made; a year back on the plan drops out, as on the page. */
export function withRope(shifts: Shifts, change: AiRopeChange): Shifts {
  const out: Shifts = Object.fromEntries(Object.entries(shifts).map(([k, years]) => [k, { ...years }]))
  for (const kind of change.straighten ?? []) {
    const key = SHIFT_KEYS[kind]
    if (key) out[key] = {}
  }
  const add = change.mode === 'add'
  for (const [key, years] of Object.entries(spread((change.adjustments ?? []) as Api[]))) {
    const into = (out[key] ??= {})
    for (const [y, points] of Object.entries(years)) {
      const year = Number(y)
      const value = add ? (into[year] ?? 0) + points : points
      if (value === 0) delete into[year]
      else into[year] = value
    }
  }
  return out
}

function stored(): Record<string, unknown> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}

function changed() {
  window.dispatchEvent(new CustomEvent(INPUTS_CHANGED))
}

/**
 * Put what an answer did on the page: its journey, replacing what the page
 * held, then its rope changes. Answers what the page held before, for Undo.
 */
export function showOnPage(action: AiPageAction): string | null {
  const before = localStorage.getItem(STORAGE_KEY)
  const inputs = action.inputs ? toPageInputs(action.inputs) : stored()
  if (action.rope?.length) {
    inputs.shifts = action.rope.reduce((shifts, change) => withRope(shifts, change), (inputs.shifts as Shifts | undefined) ?? {})
  }
  localStorage.setItem(STORAGE_KEY, JSON.stringify(inputs))
  changed()
  return before
}

/** Put back what the page held before. */
export function restorePage(before: string | null) {
  if (before === null) localStorage.removeItem(STORAGE_KEY)
  else localStorage.setItem(STORAGE_KEY, before)
  changed()
}
