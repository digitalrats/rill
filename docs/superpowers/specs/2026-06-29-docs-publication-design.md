# Docs Publication Prep — Design

**Date:** 2026-06-29
**Status:** approved

## Goal

Prepare `rill/docs/` mdBook documentation for publication on `https://rill-adrift.io`:
full content review, terminology normalization, and structural polishing.
No CI/CD deployment setup — content-only.

## Approach: Multi-pass (4 phases)

### Phase 1 — Inventory & Links

- Verify `SUMMARY.md` against files on disk (no missing/orphaned pages)
- Cross-reference all internal links (`[text](./path.md)`) and anchors (`#section`)
- Cross-reference all external URLs (check reachability)
- Clean up stale artifacts (`docs/src/src/`, orphaned build output)
- Verify `book.toml` correctness (title, site-url, edit-url)

**Deliverable:** list of broken links and structural issues, all fixed.

### Phase 2 — Terminology Normalization

Apply rules from `rill/AGENTS.md` uniformly across all chapters:

| Wrong | Correct |
|-------|---------|
| `audio thread` / `audio data` | `signal thread` / `signal data` (in prose, not code identifiers) |
| `kama-*` | `rill-*` |
| `Automata` | `Automaton` (singular) / `Automatons` (plural) |
| Russian-language docs | English (except `world-of-automatons.md` — deliberate exception) |

**Deliverable:** consistent terminology across all 21 chapters.

### Phase 3 — Content Review & Rewrite

Per chapter:
- Verify factual accuracy against current codebase state
- Check for outdated references, dead patterns, removed crates
- Improve clarity, structure, and completeness
- Split/merge sections where needed
- Add missing sections discovered during review

**Deliverable:** polished, accurate, complete content.

### Phase 4 — Final Build & Verification

- `mdbook build docs/` — ensure zero errors
- Verify rendered HTML output
- Commit changes

## Scope

| In scope | Out of scope |
|----------|-------------|
| `rill/docs/src/` (21 chapters) | `drift/docs/` |
| `rill/docs/book.toml` | CI/CD deployment setup |
| `rill/docs/architecture.md`, `rill/docs/edsl.md` (standalone) | GitHub Actions / GitHub Pages |
| `rill/docs/plans/` | `docs.rs` API docs |
| `rill/docs/src/SUMMARY.md` | Crate-level READMEs |

## Non-goals

- Setting up deployment (GitHub Actions, Netlify, etc.)
- Integrating drift docs into the book
- Adding new chapters beyond what gaps are discovered in Phase 3
- Changing the mdBook theme or visual style
