#!/usr/bin/env bash
set -euo pipefail

# --- Step 1: Configure git identity and auth ---
echo "==> Configuring git identity..."
git config --global user.name "sidequest-agent"
git config --global user.email "sidequest@bot"
git config --global url."https://x-access-token:${GH_TOKEN}@github.com/".insteadOf "https://github.com/"

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
CLAUDE_ARGS=(-p "First, analyze the codebase and create a step-by-step plan for the task below. Then implement the plan.

Task: $TASK_PROMPT" --dangerously-skip-permissions --verbose --model "$CLAUDE_MODEL")
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

# --- Step 8: Generate PR title and body via Claude ---
echo "==> Generating PR title and body..."
DIFF_OUTPUT=$(git diff "$BASE_BRANCH"...HEAD)
PR_OUTPUT=$(claude -p "Given this diff and task description, generate a pull request title and body.
Output EXACTLY in this format — first line is the title (under 70 chars), then a blank line, then the markdown body:

TITLE_LINE

BODY_MARKDOWN

Task: $TASK_PROMPT

Diff:
$DIFF_OUTPUT" --model "$CLAUDE_MODEL")

PR_TITLE=$(head -1 <<< "$PR_OUTPUT")
PR_BODY=$(tail -n +3 <<< "$PR_OUTPUT")

# Fallback if Claude output is empty
if [[ -z "$PR_TITLE" ]]; then
  PR_TITLE="sidequest: ${TASK_ID:0:8}"
fi
if [[ -z "$PR_BODY" ]]; then
  PR_BODY="## Task

${TASK_PROMPT}

---
*Automated by sidequest*"
fi

# --- Step 9: Push ---
echo "==> Pushing branch..."
git push origin HEAD

# --- Step 10: Open pull request ---
echo "==> Creating pull request..."
PR_URL=$(gh pr create \
  --title "$PR_TITLE" \
  --body "$PR_BODY" \
  --base "$BASE_BRANCH")

# --- Step 11: Done ---
echo "==> Pull request created: $PR_URL"
exit 0
