FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# Base build deps. tree-sitter generate needs a host C compiler for build.rs;
# the musl cross-compiler used at link time comes from nix-shell.
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    curl \
    xz-utils \
    libudev-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Allow the vscode user to access serial devices (/dev/ttyUSB*, /dev/ttyACM*)
# Group varies by host: typically 'dialout' or 'sudo' depending on udev rules.
RUN usermod -aG dialout,sudo vscode

# Install Nix in single-user mode as the vscode user.
#
# Why single-user (--no-daemon):
#   - No systemd / nix-daemon process required → works in any container runtime.
#   - /nix is owned by vscode, no daemon socket / multi-user gymnastics.
#   - Identical CLI surface as the host (`nix-shell -p pkgsCross.…`), so the
#     wrapper scripts in scripts/nix-musl-cc-*.sh "just work".
#
# Why install in the Dockerfile rather than post-create:
#   - Image is self-contained; new containers spin up without bootstrap latency.
#   - No /nix volume mount in devcontainer.json, so the previous shadow-mount
#     chicken-and-egg can't recur.
USER vscode
RUN curl --proto '=https' --tlsv1.2 -sSf -L https://nixos.org/nix/install \
    | sh -s -- --no-daemon
USER root

# Make Nix binaries visible to every subprocess (not just interactive shells).
# This is what makes `nix-shell` resolvable from cargo's linker invocation.
ENV PATH="/home/vscode/.nix-profile/bin:${PATH}"

# Add musl cross-compilation targets.
RUN rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl