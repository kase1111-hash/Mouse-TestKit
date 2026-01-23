# Security Policy

## Supported Versions

The following versions of Mouse-TestKit are currently supported with security updates:

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue, please report it responsibly.

### How to Report

1. **Do not** open a public GitHub issue for security vulnerabilities
2. Email the maintainers directly or use GitHub's private vulnerability reporting feature
3. Include as much detail as possible:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

### What to Expect

- **Acknowledgment**: We will acknowledge receipt of your report within 48 hours
- **Assessment**: We will investigate and assess the severity within 7 days
- **Updates**: We will keep you informed of our progress
- **Resolution**: We aim to resolve critical vulnerabilities within 30 days
- **Credit**: We will credit you in the release notes (unless you prefer anonymity)

## Security Considerations

### Input Handling

Mouse-TestKit reads input from hardware devices. The application:

- Uses platform-specific APIs for input capture (evdev on Linux, WinAPI on Windows)
- Does not transmit any data over the network
- Does not store sensitive user information
- Processes input data locally only

### Dependencies

We regularly review and update dependencies to address known vulnerabilities. The project uses:

- Rust's memory-safe guarantees
- Well-maintained crates from the Rust ecosystem
- GitHub's Dependabot for automated security updates

### Build Security

- CI/CD pipeline runs security checks via `cargo clippy`
- Release builds are created through GitHub Actions with auditable build logs
- All dependencies are locked via `Cargo.lock` for reproducible builds

## Best Practices for Users

1. **Download from official sources**: Only download binaries from the official GitHub releases page
2. **Verify checksums**: When available, verify release checksums
3. **Keep updated**: Use the latest version to benefit from security fixes
4. **Review permissions**: The application requires input device access - review platform-specific permissions
