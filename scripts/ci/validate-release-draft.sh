#!/usr/bin/env bash
set -euo pipefail

if [[ ! "${RELEASE_TAG:-}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "Invalid release tag: ${RELEASE_TAG:-}" >&2
  exit 1
fi

# A newly created draft may not be discoverable by tag immediately.
max_attempts=12
retry_delay_seconds=10
for ((attempt = 1; attempt <= max_attempts; attempt++)); do
  if release_state=$(gh release view "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY" --json isDraft --jq '.isDraft' 2>&1); then
    if [[ "$release_state" != 'true' ]]; then
      echo "Release $RELEASE_TAG must still be a draft before its assets are built" >&2
      exit 1
    fi
    echo "Release $RELEASE_TAG is a draft; continuing"
    exit 0
  fi

  if [[ "$release_state" != *'release not found'* && "$release_state" != *'HTTP 404'* ]]; then
    echo "$release_state" >&2
    exit 1
  fi

  if (( attempt == max_attempts )); then
    echo "Release $RELEASE_TAG was not discoverable after $max_attempts attempts" >&2
    echo "$release_state" >&2
    exit 1
  fi

  echo "Release $RELEASE_TAG is not visible yet; retrying in ${retry_delay_seconds}s ($attempt/$max_attempts)"
  sleep "$retry_delay_seconds"
done
