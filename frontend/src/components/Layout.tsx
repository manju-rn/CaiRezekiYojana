import { Suspense, useEffect, useState } from 'react'
import { Link, Outlet, useLocation } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Menu, Moon, RefreshCw, Settings as SettingsIcon, Sun, X } from 'lucide-react'

import { api } from '../api/client'
import type { RefreshReport } from '../api/types'
import { useAmountStyle } from '../lib/amounts'
import { useTheme } from '../lib/theme'
import Brand from './Brand'
import { Spinner } from './ui'
import { PageTitleProvider, usePageHeading } from './PageTitle'
import { NAV, RailLink, RailToggle, useExpanded, type NavItem } from './nav'

/** The app's shell: its pages, and Settings at the foot. */
export default function Layout() {
  return <Shell items={NAV} foot={{ to: '/settings', label: 'Settings', icon: SettingsIcon }} />
}

/** The frame every screen sits in: a rail of `items`, `foot` pinned under it, and the header. */
export function Shell(props: { items: NavItem[]; foot?: NavItem }) {
  return (
    <PageTitleProvider>
      <Frame {...props} />
    </PageTitleProvider>
  )
}

/** The banner's controls: a rounded pill, lit inside while on or in use. */
const pill =
  'relative flex h-8 items-center gap-0.5 rounded-full border border-[color:var(--hairline)] p-0.5 transition hover:bg-ink/5'
const knob = (on: boolean) =>
  `flex h-6 items-center justify-center gap-1.5 rounded-full text-xs font-medium transition ${
    on ? 'bg-series-1 text-white' : 'text-ink-muted'
  }`

function Frame({
  items,
  foot,
}: {
  items: NavItem[]
  foot?: NavItem
}) {
  const { theme, toggle: toggleTheme } = useTheme()
  const [expanded, toggleExpanded] = useExpanded('cairezeki-nav-expanded')
  const [drawerOpen, setDrawerOpen] = useState(false)
  const location = useLocation()
  const queryClient = useQueryClient()

  // A page picked from the drawer closes it.
  useEffect(() => setDrawerOpen(false), [location.pathname])

  const health = useQuery({
    queryKey: ['health'],
    queryFn: () => api.get<{ status: string; version: string; database: string }>('/health'),
    staleTime: Infinity,
  })

  const refresh = useMutation({
    mutationFn: () => api.post<RefreshReport[]>('/dashboard/refresh'),
    // Everything downstream of prices and FX is now stale.
    onSuccess: () => queryClient.invalidateQueries(),
  })

  const rail = (open: boolean) => (
    <div className="flex h-full flex-col gap-1 p-2">
      <nav aria-label="Main" className="flex flex-col gap-1">
        {items.map((item) => (
          <RailLink key={item.to} item={item} expanded={open} />
        ))}
      </nav>
      <div className="mt-auto flex flex-col gap-1 border-t border-[color:var(--hairline)] pt-2">
        {foot && <RailLink item={foot} expanded={open} />}
        <div className="hidden md:block">
          <RailToggle expanded={expanded} onToggle={toggleExpanded} />
        </div>
      </div>
    </div>
  )

  return (
    <div className="flex min-h-full flex-col">
      {/* The banner spans the rail too, so the mark sits above it. */}
      <header className="banner sticky top-0 z-40 bg-surface">
        <div className="flex h-14 items-center gap-3 px-3 md:px-4">
          <button
            type="button"
            className="-ml-1 rounded-md p-1.5 text-ink-secondary hover:bg-ink/5 md:hidden"
            onClick={() => setDrawerOpen((o) => !o)}
            aria-label="Open navigation"
          >
            {drawerOpen ? <X size={20} /> : <Menu size={20} />}
          </button>

          <Link to="/planner" className="shrink-0" aria-label="CaiRezekiYojana home">
            <Brand compact />
          </Link>

          <span aria-hidden className="h-7 w-px shrink-0 bg-[color:var(--edge)]" />

          <BannerHeading items={items} foot={foot} />

          <div className="ml-auto flex shrink-0 items-center gap-2">
            {refresh.isError && (
              <span className="hidden text-xs text-status-critical sm:inline">Refresh failed</span>
            )}
            {(
              <button
                type="button"
                onClick={() => refresh.mutate()}
                disabled={refresh.isPending}
                title={refresh.isPending ? 'Refreshing prices & FX…' : 'Refresh prices & FX'}
                aria-label="Refresh prices & FX"
                className={pill}
              >
                <span className={`${knob(refresh.isPending)} w-6`}>
                  <RefreshCw
                    size={14}
                    strokeWidth={2}
                    className={refresh.isPending ? 'animate-spin' : ''}
                    aria-hidden
                  />
                </span>
              </button>
            )}
            {<AmountSwitch />}
            <ThemeSwitch theme={theme} onToggle={toggleTheme} />
          </div>
        </div>
      </header>

      <div className="flex flex-1">
        <aside
          className={`sticky top-14 z-30 hidden h-[calc(100vh-3.5rem)] shrink-0 border-r border-[color:var(--edge)] bg-surface transition-[width] md:block ${
            expanded ? 'w-56' : 'w-14'
          }`}
        >
          {rail(expanded)}
        </aside>

        {/* Below md the rail becomes a drawer, always with its labels. */}
        {drawerOpen && (
          <div className="fixed inset-0 z-50 md:hidden">
            <div className="absolute inset-0 bg-black/50" onClick={() => setDrawerOpen(false)} />
            <aside className="absolute inset-y-0 left-0 w-60 border-r border-[color:var(--hairline)] bg-surface">
              {rail(true)}
            </aside>
          </div>
        )}

        <div className="flex min-w-0 flex-1 flex-col">
          <main className="mx-auto w-full max-w-[1400px] flex-1 px-4 py-5">
            {/* Each page is loaded when first opened: the frame stays while it comes. */}
            <Suspense fallback={<Spinner />}>
              <Outlet />
            </Suspense>
          </main>

          <footer className="mx-auto w-full max-w-[1400px] px-4 pb-8 pt-2 text-[11px] text-ink-muted">
            CaiRezekiYojana {health.data?.version ?? ''} · {health.data?.database ?? 'database'} · Prices
            and rates are indicative and may be delayed.
          </footer>
        </div>
      </div>
    </div>
  )
}

