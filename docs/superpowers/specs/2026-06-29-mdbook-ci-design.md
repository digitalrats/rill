# mdBook CI/CD — GitHub Pages Deployment Design

**Date:** 2026-06-29
**Status:** approved

## Goal

Automatically build and deploy the Rill mdBook documentation site to
`https://rill-adrift.io` via GitHub Pages whenever `main` receives a push
that touches `docs/`.

## Architecture

Single GitHub Actions workflow (`deploy-docs.yml`) triggered on push to
`main` with `paths: docs/**`. Builds mdBook and deploys static HTML to the
`gh-pages` branch via `peaceiris/actions-gh-pages`.

```
push to main (docs/** changed)
  │
  ▼
checkout repo
  │
  ▼
install mdbook (peaceiris/actions-mdbook)
  │
  ▼
mdbook build docs/           → docs/book/
  │
  ▼
deploy to gh-pages branch    → peaceiris/actions-gh-pages
  │
  ▼
GitHub Pages serves from gh-pages branch root
  → https://rill-adrift.io
```

## Components

### 1. Workflow file
- **File:** `.github/workflows/deploy-docs.yml`
- **Trigger:** `push` on `main`, `paths: docs/**`
- **Permissions:** `contents: write` (to push to `gh-pages`)
- **Job:** single `deploy` job on `ubuntu-latest`

### 2. Build steps
1. `actions/checkout@v4`
2. `peaceiris/actions-mdbook@v2` — installs mdBook (with version pin)
3. `mdbook build docs/` — produces `docs/book/`
4. `peaceiris/actions-gh-pages@v4` — publishes `docs/book/` to `gh-pages` branch

### 3. Domain
- Domain `rill-adrift.io` already configured in `book.toml` as `site-url`
- GitHub Pages must be enabled in repo settings with custom domain
- A `CNAME` file will be created by `actions-gh-pages` if configured

## Scope

| In scope | Out of scope |
|----------|-------------|
| `.github/workflows/deploy-docs.yml` | Setting up DNS for `rill-adrift.io` |
| mdBook build + deploy to `gh-pages` | GitHub Pages enablement in repo settings |
| Trigger on push to `main`, `docs/**` | PR preview deploys |
| Single workflow file | Multi-environment deploys |

## Non-goals

- Preview/staging deployments
- API docs deployment (docs.rs handles this)
- Drift docs deployment
- Custom mdBook plugins
- Docker-based builds
