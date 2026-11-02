/** Number, money and date formatting. */

import { amountStyle } from './amounts'

const CURRENCY_LOCALE: Record<string, string> = {
  // Indian grouping (1,23,456) is what an Indian portfolio should read as.
  INR: 'en-IN',
  SGD: 'en-SG',
  AUD: 'en-AU',
  USD: 'en-US',
  GBP: 'en-GB',
  EUR: 'de-DE',
  HKD: 'en-HK',
  JPY: 'ja-JP',
  CAD: 'en-CA',
}

const ZERO_DECIMAL = new Set(['JPY'])

/** Between the currency symbol and the first digit: a narrow no-break space,
 *  two or three pixels at body size, so "₹75" does not set as one glyph and
 *  the symbol can never be orphaned at a line break. */
const SYMBOL_GAP = ' '

export interface MoneyOptions {
  /** Short form for headlines and legends: ₹1.2Cr, $3.4M - one decimal at
   *  most, dropped where it is zero. */
  compact?: boolean
  /** Whole units, no minor-unit digits: portfolio-scale figures where the
   *  paise are noise. Unit prices keep their decimals. With the banner's
   *  amounts set short, these read in Cr / L or M / K instead. */
  whole?: boolean
}

export function money(value: number | null | undefined, currency: string, opts?: MoneyOptions): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—'
  if (opts?.whole && !opts.compact && amountStyle() === 'short') return bigMoney(value, currency)
  const locale = CURRENCY_LOCALE[currency] ?? 'en-US'
  const digits = ZERO_DECIMAL.has(currency) || opts?.compact || opts?.whole ? 0 : 2
  const maxDigits = opts?.compact && !ZERO_DECIMAL.has(currency) ? 1 : digits

  const parts = new Intl.NumberFormat(locale, {
    style: 'currency',
    currency,
    minimumFractionDigits: digits,
    maximumFractionDigits: maxDigits,
    notation: opts?.compact ? 'compact' : 'standard',
  // -0 (an empty sum that went through a negation) would print as "-₹ 0".
  }).formatToParts(value === 0 ? 0 : value)

  // Only where the locale runs the symbol straight into the digits. One that
  // already spaces it, or sets it after the figure, is left as it is.
  return parts
    .map((part, i) => {
      const next = parts[i + 1]
      const glued = part.type === 'currency' && next !== undefined && next.type !== 'literal'
      return glued ? part.value + SYMBOL_GAP : part.value
    })
    .join('')
}

/**
 * "USD 1.23M": the code in front of the figure. For amounts set beside the
 * same amount in other currencies, where a bare `$` could be US or
 * Singapore dollars. Compact keeps one decimal at most, as `money` does.
 */
export function codeMoney(
  value: number | null | undefined,
  currency: string,
  opts?: MoneyOptions,
): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—'
  const locale = CURRENCY_LOCALE[currency] ?? 'en-US'
  const digits = ZERO_DECIMAL.has(currency) || opts?.whole ? 0 : 2
  const figure = new Intl.NumberFormat(
    locale,
    opts?.compact
      ? { notation: 'compact', maximumFractionDigits: ZERO_DECIMAL.has(currency) ? 0 : 1 }
      : { minimumFractionDigits: digits, maximumFractionDigits: digits },
  ).format(value)
  return `${currency} ${figure}`
}

/** Short symbols for tight spots; the dollars keep a prefix so US and
 *  Singapore dollars stay apart. Unlisted currencies show their code. */
const CURRENCY_SYMBOL: Record<string, string> = {
  INR: '₹',
  SGD: 'S$',
  USD: '$',
  AUD: 'A$',
  CAD: 'C$',
  HKD: 'HK$',
  GBP: '£',
  EUR: '€',
  JPY: '¥',
}

export function currencySymbol(currency: string): string {
  return CURRENCY_SYMBOL[currency] ?? currency
}

/** The locale a currency's figures are grouped in: 1,23,456 for INR. */
export function currencyLocale(currency: string): string {
  return CURRENCY_LOCALE[currency] ?? 'en-US'
}

/** Short units, largest first: crore and lakh for rupees, B/M/K elsewhere. */
const SHORT_UNITS: Record<'INR' | 'other', [number, string][]> = {
  INR: [
    [1e7, ' Cr'],
    [1e5, ' L'],
    [1e3, ' K'],
  ],
  other: [
    [1e9, 'B'],
    [1e6, 'M'],
    [1e3, 'K'],
  ],
}

