/**
 * Thin fetch wrapper.
 *
 * The server returns `{ error, message }` on failure; we surface `message`
 * because it is written for a person to read.
 */

const BASE = '/api/v1'

/** Fired on any 401, so the app can drop back to the sign-in screen. */
export const UNAUTHORIZED_EVENT = 'cairezeki:unauthorized'

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

async function parse<T>(res: Response): Promise<T> {
  if (res.status === 204) return undefined as T

  const text = await res.text()
  const body = text ? JSON.parse(text) : null

  if (!res.ok) {
    // The session ended - expired, signed out elsewhere, or the account was
    // disabled or locked. The auth context listens and shows the sign-in.
    if (res.status === 401) window.dispatchEvent(new CustomEvent(UNAUTHORIZED_EVENT))
    throw new ApiError(
      res.status,
      body?.error ?? 'error',
      body?.message ?? `Request failed with status ${res.status}`,
    )
  }
  return body as T
}

/** A read that has not answered in this long is given up on, not waited for forever. */
const READ_TIMEOUT_MS = 20_000

/**
 * A request that never got an answer - timed out, or the connection failed -
 * as an `ApiError` with status 0, so callers deal with one kind of failure.
 */
function unanswered(e: unknown): never {
  if (e instanceof ApiError) throw e
  const timedOut = e instanceof DOMException && (e.name === 'TimeoutError' || e.name === 'AbortError')
  throw new ApiError(
    0,
    timedOut ? 'timeout' : 'network',
    timedOut ? 'The server took too long to answer.' : 'Could not reach the server.',
  )
}

function url(path: string, params?: Record<string, unknown>): string {
  const u = new URL(BASE + path, window.location.origin)
  for (const [k, v] of Object.entries(params ?? {})) {
    if (v !== undefined && v !== null && v !== '') u.searchParams.set(k, String(v))
  }
  return u.pathname + u.search
}

export const api = {
  get: <T>(path: string, params?: Record<string, unknown>, opts?: { timeoutMs?: number }) =>
    fetch(url(path, params), { signal: AbortSignal.timeout(opts?.timeoutMs ?? READ_TIMEOUT_MS) })
      .then(parse<T>)
      .catch(unanswered),

  post: <T>(path: string, body?: unknown) =>
    fetch(url(path), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    }).then(parse<T>),

  put: <T>(path: string, body: unknown) =>
    fetch(url(path), {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }).then(parse<T>),

  del: <T>(path: string) => fetch(url(path), { method: 'DELETE' }).then(parse<T>),

  /** POST returning a file, for downloads that need a request body. */
  download: async (path: string, body: unknown): Promise<{ blob: Blob; filename: string }> => {
    const res = await fetch(url(path), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (!res.ok) await parse<never>(res)
    const disposition = res.headers.get('Content-Disposition') ?? ''
    const filename = /filename="([^"]+)"/.exec(disposition)?.[1] ?? 'download'
    return { blob: await res.blob(), filename }
  },

  upload: <T>(path: string, file: File, params?: Record<string, unknown>) => {
    const form = new FormData()
    form.append('file', file)
    return fetch(url(path, params), { method: 'POST', body: form }).then(parse<T>)
  },

  /** Absolute href for downloads, so the browser handles them natively. */
  href: (path: string) => BASE + path,
}
