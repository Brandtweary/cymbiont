#!/bin/bash
# Post-commit hook template: Regenerate codebase maps with code2prompt
#
# This hook automatically regenerates your codebase map after each commit,
# keeping your Cymbiont knowledge graph in sync with code changes.
#
# INSTALLATION:
#
# 1. Install rust code2prompt:
#    cargo install code2prompt
#    (Rust version recommended - faster and more reliable than Python versions)
#
# 2. Customize generate_codebase_maps.py template for your project
#    (See generate_codebase_maps.template.py in this directory)
#
# 3. Install this hook in your project's .git/hooks/:
#    cp post-commit.template.sh /path/to/your/project/.git/hooks/post-commit
#    chmod +x /path/to/your/project/.git/hooks/post-commit
#
# 4. Edit the installed hook to set your paths (see CUSTOMIZE section below)
#
# CUSTOMIZE (edit these after copying to .git/hooks/):
#
# Set these variables to match your project structure:

# Absolute path to your project root (where the git repo lives)
PROJECT_DIR="/absolute/path/to/your/project"

# Name of your project (used for output directory naming)
PROJECT_NAME="your-project-name"

# Absolute path to where generate_codebase_maps.py is located
SCRIPT_PATH="/absolute/path/to/generate_codebase_maps.py"

# SCRIPT START (no changes needed below this line)

set -e

echo "Post-commit: Regenerating codebase map for $PROJECT_NAME..."

# Run the codebase map generator
"$SCRIPT_PATH" "$PROJECT_DIR" "$PROJECT_NAME"

echo "✓ Codebase map updated. Changes will sync to KG on next hourly sync."
