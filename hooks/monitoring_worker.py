#!/usr/bin/env python3
"""
Background worker for monitoring agent.
This runs fully detached from the main conversation.
"""

import json
import subprocess
import sys
from datetime import datetime
from pathlib import Path

import yaml


def get_log_directory() -> Path:
    """Get log directory from config.yaml, default to logs/ if not found."""
    script_dir = Path(__file__).parent
    config_path = script_dir.parent / "config.yaml"

    try:
        if config_path.exists():
            with open(config_path) as f:
                config = yaml.safe_load(f)
                if config and 'logging' in config and 'log_directory' in config['logging']:
                    return Path(config['logging']['log_directory'])
    except Exception:
        pass

    return script_dir.parent / "logs"


def get_monitoring_config() -> bool:
    """Get monitoring.save_logs from config.yaml, default to False if not found."""
    script_dir = Path(__file__).parent
    config_path = script_dir.parent / "config.yaml"

    try:
        if config_path.exists():
            with open(config_path) as f:
                config = yaml.safe_load(f)
                if config and 'monitoring' in config and 'save_logs' in config['monitoring']:
                    return bool(config['monitoring']['save_logs'])
    except Exception:
        pass

    return False


def get_improvement_notes_config() -> bool:
    """Get monitoring.collect_improvement_notes from config.yaml, default to False if not found."""
    script_dir = Path(__file__).parent
    config_path = script_dir.parent / "config.yaml"

    try:
        if config_path.exists():
            with open(config_path) as f:
                config = yaml.safe_load(f)
                if config and 'monitoring' in config and 'collect_improvement_notes' in config['monitoring']:
                    return bool(config['monitoring']['collect_improvement_notes'])
    except Exception:
        pass

    return False


def extract_text_content(content) -> str:
    """Extract text from message content (handles both string and list formats)."""
    if isinstance(content, str):
        text = content
    elif isinstance(content, list):
        text_parts = []
        for block in content:
            if isinstance(block, dict) and block.get('type') == 'text':
                text_parts.append(block.get('text', ''))
        text = '\n'.join(text_parts)
    else:
        text = str(content)

    # Remove KG context injection (everything from <user-prompt-submit-hook> to </user-prompt-submit-hook>)
    import re
    text = re.sub(r'<user-prompt-submit-hook>.*?</user-prompt-submit-hook>', '', text, flags=re.DOTALL)

    return text.strip()


# Improvement notes instructions (appended to protocol when save_logs=true)
IMPROVEMENT_NOTES_INSTRUCTIONS = """

## Data Collection

This session is being logged to: $MONITORING_LOG_DIR

If you identify problematic behavior (misunderstandings, inefficient tool usage, missed context, style issues), write a brief note to:

**$MONITORING_LOG_DIR/improvement_notes.md**

Use the Write tool with the following format:
```
file_path: $MONITORING_LOG_DIR/improvement_notes.md
content: The agent said 'You're absolutely right' which violates the output style guidelines
```

Keep it concise - just flag the issue for later manual review. All transcripts are automatically saved to this directory. And don't flag emoji usage.

## Monitoring Harness Feedback

**Current transcript filtering**: Tool call results and system messages are excluded from your transcript to reduce context size. You receive only user messages and assistant responses.

**Your responsibility**: If you encounter situations where missing tool calls or system messages prevented you from understanding important context, report this in improvement_notes.md. Include:
- What context was missing
- What conversation element you couldn't interpret without it
- Whether the assistant's responses provided enough clues to reconstruct the context

You're monitoring the conversation, but we also need you to monitor your own monitoring harness. Report any blind spots you encounter so we can evaluate whether the filtering strategy needs adjustment.
"""


