/**
 * CaiYatra: the corpus over a whole journey - built up from today, then
 * drawn down from retirement.
 *
 * The same yearly step as `retirement.ts` (additional amounts at the start
 * of the year, then a year's return, currencies drifting per leg), with the
 * withdrawal in two phases:
 *
 * - Before the retirement year, by default nothing comes out: the corpus
 *   grows on its return and the additional amounts (savings, a bonus). A
 *   plan can instead take a share of each year's opening balance, or a fixed
 *   amount in today's money that rises with inflation.
 * - From the retirement year - the tipping point - the plan's `withdrawal`
 *   applies, in today's money and rising with inflation.
 *
 * A plan that retires this year draws down from today, the corpus as it stands.
 *
 * Any year can be shifted from the plan's return, inflation or withdrawal
 * (`Plan.shifts`): a bad year early in retirement, a big spend.
 */

import {
  RISK_PATHS,
  YEARS,
  inflationIn,
  priceLevel,
  projectWith,
  returnIn,
  sampleBand,
  yearLength,
  yearsBefore,
  type Plan,
  type Projection,
  type RiskBand,
} from './retirement'

/** Retirement still needs this many years of drawdown on the chart. */
export const RETIRED_YEARS = 40

export type PreWithdrawal = 'none' | 'pct' | 'amount'

export interface FlowPlan extends Plan {
  /** Calendar year withdrawals at `withdrawal` begin. */
  retireYear: number
  /** What comes out before then. */
  preKind: PreWithdrawal
  /** For 'pct': share of each year's opening balance, %. */
  prePct: number
  /** For 'amount': a year, in today's money; it grows with inflation. */
  preAmount: number
  /** How many years the plan runs, the first (short) one included. */
  years: number
}

export interface Flow extends Projection {
  /** Plan year (1-based) in which retirement begins. */
  retireIndex: number
  /** The balance as retirement begins, and the same in today's money. */
  atRetirement: number
  atRetirementReal: number
  /** The highest year-end balance and its year: the top of the journey. */
  peak: { year: number; value: number }
  /** Whether the corpus is gone before retirement begins. */
  failsBeforeRetiring: boolean
  /** Full years of retirement it covers; to the horizon when it never runs out. */
  yearsInRetirement: number
}

/** Years to run: at least YEARS, and RETIRED_YEARS past retirement. */
export const flowYears = (startYear: number, retireYear: number) =>
  Math.max(YEARS, Math.max(0, retireYear - startYear) + RETIRED_YEARS)

export const retireIndexOf = (plan: FlowPlan) => Math.max(1, plan.retireYear - plan.startYear + 1)

const inflated = priceLevel

/** What comes out in plan year `index` before that year's shift, given its opening balance. */
function plannedWithdrawal(plan: FlowPlan, index: number, opening: number): number {
  const length = yearLength(plan, index)
  if (index >= retireIndexOf(plan)) return plan.withdrawal * inflated(plan, index) * length
  if (plan.preKind === 'pct') return opening > 0 ? opening * (plan.prePct / 100) * length : 0
  if (plan.preKind === 'amount') return plan.preAmount * inflated(plan, index) * length
  return 0
}

/** What comes out in plan year `index`, given its opening balance: a shifted year takes that share of it more or less, never below nothing. */
export function flowWithdrawal(plan: FlowPlan, index: number, opening: number): number {
  const planned = plannedWithdrawal(plan, index, opening)
  const shift = plan.shifts?.withdrawalPct?.[plan.startYear + index - 1] ?? 0
  if (shift === 0 || opening <= 0) return planned
  return Math.max(0, planned + opening * (shift / 100) * yearLength(plan, index))
}

/** The journey year by year; `fxDraws` as for `project`. */
export function projectFlow(plan: FlowPlan, fxDraws?: number[][]): Flow {
  const run = projectWith(plan, plan.years, (index, opening) => flowWithdrawal(plan, index, opening), fxDraws)
  const retireIndex = retireIndexOf(plan)
  const atRetirement = retireIndex === 1 ? plan.corpus : run.rows[retireIndex - 2].closing
  const peak = run.rows.reduce(
    (best, r) => (r.closing > best.value ? { year: r.year, value: r.closing } : best),
    { year: plan.startYear, value: plan.corpus },
  )
  const failIndex = run.runsOutIn === null ? null : run.runsOutIn - plan.startYear + 1
  const failsBeforeRetiring = failIndex !== null && failIndex < retireIndex
  const yearsInRetirement =
    failIndex === null
      ? plan.years - retireIndex + 1
      : Math.max(0, Math.floor(yearsBefore(plan, failIndex) - yearsBefore(plan, retireIndex) + 1e-9))
  return {
    ...run,
    retireIndex,
    atRetirement,
    atRetirementReal: atRetirement / inflated(plan, retireIndex),
    peak,
    failsBeforeRetiring,
    yearsInRetirement,
  }
}

/** Currency risk over the journey: `sampleBand` run over the whole flow. */
export const flowBand = (plan: FlowPlan, paths = RISK_PATHS): RiskBand =>
  sampleBand(plan.legs, plan.years, (draws) => projectFlow(plan, draws), paths)

/** What a journey actually ran at, the rope's shifts included. */
export interface FlowAverages {
  /** Yearly return and inflation, %, over the years it lasts. */
  returnPct: number
  inflationPct: number
  /** Each retired year's withdrawal in today's money, as a share of the corpus at retirement, %; null when it never retires. */
  withdrawalPct: number | null
}

/**
 * Averages over the years the corpus lasts (to the year it runs out, else the
 * horizon), each year weighted by its length so the short first year counts
 * for less. With no shifts they are the plan's own figures.
 */
export function flowAverages(plan: FlowPlan, flow: Flow): FlowAverages {
  const last = flow.runsOutIn === null ? plan.years : flow.runsOutIn - plan.startYear + 1
  let weight = 0
  let ret = 0
  let infl = 0
  let retired = 0
  let drawn = 0
  for (let index = 1; index <= last; index++) {
    const length = yearLength(plan, index)
    weight += length
    ret += returnIn(plan, index) * length
    infl += inflationIn(plan, index) * length
    if (index >= flow.retireIndex) {
      retired += length
      drawn += flow.rows[index - 1].withdrawal / inflated(plan, index)
    }
  }
  return {
    returnPct: ret / weight,
    inflationPct: infl / weight,
    withdrawalPct: retired > 0 && flow.atRetirementReal > 0 ? (drawn / retired / flow.atRetirementReal) * 100 : null,
  }
}
