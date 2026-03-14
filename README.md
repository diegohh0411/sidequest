# sidequest

Lightweight CLI that runs Claude Code in disposable Docker containers to implement features and open PRs.

## Prerequisites

- **Docker** — installed and running
- **Rust toolchain** — 1.75+ (`rustup` recommended)
- **Auth** — one of:
  - **Claude Pro/Max account** — if you've used `claude` interactively on this machine, your session at `~/.claude` is mounted read-only into the container automatically (no API key needed)
  - **Anthropic API key** — set `ANTHROPIC_API_KEY` to use API billing instead; this takes precedence over the mounted session
- **GitHub PAT** — with `repo` scope (`GH_TOKEN`)

## Quick Start

```bash
# 1. Build sidequest
cargo build --release

# 2. Build the Docker image (one-time setup)
./target/release/sidequest build-image

# 3. Set your GitHub token
export GH_TOKEN="ghp_your_token_here"

# 4. Run a task
#    If you have a Claude Pro/Max account and have run `claude` at least once, no API key needed:
./target/release/sidequest run \
  --repo "owner/repo" \
  --prompt "Implement the /health endpoint in src/routes/health.rs returning 200 OK with a JSON body"

#    Alternatively, set ANTHROPIC_API_KEY to use API billing:
export ANTHROPIC_API_KEY="sk-ant-your_key_here"
./target/release/sidequest run \
  --repo "owner/repo" \
  --prompt "Implement the /health endpoint in src/routes/health.rs returning 200 OK with a JSON body"
```

## Usage

### `sidequest build-image`

Builds the Docker image locally from `docker/Dockerfile`. Run this once before your first task, or whenever the Dockerfile changes.

### `sidequest run`

Runs a coding task in a disposable Docker container.

```
sidequest run \
  --repo "owner/repo" \
  --prompt "Your task description here" \
  --base-branch main    # optional, defaults to "main"
```

What happens:
1. A UUID is generated for the task
2. A container is created with your secrets passed as env vars
3. Inside the container: the repo is cloned, Claude Code runs your prompt, changes are committed, pushed, and a PR is opened
4. Logs are streamed to your terminal in real-time
5. The container is removed after completion

### `sidequest logs`

View logs from a past task run (if the container still exists).

```
sidequest logs --task-id <uuid>
```

## Architecture

```
Developer's laptop
       │
       ▼
   ┌─────────┐     ┌──────────────────────────────────────┐
   │ sidequest│────▶│ Docker container (disposable)        │
   │   CLI    │     │                                      │
   └─────────┘     │  1. Clone repo                       │
       │           │  2. Run Claude Code (headless)        │
       │           │  3. Commit & push changes             │
       ▼           │  4. Open PR via gh CLI                │
   Stream logs     └──────────────────────────────────────┘
   to terminal                    │
                                  ▼
                            GitHub PR created
```

**Flow:**
1. `sidequest run` creates a Docker container from the pre-baked `sidequest-agent` image
2. Environment variables (secrets, task config) are injected at container creation time
3. The container's entrypoint script handles the full workflow: clone → branch → Claude Code → commit → push → PR
4. sidequest streams the container's stdout/stderr to your terminal
5. After completion, the container is force-removed

## Configuration

Create a `config.toml` in your working directory or at `~/.config/sidequest/config.toml`:

```toml
[docker]
image_name = "sidequest-agent"         # name for the pre-baked image
container_prefix = "sq-task-"           # container name prefix

[github]
# GH_TOKEN is always read from env var, never from config
default_base_branch = "main"
branch_prefix = "sidequest/"            # branches will be named sidequest/<task-id-short>

[claude]
# ANTHROPIC_API_KEY is always read from env var, never from config
model = "sonnet"                        # passed to claude via --model flag
max_turns = -1                          # unlimited turns, let it cook
```

If no config file is found, sensible defaults are used (see `config.example.toml`).

## Security

- **Secrets are env-var-only.** `GH_TOKEN` and `ANTHROPIC_API_KEY` (when used) are never stored in config files or baked into the Docker image. They are passed to the container at runtime via environment variables. When using the mounted session auth, `~/.claude` is mounted read-only and no credentials are copied.
- **Containers are ephemeral and isolated.** Each task runs in a fresh, disposable Docker container. The container is force-removed after the task completes, regardless of success or failure.
- **`--dangerously-skip-permissions` is safe here.** The flag is required for headless Claude Code operation. It is safe because the container *is* the sandbox — Claude Code can only affect files inside the container, which is destroyed after the run.

## License

MIT
