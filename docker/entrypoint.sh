#!/usr/bin/env bash
set -euo pipefail

# --- Step 1: Configure git identity ---
echo "==> Configuring git identity..."
git config --global user.name "sidequest-agent"
git config --global user.email "sidequest@bot"

# --- Step 2: Clone the repository ---
echo "==> Cloning repository: $REPO"
gh repo clone "$REPO" /workspace

# --- Step 3: Create a new branch ---
BRANCH_NAME="${BRANCH_PREFIX}${TASK_ID:0:8}"
echo "==> Checking out base branch '$BASE_BRANCH' and creating '$BRANCH_NAME'..."
cd /workspace
git checkout "$BASE_BRANCH"
git checkout -b "$BRANCH_NAME"

# --- Step 4: Run Claude Code ---
echo "==> Running Claude Code (model=$CLAUDE_MODEL, max-turns=$CLAUDE_MAX_TURNS)..."
CLAUDE_ARGS=(-p "$TASK_PROMPT" --dangerously-skip-permissions --model "$CLAUDE_MODEL")
if [[ "$CLAUDE_MAX_TURNS" != "-1" ]]; then
  CLAUDE_ARGS+=(--max-turns "$CLAUDE_MAX_TURNS")
fi
claude "${CLAUDE_ARGS[@]}"

# --- Step 5: Stage all changes ---
echo "==> Staging changes..."
git add -A

# --- Step 6: Check for changes ---
if git diff --cached --quiet; then
  echo "==> No changes made by Claude Code."
  exit 0
fi

# --- Step 7: Commit ---
echo "==> Committing changes..."
git commit -m "feat(sidequest): $TASK_ID" -m "$TASK_PROMPT"

# --- Step 8: Push ---
echo "==> Pushing branch..."
git push origin HEAD

# --- Step 9: Open pull request ---
echo "==> Creating pull request..."
PR_URL=$(gh pr create \
  --title "sidequest: ${TASK_ID:0:8}" \
  --body "## Task

${TASK_PROMPT}

---
*Automated by sidequest*" \
  --base "$BASE_BRANCH")

# --- Step 10: Done ---
echo "==> Pull request created: $PR_URL"
exit 0
