import { Suspense, lazy } from 'react'
import { Navigate, Route, Routes } from 'react-router-dom'

import Layout from './components/Layout'
import { Spinner } from './components/ui'
import { useAmountStyle } from './lib/amounts'

// Each page is its own chunk, loaded the first time it is opened, so the first
// screen - and a shared link, which needs only the planner - does not wait for the rest.
const DeleteData = lazy(() => import('./pages/DeleteData'))
const DhanYatra = lazy(() => import('./pages/DhanYatra'))
const Import = lazy(() => import('./pages/Import'))
const Planner = lazy(() => import('./pages/Planner'))
const Settings = lazy(() => import('./pages/Settings'))
const SettingsLayout = lazy(() => import('./pages/SettingsLayout'))
const SharedPlan = lazy(() => import('./pages/SharedPlan'))

export default function App() {
  // The route elements are made here, so re-rendering on a change of amount
  // style re-renders every page with its figures in the new form.
  useAmountStyle()
  return (
    <Routes>
      {/* A shared plan's link: the planner alone, read-only, whoever opens it. */}
      <Route
        path="shared/:token"
        element={
          <Suspense fallback={<Spinner />}>
            <SharedPlan />
          </Suspense>
        }
      />
      <Route element={<Layout />}>
        <Route index element={<Navigate to="/planner" replace />} />
        <Route path="planner" element={<Planner />} />
        <Route path="dhan-yatra" element={<DhanYatra />} />
        <Route path="settings" element={<SettingsLayout />}>
          <Route index element={<Navigate to="general" replace />} />
          <Route path="general" element={<Settings />} />
          <Route path="import" element={<Import />} />
          <Route path="delete" element={<DeleteData />} />
        </Route>
        <Route path="import" element={<Navigate to="/settings/import" replace />} />
        <Route path="*" element={<Navigate to="/planner" replace />} />
      </Route>
    </Routes>
  )
}
