#!/usr/bin/env bash
set -euo pipefail

# --- Step 1: Configure git identity ---
echo "==> Configuring git identity..."
git config --global user.name "sidequest-agent"
git config --global user.email "sidequest@bot"

# --- Step 2: Authenticate GitHub CLI ---
echo "==> Authenticating GitHub CLI..."
echo "$GH_TOKEN" | gh auth login --with-token

# --- Step 3: Clone the repository ---
echo "==> Cloning repository: $REPO"
gh repo clone "$REPO" /workspace

# --- Step 4: Create a new branch ---
BRANCH_NAME="${BRANCH_PREFIX}${TASK_ID:0:8}"
echo "==> Checking out base branch '$BASE_BRANCH' and creating '$BRANCH_NAME'..."
cd /workspace
git checkout "$BASE_BRANCH"
git checkout -b "$BRANCH_NAME"

# --- Step 5: Run Claude Code ---
echo "==> Running Claude Code (model=$CLAUDE_MODEL, max-turns=$CLAUDE_MAX_TURNS)..."
claude -p "$TASK_PROMPT" \
  --dangerously-skip-permissions \
  --model "$CLAUDE_MODEL" \
  --max-turns "$CLAUDE_MAX_TURNS"

# --- Step 6: Stage all changes ---
echo "==> Staging changes..."
git add -A

# --- Step 7: Check for changes ---
if git diff --cached --quiet; then
  echo "No changes made"
  exit 0
fi

# --- Step 8: Commit ---
echo "==> Committing changes..."
git commit -m "feat(sidequest): $TASK_ID" -m "$TASK_PROMPT"

# --- Step 9: Push ---
echo "==> Pushing branch..."
git push origin HEAD

# --- Step 10: Open pull request ---
echo "==> Creating pull request..."
PR_URL=$(gh pr create \
  --title "sidequest: ${TASK_ID:0:8}" \
  --body "## Task

${TASK_PROMPT}

---
*Automated by sidequest*" \
  --base "$BASE_BRANCH")

# --- Step 11: Done ---
echo "==> Pull request created: $PR_URL"
exit 0
