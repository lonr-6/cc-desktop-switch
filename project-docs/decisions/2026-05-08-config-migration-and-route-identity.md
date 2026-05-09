# Decision: Config Migration And Route Identity

Date: 2026-05-08

## Status

Adopted for Rust mainline planning. Implementation still needs tests before release.

## Problem

The Rust rewrite must read existing Python stable-line configs without breaking users. It also needs stable Claude-safe route IDs that do not change unexpectedly when a provider is renamed.

## Proposed Rules

### Config Schema

- Introduce `schemaVersion`.
- Load old configs without `schemaVersion` as Python stable-line config.
- Migrate in memory first, then write a backup before saving the Rust schema.
- Keep full backup import/export available.

### Provider Identity

- Each provider receives a stable `providerId`.
- Display name can change without changing `providerId`.
- Presets can suggest a provider slug, but imported custom providers still get stable IDs.

### Route Identity

Default route format:

```text
claude-<provider-slug>-<model-slug>
```

Examples:

- `claude-deepseek-v4-pro`
- `claude-kimi-k2-6`
- `claude-zhipu-glm-4-7`

If two routes collide, append a short stable suffix from `providerId`.

### Rename Behavior

Provider display-name rename does not change existing route IDs unless the user explicitly regenerates routes.

### Default

`Default` is only a form/config convenience field.

It must not:

- become a Desktop-visible route
- be returned from `/v1/models`
- be used as gateway runtime fallback
- answer unmapped Claude Desktop routes

For gateway requests from Claude Desktop, unmapped routes return 400 with an actionable error.
