FROM debian:bookworm-slim AS pyvenv
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 \
    python3-venv \
    python3-pip \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY requirements-sandbox.txt /tmp/requirements-sandbox.txt
RUN python3 -m venv /opt/de-venv \
    && /opt/de-venv/bin/pip install --no-cache-dir --upgrade pip \
    && /opt/de-venv/bin/pip install --no-cache-dir -r /tmp/requirements-sandbox.txt \
    && /opt/de-venv/bin/python3 -c "import numpy, scipy, statsmodels, pyarrow; print('de-venv ok')"

FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef
RUN apt-get update && apt-get install -y pkg-config cmake clang libssl-dev python3 && rm -rf /var/lib/apt/lists/*
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS buildcache
COPY --from=planner /app/recipe.json recipe.json

FROM buildcache AS dev
# Install dev tools
RUN cargo install cargo-watch
# Grab dependencies into target
RUN cargo chef cook --recipe-path recipe.json
# This enables us to keep /app/target inside the container when we mount /app from host
VOLUME /app/target
# So we hold the daemon mode of docker run
CMD ["sleep", "infinity"]

FROM buildcache AS buildcacherelease
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json

FROM buildcacherelease AS build
COPY --from=pyvenv /opt/de-venv /opt/de-venv
ENV DE_BE__SANDBOX__PYTHON_BINARY=/opt/de-venv/bin/python3
COPY . .
RUN cargo test -- --test-threads=1
RUN cargo build --release

# We do not need the Rust toolchain to run the binary!
FROM debian:bookworm-slim AS runtime

# Install common runtime dependencies and Python 3 runtime for the pinned venv
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    python3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=pyvenv /opt/de-venv /opt/de-venv
RUN /opt/de-venv/bin/python3 -c "import numpy, scipy, statsmodels, pyarrow"

WORKDIR /app
COPY --from=build /app/target/release/de-be /usr/local/bin/de-be
COPY config ./config

ENV DE_BE__SANDBOX__PYTHON_BINARY=/opt/de-venv/bin/python3

# Verify that the binary can run in this environment (e.g. check version or help)
RUN /usr/local/bin/de-be --help

EXPOSE 8080 8079

ENTRYPOINT ["/usr/local/bin/de-be"]
CMD ["start"]
