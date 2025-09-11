FROM nvidia/cuda:12.2.2-devel-ubuntu20.04

WORKDIR /app

# Install base dependencies.
ARG DEBIAN_FRONTEND=noninteractive
RUN apt update && apt install --yes --force-yes curl nvidia-cuda-toolkit

# New groth16 implementation in Risc0 v3 requires extra host dependencies, due to `circom-witnesscalc`.
# Dependency tree: risc0-zkvm/cuda -> risc0-groth16/cuda -> circom-witnesscalc.
# Related PR: https://github.com/risc0/risc0/pull/3296.
RUN apt update && apt install --yes --force-yes clang protobuf-compiler

# Install Rust.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Install Risc0.
RUN curl -L https://risczero.com/install | bash
ENV PATH="/root/.risc0/bin:${PATH}"
RUN rzup install && rzup install r0vm 3.0.3

# Copy the project files.
COPY . /app

# Copy the entrypoint script (checks GPU and compiles the prover).
RUN chmod +x /app/entrypoint.sh

# Set the entrypoint.
ENTRYPOINT ["/app/entrypoint.sh"]

# Run prover.
CMD ["./target/release/reth-proofs-zkvm-risc0-host"]
