FROM nvidia/cuda:12.2.2-devel-ubuntu20.04

WORKDIR /app

# Install base dependencies.
ARG DEBIAN_FRONTEND=noninteractive
RUN apt update && apt install --yes --force-yes curl nvidia-cuda-toolkit

# Install Rust.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Install Risc0.
RUN curl -L https://risczero.com/install | bash
ENV PATH="/root/.risc0/bin:${PATH}"
RUN rzup install && rzup install r0vm 3.0.0-rc.1

# Copy the project files.
COPY . /app

# Compile Risc0 host.
#RUN cargo build -p reth-proofs-zkvm-risc0-host --release

# Run prover.
#CMD ["./target/release/reth-proofs-zkvm-risc0-host"]
