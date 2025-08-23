#!/bin/bash
# Run cargo test with output captured to logs/test.log
cd "$(dirname "$0")"
cargo test "$@" 2>&1 | tee logs/test.log