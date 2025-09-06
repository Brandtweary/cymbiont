#!/bin/bash
# Backup emergency deployment manual with ASCII-only version
# This ensures documentation survives encoding issues and data corruption

set -e

# Colors for output (optional, will work without)
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Source and destination paths
SOURCE="CYMBIONT_EMERGENCY_DEPLOYMENT.md"
ASCII_DEST="CYMBIONT_EMERGENCY_DEPLOYMENT_ASCII.txt"
BACKUP_DIR="emergency_backups"

# Create backup directory if it doesn't exist
mkdir -p "$BACKUP_DIR"

# Check if source exists
if [ ! -f "$SOURCE" ]; then
    echo "Error: $SOURCE not found!"
    exit 1
fi

echo -e "${GREEN}Creating ASCII-safe backup of emergency manual...${NC}"

# Create ASCII version by stripping emojis and non-ASCII characters
# Also add header explaining this is the ASCII version
{
    echo "==============================================================================="
    echo "CYMBIONT EMERGENCY DEPLOYMENT MANUAL (ASCII-SAFE VERSION)"
    echo "Generated from: $SOURCE on $(date)"
    echo "This version strips all Unicode for maximum compatibility"
    echo "==============================================================================="
    echo ""
    
    # Strip emojis and convert to ASCII
    # The penguin emoji becomes [LINUX PENGUIN]
    # The robot emoji becomes [ROBOT]
    # Arrows become ->
    cat "$SOURCE" | \
        sed 's/🐧/[LINUX PENGUIN]/g' | \
        sed 's/🤖/[ROBOT]/g' | \
        sed 's/→/->/g' | \
        iconv -c -f UTF-8 -t ASCII//TRANSLIT
} > "$ASCII_DEST"

# Create timestamped backup
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
cp "$SOURCE" "$BACKUP_DIR/manual_${TIMESTAMP}.md"
cp "$ASCII_DEST" "$BACKUP_DIR/manual_${TIMESTAMP}_ascii.txt"

# Keep only last 5 backups to save space
ls -t "$BACKUP_DIR"/manual_*.md | tail -n +6 | xargs -r rm
ls -t "$BACKUP_DIR"/manual_*_ascii.txt | tail -n +6 | xargs -r rm

# Create symlinks for easy access
ln -sf "$PWD/$SOURCE" "$BACKUP_DIR/latest.md"
ln -sf "$PWD/$ASCII_DEST" "$BACKUP_DIR/latest_ascii.txt"

# Report success
echo -e "${GREEN}✓ Created ASCII version: $ASCII_DEST${NC}"
echo -e "${GREEN}✓ Backed up to: $BACKUP_DIR/manual_${TIMESTAMP}.md${NC}"
echo -e "${YELLOW}  Total backups: $(ls "$BACKUP_DIR"/manual_*.md 2>/dev/null | wc -l)${NC}"

# Verify ASCII-only
if file "$ASCII_DEST" | grep -q "ASCII text"; then
    echo -e "${GREEN}✓ Verified: Output is pure ASCII${NC}"
else
    echo -e "${YELLOW}⚠ Warning: Output may contain non-ASCII characters${NC}"
fi