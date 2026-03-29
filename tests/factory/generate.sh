#!/bin/bash
# Regenerate all test fixtures from factory.
# Run from project root: ./tests/factory/generate.sh

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
cargo run --example gen_c2pa_fixtures
