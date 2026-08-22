//! User-owned provider configuration, the Rust counterpart of opencode's
//! auth.json: API keys and optional base-URL overrides per provider, stored
//! under the user's profile so every project shares one setup. Env vars stay
//! a fallback (and keep `.env` working for development).

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    pub fn load() -> Self {
        match std::fs::read_to_string(Self::path()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
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
    fn roundtrip_keeps_keys_and_skips_empty_optionals() {
        let mut s = Settings::default();
        s.default_model = "anthropic/claude-sonnet-4-5".into();
        s.providers.insert(
            "anthropic".into(),
            ProviderCfg { api_key: Some("sk-test".into()), base_url: None },
        );
        let text = serde_json::to_string(&s).unwrap();
        assert!(text.contains("sk-test"));
        assert!(!text.contains("base_url"), "unset fields should be omitted");
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.cfg_for("anthropic").unwrap().api_key.as_deref(), Some("sk-test"));
    }
}
