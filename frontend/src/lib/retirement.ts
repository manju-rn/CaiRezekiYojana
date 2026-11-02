/**
 * Retirement drawdown: how long a corpus lasts at a given withdrawal.
 *
 * Everything here runs in the browser - it is arithmetic on assumptions,
 * not on stored data, so the screen needs no round trip. The corpus and its
 * currency split come from the dashboard; the rest is what the reader types.
 *
 * The server has the same arithmetic (`backend/src/domain/retirement.rs`)
 * for `POST /retirement/simulate` and the MCP tool drawn from it. Change
 * the model or the reference series in both.
 *
 * One step per year: withdrawals and additional amounts at the start of the
 * year, then a year's return on what is left. A plan that starts this year
 * starts today, so its first year is only what is left of the calendar
 * year: that share of the withdrawal, of recurring additional amounts and of
 * the year's growth. Later years are whole calendar years, with inflation
 * counted from today. The mix across currencies is
 * held at today's weights (rebalanced yearly). A corpus that has run out
 * earns nothing and keeps accruing the shortfall, so the table can show how
 * far below zero the plan goes.
 */

// -- Reference data ---------------------------------------------------------

/** Calendar years the reference series cover: ten annual changes. */
export const REF_FIRST_YEAR = 2015
export const REF_LAST_YEAR = 2025

/**
 * Consumer-price inflation, % change in the annual average, 2016-2025.
 * World Bank FP.CPI.TOTL.ZG (India, Singapore, Australia, US), Eurostat HICP
 * (euro area), ONS CPI (UK). The 2025 figures are provisional.
 */
export const INFLATION: Record<string, { country: string; rates: number[] }> = {
  INR: { country: 'India', rates: [4.95, 3.33, 3.94, 3.73, 6.62, 5.13, 6.7, 5.65, 4.95, 2.2] },
  SGD: { country: 'Singapore', rates: [-0.53, 0.58, 0.44, 0.57, -0.18, 2.3, 6.12, 4.82, 2.39, 0.9] },
  AUD: { country: 'Australia', rates: [1.28, 1.95, 1.91, 1.61, 0.85, 2.86, 6.59, 5.6, 3.16, 2.8] },
  USD: { country: 'United States', rates: [1.26, 2.13, 2.44, 1.81, 1.23, 4.7, 8.0, 4.12, 2.95, 2.7] },
  GBP: { country: 'United Kingdom', rates: [0.7, 2.7, 2.5, 1.8, 0.9, 2.6, 9.1, 7.3, 2.5, 3.4] },
  EUR: { country: 'Euro area', rates: [0.2, 1.5, 1.8, 1.2, 0.3, 2.6, 8.4, 5.4, 2.4, 2.1] },
}

/**
 * Rupees per unit of each currency at each year end, 2015-2025 (reference
 * rates, rounded). Any pair is the ratio of two rows, so one table covers
 * every base currency it names.
 */