/**
 * An amount in the largest unit it fills, two decimals: ₹ 1.23 Cr, ₹ 45.60 L,
 * ₹ 7.50 K for rupees; $ 1.23M, $ 45.60K elsewhere. Under a thousand it has
 * no unit.
 */
export function bigMoney(value: number | null | undefined, currency: string): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—'
  const round2 = (v: number) => Math.round(v * 100) / 100
  // Chosen after rounding, so 99,99,999 reads 1.00 Cr rather than 100.00 L.
  const [divisor, unit] = SHORT_UNITS[currency === 'INR' ? 'INR' : 'other'].find(
    ([d]) => Math.abs(round2(value / d)) >= 1,
  ) ?? [1, '']
  const scaled = round2(value / divisor)
  const parts = new Intl.NumberFormat(currencyLocale(currency), {
    style: 'currency',
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).formatToParts(scaled === 0 ? 0 : scaled)
  return parts
    .map((part, i) => {
      const next = parts[i + 1]
      const glued = part.type === 'currency' && next !== undefined && next.type !== 'literal'
      const value = glued ? part.value + SYMBOL_GAP : part.value
      // The unit follows the last digit, ahead of a symbol set after the figure.
      return part.type === 'fraction' ? value + unit : value
    })
    .join('')
}

export function num(value: number | null | undefined, digits = 2): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—'
  return new Intl.NumberFormat('en-US', {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value)
}

/** Units/shares: show what is there, without trailing-zero noise. */
export function qty(value: number | null | undefined): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—'
  return new Intl.NumberFormat('en-US', { maximumFractionDigits: 4 }).format(value)
}

export function pct(value: number | null | undefined, digits = 2): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—'
  return `${value >= 0 ? '' : ''}${num(value, digits)}%`
}

/** Signed, for gains and losses where direction is the point. */
export function signedPct(value: number | null | undefined, digits = 2): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—'
  return `${value >= 0 ? '+' : ''}${num(value, digits)}%`
}

export function signedMoney(
  value: number | null | undefined,
  currency: string,
  opts?: MoneyOptions,
): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—'
  const formatted = money(Math.abs(value), currency, opts)
  return value < 0 ? `−${formatted}` : `+${formatted}`
}

/** An FX rate needs more precision than money - JPY crosses are tiny. */
export function rate(value: number | null | undefined): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—'
  const digits = Math.abs(value) >= 100 ? 2 : Math.abs(value) >= 1 ? 4 : 6
  return num(value, digits)
}

export function date(value: string | null | undefined): string {
  if (!value) return '—'
  // A bare `YYYY-MM-DD` parses as UTC midnight, which renders as the day
  // before in any negative-offset zone. Pin it to local midnight so a trade
  // date never drifts; timestamps already carry their own offset.
  const d = new Date(/^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T00:00:00` : value)
  if (Number.isNaN(d.getTime())) return value
  return d.toLocaleDateString('en-GB', { day: '2-digit', month: 'short', year: 'numeric' })
}

export function relativeDays(days: number): string {
  if (days < 0) return `${Math.abs(days)}d ago`
  if (days === 0) return 'today'
  if (days === 1) return 'tomorrow'
  if (days < 60) return `in ${days}d`
  return `in ${Math.round(days / 30)}mo`
}

/** Tailwind class for a gain/loss figure. Neutral at exactly zero. */
export function deltaClass(value: number | null | undefined): string {
  if (value === null || value === undefined || value === 0) return 'text-ink-secondary'
  return value > 0 ? 'text-gain' : 'text-loss'
}

export function humanise(code: string): string {
  return code
    .split('_')
    .map((w) => w.charAt(0) + w.slice(1).toLowerCase())
    .join(' ')
}

/**
 * The ticker, or the ISIN when that is all the statement gave us. Mutual
 * funds rarely have a ticker, so their symbol is the ISIN itself and is
 * shown once, labelled.
 */
export function instrumentIdentifier(p: { symbol: string; isin: string | null }): string {
  if (p.isin && p.isin === p.symbol) return `ISIN ${p.isin}`
  if (p.isin) return `${p.symbol} · ISIN ${p.isin}`
  return p.symbol
}
