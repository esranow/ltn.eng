FROM rust:latest

# Install Python3, Z3, build tools, and minimal dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 \
    python3-pip \
    python3-venv \
    python3-dev \
    libz3-dev \
    z3 \
    pkg-config \
    libssl-dev \
    cmake \
    clang \
    libclang-dev \
    && rm -rf /var/lib/apt/lists/*

# Create mount point directories
RUN mkdir -p /workspace /graph /ref/beam /ref/rippl

# Create non-root build user for defense-in-depth
RUN useradd -m -s /bin/bash ltn \
    && chown -R ltn:ltn /workspace /graph

# Pre-warm cargo registry index (speeds up first build)
USER ltn
RUN cargo init /tmp/warmup && cd /tmp/warmup && cargo fetch 2>/dev/null || true && rm -rf /tmp/warmup

WORKDIR /workspace

# Keep container alive as a persistent sandbox
CMD ["tail", "-f", "/dev/null"]
