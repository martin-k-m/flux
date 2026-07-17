# Security Policy

Flux is v0.2 software. We take security seriously and appreciate
responsible disclosure.

## Supported versions

Only the latest `0.1.x` release line receives security fixes while the project is
pre-1.0.

| Version | Supported |
| ------- | --------- |
| 0.1.x   | ✅        |
| < 0.1   | ❌        |

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities.**

Instead, report privately via GitHub's
[private vulnerability reporting](https://github.com/martin-k-m/flux/security/advisories/new)
("Report a vulnerability" under the repo's *Security* tab). If that isn't
available, open a minimal issue asking a maintainer to open a private channel —
without disclosing details.

Please include:

- a description of the vulnerability and its impact,
- steps to reproduce (a minimal `.flux` or project if relevant),
- the Flux version (`flux --version`) and your OS.

We aim to acknowledge reports within a few days and will keep you updated as we
investigate and prepare a fix.

## Scope & notes

Flux runs the commands declared in your `.flux` file and shells out to your local
toolchain. Treat `.flux` files from untrusted sources the same way you'd treat any
build script — review before running.

- **Secrets** are encrypted at rest under `.flux-cache/secrets/` (ChaCha20). The
  key lives alongside the ciphertext, so this protects against *casual* exposure,
  not an attacker who already has read access to `.flux-cache/`. See the module
  docs for the exact threat model. Add `.flux-cache/` to `.gitignore` (the
  default `.gitignore` already does).
- Flux itself does not perform vulnerability scanning. You can hand a pipeline
  step off to your own scanner with a `tool <name>` hook (see the `.flux`
  language reference).
