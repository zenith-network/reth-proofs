#!/bin/bash

# Make sure container was run with GPU access.
echo "> Checking GPU access..."
if ! nvidia-smi >/dev/null 2>&1; then
  echo "❌ ERROR: GPU not accessible. Make sure you launched container with --gpus flag." >&2
  exit 1
fi
echo "✅ GPU access OK."

# Compile Risc0 host.
echo "> Compiling Risc0 host..."
cargo build -p reth-proofs-zkvm-risc0-host --no-default-features --features cuda --release --bin reth-proofs-zkvm-risc0-host
echo "✅ Risc0 host compiled."

# Run the actual CMD passed to the container.
echo "> Executing command: $*"
exec "$@"
