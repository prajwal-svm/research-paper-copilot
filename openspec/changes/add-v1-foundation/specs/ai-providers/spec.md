# ai-providers

## ADDED Requirements

### Requirement: Bring-your-own-key and local models
The system SHALL support user-supplied API keys for Anthropic, OpenAI, and OpenRouter, and local models via Ollama, configurable in-app with a guided one-click-style setup. Keys SHALL be stored in the OS keychain, never in bundles or plaintext config, and SHALL never leave the machine except to the chosen provider.

#### Scenario: Key setup
- **WHEN** a user pastes an Anthropic API key
- **THEN** the app validates it with a test call, stores it in the OS keychain, and AI actions become available immediately

#### Scenario: Ollama detection
- **WHEN** Ollama is running locally with a compatible model
- **THEN** the app offers it as a provider without any key

### Requirement: Per-action model routing
AI actions SHALL be routable to different models by cost/latency class (e.g., lightweight model for hover summaries, strongest available for derivations), with sensible defaults and user override per provider.

#### Scenario: Cheap action, cheap model
- **WHEN** a citation hover card needs generation and a lightweight model is configured
- **THEN** the request routes to it rather than the premium model, within the same latency budget

### Requirement: Designed no-key mode
With no provider configured, all non-AI functionality (reading, search, notes, bookmarks, previously generated or bundled enrichment) SHALL work fully; AI entry points SHALL show a friendly explanation and setup path, never a raw error. This mode is a designed experience per docs/ux-principles.md.

#### Scenario: AI action without any provider
- **WHEN** the user clicks Explain with no key and no Ollama
- **THEN** they see what the action would do, why a provider is needed, and a direct link to setup — and the sample paper's pre-generated content remains fully browsable

### Requirement: Privacy boundary
Paper content SHALL be sent only to the provider the user explicitly configured, only when an AI action is invoked; no telemetry SHALL include paper content or user notes. A visible indicator SHALL show when data is being sent to a provider.

#### Scenario: Offline reading is fully private
- **WHEN** the user reads and annotates papers without invoking AI actions
- **THEN** no network requests containing paper or user content are made
