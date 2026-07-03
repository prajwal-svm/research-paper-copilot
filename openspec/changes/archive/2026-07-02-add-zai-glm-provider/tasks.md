# Tasks — add-zai-glm-provider

## 1. Provider core

- [x] 1.1 Add configurable base URL to the Anthropic provider (default: official API); include host in validation test-call and egress indicator
- [x] 1.2 Add preset registry (data-driven JSON): name, base URL, tier→model defaults, recommended timeout; ship "Z.ai GLM Coding Plan" entry (strong/balanced `glm-5.2`, light `glm-4.7`, timeout 300000 ms)
- [x] 1.3 Editable tier→model mapping per provider with revert-to-preset; invalid-model errors surfaced plainly
- [x] 1.4 Per-provider request timeout with cancel-anytime streaming UX; 1M-context option applying `[1m]` suffix and expanded context budget

## 2. Settings UI

- [x] 2.1 Provider picker: Z.ai preset flow (base URL pre-filled, key prompt → keychain, validate against Z.ai)
- [x] 2.2 Model-mapping editor + 1M-context toggle + timeout field; custom-URL trust styling (host always visible)

## 3. Docs & verification

- [x] 3.1 `docs/dev/glm-devpack.md` documents the in-app Z.ai provider option (product-only; repo dev tooling stays native Anthropic)
- [x] 3.2 Update README provider table with Z.ai GLM row
- [x] 3.3 Integration test against a mock Anthropic-compatible endpoint (custom base URL, streaming, timeout, bad-protocol failure)
- [x] 3.4 Verify all ai-providers v1 scenarios still pass (no regression on default Anthropic path)
