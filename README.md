# CaiRezekiYojana

A single-owner planner, carved out of Nestivesh: just the planning half, with
no sign-in and **SGD** as the default currency.

- **RezekiYojana** (`/planner`) - the allocation planner. Plan moves of money
  out of what you hold today (a category, one fund in it, or fresh money) into
  other categories and funds, and see today against planned, with sessions you
  can save, download and share as a read-only link.
- **CaiYatra** (`/dhan-yatra`) - the journey of money. Today's portfolio is the
  corpus; it is built up to the year you retire and drawn down at up to four
  withdrawals side by side, with a year-by-year table, inflation, currency risk
  and a rope to move any year off the plan.
- **Settings** - preferences, the portfolio **import**, and delete-data.

React frontend, Rust backend (Axum + SeaORM), one Docker image serving both on a
single port. The database is **Supabase** (Postgres).

## What it keeps from Nestivesh, and what it drops

Both pages plan against a portfolio, so the portfolio data layer stays: holdings,
deposits and cash valued into the base currency, with prices, NAVs and FX rates
refreshed from the same free feeds (Yahoo, AMFI, Frankfurter). What goes:

- Sign-in, sign-up, SSO/OAuth, accounts, roles, API keys and the admin screens.
  There is one built-in owner account; every request acts as it.
- AI mode, the MCP server, the assistant on the open CaiYatra page, export.
- The Dashboard, Holdings, Deposits, Cash, Accounts, Transactions and Forex
  pages, and the documentation site. (The deposits, cash and holdings *APIs*
  remain, because the planner reads them.)
- The logo. The brand is the text **CRY** on a blue badge.

Defaults differ in one more way: the base currency is **SGD**.

> **There is no sign-in.** Whoever can open the app can read, change and delete
> the portfolio and the plans. Keep it on a private network, or put
> authentication in front of it (see `deploy/40-ingress.yaml`).

## Importing a portfolio

The planner needs what is held, not how it was bought, so there is a single
import: **one CSV, one row per holding** - stocks, ETFs, mutual funds, deposits
and cash. Settings > Import validates it first (*Validate only*), reports errors
by row number, and the template is a download away. `Category` says what a row
is (`STOCK`, `ETF`, `REIT`, `MUTUAL_FUND`, `DEPOSIT`, `CASH`); the columns that
matter for each are listed on the Import page.

`samples/cairezeki-sample-portfolio.csv` is a ready-made SGD 100,000 portfolio:
15% in two SGX ETFs (G3B, SRT), 50% in three UCITS ETFs (SWRD, WSML, IGLN), 10%
in Indian mutual funds (DSP Large Cap, DSP Money Market), 25% cash. See
`samples/README.md`.

Underneath, each row is turned into one BUY (or a deposit, or a cash balance)
and applied by the existing importer, so matching, validation and duplicate
checks are unchanged. An import adds to what is held; to replace the portfolio,
delete it first (Settings > Delete data).

## Running it

### Against Supabase

1. In the Supabase dashboard open **Connect** and copy the **Session pooler**
   connection string (port 5432). Put the database password in, and keep
   `?sslmode=require`. See `deploy/01-secrets.yaml` for the details and the
   other connection types.
2. Run the image with it:

   ```sh
   docker build -t cairezeki:latest .
   docker run -p 8080:8080 \
     -e CAIREZEKI_DATABASE_URL='postgresql://postgres.<ref>:<password>@<host>:5432/postgres?sslmode=require' \
     cairezeki:latest
   ```

   Open <http://localhost:8080>. The tables are created on first start, in the
   `public` schema, and **row-level security is switched on for each** so that
   Supabase's public REST API (the anon key) cannot read them; the app itself
   connects directly and is not affected.

### Locally, without Supabase

`docker compose up -d` falls back to SQLite on a volume when
`CAIREZEKI_DATABASE_URL` is not set. For development: `cp .env.example .env`,
`cargo run` in `backend/`, and `npm run dev` in `frontend/` (the dev server
proxies `/api` to `CAIREZEKI_API`, default `http://localhost:8080`).

### Configuration

| Variable | Default | |
|---|---|---|
| `CAIREZEKI_DATABASE_URL` | `sqlite://./data/cairezeki.db?mode=rwc` | The Supabase connection string, or SQLite |
| `CAIREZEKI_BIND` | `0.0.0.0:8080` | Listen address |
| `CAIREZEKI_STATIC_DIR` | `/app/static` (image) | The built React app |
| `CAIREZEKI_AUTO_MIGRATE` | `true` | Apply migrations at startup |
| `CAIREZEKI_LOG_DIR` | beside SQLite; `none` in the image | Daily-rolled log files |
| `CAIREZEKI_UPLOADS_DIR` | beside SQLite; `none` in the image | Keep every uploaded file |
| `CAIREZEKI_CORS_ORIGINS` | *(unset)* | Only if the UI is hosted separately |
| `CAIREZEKI_LOG` | `cairezeki=info,...` | `tracing` filter |

The API is described at `/api/docs` (Swagger) and `/api/v1/openapi.json`.

## Deploying (Argo Workflows + Kubernetes)

- `argoworkflow/` builds the image from **GitHub** with rootless BuildKit and
  pushes it to Docker Hub. Its `01-secrets.yaml` holds the GitHub and Docker Hub
  credentials, **blank** - fill them in separately. See its README.
- `deploy/` is the Kubernetes side: config, the database secret
  (`01-secrets.yaml`, **blank** - the Supabase URL goes there), deployment,
  service, ingress and a refresh CronJob. `kubectl apply -k deploy` after the
  secret exists.

Release tags: `scripts/release.sh <version>` writes `VERSION` and tags
`cairezeki.<version>`.

## Layout

```
backend/    Rust: api/ (handlers), services/ (orchestration), domain/ (pure maths),
            providers/ (price, NAV, FX feeds), importer/, entities/, migration/
frontend/   React + Vite + Tailwind
samples/    The sample portfolio CSV
deploy/     Kubernetes manifests
argoworkflow/  The GitHub-to-Docker-Hub build
```

## Not yet built or run

This project was assembled by pruning Nestivesh and was **not compiled** - there
is no Rust toolchain on the machine it was made on. The frontend type-checks and
builds. The first Argo build is the first compile of the backend; expect to fix a
few compile errors from it.

## Placeholders to set locally

The repository carries no secrets, account names or domains. After cloning, set
these in your own copy (and keep those edits out of commits):

| Placeholder | In | Is |
|---|---|---|
| `YOUR-GITHUB-OWNER` | `argoworkflow/10-build-workflowtemplate.yaml` | the GitHub repository's owner |
| `YOUR-DOCKERHUB-USER` | the same file, `deploy/30-deployment.yaml` | the Docker Hub namespace the image is pushed to |
| `mydomain.example.com`, `YOUR-CLUSTER-ISSUER` | `deploy/40-ingress.yaml` | the host and the cert-manager issuer |
| blank `CAIREZEKI_DATABASE_URL` | `deploy/01-secrets.yaml` | the Supabase connection |
| blank username/token pairs | `argoworkflow/01-secrets.yaml` | GitHub and Docker Hub credentials |