/**
 * The current page's title in the banner: what the page set with
 * <PageTitle>, or else the label of the rail entry it sits under.
 */
function BannerHeading({ items, foot }: { items: NavItem[]; foot?: NavItem }) {
  const heading = usePageHeading()
  const { pathname } = useLocation()
  const entry = [...items, ...(foot ? [foot] : [])].find((i) =>
    i.end ? pathname === i.to : pathname === i.to || pathname.startsWith(`${i.to}/`),
  )
  const title = heading?.title ?? entry?.label
  if (!title) return <div className="min-w-0 flex-1" />
  // A page nested under an entry (Settings → General) names its parent too.
  const parent = entry && pathname !== entry.to && entry.label !== title ? entry.label : undefined
  return (
    <div className="flex min-w-0 flex-1 flex-col justify-center leading-tight">
      <h1 className="truncate text-base font-semibold text-ink sm:text-lg">
        {parent && <span className="font-normal text-ink-muted">{parent} › </span>}
        {title}
      </h1>
      {heading?.description && (
        <p className="hidden truncate text-xs text-ink-muted md:block" title={heading.description}>
          {heading.description}
        </p>
      )}
    </div>
  )
}

/** Amounts in full or short (Cr / L, M / K), for every page; the current one lit. */
function AmountSwitch() {
  const { style, setStyle } = useAmountStyle()
  const short = style === 'short'
  return (
    <button
      type="button"
      role="switch"
      aria-checked={short}
      aria-label="Short amounts, in Cr / L or M / K"
      title={short ? 'Show amounts in full' : 'Show amounts in Cr / L or M / K'}
      onClick={() => setStyle(short ? 'full' : 'short')}
      className={pill}
    >
      <span className={`${knob(!short)} px-2`}>Full</span>
      <span className={`${knob(short)} px-2`}>Cr / M</span>
    </button>
  )
}

/** Sun and moon side by side, the current theme's lit. */
function ThemeSwitch({ theme, onToggle }: { theme: 'light' | 'dark'; onToggle: () => void }) {
  const dark = theme === 'dark'
  return (
    <button
      type="button"
      role="switch"
      aria-checked={dark}
      aria-label="Dark mode"
      title={dark ? 'Switch to light mode' : 'Switch to dark mode'}
      onClick={onToggle}
      className={pill}
    >
      <span className={`${knob(!dark)} w-6`}>
        <Sun size={14} strokeWidth={2} aria-hidden />
      </span>
      <span className={`${knob(dark)} w-6`}>
        <Moon size={14} strokeWidth={2} aria-hidden />
      </span>
    </button>
  )
}
