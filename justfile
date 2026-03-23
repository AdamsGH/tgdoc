out    := "docs"
config := "sources.toml"

# List available recipes
default:
    @just --list

# Build the binary
build:
    cargo build --release

# Fetch all sources (or a single one: just fetch tg-bot-api)
fetch source="":
    cargo run --release -- fetch --out {{out}} --config {{config}} {{source}}

# Dry-run: print heading tree without writing files
dry source="":
    cargo run --release -- fetch --out {{out}} --config {{config}} --dry {{source}}

# Re-fetch (clean docs first)
refetch source="":
    rm -rf {{out}}
    just fetch {{source}}

# Pack docs/ into a timestamped tar.gz
pack:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ ! -d "{{out}}" ]; then
        echo "docs/ not found - run 'just fetch' first"
        exit 1
    fi
    archive="tgdoc-$(date +%Y%m%d-%H%M%S).tar.gz"
    tar -czf "$archive" -C {{out}} .
    echo "Packed $(find {{out}} -name '*.md' | wc -l) files -> $archive ($(du -sh "$archive" | cut -f1))"

# Fetch then pack in one step
all: fetch pack

# Remove generated docs and archives
clean:
    rm -rf {{out}} *.tar.gz

# Remove docs, archives, and build artifacts
clean-all:
    rm -rf {{out}} *.tar.gz target

# Tag and push a release - triggers the GitHub Actions workflow.
# Uses version from Cargo.toml by default; pass an explicit version to bump first.
#   just tag-release          -> tags v<Cargo.toml version>
#   just tag-release 1.1.0   -> bumps Cargo.toml, commits, then tags v1.1.0
tag-release version="":
    #!/usr/bin/env bash
    set -euo pipefail
    cargo_ver=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
    if [ -n "{{version}}" ]; then
        new_ver="{{version}}"
        if [ "$new_ver" != "$cargo_ver" ]; then
            sed -i "s/^version = \"${cargo_ver}\"/version = \"${new_ver}\"/" Cargo.toml
            cargo generate-lockfile 2>/dev/null || true
            git add Cargo.toml Cargo.lock
            git commit -m "chore: bump version to ${new_ver}"
        fi
    else
        new_ver="$cargo_ver"
    fi
    tag="v${new_ver}"
    if git rev-parse "$tag" >/dev/null 2>&1; then
        echo "Tag $tag already exists." >&2
        exit 1
    fi
    git tag -a "$tag" -m "Release ${tag}"
    git push origin HEAD "$tag"
    echo "Tagged and pushed ${tag}"

# Force-push an existing tag (re-triggers the release workflow)
#   just retag          -> re-tags current Cargo.toml version
#   just retag 1.1.0   -> re-tags v1.1.0
retag version="":
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -n "{{version}}" ]; then
        tag="v{{version}}"
    else
        tag="v$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')"
    fi
    git tag -f -a "$tag" -m "Release ${tag}"
    git push --force origin "$tag"
    echo "Re-pushed ${tag}"
