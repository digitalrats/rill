#!/bin/bash
# scripts/bump-version.sh
#
# DEPRECATED — kept only as a reference.
# Rill now uses independent versioning per crate.
# Use `cargo set-version --package <crate> <version>` (cargo-edit) or
# manually edit version in the crate's Cargo.toml, then update
# the corresponding `version` in [workspace.dependencies] in root Cargo.toml.

set -e

if [ -z "$1" ]; then
    echo "Usage: $0 <new-version>"
    echo "Example: $0 0.3.0"
    exit 1
fi

NEW_VERSION=$1

echo "🔄 Bumping all crates to version $NEW_VERSION"

# Обновляем версию в корневом Cargo.toml
sed -i "s/^version = .*/version = \"$NEW_VERSION\"/" Cargo.toml

# Обновляем версии во всех крейтах
for crate in rill-*/Cargo.toml; do
    if [ -f "$crate" ]; then
        echo "  📦 $crate"
        sed -i "s/^version = .*/version = \"$NEW_VERSION\"/" "$crate"
        
        # Обновляем зависимости на другие rill-крейты
        sed -i "s/\(rill-[a-z-]* = .* version = \"\)[0-9]*\.[0-9]*\.[0-9]*\(\".*\)/\1$NEW_VERSION\2/" "$crate"
    fi
done

echo "✅ Done! Version updated to $NEW_VERSION"
echo ""
echo "Next steps (DEPRECATED):"
echo "  1. Review changes: git diff"
echo "  2. Commit: git add . && git commit -m \"chore(release): prepare $NEW_VERSION\""
echo "  3. Continue with release: git flow release finish $NEW_VERSION"
