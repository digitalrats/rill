# Contributing

Rill is open for contributions. Areas where help is especially valued:

- **Audio backends**: ALSA, CoreAudio, WASAPI, JACK, PipeWire
- **DSP algorithms**: new effects, optimization of existing ones
- **Documentation**: examples, tutorials, translations
- **Testing**: on different platforms and hardware

## How to start

1. Fork the [repository](https://github.com/DigitalRats/rill)
2. Create a feature branch (`git checkout -b feature/amazing-effect`)
3. Run tests (`cargo test --workspace`)
4. Submit a pull request

## Git Flow

The project uses [Git Flow](https://www.atlassian.com/git/tutorials/comparing-workflows/gitflow-workflow):

- `main` — stable releases
- `develop` — integration branch
- `feature/*` — new features
- `release/*` — release preparation
- `hotfix/*` — urgent fixes

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`.
