#!/bin/bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
mkdir -p .build
swiftc -swift-version 6 -strict-concurrency=complete -warnings-as-errors \
    -module-cache-path "$PWD/.build/ModuleCache" \
    ProbePolicy.swift Tests/main.swift -o .build/ProbePolicyTests
exec .build/ProbePolicyTests
