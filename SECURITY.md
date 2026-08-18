# Security Policy

CC Desktop Switch handles user-supplied provider credentials, writes Claude Desktop configuration, runs an authenticated local gateway, and forwards model traffic to user-selected upstream endpoints. Reports involving credentials, local authorization, configuration import/export, upstream URL handling, headers, installers, updates, or filesystem writes should be treated as security-sensitive.

## Supported Versions

Security fixes are developed against the latest published release and the `main` branch. Before reporting, reproduce the issue on the latest version when doing so is safe and does not risk exposing credentials or third-party data.

## Reporting a Vulnerability

**Do not disclose vulnerability details in a public issue, discussion, pull request, or log attachment.**

1. Use GitHub's private vulnerability reporting flow under **Security → Advisories → Report a vulnerability**.
2. If that button is unavailable, open a public issue titled `[security] private contact requested` and tag `@lonr-6`, but include no technical details, exploit steps, logs, or secrets. A private channel can then be established.

A useful report includes:

- affected version or commit;
- operating system and installation type;
- security impact and required attacker capabilities;
- minimal, sanitized reproduction steps or proof of concept;
- whether credentials, files, network services, or Claude Desktop policy were exposed or modified;
- a proposed remediation, if available.

The maintainer will validate the report, determine affected versions, coordinate a fix and disclosure, and credit the reporter unless anonymity is requested. Public disclosure should wait until a fix or mitigation is available.

## Sensitive Data

Never attach real values for any of the following:

- provider API keys or bearer tokens;
- the local gateway API key;
- complete `config.json` files or backups;
- `Authorization`, `x-api-key`, cookie, or proxy-authentication headers;
- private upstream URLs containing credentials;
- signing keys, CI secrets, or unredacted installer logs.

Use placeholders such as `sk-redacted` and remove unrelated personal or provider data. If a secret was exposed, revoke or rotate it before continuing the report.

## Security-Relevant Areas

Examples of in-scope findings include:

- credentials readable by another local user, leaked in logs, or exposed through exports;
- bypasses of local admin or gateway authentication;
- server-side request forgery or unsafe redirects through custom provider URLs;
- request-header injection, credential forwarding to the wrong host, or unsafe proxy behavior;
- malicious configuration imports, path traversal, unsafe file replacement, or destructive policy writes;
- installer or updater tampering, signature/hash bypasses, or release supply-chain weaknesses;
- dependency or third-party contribution changes that introduce code execution or credential access.

Third-party provider outages, model-quality problems, account billing disputes, and behavior caused solely by a provider changing its API are normally support issues rather than vulnerabilities unless CC Desktop Switch creates an additional security impact.

## Safe Testing

Test only with accounts, systems, and credentials you control. Do not access another user's data, disrupt public services, retain exposed secrets, or use the project to scan unrelated networks. Stop testing once the impact is demonstrated and report the smallest reproducible case.

## Release Integrity

Official release assets may include SHA-256 checksum files, signatures, and the corresponding public key. Verify downloads before installation, especially when obtained through mirrors or third-party redistribution. Report any mismatch through the private process above.
