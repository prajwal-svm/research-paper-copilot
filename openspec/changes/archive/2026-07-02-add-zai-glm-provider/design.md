# Design — add-zai-glm-provider

## Context

The v1 provider layer (`copilot-core`) already implements the Anthropic protocol with per-action tier routing and keychain storage. Z.ai's GLM Coding Plan is Anthropic-compatible at `https://api.z.ai/api/anthropic` (docs.z.ai/devpack). Latest model: GLM-5.2, with `[1m]` suffix for 1M context.

## Goals / Non-Goals

**Goals:** custom base URL on the Anthropic provider; preset registry with a Z.ai entry; editable tier→model mapping; long timeouts.

**Non-Goals:** Z.ai's OpenAI-compatible endpoint; effort-level mapping UI (provider-side default is fine for v1 scope); any developer-tooling integration — Claude Code for this repo stays native Anthropic (user decision, 2026-07-02).

## Decisions

1. **Generalize, don't specialize:** implement "Anthropic-compatible endpoint" as the feature; Z.ai is a *preset* (name, base URL, default mapping, recommended timeout). Future compatible providers cost one registry entry. Alternative (hardcoded ZaiProvider) rejected — duplicate protocol code.
2. **Preset defaults, sourced 2026-07 from docs.z.ai/devpack/latest-model:** strong → `glm-5.2` (`glm-5.2[1m]` with 1M option), balanced → `glm-5.2`, light → `glm-4.7`. Stored as data (JSON registry), not code, and user-editable — model churn must not require releases.
3. **Trust boundary:** custom endpoints change where paper content goes. The egress indicator and provider settings always display the actual host; presets are visually distinguished from user-entered URLs.
4. **Timeouts:** per-provider `request_timeout_ms`; Z.ai preset default 300000 (5 min) — generous for reasoning models but bounded; user-configurable. (Z.ai's own Claude Code guidance uses very large timeouts; we keep cancel-anytime UX instead of hour-long silent waits.)
5. **1M context correction (verified against api.z.ai, 2026-07-02):** the `[1m]` model-id suffix from Z.ai's devpack docs is a *Claude Code router* convention — the raw Anthropic-compatible API rejects `glm-5.2[1m]` (and `-1m`/`-long` variants) with "Unknown Model". The preset therefore ships `supports_one_m: false`; a stale `one_m_context` flag in saved configs is inert (no suffix defined → plain id). Related hardening: Z.ai delivers request errors as HTTP 200 + SSE `event: error`, which the client now surfaces as provider errors instead of empty answers.
6. **Auth headers (learned from docs.z.ai/api-reference during implementation):** Z.ai uses HTTP Bearer exclusively — no `x-api-key`. The Anthropic client therefore sends **both** `x-api-key` and `Authorization: Bearer` (official Anthropic reads the former and ignores the latter; compatible endpoints read the latter). Validation of non-default base URLs probes `/v1/messages` with a 1-token request instead of `/v1/models`, which third-party surfaces may not implement.
6. **Product-only scope:** this change touches only the app's provider layer. Repo dev tooling (Claude Code) is untouched and stays on native Anthropic; `docs/dev/glm-devpack.md` documents the in-app provider option only.

## Risks / Trade-offs

- [Compatibility drift on third-party endpoints] → validation test-call on save; protocol errors surfaced plainly; Anthropic default untouched.
- [Model IDs going stale] → editable mapping + registry-as-data; docs link to Z.ai's latest-model page.
- [User confusion about data destination] → egress indicator names the host, not the protocol brand.

## Migration Plan

Additive; no format changes. Existing Anthropic configs keep working (base URL default).

## Open Questions

- Whether to auto-suggest the `[1m]` window only when a chat's assembled context exceeds the standard budget (nice-to-have).
