import { createContext, useContext, useLayoutEffect, useState, type ReactNode } from 'react'

export interface PageHeading {
  title: string
  description?: string
}

const SetHeading = createContext<(h: PageHeading | null) => void>(() => {})
const Heading = createContext<PageHeading | null>(null)

/** Holds the heading of whichever page is showing, for the banner to read. */
export function PageTitleProvider({ children }: { children: ReactNode }) {
  const [heading, setHeading] = useState<PageHeading | null>(null)
  return (
    <SetHeading.Provider value={setHeading}>
      <Heading.Provider value={heading}>{children}</Heading.Provider>
    </SetHeading.Provider>
  )
}

/** The current page's heading, or null where the page has not set one. */
export function usePageHeading() {
  return useContext(Heading)
}

/**
 * A page's title and one-line description. Rendered in the page, shown in
 * the banner: nothing appears where this sits. Layout effect, so the banner
 * never paints the previous page's title over the new page.
 */
export function PageTitle({ title, description }: PageHeading) {
  const set = useContext(SetHeading)
  useLayoutEffect(() => {
    set({ title, description })
    return () => set(null)
  }, [set, title, description])
  return null
}
