#!/usr/bin/env bash
set -euo pipefail

# --- Step 1: Configure git identity and auth ---
echo "==> Configuring git identity..."
git config --global user.name "sidequest-agent"
git config --global user.email "sidequest@bot"
git config --global url."https://x-access-token:${GH_TOKEN}@github.com/".insteadOf "https://github.com/"

# --- Step 2: Clone the repository and checkout PR branch ---
echo "==> Cloning repository: $REPO"
gh repo clone "$REPO" /workspace
cd /workspace

echo "==> Checking out PR #${PR_NUMBER} branch..."
gh pr checkout "$PR_NUMBER"

# --- Step 3: Run Claude Code with review feedback ---
echo "==> Running Claude Code to address review feedback (model=$CLAUDE_MODEL)..."
CLAUDE_ARGS=(-p "You are addressing PR review feedback on an existing pull request.

The following review feedback was left on PR #${PR_NUMBER}:

${REVIEW_FEEDBACK}

Please:
1. Read and understand the feedback
2. Make the requested changes
3. Be thorough but only change what was requested

Do NOT create new branches or PRs — just make changes on the current branch." --dangerously-skip-permissions --verbose --model "$CLAUDE_MODEL")
if [[ "$CLAUDE_MAX_TURNS" != "-1" ]]; then
  CLAUDE_ARGS+=(--max-turns "$CLAUDE_MAX_TURNS")
fi
claude "${CLAUDE_ARGS[@]}"

# --- Step 4: Stage all changes ---
echo "==> Staging changes..."
git add -A

# --- Step 5: Check for changes ---
if git diff --cached --quiet; then
  echo "==> No changes needed — posting comment."
  gh pr comment "$PR_NUMBER" --body "I reviewed the feedback but no code changes were needed.

---
*Automated by sidequest*"
  exit 0
fi

# --- Step 6: Generate commit message ---
echo "==> Generating commit message..."
STAGED_DIFF=$(git diff --cached)
COMMIT_MSG=$(claude -p "Generate a conventional commit message for the following staged diff.
These changes address PR review feedback.
Output ONLY the commit message (subject line, optionally a blank line and body). No extra text.
Use conventional commits format: type(scope): description
Types: feat, fix, refactor, docs, test, chore, style, perf

Diff:
$STAGED_DIFF" --model "$CLAUDE_FAST_MODEL")

if [[ -z "$COMMIT_MSG" ]]; then
  COMMIT_MSG="fix: address PR review feedback"
fi

# --- Step 7: Commit and push ---
echo "==> Committing changes..."
git commit -m "$COMMIT_MSG"

echo "==> Pushing changes..."
git push origin HEAD

# --- Step 8: Comment on PR ---
echo "==> Posting update comment on PR..."
gh pr comment "$PR_NUMBER" --body "I've addressed the review feedback and pushed new changes.

**Changes made:**
$COMMIT_MSG

---
*Automated by sidequest*"

echo "==> Review feedback addressed successfully."
exit 0
