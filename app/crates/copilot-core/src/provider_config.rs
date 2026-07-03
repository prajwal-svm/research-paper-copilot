//! Provider configuration layer: presets, per-provider settings, tier→model
//! mappings (change: add-zai-glm-provider).
//!
//! The protocol implementations live in [`crate::ai`]; this module holds the
//! *data* — which endpoints exist, which models each routing tier uses, and
//! per-provider timeouts. Presets ship as embedded JSON (`presets.json`) so
//! adding another Anthropic-compatible provider is a registry entry, not
//! code, and model churn is a settings edit, not a release.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ai::{AiError, ModelClass, Provider, ProviderKind};

/// Routing tier → concrete model id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierModels {
    pub strong: String,
    pub balanced: String,
    pub light: String,
}

/// A built-in provider preset (data-driven; see presets.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub protocol: ProviderKind,
    pub base_url: String,
    pub models: TierModels,
    pub request_timeout_ms: u64,
    #[serde(default)]
    pub supports_one_m: bool,
    #[serde(default)]
    pub one_m_suffix: Option<String>,
    #[serde(default)]
    pub docs_url: Option<String>,
}

/// Built-in presets, embedded at compile time.
pub fn presets() -> Vec<ProviderPreset> {
    serde_json::from_str(include_str!("../presets.json"))
        .expect("presets.json is validated by tests")
}

pub fn preset(id: &str) -> Option<ProviderPreset> {
    presets().into_iter().find(|p| p.id == id)
}

/// A configured provider. `id` doubles as the keychain account, so each
/// configured provider has its own key slot (a Z.ai key never mixes with an
/// Anthropic key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub protocol: ProviderKind,
    pub base_url: String,
    pub models: TierModels,
    pub request_timeout_ms: u64,
    #[serde(default)]
    pub one_m_context: bool,
    /// Set when created from a preset; enables revert-to-preset.
    #[serde(default)]
    pub preset_id: Option<String>,
}

impl ProviderConfig {
    /// Built-in default config for one of the first-party provider kinds.
    pub fn default_for_kind(kind: ProviderKind) -> Self {
        ProviderConfig {
            id: kind.id().to_string(),
            name: match kind {
                ProviderKind::Anthropic => "Anthropic",
                ProviderKind::OpenAi => "OpenAI",
                ProviderKind::OpenRouter => "OpenRouter",
                ProviderKind::Ollama => "Ollama (local)",
            }
            .to_string(),
            protocol: kind,
            base_url: kind.default_base_url().to_string(),
            models: TierModels {
                strong: kind.default_model(ModelClass::Strong).to_string(),
                balanced: kind.default_model(ModelClass::Strong).to_string(),
                light: kind.default_model(ModelClass::Light).to_string(),
            },
            request_timeout_ms: 300_000,
            one_m_context: false,
            preset_id: None,
        }
    }

    pub fn from_preset(preset: &ProviderPreset) -> Self {
        ProviderConfig {
            id: preset.id.clone(),
            name: preset.name.clone(),
            protocol: preset.protocol,
            base_url: preset.base_url.clone(),
            models: preset.models.clone(),
            request_timeout_ms: preset.request_timeout_ms,
            one_m_context: false,
            preset_id: Some(preset.id.clone()),
        }
    }

    /// Model id for a routing tier, applying the 1M-context suffix to the
    /// strong tier when enabled (e.g. `glm-5.2` → `glm-5.2[1m]`).
    pub fn model_for(&self, class: ModelClass) -> String {
        let base = match class {
            ModelClass::Strong => &self.models.strong,
            ModelClass::Balanced => &self.models.balanced,
            ModelClass::Light => &self.models.light,
        };
        if self.one_m_context && class == ModelClass::Strong {
            if let Some(suffix) = self
                .preset_id
                .as_deref()
                .and_then(preset)
                .and_then(|p| p.one_m_suffix)
            {
                if !base.ends_with(&suffix) {
                    return format!("{base}{suffix}");
                }
            }
        }
        base.clone()
    }

    /// Approximate prompt-token budget for this config at a given tier.
    /// 1M-context strong tier gets an expanded budget; everything else keeps
    /// the standard one.
    pub fn context_budget_tokens(&self, class: ModelClass) -> usize {
        if self.one_m_context && class == ModelClass::Strong {
            200_000
        } else {
            8_000
        }
    }

    /// The host paper content is sent to — the egress indicator shows this,
    /// never a protocol brand name.
    pub fn host(&self) -> String {
        self.base_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or(&self.base_url)
            .to_string()
    }

    /// True when the base URL differs from the protocol's official default
    /// and isn't a shipped preset (trust styling in the UI).
    pub fn is_custom_url(&self) -> bool {
        self.preset_id.is_none() && self.base_url != self.protocol.default_base_url()
    }

    /// Build a runnable [`Provider`] using this config's key slot.
    #[cfg(feature = "native")]
    pub fn provider(&self, class: ModelClass) -> Result<Provider, AiError> {
        let api_key = crate::ai::load_key_for(&self.id)?;
        if self.protocol.needs_key() && api_key.is_none() {
            return Err(AiError::NoKey(self.id.clone()));
        }
        Ok(Provider::with_base_url(
            self.protocol,
            &self.model_for(class),
            &self.base_url,
            api_key,
        )
        .with_timeout(std::time::Duration::from_millis(self.request_timeout_ms)))
    }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Stores configured providers in `providers.json` under the app config dir.
/// The four first-party kinds are always present (defaults when the file is
/// absent); presets appear once the user adds them.
#[derive(Clone)]
pub struct ProviderStore {
    path: PathBuf,
}

impl ProviderStore {
    pub fn new(config_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(config_dir)?;
        Ok(ProviderStore {
            path: config_dir.join("providers.json"),
        })
    }