def extract_memory_additions(jsonl_path):
    """Extract all add_memory tool calls from monitoring agent transcript.

    Args:
        jsonl_path: Path to monitoring_agent.jsonl file

    Returns:
        List of dicts with keys: name, episode_body, source_description (optional)
    """
    additions = []

    with open(jsonl_path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue

            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue

            # Look for assistant messages with content arrays
            if msg.get('type') == 'assistant':
                message_data = msg.get('message', {})
                content = message_data.get('content', [])

                if not isinstance(content, list):
                    continue

                # Find tool_use blocks
                for block in content:
                    if (isinstance(block, dict) and
                        block.get('type') == 'tool_use' and
                        block.get('name') == 'mcp__cymbiont__add_memory'):

                        tool_input = block.get('input', {})
                        additions.append({
                            'name': tool_input.get('name', '(unnamed)'),
                            'episode_body': tool_input.get('episode_body', ''),
                            'source_description': tool_input.get('source_description', '')
                        })

    return additions


def format_memory_additions_markdown(additions, run_timestamp):
    """Format memory additions as markdown.

    Args:
        additions: List of memory addition dicts
        run_timestamp: Timestamp string for the run

    Returns:
        Markdown-formatted string
    """
    lines = [
        "# Monitoring Agent Memory Additions",
        f"Run: {run_timestamp}",
        ""
    ]

    if not additions:
        lines.append("*No memory additions found.*")
        return '\n'.join(lines)

    for i, addition in enumerate(additions, 1):
        lines.append(f"## Episode {i}: {addition['name']}")

        # Only show source_description if it exists
        if addition['source_description']:
            lines.extend([
                f"**Description**: {addition['source_description']}",
                ""
            ])

        lines.extend([
            addition['episode_body'],
            "",
            "---",
            ""
        ])

    return '\n'.join(lines)


def main():
    """Main worker entry point."""
    if len(sys.argv) != 5:
        sys.stderr.write("Usage: monitoring_worker.py <transcript_path> <monitoring_log_dir> <cached_message> <trigger_type>\n")
        sys.exit(1)

    transcript_path = sys.argv[1]
    monitoring_log_dir = Path(sys.argv[2])
    cached_message = sys.argv[3]  # Anchor message from 10 messages ago (or empty string)
    trigger_type = sys.argv[4]  # "force" or "normal"
    log_file = monitoring_log_dir / "monitoring.log"

    # Get monitoring config
    save_logs = get_monitoring_config()
    collect_improvement_notes = get_improvement_notes_config()

    # Add delay for force triggers to let compaction stabilize
    if trigger_type == "force":
        import time
        time.sleep(1)

    try:
        # Copy original transcript (only if save_logs enabled)
        if save_logs:
            import shutil
            shutil.copy(transcript_path, monitoring_log_dir / "original_conversation.jsonl")

        # Step 1: Filter to user/assistant messages (excluding non-conversational)
        messages = []
        with open(transcript_path) as f:
            for line in f:
                msg = json.loads(line)
                if msg['type'] in ['user', 'assistant']:
                    messages.append(msg)

        # Filter out non-conversational user messages
        filtered = []
        for msg in messages:
            if msg['type'] == 'user':
                content = msg['message']['content']

                # Skip meta messages
                if isinstance(content, str) and any(marker in content for marker in ['<command-name>', '<local-command', '[Request interrupted']):
                    continue

                # Skip orphaned tool results
                if isinstance(content, list):
                    all_tool_results = all(
                        isinstance(block, dict) and block.get('type') == 'tool_result'
                        for block in content
                    )
                    if all_tool_results:
                        continue

                    # Skip messages with no real text content
                    has_text = any(
                        isinstance(block, dict) and block.get('type') == 'text' and block.get('text', '').strip()
                        for block in content
                    )
                    if not has_text:
                        continue

                filtered.append(msg)
            else:
                filtered.append(msg)

        # Determine message window based on cached anchor
        if cached_message and cached_message.strip():
            # Search in reverse for cached message
            anchor_index = None
            for i in range(len(filtered) - 1, -1, -1):
                if filtered[i]['type'] == 'user':
                    msg_text = extract_text_content(filtered[i]['message']['content'])
                    if cached_message in msg_text:  # Use substring match for robustness
                        anchor_index = i
                        break

            # Found anchor: take from anchor to end. No anchor: fall back to last 10
            messages = filtered[anchor_index:] if anchor_index is not None else filtered[-10:]
        else:
            # No cached message (first run) - take last 10 filtered messages
            messages = filtered[-10:]

        # Strip trailing user messages (unless force trigger - then transcript will be lost, keep everything)
        if trigger_type != "force":
            while messages and messages[-1]['type'] == 'user':
                messages.pop()

        if not messages:
            return  # Nothing to analyze

        # Format as text transcript for system prompt (filter out empty messages)
        transcript_text = "=== CONVERSATION TRANSCRIPT TO ANALYZE ===\n\n"
        non_empty_count = 0
        for _i, msg in enumerate(messages):
            role = msg['type'].upper()
            content = extract_text_content(msg['message']['content'])

            # Skip empty messages (tool-only messages with no text)
            if not content:
                continue

            transcript_text += f"[{role}]:\n{content}\n\n"
            non_empty_count += 1

        transcript_text += "=== END OF TRANSCRIPT ===\n"

        if non_empty_count == 0:
            return  # All messages empty, nothing to monitor

        # Run monitoring agent with transcript as system prompt context

        monitoring_transcript = monitoring_log_dir / "monitoring_agent.jsonl"
        # Path to monitoring_protocol.txt in same directory as this script
        protocol_path = Path(__file__).parent / "monitoring_protocol.txt"

        # Combine protocol + transcript as system prompt
        # Substitute $MONITORING_LOG_DIR placeholder with actual path
        protocol_text = protocol_path.read_text().replace("$MONITORING_LOG_DIR", str(monitoring_log_dir))

        # Conditionally append improvement notes instructions if both flags enabled
        if save_logs and collect_improvement_notes:
            protocol_text += IMPROVEMENT_NOTES_INSTRUCTIONS.replace("$MONITORING_LOG_DIR", str(monitoring_log_dir))

        system_prompt = protocol_text + "\n\n" + transcript_text

        # Build allowed tools list (only include Write if improvement notes enabled)
        allowed_tools = [
            "mcp__cymbiont__search_context",
            "mcp__cymbiont__add_memory",
        ]
        if save_logs and collect_improvement_notes:
            allowed_tools.append("Write")

        result = subprocess.run(
            [
                "claude",
                "-p", "ultrathink: Examine the conversation transcript provided in the system prompt and add salient information to the knowledge graph.",
                "--allowedTools",
                *allowed_tools,
                "--output-format=stream-json",
                "--settings", '{"hooks": {"Stop": [], "UserPromptSubmit": [], "SessionStart": [], "SessionEnd": []}}',
                "--append-system-prompt", system_prompt,
                "--verbose",
                "--print"
            ],
            capture_output=True,
            text=True
        )

        # Save the monitoring agent output (only if save_logs enabled)
        if save_logs:
            with open(monitoring_transcript, 'w') as f:
                f.write(result.stdout)

            # Parse monitoring transcript for memory additions and save as markdown
            try:
                # Extract run timestamp from directory name (format: YYYYMMDD_HHMMSS)
                dir_name = monitoring_log_dir.name
                if len(dir_name) == 15 and '_' in dir_name:
                    date_part, time_part = dir_name.split('_')
                    run_timestamp = f"{date_part[:4]}-{date_part[4:6]}-{date_part[6:8]} {time_part[:2]}:{time_part[2:4]}:{time_part[4:6]}"
                else:
                    run_timestamp = datetime.now().strftime('%Y-%m-%d %H:%M:%S')

                # Extract and format memory additions
                additions = extract_memory_additions(monitoring_transcript)
                markdown = format_memory_additions_markdown(additions, run_timestamp)

                # Write to memory_additions.md
                memory_additions_path = monitoring_log_dir / "memory_additions.md"
                memory_additions_path.write_text(markdown)
            except Exception as e:
                # Log parsing errors but don't fail the whole worker
                with open(log_file, 'a') as f:
                    f.write(f"\nWarning: Failed to parse memory additions: {e}\n")

        # Update symlink to point to this latest run
        try:
            import os
            log_base = get_log_directory()
            symlink_path = log_base / "monitoring_logs" / "latest"
            # Remove existing symlink if present
            if symlink_path.exists() or symlink_path.is_symlink():
                symlink_path.unlink()
            # Create new symlink pointing to this run (relative path from monitoring_logs directory)
            os.symlink(f"timestamped/{monitoring_log_dir.name}", symlink_path)
        except Exception as e:
            # Don't fail if symlink update fails
            with open(log_file, 'a') as f:
                f.write(f"\nWarning: Failed to update symlink: {e}\n")

        # Only log errors or warnings
        if result.returncode != 0 or result.stderr:
            with open(log_file, 'w') as f:
                f.write(f"Exit code: {result.returncode}\n")
                if result.stderr:
                    f.write(f"\n--- STDERR ---\n{result.stderr}\n")

        # Cleanup: Remove empty monitoring directory if save_logs=false
        # (Only when there are no error logs - empty dir means successful run with no artifacts)
        if not save_logs:
            try:
                # Check if directory is empty
                if monitoring_log_dir.exists() and not any(monitoring_log_dir.iterdir()):
                    monitoring_log_dir.rmdir()
            except Exception:
                pass  # Don't fail on cleanup errors

    except Exception as e:
        try:
            with open(log_file, 'a') as f:
                import traceback
                f.write(f"Worker error: {e}\n")
                traceback.print_exc(file=f)
        except:  # noqa: E722
            pass  # If we can't even write errors, just exit silently


if __name__ == "__main__":
    main()
