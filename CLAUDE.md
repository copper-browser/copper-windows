## CI/CD

This repo runs its CI and its tagged releases on **Roadrunner** (Exowatt's Woodpecker + Dagger CI, `.woodpecker/`): Linux agents cross-build for Windows with cargo-xwin, so the Windows-only tests do not run in CI. Check builds with `gh rr status`, `gh rr watch`, `gh rr logs`, `gh rr rerun` (install: `gh extension install Exowatt-Labs/gh-roadrunner && gh alias set rr roadrunner --clobber`) — `gh run` only sees GitHub Actions. Console: https://cicd.apps.exowatt.com.