    pub fn load(&self) -> Vec<ProviderConfig> {
        let stored: Vec<ProviderConfig> = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        // First-party defaults always exist; stored entries override by id.
        let mut configs: Vec<ProviderConfig> = ProviderKind::ALL
            .into_iter()
            .map(|kind| {
                stored
                    .iter()
                    .find(|c| c.id == kind.id())
                    .cloned()
                    .unwrap_or_else(|| ProviderConfig::default_for_kind(kind))
            })
            .collect();
        // Preset/custom entries follow in stored order.
        for config in stored {
            if !ProviderKind::ALL.iter().any(|k| k.id() == config.id) {
                configs.push(config);
            }
        }
        configs
    }

    pub fn save(&self, configs: &[ProviderConfig]) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(configs).expect("configs serialize");
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &self.path)
    }

    /// Insert or replace one config, persisting the full list.
    pub fn upsert(&self, config: ProviderConfig) -> std::io::Result<Vec<ProviderConfig>> {
        let mut configs = self.load();
        match configs.iter_mut().find(|c| c.id == config.id) {
            Some(slot) => *slot = config,
            None => configs.push(config),
        }
        self.save(&configs)?;
        Ok(configs)
    }

    pub fn remove(&self, id: &str) -> std::io::Result<Vec<ProviderConfig>> {
        let mut configs = self.load();
        configs.retain(|c| c.id != id);
        self.save(&configs)?;
        Ok(self.load()) // reload so first-party defaults reappear
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_parse_and_include_zai() {
        let all = presets();
        let zai = all.iter().find(|p| p.id == "zai-glm").expect("zai preset");
        assert_eq!(zai.protocol, ProviderKind::Anthropic);
        assert_eq!(zai.base_url, "https://api.z.ai/api/anthropic");
        assert_eq!(zai.models.strong, "glm-5.2");
        assert_eq!(zai.models.balanced, "glm-5.2");
        assert_eq!(zai.models.light, "glm-4.7");
        assert_eq!(zai.request_timeout_ms, 300_000);
        // Verified 2026-07-02 against api.z.ai: the raw Anthropic API has no
        // 1M model id ("glm-5.2[1m]" → Unknown Model); the [1m] suffix is a
        // Claude-Code-router convention only. Preset must not offer it.
        assert!(!zai.supports_one_m);
        assert!(zai.one_m_suffix.is_none());
    }

    #[test]
    fn one_m_without_preset_suffix_is_inert() {
        let mut config = ProviderConfig::from_preset(&preset("zai-glm").unwrap());
        assert_eq!(config.model_for(ModelClass::Strong), "glm-5.2");
        // Even with the flag persisted from an older config, no suffix is
        // applied when the preset defines none — the plain id keeps working.
        config.one_m_context = true;
        assert_eq!(config.model_for(ModelClass::Strong), "glm-5.2");
        assert_eq!(config.model_for(ModelClass::Light), "glm-4.7");
        // The expanded budget only makes sense with a real long-context
        // model; it still keys off the flag, which the UI can no longer set.
        assert_eq!(config.context_budget_tokens(ModelClass::Light), 8_000);
    }

    #[test]
    fn host_and_trust_flags() {
        let zai = ProviderConfig::from_preset(&preset("zai-glm").unwrap());
        assert_eq!(zai.host(), "api.z.ai");
        assert!(!zai.is_custom_url(), "presets are not 'custom' URLs");

        let mut anthropic = ProviderConfig::default_for_kind(ProviderKind::Anthropic);
        assert_eq!(anthropic.host(), "api.anthropic.com");
        assert!(!anthropic.is_custom_url());
        anthropic.base_url = "https://proxy.example.com/anthropic".into();
        assert!(anthropic.is_custom_url());
        assert_eq!(anthropic.host(), "proxy.example.com");
    }

    #[test]
    fn store_roundtrip_defaults_and_revert() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProviderStore::new(tmp.path()).unwrap();

        // Defaults present without any file.
        let configs = store.load();
        assert_eq!(configs.len(), 4);
        assert!(configs.iter().any(|c| c.id == "anthropic"));

        // Add the Z.ai preset + edit its mapping.
        let mut zai = ProviderConfig::from_preset(&preset("zai-glm").unwrap());
        zai.models.strong = "glm-6-preview".into();
        let configs = store.upsert(zai).unwrap();
        assert_eq!(configs.len(), 5);

        // Reload persists the edit; revert-to-preset restores defaults.
        let loaded = store.load();
        let stored_zai = loaded.iter().find(|c| c.id == "zai-glm").unwrap();
        assert_eq!(stored_zai.models.strong, "glm-6-preview");
        let reverted =
            ProviderConfig::from_preset(&preset(stored_zai.preset_id.as_deref().unwrap()).unwrap());
        assert_eq!(reverted.models.strong, "glm-5.2");

        // Remove brings back to the 4 defaults.
        let configs = store.remove("zai-glm").unwrap();
        assert_eq!(configs.len(), 4);
    }
}
