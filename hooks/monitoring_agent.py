#!/usr/bin/env python3
"""
Automated Episode Monitoring Hook

Runs after every user prompt. Spawns monitoring worker every 10 messages.
Also runs on PreCompact/SessionEnd with --force flag to capture remaining messages.
This script exits immediately to avoid blocking the main conversation.
"""

import glob
import json
import sys
import subprocess
import os
import yaml
from datetime import datetime
from pathlib import Path


def get_log_directory() -> Path:
    """Get log directory from config.yaml, default to logs/ if not found."""
    script_dir = Path(__file__).parent
    config_path = script_dir.parent / "config.yaml"

    try:
        if config_path.exists():
            with open(config_path, 'r') as f:
                config = yaml.safe_load(f)
                if config and 'logging' in config and 'directory' in config['logging']:
                    return Path(config['logging']['directory'])
    except Exception:
        pass

    return script_dir.parent / "logs"


def main():
    """Main hook entry point - manages counter and spawns worker every 10 messages."""
    # Check for --force flag (used by PreCompact/SessionEnd hooks)
    force_trigger = len(sys.argv) > 1 and sys.argv[1] == '--force'

    try:
        # Read hook input from stdin
        input_data = json.load(sys.stdin)

        # State files in monitoring_logs subdirectory
        log_base = get_log_directory()
        state_dir = log_base / "monitoring_logs"
        state_dir.mkdir(parents=True, exist_ok=True)
        counter_file = state_dir / "message_count.txt"
        cache_file = state_dir / "last_cached_message.txt"

        if force_trigger:
            # Force trigger (PreCompact/SessionEnd) - capture remaining messages
            # Check for recent PreCompact flag (deduplication)
            precompact_flag = state_dir / "precompact_ran.flag"

            # Determine trigger source from hook type in input data
            # Log available keys for debugging
            debug_log = state_dir / "monitoring_agent.log"
            with open(debug_log, 'w') as f:
                f.write(f"{datetime.now()}: Force trigger - available keys: {list(input_data.keys())}\n")

            # Get hook event name from input data
            hook_event_name = input_data.get('hook_event_name', '')
            is_precompact = 'precompact' in str(hook_event_name).lower() or 'PreCompact' in str(sys.argv)

            with open(debug_log, 'a') as f:
                f.write(f"{datetime.now()}: Hook event='{hook_event_name}', is_precompact={is_precompact}, argv={sys.argv}\n")

            if is_precompact:
                # Mark that PreCompact ran (SessionEnd will check this)
                precompact_flag.write_text(str(datetime.now().timestamp()))
                with open(debug_log, 'a') as f:
                    f.write(f"{datetime.now()}: Created PreCompact flag\n")
            else:
                # SessionEnd - check if PreCompact ran recently
                if precompact_flag.exists():
                    try:
                        precompact_time = float(precompact_flag.read_text())
                        time_since = datetime.now().timestamp() - precompact_time
                        # If PreCompact ran within last 10 minutes, skip SessionEnd
                        if time_since < 600:  # 10 minutes
                            with open(debug_log, 'a') as f:
                                f.write(f"{datetime.now()}: SessionEnd skipped (PreCompact ran {time_since:.1f}s ago)\n")
                            precompact_flag.unlink()  # Clear flag for next time
                            return  # Skip this SessionEnd
                        else:
                            with open(debug_log, 'a') as f:
                                f.write(f"{datetime.now()}: SessionEnd proceeding (PreCompact was {time_since:.1f}s ago)\n")
                    except Exception as e:
                        with open(debug_log, 'a') as f:
                            f.write(f"{datetime.now()}: Error parsing PreCompact flag: {e}\n")

            # Read cached anchor message if available
            if cache_file.exists():
                cached_message = cache_file.read_text().strip()
            else:
                cached_message = ""

            # Set sentinel value (-1) to signal hard reset
            # Next UserPromptSubmit will see -1 and start fresh interval at count=1
            counter_file.write_text("-1")

        else:
            # Normal UserPromptSubmit flow - use 10-message counter
            user_prompt = input_data.get('prompt', '')

            if not user_prompt:
                return  # No prompt to process

            # Skip non-conversational messages (system messages, interrupts, monitoring invocations)
            if any(marker in user_prompt for marker in [
                '[Request interrupted',
                '<command-name>',
                'Examine the conversation transcript provided in the system prompt'
            ]):
                return  # Don't count or cache system messages

            # Read counter (default 0 if missing)
            if counter_file.exists():
                count = int(counter_file.read_text().strip())
            else:
                count = 0

            # Handle sentinel value: -1 means start fresh interval after hard reset
            if count == -1:
                count = 1
                cache_file.write_text(user_prompt)
                counter_file.write_text(str(count))
                return  # First message of new interval, don't spawn worker yet

            # Increment counter
            count += 1

            # Cache first message on fresh start (no prior cache exists)
            if count == 1 and not cache_file.exists():
                cache_file.write_text(user_prompt)

            # Check if we should trigger monitoring agent
            if count < 10:
                # Not yet time - just save counter and exit
                counter_file.write_text(str(count))
                return

            # Counter >= 10: spawn worker
            # Read cached anchor message BEFORE overwriting
            if cache_file.exists():
                cached_message = cache_file.read_text().strip()
            else:
                # No cached message - this is first run or new chat
                # Use empty string to signal worker to use fallback
                cached_message = ""

            # Cache THIS message for next interval (before spawning worker)
            cache_file.write_text(user_prompt)

            # Reset counter (before worker spawn in case of errors)
            counter_file.write_text("0")

        # TODO: Remove this workaround once Claude Code fixes stale session_id bug
        # WORKAROUND: Claude Code bug passes stale session ID after reload
        # Manually find most recently modified transcript
        session_dir = Path.home() / ".claude/projects/-home-brandt-projects-hector"
        transcripts = glob.glob(str(session_dir / "*.jsonl"))
        if transcripts:
            transcript_path = max(transcripts, key=os.path.getmtime)
        else:
            # Fallback to hook input if no transcripts found
            transcript_path = input_data.get('transcript_path', '')

        # Validate transcript path
        if not transcript_path or not os.path.exists(transcript_path):
            return

        # Create timestamped log directory
        timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
        log_base = get_log_directory()
        monitoring_log_dir = log_base / "monitoring_logs" / "timestamped" / timestamp
        monitoring_log_dir.mkdir(parents=True, exist_ok=True)

        # Spawn fully detached background worker with cached message
        worker_script = Path(__file__).parent / "monitoring_worker.py"
        # Pass trigger type: "force" or "normal"
        trigger_type = "force" if force_trigger else "normal"
        subprocess.Popen(
            ["python3", str(worker_script), transcript_path, str(monitoring_log_dir), cached_message, trigger_type],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            stdin=subprocess.DEVNULL,
            start_new_session=True,
            cwd=str(monitoring_log_dir)
        )

    except Exception as e:
        # Log error but don't block - no console output
        try:
            timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
            log_base = get_log_directory()
            error_log = log_base / "monitoring_logs" / "timestamped" / f"error_{timestamp}.log"
            error_log.parent.mkdir(parents=True, exist_ok=True)
            with open(error_log, 'w') as f:
                import traceback
                f.write(f"Hook error: {e}\n")
                traceback.print_exc(file=f)
        except:
            pass


if __name__ == "__main__":
    main()
