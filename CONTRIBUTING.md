# Contributing to Mouse-TestKit

Thank you for your interest in contributing to Mouse-TestKit! This document provides guidelines and information for contributors.

## Getting Started

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/YOUR-USERNAME/Mouse-TestKit.git
   cd Mouse-TestKit
   ```
3. Add the upstream remote:
   ```bash
   git remote add upstream https://github.com/kase1111-hash/Mouse-TestKit.git
   ```
4. Create a branch for your changes:
   ```bash
   git checkout -b feature/your-feature-name
   ```

## Development Setup

### Prerequisites

- Rust 1.70 or later (install via [rustup](https://rustup.rs/))
- Platform-specific dependencies (see [BUILD.md](docs/BUILD.md))

### Building

```bash
# Debug build
cargo build --bin mouse-testkit-gui

# Release build
cargo build --release --bin mouse-testkit-gui
```

### Running Tests

```bash
cargo test
```

### Code Formatting

This project uses `rustfmt` for code formatting:

```bash
# Check formatting
cargo fmt --check

# Apply formatting
cargo fmt
```

### Linting

This project uses `clippy` for linting:

```bash
cargo clippy -- -D warnings
```

## Making Changes

### Commit Messages

- Use clear, descriptive commit messages
- Start with a verb in the imperative mood (e.g., "Add", "Fix", "Update")
- Keep the first line under 72 characters
- Reference issues when applicable (e.g., "Fix #123")

Example:
```
Add jitter test threshold configuration

- Add configurable threshold parameter to jitter test
- Update UI to show threshold slider
- Add documentation for new feature

Fixes #42
```

### Code Style

- Follow Rust idioms and best practices
- Run `cargo fmt` before committing
- Ensure `cargo clippy` passes without warnings
- Add documentation comments for public APIs
- Write tests for new functionality

### Pull Request Process

1. Ensure your code builds and all tests pass
2. Update documentation if needed
3. Create a pull request with a clear description of changes
4. Reference any related issues
5. Wait for review and address feedback

## Types of Contributions

### Bug Reports

When reporting bugs, please include:

- Operating system and version
- Rust version (`rustc --version`)
- Steps to reproduce the issue
- Expected behavior
- Actual behavior
- Any relevant logs or screenshots

### Feature Requests

Feature requests are welcome! Please:

- Check existing issues to avoid duplicates
- Describe the use case for the feature
- Explain how the feature should work
- Consider if you'd be willing to implement it

### Code Contributions

We welcome contributions including:

- Bug fixes
- New test types
- Performance improvements
- Cross-platform compatibility improvements
- Documentation improvements
- UI/UX enhancements

### Documentation

Documentation improvements are always appreciated:

- Fix typos and clarify wording
- Add missing documentation
- Improve examples
- Update outdated information

## Project Structure

```
Mouse-TestKit/
├── src/
│   ├── main.rs           # CLI entry point
│   ├── input.rs          # Cross-platform input handling
│   ├── gui/              # GUI application
│   │   ├── main.rs       # GUI entry point
│   │   ├── app.rs        # Main application state
│   │   └── panels/       # Individual test panels
│   ├── tests/            # Test implementations
│   ├── display/          # Graphing utilities
│   └── usb/              # USB device detection
├── docs/                 # Documentation
└── Cargo.toml            # Project manifest
```

## Questions?

If you have questions about contributing, feel free to:

- Open an issue for discussion
- Ask in pull request comments

## License

By contributing to Mouse-TestKit, you agree that your contributions will be licensed under the MIT License.
