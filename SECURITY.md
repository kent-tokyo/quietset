# Security Policy

## Reporting a vulnerability

Please report security vulnerabilities privately using
[GitHub's private vulnerability reporting](https://github.com/kent-tokyo/quietset/security/advisories/new)
("Security" tab → "Report a vulnerability") rather than opening a public issue.

We'll acknowledge reports and aim to provide a fix or mitigation as soon as practical given the
scope of the project.

## Supported versions

quietset is pre-1.0 (currently on the `0.x` series). Only the latest published version on
[crates.io](https://crates.io/crates/quietset) is supported; please upgrade before reporting an
issue that may already be fixed.

## Dependencies

Dependencies are scanned automatically via [Dependabot](.github/dependabot.yml) and
`cargo audit` runs in CI on every push and pull request.
