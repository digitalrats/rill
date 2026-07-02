# mdBook CI/CD — GitHub Pages Deployment Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically build and deploy Rill mdBook docs to rill-adrift.io via GitHub Pages on push to main.

**Architecture:** Single GitHub Actions workflow using peaceiris/actions-mdbook for build and peaceiris/actions-gh-pages for deployment to gh-pages branch.

**Tech Stack:** GitHub Actions, mdBook, peaceiris community actions

---

### Task 1: Create deploy-docs workflow

**Files:**
- Create: `.github/workflows/deploy-docs.yml`

- [ ] **Step 1: Create the workflow directory**

```bash
mkdir -p .github/workflows
```

- [ ] **Step 2: Write the workflow file**

Write to `.github/workflows/deploy-docs.yml`:

```yaml
name: Deploy mdBook to GitHub Pages

on:
  push:
    branches: [main]
    paths:
      - 'docs/**'

permissions:
  contents: write

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install mdBook
        uses: peaceiris/actions-mdbook@v2
        with:
          mdbook-version: '0.4.x'

      - name: Build book
        run: mdbook build docs/

      - name: Deploy to GitHub Pages
        uses: peaceiris/actions-gh-pages@v4
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: docs/book
          cname: rill-adrift.io
```

- [ ] **Step 3: Verify mdBook builds locally**

```bash
mdbook build docs/
```

Expected: zero errors, zero warnings. Output in `docs/book/`.

- [ ] **Step 4: Verify workflow YAML is valid (optional)**

```bash
# Install actionlint if available, or just verify with yamllint
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/deploy-docs.yml'))" && echo "YAML valid"
```

Expected: "YAML valid"

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/deploy-docs.yml
git commit -m 'ci: add mdBook deploy to GitHub Pages on push to main'
```
