#!/usr/bin/env bash
# Puts files on a GitHub release, replacing any of the same name.
#
#   GH_TOKEN=... GITHUB_REPOSITORY=owner/name packaging/upload-assets.sh <release-id> <file>...
#
# The release is named by its id and not its tag because it is a draft until the
# last job of the workflow publishes it, and a draft has no tag to look up.
# Replacing rather than adding is what makes re-running a job harmless: an asset
# name is unique per release, and GitHub refuses the second upload.
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: $0 <release-id> <file>..." >&2
  exit 2
fi
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is not set (owner/name)}"

id="$1"
shift

for file in "$@"; do
  name="$(basename "$file")"
  case "$name" in
    *.json) type="application/json" ;;
    *.sig) type="text/plain" ;;
    *) type="application/octet-stream" ;;
  esac

  # Up to 100 assets: a release here has about a dozen.
  old="$(gh api "repos/$GITHUB_REPOSITORY/releases/$id/assets?per_page=100" \
    --jq "map(select(.name == \"$name\")) | .[0].id // empty")"
  if [ -n "$old" ]; then
    gh api --method DELETE "repos/$GITHUB_REPOSITORY/releases/assets/$old"
  fi

  gh api --method POST \
    "https://uploads.github.com/repos/$GITHUB_REPOSITORY/releases/$id/assets?name=$name" \
    --header "Content-Type: $type" \
    --input "$file" > /dev/null
  echo "uploaded $name ($(wc -c < "$file") bytes)"
done
