#!/usr/bin/env bash
set -euo pipefail

# GitHub signs createCommitOnBranch commits; a normal bot git commit is unsigned
# and cannot update branches with the "required signatures" ruleset.
branch="${1:?usage: commit-manifest-verified.sh <branch>}"
: "${GH_TOKEN:?GH_TOKEN is required}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"

if [[ "$(git symbolic-ref --quiet --short HEAD)" != "$branch" ]]; then
  echo "::error::Checkout must be on $branch before publishing its manifest" >&2
  exit 1
fi

manifest=manifest/version.json
if [[ ! -f "$manifest" ]]; then
  echo "::error::Missing $manifest" >&2
  exit 1
fi
if [[ -z "$(git status --porcelain --untracked-files=all -- "$manifest")" ]]; then
  echo "Manifest unchanged; nothing to commit"
  exit 0
fi

# A stale scheduled run must not overwrite a newer maintainer commit.
expected_head_oid=$(git rev-parse HEAD)
contents=$(base64 < "$manifest" | tr -d '\n')
query=$(cat <<'GRAPHQL'
mutation(
  $repository: String!
  $refName: String!
  $expectedHeadOid: GitObjectID!
  $contents: Base64String!
) {
  createCommitOnBranch(input: {
    branch: {repositoryNameWithOwner: $repository, refName: $refName}
    expectedHeadOid: $expectedHeadOid
    message: {headline: "chore(manifest): update manifest [skip ci]"}
    fileChanges: {additions: [{path: "manifest/version.json", contents: $contents}]}
  }) {
    commit { oid url signature { isValid } }
  }
}
GRAPHQL
)

response=$(gh api graphql \
  -f query="$query" \
  -f repository="$GITHUB_REPOSITORY" \
  -f refName="$branch" \
  -f expectedHeadOid="$expected_head_oid" \
  -f contents="$contents")

# Do not report success for an unsigned/invalid commit or GraphQL error.
printf '%s\n' "$response" | node -e '
const fs = require("node:fs");
const result = JSON.parse(fs.readFileSync(0, "utf8"));
const commit = result.data?.createCommitOnBranch?.commit;
if (result.errors?.length || !commit?.signature?.isValid) {
  console.error("Manifest commit was not verified:", result.errors ?? commit);
  process.exit(1);
}
console.log("Created verified manifest commit:", commit.url);
'
