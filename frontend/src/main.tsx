import React from 'react'
import ReactDOM from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import App from './App'
import './index.css'

// Pages load as they are opened. A tab still open from before a release asks
// for the old release's files, which are gone: load the new release instead.
window.addEventListener('vite:preloadError', (e) => {
  const key = 'cairezeki:reloaded-for-release'
  // Once a minute at most, so a file that is really missing does not reload forever.
  const last = Number(sessionStorage.getItem(key) ?? 0)
  if (Date.now() - last < 60_000) return
  e.preventDefault()
  sessionStorage.setItem(key, String(Date.now()))
  window.location.reload()
})

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Prices move slowly enough that refetching on every focus is noise.
      staleTime: 60_000,
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
})

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </React.StrictMode>,
)
