#!/usr/bin/env bash
# Sync GitHub issue labels from .github/labels.json
#
# This script creates or updates issue labels in the GitHub repository to match
# the configuration in .github/labels.json. It is idempotent: running it multiple
# times will converge on the desired state without duplicate or orphaned labels.
#
# Usage:
#   scripts/sync-labels.sh
#
# Requirements:
#   - gh CLI (https://cli.github.com/)
#   - Authentication: `gh auth login` (or GITHUB_TOKEN set)
#   - Permissions: `admin:org_hook` or repo admin access
#
# This script is meant to be run by hand by the maintainer. It is not automated.

set -euo pipefail

# Find the repo root (directory containing .github/labels.json)
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LABELS_FILE="${REPO_ROOT}/.github/labels.json"

if [[ ! -f "$LABELS_FILE" ]]; then
    echo "Error: $LABELS_FILE not found"
    exit 1
fi

# Check that gh is installed and authenticated
if ! command -v gh &> /dev/null; then
    echo "Error: gh CLI not found. Install it from https://cli.github.com/"
    exit 1
fi

if ! gh auth status &> /dev/null; then
    echo "Error: Not authenticated with gh. Run: gh auth login"
    exit 1
fi

# Get the repository (format: owner/repo, e.g., ninad-k/Sevak)
REPO="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
echo "Syncing labels for $REPO..."

# Read labels from JSON and sync each one
# jq filter: for each label, output name, color, description on separate lines
while IFS= read -r label_json; do
    if [[ -z "$label_json" ]]; then
        continue
    fi

    # Extract fields from JSON
    NAME=$(echo "$label_json" | jq -r '.name')
    COLOR=$(echo "$label_json" | jq -r '.color')
    DESCRIPTION=$(echo "$label_json" | jq -r '.description')

    # --force creates the label, or updates its color and description if it exists
    echo "Syncing label: $NAME"
    gh label create "$NAME" --color "$COLOR" --description "$DESCRIPTION" --force
done < <(jq -c '.[]' "$LABELS_FILE")

echo "Done. All labels are in sync."
