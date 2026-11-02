import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useRef, useState } from 'react'

import { PageTitle } from '../components/PageTitle'
import { api } from '../api/client'
import type { ImportBatch, ImportOutcome, ImportSpec, UploadAudit } from '../api/types'
import { Badge, Button, Card, ErrorBanner, Spinner } from '../components/ui'
import { date, humanise } from '../lib/format'

/** The one format: a single CSV holding every holding. */
const KIND = 'portfolio'

/**
 * The Import screen.
 *
 * The format documentation is fetched from the server rather than written
 * here, so what the UI promises and what the parser accepts cannot drift.
 */
export default function Import() {
  const queryClient = useQueryClient()
  const fileInput = useRef<HTMLInputElement>(null)

  const [file, setFile] = useState<File | null>(null)
  const [outcome, setOutcome] = useState<ImportOutcome | null>(null)

  const specs = useQuery({
    queryKey: ['import-formats'],
    queryFn: () => api.get<ImportSpec[]>('/imports/formats'),
  })
  const batches = useQuery({
    queryKey: ['import-batches'],
    queryFn: () => api.get<ImportBatch[]>('/imports'),
  })

  const upload = useMutation({
    mutationFn: ({ dryRun }: { dryRun: boolean }) =>
      api.upload<ImportOutcome>(`/imports/${KIND}`, file!, { dry_run: dryRun }),
    onSuccess: (result) => {
      setOutcome(result)
      // Every upload, a validation included, is a step in the audit.
      queryClient.invalidateQueries({ queryKey: ['upload-audit'] })
      if (!result.dry_run) {
        queryClient.invalidateQueries()
        setFile(null)
        if (fileInput.current) fileInput.current.value = ''
      }
    },
  })

  const undo = useMutation({
    mutationFn: (id: number) => api.del(`/imports/${id}/undo`),
    onSuccess: () => queryClient.invalidateQueries(),
  })

  const spec = specs.data?.[0]

  return (
    <div className="space-y-5">
      <PageTitle
        title="Import your portfolio"
        description="One CSV with every holding: stocks, ETFs, mutual funds, deposits and cash. A ready-made example is samples/cairezeki-sample-portfolio.csv."
      />

      {specs.isLoading && <Spinner />}
      {specs.error && <ErrorBanner error={specs.error} />}

      {spec && (
        <Card
          title={`1 · The ${spec.title} format`}
          subtitle={spec.description}
          actions={
            <a
              href={api.href(`/imports/templates/${KIND}`)}
              className="rounded-md border border-[color:var(--hairline)] px-3 py-1.5 text-sm font-medium text-ink hover:bg-ink/5"
              download
            >
              Download template
            </a>
          }
        >
          <ul className="mb-4 list-disc space-y-1 pl-5 text-xs text-ink-secondary">
            {spec.notes.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>

          <div className="overflow-x-auto">
            <table className="w-full min-w-[720px] text-xs">
              <thead>
                <tr className="border-b border-[color:var(--hairline)] text-left text-ink-muted">
                  <th className="px-2 py-1.5 font-medium">Column</th>
                  <th className="px-2 py-1.5 font-medium">Required</th>
                  <th className="px-2 py-1.5 font-medium">Example</th>
                  <th className="px-2 py-1.5 font-medium">Meaning</th>
                  <th className="px-2 py-1.5 font-medium">Also accepted as</th>
                </tr>
              </thead>
              <tbody>
                {spec.columns.map((c) => (
                  <tr
                    key={c.name}
                    className="border-b border-[color:var(--hairline)] last:border-0"
                  >
                    <td className="px-2 py-1.5 font-medium text-ink">{c.heading}</td>
                    <td className="px-2 py-1.5">
                      {c.required ? <Badge tone="info">required</Badge> : <span className="text-ink-muted">optional</span>}
                    </td>
                    <td className="tabular px-2 py-1.5 text-ink-secondary">{c.example || '—'}</td>
                    <td className="px-2 py-1.5 text-ink-secondary">{c.description}</td>
                    <td className="px-2 py-1.5 text-ink-muted">
                      {c.aliases.length ? c.aliases.join(', ') : '—'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}

      <Card
        title="2 · Upload"
        subtitle="Validate first to see what would happen without writing anything."
      >
        <div className="flex flex-wrap items-center gap-3">
          <input
            ref={fileInput}
            type="file"
            accept=".csv,.tsv,.xlsx,.xls,.ods"
            onChange={(e) => {
              setFile(e.target.files?.[0] ?? null)
              setOutcome(null)
            }}
            className="text-sm text-ink-secondary file:mr-3 file:rounded-md file:border file:border-[color:var(--hairline)] file:bg-surface file:px-3 file:py-1.5 file:text-sm file:font-medium file:text-ink"
          />
          <Button
            onClick={() => upload.mutate({ dryRun: true })}
            disabled={!file || upload.isPending}
          >
            Validate only
          </Button>
          <Button
            variant="primary"
            onClick={() => upload.mutate({ dryRun: false })}
            disabled={!file || upload.isPending}
          >
            {upload.isPending ? 'Working…' : 'Import'}
          </Button>
        </div>

        {upload.isError && (
          <div className="mt-4">
            <ErrorBanner error={upload.error} />
          </div>
        )}
        {outcome && <Outcome outcome={outcome} />}
      </Card>

      <Card title="Recent imports">
        {batches.isLoading ? (
          <Spinner />
        ) : batches.data?.length === 0 ? (
          <p className="py-4 text-sm text-ink-muted">No imports yet.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[720px] text-sm">
              <thead>
                <tr className="border-b border-[color:var(--hairline)] text-left text-xs text-ink-muted">
                  <th className="px-2 py-2 font-medium">When</th>
                  <th className="px-2 py-2 font-medium">File</th>
                  <th className="px-2 py-2 font-medium">Format</th>
                  <th className="px-2 py-2 text-right font-medium">Rows</th>
                  <th className="px-2 py-2 font-medium">Result</th>
                  <th className="px-2 py-2" />
                </tr>
              </thead>
              <tbody>
                {batches.data?.map((b) => (
                  <tr
                    key={b.id}
                    className="border-b border-[color:var(--hairline)] last:border-0"
                  >
                    <td className="px-2 py-2 text-xs text-ink-secondary">{date(b.created_at)}</td>
                    <td className="max-w-[16rem] truncate px-2 py-2 text-ink">{b.filename}</td>
                    <td className="px-2 py-2 text-xs text-ink-secondary">{humanise(b.kind)}</td>
                    <td className="tabular px-2 py-2 text-right text-xs">
                      {b.ok_count} ok
                      {b.skipped_count > 0 && ` · ${b.skipped_count} skipped`}
                      {b.error_count > 0 && ` · ${b.error_count} failed`}
                    </td>
                    <td className="px-2 py-2">
                      <Badge
                        tone={
                          b.status === 'COMPLETED'
                            ? 'good'
                            : b.status === 'PARTIAL'
                              ? 'warning'
                              : 'critical'
                        }
                      >
                        {humanise(b.status)}
                      </Badge>
                      {b.message && (
                        <div className="mt-0.5 text-[11px] text-ink-muted">{b.message}</div>
                      )}
                    </td>
                    <td className="px-2 py-2 text-right">
                      {b.ok_count > 0 && (
                          <Button
                            variant="ghost"
                            onClick={() => {
                              if (
                                window.confirm(
                                  `Remove the ${b.ok_count} row(s) imported from ${b.filename}? This cannot be undone.`,
                                )
                              ) {
                                undo.mutate(b.id)
                              }
                            }}
                          >
                            Undo
                          </Button>
                        )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>

      <UploadAuditCard />
    </div>
  )
}

function Outcome({ outcome }: { outcome: ImportOutcome }) {
  const clean = outcome.error_count === 0
  return (
    <div className="mt-4 rounded-lg border border-[color:var(--hairline)] p-3">
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={clean ? 'good' : outcome.ok_count > 0 ? 'warning' : 'critical'}>
          {outcome.dry_run ? 'Validation' : humanise(outcome.status)}
        </Badge>
        <span className="text-sm text-ink">
          {outcome.ok_count} of {outcome.row_count} row(s){' '}
          {outcome.dry_run ? 'would import' : 'imported'}
          {outcome.skipped_count > 0 && `, ${outcome.skipped_count} already on file`}
          {outcome.error_count > 0 && `, ${outcome.error_count} rejected`}.
        </span>
      </div>
      {outcome.stored_as && (
        <p className="mt-2 text-xs text-ink-muted">
          Kept read-only as uploads/{outcome.stored_as}
          {outcome.audit_seq !== null && ` · audit #${outcome.audit_seq}`}
        </p>
      )}

      {outcome.created_accounts.length > 0 && (
        <p className="mt-2 text-xs text-ink-secondary">
          New accounts: {outcome.created_accounts.join(', ')}
        </p>
      )}
      {outcome.created_instruments.length > 0 && (
        <p className="mt-1 text-xs text-ink-secondary">
          New instruments: {outcome.created_instruments.slice(0, 12).join(', ')}
          {outcome.created_instruments.length > 12 &&
            ` and ${outcome.created_instruments.length - 12} more`}
        </p>
      )}
      {outcome.unrecognised_columns.length > 0 && (
        <p className="mt-1 text-xs text-ink-muted">
          Ignored columns: {outcome.unrecognised_columns.join(', ')}
        </p>
      )}

      {outcome.errors.length > 0 && (
        <div className="mt-3">
          <div className="mb-1 text-xs font-medium text-ink">Rejected rows</div>
          <ul className="max-h-64 space-y-1 overflow-y-auto text-xs">
            {outcome.errors.map((e) => (
              <li key={e.row} className="flex gap-2">
                <span className="tabular shrink-0 text-ink-muted">Row {e.row}</span>
                <span className="text-ink-secondary">{e.message}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
}

const ACTION_LABEL: Record<UploadAudit['action'], string> = {
  VALIDATE: 'Validated',
  IMPORT: 'Imported',
  UNDO: 'Undone',
}

/**
 * Every upload and undo, in order, newest first. Nothing here is ever
 * removed: each file is kept read-only and can be downloaded exactly as it
 * was uploaded, and its SHA-256 shows it has not changed since. The same
 * sequence is in audit.txt in the uploads folder.
 */
function UploadAuditCard() {
  const audit = useQuery({
    queryKey: ['upload-audit'],
    queryFn: () => api.get<UploadAudit[]>('/uploads'),
  })

  return (
    <Card
      title="Upload audit"
      subtitle="Every file uploaded and every undo, in order. Files are kept read-only in the uploads folder, which also holds this sequence as audit.txt."
    >
      {audit.isLoading ? (
        <Spinner />
      ) : audit.error ? (
        <ErrorBanner error={audit.error} />
      ) : audit.data?.length === 0 ? (
        <p className="py-4 text-sm text-ink-muted">Nothing uploaded yet.</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[900px] text-sm">
            <thead>
              <tr className="border-b border-[color:var(--hairline)] text-left text-xs text-ink-muted">
                <th className="px-2 py-2 text-right font-medium">#</th>
                <th className="px-2 py-2 font-medium">When (UTC)</th>
                <th className="px-2 py-2 font-medium">Step</th>
                <th className="px-2 py-2 font-medium">File</th>
                <th className="px-2 py-2 font-medium">SHA-256</th>
                <th className="px-2 py-2 font-medium">Result</th>
              </tr>
            </thead>
            <tbody>
              {audit.data?.map((a) => (
                <tr key={a.id} className="border-b border-[color:var(--hairline)] align-top last:border-0">
                  <td className="tabular px-2 py-2 text-right text-ink-muted">{a.id}</td>
                  <td className="tabular whitespace-nowrap px-2 py-2 text-xs text-ink-secondary">
                    {a.created_at.slice(0, 19).replace('T', ' ')}
                  </td>
                  <td className="px-2 py-2">
                    <div className="text-ink">{ACTION_LABEL[a.action] ?? a.action}</div>
                    <div className="text-xs text-ink-muted">{humanise(a.kind)}</div>
                  </td>
                  <td className="max-w-[22rem] px-2 py-2">
                    <div className="break-words text-ink">{a.original_name}</div>
                    {a.stored_as ? (
                      <a
                        href={api.href(`/uploads/${a.id}/file`)}
                        className="break-all text-xs text-series-1 hover:underline"
                        title="Download the kept copy"
                      >
                        {a.stored_as}
                      </a>
                    ) : (
                      <div className="text-xs text-ink-muted">not kept</div>
                    )}
                  </td>
                  <td className="tabular px-2 py-2 text-xs text-ink-muted" title={a.sha256 ?? undefined}>
                    {a.sha256 ? `${a.sha256.slice(0, 12)}…` : '—'}
                  </td>
                  <td className="px-2 py-2">
                    <Badge
                      tone={
                        a.status === 'COMPLETED' || a.status === 'VALIDATED'
                          ? 'good'
                          : a.status === 'PARTIAL' || a.status === 'REVERTED'
                            ? 'warning'
                            : 'critical'
                      }
                    >
                      {humanise(a.status)}
                    </Badge>
                    <div className="mt-0.5 text-[11px] text-ink-muted">
                      {a.action === 'UNDO'
                        ? `${a.ok_count} transaction(s) removed from batch ${a.batch_id}`
                        : `${a.ok_count} ok · ${a.skipped_count} skipped · ${a.error_count} failed${
                            a.batch_id !== null ? ` · batch ${a.batch_id}` : ''
                          }`}
                    </div>
                    {a.message && a.action !== 'UNDO' && (
                      <div className="mt-0.5 text-[11px] text-status-serious">{a.message}</div>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  )
}
