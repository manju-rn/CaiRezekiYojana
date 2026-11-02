#!/bin/sh
# Cuts a release: writes the version to VERSION (which the app shows in its
# footer), commits it, and tags that commit cairezeki.<version>.
#
#   scripts/release.sh 1.6-beta "profiles and access: ..."
#
# Nothing is pushed; afterwards run
#   git push && git push origin cairezeki.<version>
set -eu

if [ $# -lt 1 ]; then
  echo "usage: $0 <version> [tag message]" >&2
  exit 1
fi

VERSION=$1
TAG="cairezeki.$VERSION"
MESSAGE=${2:-"cairezeki $VERSION"}

cd "$(git rev-parse --show-toplevel)"

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "tag $TAG already exists" >&2
  exit 1
fi

printf '%s\n' "$VERSION" > VERSION
git add VERSION
# A re-run for a version already in VERSION has nothing new to commit.
git diff --cached --quiet -- VERSION || git commit -m "Release $VERSION" -- VERSION
git tag -a "$TAG" -m "$MESSAGE"

echo "Tagged $TAG. Push with: git push && git push origin $TAG"
