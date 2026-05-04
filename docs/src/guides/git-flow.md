# Git Flow and contribution workflow

Rill uses [Git Flow](https://www.atlassian.com/git/tutorials/comparing-workflows/gitflow-workflow)
for release management with [Conventional Commits](https://www.conventionalcommits.org/).

## Branch structure

| Branch | Purpose |
|--------|---------|
| `main` | Stable releases |
| `develop` | Integration branch |
| `feature/*` | New features (branch off `develop`) |
| `release/*` | Release candidates (branch off `develop`) |
| `hotfix/*` | Urgent fixes (branch off `main`) |

## Workflow

### Setting up

```bash
git clone https://github.com/DigitalRats/rill
cd rill
git flow init -d
```

### Creating a feature

```bash
git flow feature start my-awesome-effect
# ... work, commit, test ...
git flow feature finish my-awesome-effect
```

### Preparing a release

```bash
git flow release start 0.3.0
# update versions in Cargo.toml
cargo test --workspace
git flow release finish 0.3.0
git push --all origin
git push --tags origin
```

### Hotfix

```bash
git flow hotfix start 0.2.1
# fix, commit, test
git flow hotfix finish 0.2.1
git push --all origin
git push --tags origin
```

## Commit conventions

```
<type>(<scope>): <description>

[optional body]
```

**Types:** `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

**Examples:**
```
feat(core): add ParameterId with validation
fix(automation): prevent crash when LFO frequency is zero
docs(readme): add git flow section
```

## Versioning

All crates in the workspace version [synchronously](https://semver.org/).
Breaking changes, new features, and patches bump together.
