#!/bin/bash
# Run cargo run with output captured to logs/run.log
cd "$(dirname "$0")"
cargo run "$@" 2>&1 | tee logs/run.log