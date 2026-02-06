# Contributing to aiobscura

Thank you for your interest in contributing to aiobscura! This document provides guidelines and information for contributors.

## Getting Started

### Prerequisites

- Rust toolchain (latest stable) via [rustup](https://rustup.rs/) or [mise](https://mise.jdx.dev/)
- SQLite 3.x
- Git

### Development Setup

```bash
# Clone the repository
git clone https://github.com/kulesh/aiobscura.git
cd aiobscura

# Build the project
cargo build

# Run tests
cargo nextest run  # or `cargo test` if nextest is not installed

# Run the TUI
cargo run -p aiobscura --bin aiobscura

# Run sync (one-shot or watch mode)
cargo run -p aiobscura --bin aiobscura-sync
cargo run -p aiobscura --bin aiobscura-sync -- --watch
```

### Runtime Coordination

- `aiobscura-sync` exits if `aiobscura` is already running for the same database path.
- If `aiobscura-sync` is already running, `aiobscura` starts in read-only mode.

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` to check for common issues
- Follow existing code patterns in the codebase

## Pull Request Process

1. Fork the repository and create a feature branch
2. Make your changes with clear, descriptive commits
3. Ensure all tests pass (`cargo nextest run`)
4. Update documentation if needed
5. Submit a PR with a clear description of the changes

## Reporting Issues

When reporting bugs, please include:

- Your operating system and version
- Rust version (`rustc --version`)
- Steps to reproduce the issue
- Expected vs actual behavior
- Any relevant error messages or logs

## Development Guidelines

### Error Handling

- Core library (`aiobscura-core`): Use custom `Error` enum with `thiserror`
- TUI binary (`aiobscura`): Use `anyhow::Result` for ergonomic error propagation

### Testing

- Add tests for new functionality
- Integration tests go in `tests/`
- Unit tests can be inline with `#[cfg(test)]` modules

### Documentation

- Document public APIs with rustdoc comments
- Update relevant files in `docs/` for architectural changes

## Questions?

Feel free to open an issue for questions or discussions about the project.
