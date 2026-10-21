#!/bin/bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
mkdir -p .scratch
swiftc -swift-version 6 -strict-concurrency=complete -warnings-as-errors \
    -module-cache-path "$PWD/.scratch/ModuleCache" \
    ProbeContracts.swift ProbePolicy.swift AXBoundary.swift Tests/main.swift -o .scratch/ContentProbeTests
exec .scratch/ContentProbeTests
