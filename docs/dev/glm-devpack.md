# Z.ai GLM as an in-app AI provider

**Scope note (user decision, 2026-07-02):** the Z.ai GLM integration is a **product feature only** — an AI provider option inside Research Paper Copilot. Development of this repo with Claude Code stays on native Anthropic; do not point Claude Code at Z.ai.

## What it is

Z.ai's [GLM Coding Plan](https://docs.z.ai/devpack/overview) exposes an **Anthropic-compatible API** at `https://api.z.ai/api/anthropic`. Since our provider layer already speaks the Anthropic protocol (task 5.1), the app supports it via a configurable base URL plus a built-in preset:

| Setting | Value |
|---|---|
| Base URL | `https://api.z.ai/api/anthropic` |
| Auth | Z.ai API key (stored in OS keychain) |
| Strong tier ("opus-class") | `glm-5.2` — latest GLM per [docs.z.ai/devpack/latest-model](https://docs.z.ai/devpack/latest-model); `glm-5.2[1m]` with the 1M-context option |
| Balanced tier | `glm-5.2` |
| Light tier ("haiku-class") | `glm-4.7` |
| Request timeout | 300 000 ms preset default (reasoning models are slow; streaming + cancel-anytime UX still apply) |

Tier→model mappings are data-driven and user-editable, so a future GLM release is a settings edit, not an app update.

## Where it's specified

Full requirements, design decisions, and tasks: OpenSpec change [`add-zai-glm-provider`](../../openspec/changes/add-zai-glm-provider/proposal.md) — modifies the `ai-providers` capability (Anthropic-compatible custom endpoints, Z.ai preset, editable mapping, long-timeout tolerance, egress-indicator trust rules).
