# Cymbiont Hooks

This directory contains Claude Code hooks and git hook templates for automated Cymbiont functionality.

## Claude Code Hooks

These hooks run automatically when using Claude Code with Cymbiont:

### `inject_kg_context.py`
**Hook Type:** UserPromptSubmit

Automatically injects relevant knowledge graph context into every user message. Runs dual queries (user message + agent's previous response) to fetch ~6 nodes and ~12 facts from the knowledge graph, injected as XML in the prompt.

### `monitoring_agent.py`
**Hook Types:** UserPromptSubmit, PreCompact, SessionEnd

Counter-based monitoring system that spawns a background agent every 10 messages to identify and capture salient information from conversations. Also runs on compaction and session end to capture remaining messages.

### `monitoring_worker.py`
**Background Worker**

Spawned by `monitoring_agent.py` to run Claude Code in the background, analyze conversation transcripts, and add episodes to the knowledge graph. Runs fully detached to avoid blocking the main conversation.

**Observability logs** are controlled by `monitoring.save_logs` in `config.yaml`:
- When `true`: Saves original transcripts, agent output, memory summaries, improvement notes (for system developers)
- When `false` (default): Episodes still added to graph, but without debug artifacts (reduces disk usage)

### `monitoring_protocol.txt`
**Configuration**

Instructions provided to the monitoring agent about what information to capture and how to format it. Guides the agent to capture semantic/episodic memory and avoid operational noise.

## Git Hook Templates

These templates automate codebase map generation for syncing code changes to your knowledge graph:

### `post-commit.template.sh`
**Hook Type:** Git post-commit hook

Template for automatically regenerating codebase maps after each commit. Copy to `.git/hooks/post-commit` in your project and customize the paths.

**Installation:**
1. Install rust code2prompt: `cargo install code2prompt`
2. Customize `generate_codebase_maps.template.py` for your project
3. Copy this template to `.git/hooks/post-commit`
4. Edit the copied file to set your project paths
5. Make it executable: `chmod +x .git/hooks/post-commit`

### `generate_codebase_maps.template.py`
**Script**

Python script that runs code2prompt on your project and splits the output into individual per-file markdown documents. These files are placed in your corpus directory and synced to the knowledge graph hourly.

**Customize:**
- `CORPUS_DIR`: Where to write codebase map files
- `INCLUDE_PATTERNS`: Which file types to include
- `EXCLUDE_PATTERNS`: What to skip (build artifacts, dependencies, etc.)

**Usage:**
```bash
./generate_codebase_maps.py /path/to/project project-name
```

## Installation

### Claude Code Hooks
See the main Cymbiont README for installation instructions. Hooks should be configured in `~/.claude/settings.json`.

### Git Hooks
1. Customize the template scripts for your project structure
2. Copy `post-commit.template.sh` to your project's `.git/hooks/post-commit`
3. Edit the copied file to set your paths
4. Make it executable

## Notes

- Claude Code hooks run automatically when configured in settings.json
- Git hooks are per-repository and must be installed in each project
- Codebase maps sync hourly by default (configurable in Cymbiont config.yaml)
- All hooks are designed to be non-blocking and fail gracefully
