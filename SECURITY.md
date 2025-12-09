# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in aiobscura, please report it by emailing the maintainers directly rather than opening a public issue.

**Please include:**

- A description of the vulnerability
- Steps to reproduce the issue
- Potential impact
- Any suggested fixes (if available)

## Response Timeline

- **Acknowledgment**: Within 48 hours
- **Initial assessment**: Within 1 week
- **Resolution timeline**: Depends on severity, typically within 30 days for critical issues

## Scope

This security policy applies to:

- The `aiobscura-core` library
- The `aiobscura-tui` binary
- The `aiobscura-wrapped` analytics tool

## Known Considerations

aiobscura is designed as a **local, personal tool** that reads log files from AI coding assistants on your machine. It does not:

- Transmit data over the network (except for optional LLM assessment features)
- Store credentials or secrets
- Run as a network service

The primary security consideration is ensuring the tool doesn't inadvertently expose sensitive information from your coding sessions.

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
