# Claudius review

`.github/workflows/claude-code-review.yml` reviews non-draft pull requests
when opened, updated, or marked ready for review. It does not run after merge
or save review learnings. Optional MemCan access is search-only.

## Repository setup

Install a GitHub App on this repository with Contents read, Issues write,
and Pull requests write permissions. Configure these Actions settings:

| Kind | Name | Purpose |
| --- | --- | --- |
| Variable | `REVIEW_APP_CLIENT_ID` | Review App client ID |
| Secret | `REVIEW_APP_PRIVATE_KEY` | Review App PEM private key |
| Secret | `CLAUDE_CODE_OAUTH_TOKEN` | Claude authentication |
| Variable (optional) | `CLAUDE_MODEL` | Coordinator model; defaults to `sonnet` |
| Secret (optional) | `MEMCAN_URL` | MemCan server URL |
| Secret (optional) | `MEMCAN_API_KEY` | MemCan authentication |

The workflow mints a token scoped to this repository and posts reviews as the
App. Missing App credentials fail the job; there is no token fallback.
With Contents read, fixed review threads are listed but require manual resolution.
MemCan is optional; without its credentials the action skips memory features.

The action uses `@main` because upstream does not currently publish a `v1` tag.
See the [upstream setup guide](https://github.com/lklimek/claudius-review-action#readme)
for App installation details.

## Verification

Run `actionlint .github/workflows/claude-code-review.yml .github/workflows/claude.yml`.
Older actionlint catalogs may incorrectly require `app-id` and reject `client-id`;
the current `actions/create-github-app-token@v3` declares `client-id` instead.
After configuring the App, open or update a non-draft PR and verify that the
review and report artifact are produced under the App identity.
