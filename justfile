# hermes_rs development recipes

# Run all quality gates
gate: fmt clippy test
    @echo "All gates passed."

# Format check
fmt:
    cargo fmt --check

# Clippy lint check
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Run tests
test:
    cargo test --workspace

# Build all crates
build:
    cargo build --workspace

# Format all code (fix, not just check)
fix-fmt:
    cargo fmt

# Run clippy with auto-fix
fix-clippy:
    cargo clippy --workspace --all-targets --fix --allow-dirty -- -D warnings
