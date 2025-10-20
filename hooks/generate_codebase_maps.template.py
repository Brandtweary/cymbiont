#!/usr/bin/env python3
"""
Generate per-file codebase maps using code2prompt and split them into corpus/.

This script generates a codebase map using code2prompt (Rust version recommended),
then splits it into individual per-file markdown documents that get synced to
your Cymbiont knowledge graph.

INSTALLATION:

1. Install rust code2prompt:
   cargo install code2prompt
   (https://github.com/mufeedvh/code2prompt)

2. Copy this template and customize the CONFIGURATION section below

3. Install as post-commit hook (optional but recommended):
   See post-commit.template.sh for hook installation instructions

USAGE:
    ./generate_codebase_maps.py <project_path> <project_name>

EXAMPLE:
    ./generate_codebase_maps.py ~/projects/myapp myapp

    This will:
    - Run code2prompt on ~/projects/myapp
    - Generate individual .md files in CORPUS_DIR/myapp/
    - Files will be synced to knowledge graph on next hourly sync

CUSTOMIZE:
Edit the CONFIGURATION section below to match your project structure.
"""

import os
import re
import sys
import subprocess
from pathlib import Path

# ============================================================================
# CONFIGURATION - Customize these for your project
# ============================================================================

# Where to write the codebase map files
# These will be synced to your knowledge graph by Cymbiont's document watcher
CORPUS_DIR = "/absolute/path/to/your/corpus/codebase_maps"

# Temporary directory for intermediate files (will be cleaned up)
TEMP_DIR = "/tmp"

# File patterns to include in codebase map
# Common: source code, configs, docs
INCLUDE_PATTERNS = "*.rs,*.py,*.toml,*.yaml,*.yml,*.md,*.sh,*.json"

# Directories and patterns to exclude
# Common: build artifacts, dependencies, logs, caches, data directories
EXCLUDE_PATTERNS = "*.pyc,__pycache__,node_modules,.git,build,dist,*.egg-info,target,.venv,logs,data,archive,venv"

# Additional code2prompt flags (optional)
# Examples: --encoding utf-8, --diff, --git-diff-branch main
ADDITIONAL_FLAGS = []

# ============================================================================
# Script implementation (no changes needed below)
# ============================================================================


def run_code2prompt(project_path: str, output_file: str) -> tuple[bool, str]:
    """
    Run code2prompt (Rust version) on the given project path.

    Returns:
        (success: bool, error_message: str)
    """
    cmd = [
        "code2prompt",
        project_path,
        "--output-file", output_file,
        "--include", INCLUDE_PATTERNS,
        "--exclude", EXCLUDE_PATTERNS,
        "--line-numbers",
        "--tokens", "format",
        "--no-clipboard",
        "--quiet",  # Suppress progress messages
    ] + ADDITIONAL_FLAGS

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=300,  # 5 minute timeout
        )

        if result.returncode != 0:
            return False, f"code2prompt failed: {result.stderr}"

        return True, ""
    except subprocess.TimeoutExpired:
        return False, "code2prompt timed out after 5 minutes"
    except Exception as e:
        return False, f"Error running code2prompt: {e}"


def parse_and_split_map(map_file: str, output_dir: str, project_name: str) -> None:
    """
    Parse the code2prompt (Rust version) output and split into individual files.

    Args:
        map_file: Path to the generated codebase map
        output_dir: Base output directory (e.g., corpus/codebase_maps/)
        project_name: Name of the project (used as subdirectory)
    """
    with open(map_file, 'r') as f:
        content = f.read()

    # Rust version format: `filename`:
    # Split on backtick-filename-backtick-colon pattern
    file_sections = re.split(r'^`([^`]+)`:$', content, flags=re.MULTILINE)

    # First section is Project Path + Source Tree, skip it
    # After split, we get: [header, path1, content1, path2, content2, ...]
    file_sections = file_sections[1:]  # Skip header

    project_output_dir = Path(output_dir) / project_name
    project_output_dir.mkdir(parents=True, exist_ok=True)

    files_written = 0

    # Process pairs: (path, content)
    for i in range(0, len(file_sections), 2):
        if i + 1 >= len(file_sections):
            break

        file_path = file_sections[i].strip()
        file_content = file_sections[i + 1]

        # Create output path
        output_path = project_output_dir / f"{file_path}.md"
        output_path.parent.mkdir(parents=True, exist_ok=True)

        # Write the file with proper markdown structure
        with open(output_path, 'w') as f:
            f.write(f"# {file_path}\n\n")
            f.write(file_content.strip())
            f.write("\n")

        files_written += 1

    print(f"✓ Split {files_written} files into {project_output_dir}")


def main():
    if len(sys.argv) != 3:
        print("Usage: ./generate_codebase_maps.py <project_path> <project_name>")
        print("\nExample:")
        print("  ./generate_codebase_maps.py ~/projects/myapp myapp")
        print("\nThis will create:")
        print(f"  {CORPUS_DIR}/myapp/*.md")
        print("\nThese files will be synced to your knowledge graph on next hourly sync.")
        sys.exit(1)

    project_path = sys.argv[1]
    project_name = sys.argv[2]

    # Validate project path exists
    if not Path(project_path).is_dir():
        print(f"Error: Project path '{project_path}' does not exist or is not a directory")
        sys.exit(1)

    # Validate corpus directory exists
    corpus_path = Path(CORPUS_DIR)
    if not corpus_path.exists():
        print(f"Error: Corpus directory '{CORPUS_DIR}' does not exist")
        print("Please create it or update CORPUS_DIR in this script")
        sys.exit(1)

    # Set up temp file path
    temp_map = Path(TEMP_DIR) / f"{project_name}_codebase_map.md"

    print(f"Generating codebase map for {project_name}...")

    # Run code2prompt
    success, error = run_code2prompt(project_path, str(temp_map))
    if not success:
        print(f"✗ {error}")
        sys.exit(1)

    print(f"✓ Generated temporary map at {temp_map}")

    # Parse and split
    print(f"Splitting map into individual files...")
    parse_and_split_map(str(temp_map), CORPUS_DIR, project_name)

    # Clean up temp file
    temp_map.unlink()
    print(f"✓ Cleaned up temporary map file")

    print(f"\n✓ Codebase maps written to {CORPUS_DIR}/{project_name}/")
    print(f"  Files will be synced to knowledge graph on next doc sync (hourly or manual)")


if __name__ == "__main__":
    main()
