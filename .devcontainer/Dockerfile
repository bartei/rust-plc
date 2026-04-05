FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# tree-sitter generate needs a C compiler (already in the image, but be explicit)
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    && rm -rf /var/lib/apt/lists/*
