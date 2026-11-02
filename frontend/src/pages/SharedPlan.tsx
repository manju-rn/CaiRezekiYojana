/**
 * What a shared link opens, at `/shared/<token>`, signed in or not: the
 * planner alone, read-only, showing only the saved sessions in the link.
 * There is no app navigation and no sign-in. Right-click and selecting text
 * are turned off, which only puts people off copying; what protects the
 * owner is that the server sends nothing but these sessions for the link.
 */
import { useEffect } from 'react'
import { useParams } from 'react-router-dom'

import Brand from '../components/Brand'
import { PageTitleProvider, usePageHeading } from '../components/PageTitle'
import { SharedLinkPlanner } from './Planner'

/** A `<meta>` in the page's head while this page is open. */
function useMeta(name: string, content: string) {
  useEffect(() => {
    const m = document.createElement('meta')
    m.name = name
    m.content = content
    document.head.appendChild(m)
    return () => m.remove()
  }, [name, content])
}

export default function SharedPlan() {
  const { token = '' } = useParams()
  // The link is the credential: kept out of search engines and other sites' logs.
  useMeta('robots', 'noindex, nofollow')
  useMeta('referrer', 'no-referrer')

  return (
    <PageTitleProvider>
      <div
        className="min-h-screen select-none text-ink"
        onContextMenu={(e) => e.preventDefault()}
        onCopy={(e) => e.preventDefault()}
        onDragStart={(e) => e.preventDefault()}
      >
        <Header />
        <main className="mx-auto max-w-[1600px] p-4 sm:p-6">
          <SharedLinkPlanner token={token} />
        </main>
      </div>
    </PageTitleProvider>
  )
}

function Header() {
  const heading = usePageHeading()
  useEffect(() => {
    document.title = heading ? `${heading.title} · CaiRezekiYojana` : 'Shared plan · CaiRezekiYojana'
  }, [heading])
  return (
    <header className="border-b border-[color:var(--hairline)] bg-surface px-4 py-3 sm:px-6">
      <div className="mx-auto flex max-w-[1600px] flex-wrap items-center gap-x-4 gap-y-1">
        <Brand compact />
        <div className="min-w-0">
          <h1 className="truncate text-sm font-semibold text-ink">{heading?.title ?? 'Shared plan'}</h1>
          <p className="text-xs text-ink-muted">A shared plan, read-only</p>
        </div>
      </div>
    </header>
  )
}
