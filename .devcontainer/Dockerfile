FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# tree-sitter generate needs a C compiler (already in the image, but be explicit)
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    curl \
    xz-utils \
    libudev-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Install Nix package manager (multi-user, daemon mode).
# Nix provides reproducible cross-compilation toolchains (musl, aarch64)
# and any CLI tools needed without polluting the base image.
RUN curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix \
    | sh -s -- install linux --no-confirm --init none \
    && echo ". /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh" >> /etc/bash.bashrc

# Allow the vscode user to access serial devices (/dev/ttyUSB*, /dev/ttyACM*)
# Group varies by host: typically 'dialout' or 'sudo' depending on udev rules.
RUN usermod -aG dialout,sudo vscode

# Add musl cross-compilation targets
RUN rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
