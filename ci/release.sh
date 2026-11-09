#!/usr/bin/env bash
# The Roadrunner release (.woodpecker/release.yaml), the port of .github/workflows/release.yml:
#   bash ci/release.sh build vX.Y.Z               # on the engine VM: dist/ with the release assets
#   GH_TOKEN=... bash ci/release.sh publish vX.Y.Z
# build checks the tag against Cargo.toml (checking out an existing tag for a manual run), then
# cross-builds copper.exe for x64 and arm64 with cargo-xwin 0.23.1 in a rust container through
# the host `dagger` CLI (`deps` + `stage` below run inside it) and exports dist/: the same
# assets as the GitHub workflow, unsigned (no Azure Trusted Signing on this lane):
#   CopperSetup-{x64,arm64}.exe (copper.exe renamed), copper-windows-{x64,arm64}.zip
#   (copper.exe + LICENSE + README.md), copper-version.json, SHA256SUMS.
# publish creates the GitHub release, or repairs an existing one, with a pinned gh.
set -euo pipefail

RUST_IMAGE=public.ecr.aws/docker/library/rust:1-trixie
CARGO_XWIN_VERSION=0.23.1
GH_VERSION=2.103.3
GH_SHA256=4c806978ffce1cec70853302a56d70ac6da4f19bae7adca4e1a8ab00633dd91d # linux_amd64
REPO=copper-browser/copper-windows
ASSETS=(CopperSetup-x64.exe CopperSetup-arm64.exe copper-windows-x64.zip copper-windows-arm64.zip copper-version.json)

usage='usage: release.sh build|publish vX.Y.Z'
cmd=${1:?$usage}
tag=${2:-${RELEASE_TAG:-}} # RELEASE_TAG: the variable of a manual run
[ -n "$tag" ] || { echo "$usage (or RELEASE_TAG for a manual run)" >&2; exit 1; }
export GIT_TERMINAL_PROMPT=0
here=$(cd "$(dirname "$0")" && pwd)

cargo_version() {
  sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml
}

# Host: the tagged source, checked like the GitHub `meta` job, then the container build.
build() {
  cd "$here/.."
  if ! git rev-parse -q --verify "refs/tags/$tag^{commit}" >/dev/null; then
    timeout 300 git fetch -q --no-tags origin "+refs/tags/$tag:refs/tags/$tag"
  fi
  local commit src=.
  commit=$(git rev-parse --verify "refs/tags/$tag^{commit}")
  if [ "$(git rev-parse HEAD)" != "$commit" ]; then
    # Manual run on a branch: build the existing tag (as workflow_dispatch did) from a
    # worktree, with this script, which older tags do not have.
    src=.release-src
    rm -rf "$src"
    git worktree prune
    git worktree add -q --detach "$src" "$commit"
  fi
  timeout 600 git -C "$src" submodule update -q --init --recursive
  local version
  version=$(cd "$src" && cargo_version)
  if [ "$tag" != "v$version" ]; then
    echo "release.sh: tag $tag does not match Cargo.toml [workspace.package] version $version" >&2
    exit 1
  fi

  rm -rf dist
  export _EXPERIMENTAL_DAGGER_RUNNER_HOST=${_EXPERIMENTAL_DAGGER_RUNNER_HOST:-unix:///run/dagger-engine/engine.sock}
  timeout 5400 dagger --progress=plain core container from --address="$RUST_IMAGE" \
    with-env-variable --name=CARGO_TERM_COLOR --value=never \
    with-env-variable --name=CARGO_INCREMENTAL --value=0 \
    with-env-variable --name=XWIN_ACCEPT_LICENSE --value=1 \
    with-env-variable --name=COMMIT --value="$commit" \
    with-env-variable --name=PUBLISHED_AT --value="$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    with-directory --path=/ci --directory=ci \
    with-directory --path=/src --directory="$src" --exclude=.git,target,dist,.release-src,.release-tools \
    with-workdir --path=/src \
    with-exec --args=bash,/ci/release.sh,deps,"$tag" \
    with-exec --args=bash,/ci/release.sh,stage,"$tag" \
    directory --path=/src/dist export --path=./dist
  (cd dist && sha256sum -c SHA256SUMS && cat copper-version.json)
}

# Container: build tools (rust-toolchain.toml's toolchain + both MSVC targets, cargo-xwin).
deps() {
  apt-get update -qq
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends clang lld llvm zip python3 >/dev/null
  rustup toolchain install --no-self-update
  rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
  cargo install --locked cargo-xwin --version "$CARGO_XWIN_VERSION"
}

# Container, in the tagged source (or any Linux box with cargo-xwin, clang, lld, llvm, zip,
# python3): build, stage, manifest and checksums into ./dist (COMMIT, PUBLISHED_AT from build).
stage() {
  : "${COMMIT:?}" "${PUBLISHED_AT:?}"
  local version arch target exe cflags
  version=$(cargo_version)
  [ "$tag" = "v$version" ] || { echo "release.sh: tag $tag != v$version" >&2; exit 1; }
  rm -rf dist
  mkdir -p dist
  for arch in x64 arm64; do
    # ring forces plain `clang` for aarch64-pc-windows-msvc, which then rejects cargo-xwin's
    # clang-cl flags (`/imsvc <dir>`); --driver-mode=cl makes that clang parse them as clang-cl.
    # (cc reads CFLAGS_<target> with dashes on top of cargo-xwin's CFLAGS_<target_underscored>.)
    case $arch in
      x64) target=x86_64-pc-windows-msvc cflags=() ;;
      arm64) target=aarch64-pc-windows-msvc cflags=("CFLAGS_aarch64-pc-windows-msvc=--driver-mode=cl") ;;
    esac
    env XWIN_ACCEPT_LICENSE=1 "${cflags[@]}" cargo xwin build --release -p copper-win --target "$target" --locked
    exe="${CARGO_TARGET_DIR:-target}/$target/release/copper.exe"
    test -s "$exe" || { echo "release.sh: missing built executable $exe" >&2; exit 1; }
    # The filename alone activates the installer path in this same binary.
    cp "$exe" "dist/CopperSetup-$arch.exe"
    zip -q -j -X "dist/copper-windows-$arch.zip" "$exe" LICENSE README.md
  done
  python3 packaging/version-manifest.py --dist dist --version "$version" --tag "$tag" \
    --commit "$COMMIT" --repo "$REPO" --published-at "$PUBLISHED_AT" >dist/copper-version.json
  (cd dist && sha256sum "${ASSETS[@]}" >SHA256SUMS && sha256sum -c SHA256SUMS)
}

