# Proposal: add-zai-glm-provider

## Why

Z.ai's GLM Coding Plan exposes an Anthropic-compatible endpoint (`https://api.z.ai/api/anthropic`), making GLM models (currently GLM-5.2 with optional 1M context) a drop-in, lower-cost alternative for users who already hold a Z.ai subscription. Our provider layer already speaks the Anthropic protocol, so supporting Z.ai — and any Anthropic-compatible endpoint — is high leverage: one small capability extension unlocks a whole class of providers.

## What Changes

- The Anthropic provider gains a configurable base URL (default unchanged: Anthropic's API).
- New built-in provider preset **"Z.ai GLM Coding Plan"**: base URL `https://api.z.ai/api/anthropic`, Z.ai API key, default model mapping with the strong/"opus-class" tier set to the latest GLM (currently `glm-5.2`, `glm-5.2[1m]` when 1M context is enabled) and the light/"haiku-class" tier to `glm-4.7`, per https://docs.z.ai/devpack/latest-model.
- Model tier mappings are user-editable so future GLM releases don't require an app update; extended request timeouts supported (Z.ai recommends long timeouts for reasoning models).
- Product doc `docs/dev/glm-devpack.md` describes the in-app provider option.

## Non-goals

- No Z.ai-specific features beyond the Anthropic-compatible surface (no OpenAI-compatible `coding/paas/v4` endpoint in this change).
- No change to routing semantics — Z.ai plugs into the existing per-action tier routing.
- **No developer-tooling integration** (user decision, 2026-07-02): Claude Code used to build this repo stays on native Anthropic; Z.ai/GLM is exclusively an in-app provider option for end users.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `ai-providers`: adds Anthropic-compatible custom endpoints and the Z.ai GLM preset with editable model-tier mapping. (Note: `ai-providers` is still a delta in the unarchived `add-v1-foundation` change; this change's spec is ADDED-only requirements that merge cleanly on archive.)

## Impact

- `app/crates/copilot-core` provider abstraction: base-URL override + preset registry entry.
- Settings UI: provider picker gains the Z.ai preset; model-mapping editor.
- Docs: provider setup guide + new `docs/dev/glm-devpack.md`.
- No format or pipeline impact.
