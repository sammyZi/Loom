//! User-owned provider configuration, the Rust counterpart of opencode's
//! auth.json: API keys and optional base-URL overrides per provider, stored
//! under the user's profile so every project shares one setup. Env vars stay
//! a fallback (and keep `.env` working for development).
//!
//! Keys themselves never reach `config.json` — they are held in memory and
//! persisted by [`crate::secrets`], sealed to the current user account.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderCfg>,
    /// Fully qualified default selection, e.g. "deepseek/deepseek-chat".
    #[serde(default)]
    pub default_model: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProviderCfg {
    /// Read from older config files so they can be migrated, never written
    /// back: `skip_serializing` is what keeps keys out of `config.json`.
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

impl Settings {
    /// `%APPDATA%\ide-ai\config.json`, or `$XDG_CONFIG_HOME|~/.config` elsewhere.
    pub fn path() -> PathBuf {
        if let Some(dir) = std::env::var_os("APPDATA").map(PathBuf::from) {
            return dir.join("ide-ai").join("config.json");
        }
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("ide-ai").join("config.json")
    }

    /// A missing file is the normal first-run state, not an error.
    ///
    /// Keys come from the sealed store; a plaintext key left over from an
    /// older build is moved there and scrubbed from `config.json` on the spot.
    pub fn load() -> Self {
        let mut settings: Self = match std::fs::read_to_string(Self::path()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        };

        let stored = crate::secrets::load();
        let mut legacy = false;
        for (id, cfg) in settings.providers.iter_mut() {
            match cfg.api_key.as_deref().map(str::trim) {
                // A key in the JSON predates the sealed store: keep it, and
                // note that the file has to be rewritten without it.
                Some(k) if !k.is_empty() => legacy = true,
                _ => cfg.api_key = stored.get(id).cloned(),
            }
        }
        if legacy {
            if let Err(e) = settings.save() {
                tracing::error!("migrating keys out of config.json failed: {e:#}");
            }
        }
        settings
    }

    /// Every key currently configured, for the sealed store.
    fn keys(&self) -> crate::secrets::Keys {
        self.providers
            .iter()
            .filter_map(|(id, cfg)| {
                let k = cfg.api_key.as_deref()?.trim();
                (!k.is_empty()).then(|| (id.clone(), k.to_string()))
            })
            .collect()
    }

    /// Writes the sealed key store first: if that fails the caller still has
    /// the old keys on disk, rather than a config that has forgotten them.
    pub fn save(&self) -> Result<()> {
        crate::secrets::store(&self.keys())?;
        self.save_config()
    }

    fn save_config(&self) -> Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        // Write-then-rename so a crash mid-write cannot shred existing keys.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, &path).with_context(|| format!("save {}", path.display()))?;
        Ok(())
    }

    pub fn cfg_for(&self, id: &str) -> Option<&ProviderCfg> {
        self.providers.get(id)
    }

    /// Key precedence: config.json first (what the UI writes), then each env
    /// var the provider declares. Never logged.
    pub fn api_key_for(&self, id: &str, env_keys: &[&str]) -> Option<String> {
        if let Some(cfg) = self.providers.get(id) {
            let k = cfg.api_key.as_deref().unwrap_or("").trim();
            if !k.is_empty() {
                return Some(k.to_string());
            }
        }
        for name in env_keys {
            if let Ok(v) = std::env::var(name) {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_file_is_default_state() {
        // Does not touch disk; only proves deserialization tolerance.
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(s.providers.is_empty());
        assert!(s.default_model.is_empty());
    }

    #[test]
    fn config_json_never_carries_a_key() {
        let mut s = Settings::default();
        s.default_model = "anthropic/claude-sonnet-4-5".into();
        s.providers.insert(
            "anthropic".into(),
            ProviderCfg { api_key: Some("sk-test".into()), base_url: None },
        );
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.contains("sk-test"), "keys belong in the sealed store, not config.json");
        assert!(!text.contains("base_url"), "unset fields should be omitted");
        assert!(text.contains("anthropic"), "the provider itself is still recorded");
        // What load() hands to the sealed store.
        assert_eq!(s.keys().get("anthropic").map(String::as_str), Some("sk-test"));
    }

    #[test]
    fn legacy_plaintext_key_is_still_read_so_it_can_be_migrated() {
        let s: Settings =
            serde_json::from_str(r#"{"providers":{"openai":{"api_key":"sk-old"}}}"#).unwrap();
        assert_eq!(s.cfg_for("openai").unwrap().api_key.as_deref(), Some("sk-old"));
    }

    #[test]
    fn blank_keys_are_not_stored() {
        let mut s = Settings::default();
        s.providers.insert(
            "openai".into(),
            ProviderCfg { api_key: Some("   ".into()), base_url: None },
        );
        assert!(s.keys().is_empty(), "whitespace is not a key");
    }
}
