# ai-providers (delta)

## ADDED Requirements

### Requirement: Anthropic-compatible custom endpoints
The Anthropic provider SHALL support a configurable base URL so any Anthropic-protocol-compatible endpoint can be used. The default SHALL remain Anthropic's official API; a non-default base URL SHALL be clearly visible in provider settings and in the data-egress indicator (the user must always know where paper content is sent).

#### Scenario: Custom base URL configured
- **WHEN** the user sets the Anthropic provider base URL to a compatible third-party endpoint and validates the key
- **THEN** all Anthropic-protocol requests route to that endpoint, validation performs a test call against it, and the egress indicator names that host instead of "Anthropic"

#### Scenario: Endpoint incompatibility
- **WHEN** a configured custom endpoint returns protocol errors during validation
- **THEN** the user sees a plain-language failure explaining the endpoint did not behave like an Anthropic-compatible API, and no partial configuration is saved

### Requirement: Z.ai GLM Coding Plan preset
Provider settings SHALL include a built-in "Z.ai GLM Coding Plan" preset that pre-fills base URL `https://api.z.ai/api/anthropic`, prompts for a Z.ai API key (stored in the OS keychain like all keys), and applies a default model-tier mapping in which the strong tier ("opus-class") is the latest GLM model (currently `glm-5.2`) and the light tier ("haiku-class") defaults to `glm-4.7`, per https://docs.z.ai/devpack/latest-model. The preset SHALL only offer model IDs the raw API accepts — verified 2026-07-02: the `[1m]` 1M-context suffix documented for Claude Code is rejected by the API ("Unknown Model") and MUST NOT be offered until Z.ai exposes a real long-context model id.

#### Scenario: One-click Z.ai setup
- **WHEN** the user selects the Z.ai preset and pastes a valid Z.ai API key
- **THEN** validation succeeds against the Z.ai endpoint and AI actions route per the preset mapping, with strong-tier actions (derivations, long explanations) using `glm-5.2`

#### Scenario: Stale 1M flag never produces an invalid model id
- **WHEN** a saved configuration still carries an enabled 1M-context flag from an earlier version
- **THEN** requests use the plain preset model id (no `[1m]` suffix) and succeed

### Requirement: In-stream provider errors surface plainly
Providers that report failures inside an HTTP-200 SSE stream (e.g. Z.ai's `event: error`) SHALL have those errors surfaced to the user with the provider's message — a stream that ends with no visible text SHALL never be presented as a successful empty answer.

#### Scenario: Unknown-model error mid-stream
- **WHEN** the endpoint answers 200 and streams an `error` event (e.g. unknown model)
- **THEN** the user sees the provider's error message, and nothing is recorded as an assistant reply

### Requirement: Editable model-tier mapping
For any provider, the mapping from routing tiers (strong / balanced / light) to concrete model IDs SHALL be user-editable, with presets providing defaults, so new models (e.g., a future GLM release) are usable without an app update.

#### Scenario: Newer GLM model released
- **WHEN** Z.ai releases a newer model and the user edits the strong-tier mapping to its ID
- **THEN** subsequent strong-tier requests use it; an invalid model ID surfaces the provider's error with a one-click revert to the preset default

### Requirement: Long-timeout tolerance for reasoning models
Provider requests SHALL support per-provider request timeouts large enough for long-reasoning models (configurable, preset default for Z.ai in line with its guidance), while the UI continues to satisfy the streaming rule — first visible feedback within the existing budget, never a frozen interface while a slow model thinks.

#### Scenario: Slow reasoning response
- **WHEN** a GLM strong-tier request takes minutes on a hard derivation
- **THEN** the request is not killed by a short default timeout, thinking/progress state is shown immediately, and the user can cancel at any time