export const INR_PER_UNIT: Record<string, number[]> = {
  INR: [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
  USD: [66.2, 67.9, 63.9, 69.8, 71.3, 73.1, 74.3, 82.7, 83.2, 85.6, 89.9],
  SGD: [46.8, 47.0, 47.8, 51.2, 53.0, 55.3, 55.1, 61.7, 63.0, 62.8, 70.0],
  AUD: [48.3, 49.0, 49.8, 49.2, 50.1, 56.3, 54.0, 56.3, 56.7, 53.0, 60.0],
  GBP: [97.6, 83.8, 86.3, 89.0, 94.5, 99.9, 100.5, 100.1, 105.9, 107.2, 120.9],
  EUR: [71.9, 71.4, 76.7, 79.9, 79.9, 89.3, 84.5, 88.5, 91.9, 88.6, 105.5],
}

/** Geometric mean of yearly percentages, as a percentage. */
export function averageRate(rates: number[]): number {
  if (rates.length === 0) return 0
  const growth = rates.reduce((g, r) => g * (1 + r / 100), 1)
  return (Math.pow(growth, 1 / rates.length) - 1) * 100
}

/** Ten-year average inflation for a currency's country, or null if unknown. */
export function historicalInflation(currency: string): number | null {
  const row = INFLATION[currency]
  return row ? averageRate(row.rates) : null
}

export interface FxHistory {
  currency: string
  base: string
  /** Value of one unit in the base currency, each year end. */
  rates: number[]
  /** The whole ten years, %: +20 means the currency bought 20% more base. */
  totalChangePct: number
  /** Compound annual change, %. */
  trendPct: number
  /** Standard deviation of the yearly change, %. */
  volatilityPct: number
}

/**
 * How a currency moved against the base over the reference decade, or null
 * where either is not in the table.
 */
export function fxHistory(currency: string, base: string): FxHistory | null {
  const c = INR_PER_UNIT[currency]
  const b = INR_PER_UNIT[base]
  if (!c || !b) return null
  const rates = c.map((v, i) => v / b[i])
  const logs = rates.slice(1).map((v, i) => Math.log(v / rates[i]))
  const mean = logs.reduce((s, l) => s + l, 0) / logs.length
  const variance = logs.reduce((s, l) => s + (l - mean) ** 2, 0) / (logs.length - 1)
  return {
    currency,
    base,
    rates,
    totalChangePct: (rates[rates.length - 1] / rates[0] - 1) * 100,
    trendPct: (Math.exp(mean) - 1) * 100,
    volatilityPct: Math.sqrt(variance) * 100,
  }
}

// -- The rupee against each currency -----------------------------------------
//
// Currency assumptions are set as the rupee against each currency - "INR/SGD:
// the rupee loses 4% a year" - and every other pair follows from two of
// those. A currency with no series takes one shared assumption, by default
// the rupee's average against the ones that have one.

/** The rupee's ten-year history against `currency`, or null without a series. */
export const inrHistory = (currency: string) => (currency === 'INR' ? null : fxHistory('INR', currency))

const KNOWN = Object.keys(INR_PER_UNIT).filter((c) => c !== 'INR')

/** The rupee's average yearly change against the currencies in the table, %. */
export const INR_OTHER_TREND = averageRate(KNOWN.map((c) => inrHistory(c)!.trendPct))

/** The rupee's average yearly volatility against them, %. */
export const INR_OTHER_VOLATILITY = KNOWN.reduce((s, c) => s + inrHistory(c)!.volatilityPct, 0) / KNOWN.length

/**
 * The yearly change of `currency` against `base`, %, given the rupee's
 * yearly change against each (`inrMove`, % - negative: the rupee loses).
 * One unit of `currency` is worth (INR per base) / (INR per currency) of base.
 */
export function pairDrift(currency: string, base: string, inrMove: (c: string) => number): number {
  const m = (c: string) => (c === 'INR' ? 0 : inrMove(c) / 100)
  return ((1 + m(base)) / (1 + m(currency)) - 1) * 100
}

/** Yearly volatility of `currency` against `base`, %: its history, else the rupee's average. */
export function pairVolatility(currency: string, base: string): number {
  if (currency === base) return 0
  return fxHistory(currency, base)?.volatilityPct ?? INR_OTHER_VOLATILITY
}

// -- The simulation -----------------------------------------------------------

export const YEARS = 50

/** One currency's share of the corpus, and what is assumed of it. */
export interface CurrencyLeg {
  currency: string
  /** Fraction of the corpus, 0-1. */
  weight: number
  /** Assumed yearly change against the base, %. Zero for the base itself. */
  driftPct: number
  /** Yearly volatility against the base, %, for the risk band. */
  volatilityPct: number
}

/** An amount on top of the withdrawal: income in, or a spend out. */
export interface ExtraAmount {
  id: string
  label: string
  /** Positive comes in (a pension, a sale), negative goes out (a wedding). */
  amount: number
  /** Calendar years, inclusive. A one-off has from === to. */
  fromYear: number
  toYear: number
  /** Grows with inflation, from today's money. */
  indexed: boolean
}

export interface Plan {
  corpus: number
  startYear: number
  /** How much of the first calendar year the plan runs, 0-1: what is left
   *  of this year when it starts this year, else 1. */
  firstYearFraction: number
  returnPct: number
  inflationPct: number
  /** Each year's withdrawal, in today's money; it grows with inflation. */
  withdrawal: number
  extras: ExtraAmount[]
  legs: CurrencyLeg[]
  /** Percentage points on top of `returnPct` / `inflationPct` in a given calendar year. */
  shifts?: YearShifts
}

/** Per calendar year, percentage points on top of the plan's assumption for that year alone. */
export interface YearShifts {
  returnPct?: Record<number, number>
  inflationPct?: Record<number, number>
  /** CaiYatra: a share of the year's opening balance drawn on top of (or held back from) the withdrawal. */
  withdrawalPct?: Record<number, number>
}

export type RowStatus = 'ok' | 'low' | 'negative'

export interface YearRow {
  year: number
  /** 1 for the first year of the plan. */
  index: number
  opening: number
  withdrawal: number
  extras: number
  returns: number
  /** What currency moves added (or took) on top of the return. */
  fxEffect: number
  closing: number
  /** The closing balance in today's money. */
  closingReal: number
  status: RowStatus
}

export interface Projection {
  rows: YearRow[]
  /** Calendar year in which the corpus first goes below zero, or null. */
  runsOutIn: number | null
  /** Full years it covers before then; YEARS when it never runs out. */
  yearsLasted: number
}

/** Fewer years of withdrawals than this left at a year end reads as "low". */
const LOW_YEARS = 2

/**
 * Share of `startYear` still ahead on `today`: the days from today to the
 * year end, today included. 1 for a plan that starts in a later year (or an
 * earlier one, which is a what-if from its first of January).
 */
export function firstYearFraction(startYear: number, today = new Date()): number {
  if (startYear !== today.getFullYear()) return 1
  const start = Date.UTC(startYear, today.getMonth(), today.getDate())
  const end = Date.UTC(startYear + 1, 0, 1)
  return (end - start) / (end - Date.UTC(startYear, 0, 1))
}

/** Length of the plan's `index`th year, in years: short only for the first. */
export const yearLength = (plan: Plan, index: number) => (index === 1 ? plan.firstYearFraction : 1)

/** Years from the plan's start (today) to the start of its `index`th year. */
export const yearsBefore = (plan: Plan, index: number) => (index === 1 ? 0 : plan.firstYearFraction + index - 2)

/** The plan's return in its `index`th year, %, with that year's shift. */
export const returnIn = (plan: Plan, index: number) =>
  plan.returnPct + (plan.shifts?.returnPct?.[plan.startYear + index - 1] ?? 0)

/** The plan's inflation in its `index`th year, %, with that year's shift. */
export const inflationIn = (plan: Plan, index: number) =>
  plan.inflationPct + (plan.shifts?.inflationPct?.[plan.startYear + index - 1] ?? 0)

/** Price levels by plan year, for plans whose inflation is shifted in some year; built once per plan. */
const levels = new WeakMap<Plan, number[]>()

/** Prices at the start of the plan's `index`th year against today's. */
export function priceLevel(plan: Plan, index: number): number {
  const shifted = plan.shifts?.inflationPct
  if (!shifted || Object.keys(shifted).length === 0) {
    return Math.pow(1 + plan.inflationPct / 100, yearsBefore(plan, index))
  }
  let cache = levels.get(plan)
  if (!cache) {
    cache = [1, 1]
    levels.set(plan, cache)
  }
  for (let k = cache.length; k <= index; k++) {
    cache[k] = cache[k - 1] * Math.pow(1 + inflationIn(plan, k - 1) / 100, yearLength(plan, k - 1))
  }
  return cache[index]
}

/** The full yearly withdrawal rate at the start of year `index`, inflated from today. */
const yearlyWithdrawal = (plan: Plan, index: number) => plan.withdrawal * priceLevel(plan, index)

/** A recurring amount runs for the part of a short year; a one-off is paid whole. */
const extrasIn = (plan: Plan, year: number, index: number) =>
  plan.extras.reduce((sum, e) => {
    if (year < e.fromYear || year > e.toYear) return sum
    const indexed = e.indexed ? priceLevel(plan, index) : 1
    const share = e.fromYear === e.toYear ? 1 : yearLength(plan, index)
    return sum + e.amount * indexed * share
  }, 0)

/**
 * The plan year by year, with each currency drifting at its assumed rate.
 * `fxDraws`, when given, replaces the drift with one sampled change per
 * year per leg - see `riskBand`.
 */
export function project(plan: Plan, fxDraws?: number[][]): Projection {
  return projectWith(plan, YEARS, (index) => yearlyWithdrawal(plan, index) * yearLength(plan, index), fxDraws)
}

/**
 * The year-by-year step behind `project`, for `years` years, taking each
 * year's withdrawal from `withdrawalIn`: given the plan year and its opening
 * balance, what comes out that year, already cut to a short first year.
 */
export function projectWith(
  plan: Plan,
  years: number,
  withdrawalIn: (index: number, opening: number) => number,
  fxDraws?: number[][],
): Projection {
  const rows: YearRow[] = []
  let balance = plan.corpus
  let runsOutIn: number | null = null
  let yearsLasted = years

  for (let index = 1; index <= years; index++) {
    const year = plan.startYear + index - 1
    const opening = balance
    const length = yearLength(plan, index)
    const r = returnIn(plan, index) / 100
    const withdrawal = withdrawalIn(index, opening)
    const extras = extrasIn(plan, year, index)
    const invested = opening - withdrawal + extras

    let returns = 0
    let fxEffect = 0
    if (invested > 0) {
      returns = invested * (Math.pow(1 + r, length) - 1)
      // Each leg earns the return in its own currency, then converts.
      const growth = plan.legs.reduce((g, leg, i) => {
        const fx = fxDraws ? fxDraws[index - 1][i] : leg.driftPct / 100
        return g + leg.weight * (Math.pow((1 + r) * (1 + fx), length) - 1)
      }, 0)
      fxEffect = invested * growth - returns
    }
    const closing = invested + returns + fxEffect
    balance = closing
    if (runsOutIn === null && closing < 0) {
      runsOutIn = year
      yearsLasted = Math.floor(yearsBefore(plan, index) + 1e-9)
    }

    const nextWithdrawal = withdrawalIn(index + 1, closing)
    rows.push({
      year,
      index,
      opening,
      withdrawal,
      extras,
      returns,
      fxEffect,
      closing,
      closingReal: closing / priceLevel(plan, index + 1),
      status: closing < 0 ? 'negative' : closing < nextWithdrawal * LOW_YEARS ? 'low' : 'ok',
    })
  }

  return { rows, runsOutIn, yearsLasted }
}

/** Small seeded generator, so the band does not shimmer on every render. */
function mulberry32(seed: number) {
  let a = seed
  return () => {
    a |= 0
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

function normal(rand: () => number) {
  const u = Math.max(rand(), 1e-12)
  return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * rand())
}

export interface RiskBand {
  /** Per year: the 10th, 50th and 90th percentile closing balance. */
  p10: number[]
  p50: number[]
  p90: number[]
  /** Share of paths, 0-1, still above zero at the end. */
  survival: number
}

export const RISK_PATHS = 500

/**
 * Currency risk: the same plan run many times, each foreign leg moving by a
 * random yearly change with its assumed drift and historical volatility
 * (log-normal, legs independent). The return itself is held fixed, so the
 * spread is the currency risk alone.
 */
export function riskBand(plan: Plan, paths = RISK_PATHS): RiskBand {
  return sampleBand(plan.legs, YEARS, (draws) => project(plan, draws), paths)
}

/** `riskBand` for any `years`-long projection over these currency legs. */
export function sampleBand(
  legs: CurrencyLeg[],
  years: number,
  run: (fxDraws: number[][]) => Projection,
  paths = RISK_PATHS,
): RiskBand {
  const rand = mulberry32(20261005)
  const byYear: number[][] = Array.from({ length: years }, () => [])
  let survived = 0

  for (let p = 0; p < paths; p++) {
    const draws = Array.from({ length: years }, () =>
      legs.map((leg) => {
        if (leg.volatilityPct === 0) return leg.driftPct / 100
        const sigma = leg.volatilityPct / 100
        // Mean of the log change chosen so the expected change is the drift.
        const mu = Math.log(1 + leg.driftPct / 100) - (sigma * sigma) / 2
        return Math.exp(mu + sigma * normal(rand)) - 1
      }),
    )
    const result = run(draws)
    result.rows.forEach((row, i) => byYear[i].push(row.closing))
    if (result.runsOutIn === null) survived++
  }

  const at = (q: number) =>
    byYear.map((values) => {
      const sorted = [...values].sort((a, b) => a - b)
      return sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]
    })
  return { p10: at(0.1), p50: at(0.5), p90: at(0.9), survival: survived / paths }
}
