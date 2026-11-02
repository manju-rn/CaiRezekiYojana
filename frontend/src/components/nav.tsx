import { useCallback, useEffect, useState } from 'react'
import { NavLink } from 'react-router-dom'
import {
  FileUp,
  PanelLeftClose,
  PanelLeftOpen,
  Shuffle,
  SlidersHorizontal,
  Trash2,
  type LucideIcon,
} from 'lucide-react'

import { MoneyOnWheels } from './icons'

export interface NavItem {
  to: string
  label: string
  icon: LucideIcon
  end?: boolean
}

/** The main screens, top to bottom. Settings is pinned apart, at the foot. */
export const NAV: NavItem[] = [
  { to: '/planner', label: 'RezekiYojana', icon: Shuffle },
  { to: '/dhan-yatra', label: 'CaiYatra', icon: MoneyOnWheels },
]

/** Everything that configures or feeds the app. */
export const SETTINGS_NAV: NavItem[] = [
  { to: 'general', label: 'General', icon: SlidersHorizontal },
  { to: 'import', label: 'Import', icon: FileUp },
  { to: 'delete', label: 'Delete data', icon: Trash2 },
]

/** Whether a rail shows its labels, remembered per rail. Collapsed by default. */
export function useExpanded(key: string) {
  const [expanded, setExpanded] = useState(() => localStorage.getItem(key) === 'true')
  useEffect(() => localStorage.setItem(key, String(expanded)), [key, expanded])
  const toggle = useCallback(() => setExpanded((e) => !e), [])
  return [expanded, toggle] as const
}

/**
 * One entry on a rail. Collapsed, only the icon shows and the label comes
 * up beside it on hover or keyboard focus.
 */
export function RailLink({
  item,
  expanded,
  onNavigate,
}: {
  item: NavItem
  expanded: boolean
  onNavigate?: () => void
}) {
  const Icon = item.icon
  return (
    <NavLink
      to={item.to}
      end={item.end}
      onClick={onNavigate}
      aria-label={expanded ? undefined : item.label}
      className={({ isActive }) =>
        `group relative flex h-9 items-center gap-3 rounded-md px-2.5 text-sm transition ${
          isActive
            ? 'bg-series-1/15 font-medium text-series-1'
            : 'text-ink-secondary hover:bg-ink/5 hover:text-ink'
        }`
      }
    >
      <Icon size={18} strokeWidth={1.75} className="shrink-0" aria-hidden />
      {expanded ? (
        <span className="truncate">{item.label}</span>
      ) : (
        <span
          role="tooltip"
          className="pointer-events-none absolute left-full top-1/2 z-50 ml-2 -translate-y-1/2 whitespace-nowrap rounded-md border border-[color:var(--hairline)] bg-surface px-2 py-1 text-xs font-medium text-ink opacity-0 shadow-lg transition group-hover:opacity-100 group-focus-visible:opacity-100"
        >
          {item.label}
        </span>
      )}
    </NavLink>
  )
}

/** Opens or closes a rail's labels. */
export function RailToggle({ expanded, onToggle }: { expanded: boolean; onToggle: () => void }) {
  const Icon = expanded ? PanelLeftClose : PanelLeftOpen
  return (
    <button
      type="button"
      onClick={onToggle}
      title={expanded ? 'Collapse' : 'Expand'}
      aria-label={expanded ? 'Collapse navigation' : 'Expand navigation'}
      aria-expanded={expanded}
      className="flex h-9 w-full items-center gap-3 rounded-md px-2.5 text-sm text-ink-muted transition hover:bg-ink/5 hover:text-ink"
    >
      <Icon size={18} strokeWidth={1.75} className="shrink-0" aria-hidden />
      {expanded && <span>Collapse</span>}
    </button>
  )
}
