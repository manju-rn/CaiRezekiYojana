import { Suspense } from 'react'
import { Outlet } from 'react-router-dom'

import { RailLink, RailToggle, SETTINGS_NAV, useExpanded } from '../components/nav'
import { Spinner } from '../components/ui'

/**
 * Settings has a rail of its own beside the main one, collapsed to icons
 * like it. Below md it folds to a row of icons over the page.
 */
export default function SettingsLayout() {
  const [expanded, toggle] = useExpanded('cairezeki-settings-nav-expanded')
  return (
    <div className="flex flex-col gap-5 md:flex-row">
      <aside
        className={`shrink-0 md:sticky md:top-[4.75rem] md:self-start ${expanded ? 'md:w-48' : 'md:w-12'}`}
      >
        <nav
          aria-label="Settings"
          className="card flex gap-1 overflow-x-auto p-1.5 md:flex-col md:overflow-visible"
        >
          {SETTINGS_NAV.map((s) => (
            <RailLink key={s.to} item={s} expanded={expanded} />
          ))}
          <div className="ml-auto hidden border-[color:var(--hairline)] md:ml-0 md:mt-1 md:block md:border-t md:pt-1">
            <RailToggle expanded={expanded} onToggle={toggle} />
          </div>
        </nav>
      </aside>
      <div className="min-w-0 flex-1">
        <Suspense fallback={<Spinner />}>
          <Outlet />
        </Suspense>
      </div>
    </div>
  )
}