# Host: create the GitHub release, or re-upload assets and edit it (a rerun repairs a release).
publish() {
  : "${GH_TOKEN:?GH_TOKEN must be set}"
  cd "$here/.."
  local f
  for f in "${ASSETS[@]}" SHA256SUMS; do
    test -s "dist/$f" || { echo "release.sh: missing release asset dist/$f" >&2; exit 1; }
  done
  (cd dist && sha256sum -c --quiet SHA256SUMS)
  local version commit prerelease latest
  version=$(jq -r .version dist/copper-version.json)
  commit=$(jq -r .commit dist/copper-version.json)
  [ "$tag" = "v$version" ] || { echo "release.sh: dist is $version, not $tag" >&2; exit 1; }
  prerelease=false
  case "$version" in *-*) prerelease=true ;; esac
  latest=true
  [ "$prerelease" = false ] || latest=false

  local tools="$PWD/.release-tools" gh_name="gh_${GH_VERSION}_linux_amd64"
  mkdir -p "$tools"
  local tarball="$tools/$gh_name.tar.gz"
  [ -f "$tarball" ] || timeout 300 curl -fsSL --retry 3 -o "$tarball" \
    "https://github.com/cli/cli/releases/download/v$GH_VERSION/$gh_name.tar.gz"
  echo "$GH_SHA256  $tarball" | sha256sum -c --quiet -
  tar -xzf "$tarball" -C "$tools"
  export GH_CONFIG_DIR="$tools/gh-config" GH_PROMPT_DISABLED=1 GH_NO_UPDATE_NOTIFIER=1
  local gh="$tools/$gh_name/bin/gh"

  local notes="$tools/release-notes.md"
  {
    echo "Copper $version for Windows — built from $REPO@${commit:0:12}."
    echo
    echo 'Download CopperSetup-x64.exe (Windows x64) or CopperSetup-arm64.exe (Windows ARM64) to install Copper per-user.'
    echo 'The copper-windows-*.zip files contain the same browser/CLI executable as copper.exe, plus the license and README.'
    echo 'No separate installer code or VC++ redistributable is required. WebView2 Evergreen is required at runtime.'
    echo 'Verify downloads against SHA256SUMS; copper-version.json gives individual asset URLs and hashes.'
  } >"$notes"
  if timeout 120 "$gh" release view "$tag" --repo "$REPO" >/dev/null 2>&1; then
    timeout 1800 "$gh" release upload "$tag" dist/* --clobber --repo "$REPO"
    timeout 120 "$gh" release edit "$tag" --repo "$REPO" \
      --title "Copper $version for Windows" --notes-file "$notes" \
      --prerelease="$prerelease" --latest="$latest"
  else
    local flags=(--latest)
    [ "$prerelease" = false ] || flags=(--prerelease --latest=false)
    timeout 1800 "$gh" release create "$tag" --repo "$REPO" --verify-tag \
      --title "Copper $version for Windows" --notes-file "$notes" \
      "${flags[@]}" dist/*
  fi
}

case "$cmd" in
  build) build ;;
  deps) deps ;;
  stage) stage ;;
  publish) publish ;;
  *) echo "$usage" >&2; exit 1 ;;
esac
