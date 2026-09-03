use std::str::FromStr;

use anyhow::Error;
use executors::{
    executors::BaseCodingAgent,
    profile::{ExecutorProfileId, canonical_variant_key},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utils;
pub use v5::{EditorConfig, EditorType, GitHubConfig, NotificationConfig, SoundFile, ThemeMode};

use crate::services::config::versions::{v4::ProfileVariantLabel, v5};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, TS, Default)]
#[ts(export)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UiLanguage {
    #[default]
    Browser, // Detect from browser
    En,     // Force English
    Fr,     // Force French
    Ja,     // Force Japanese
    Es,     // Force Spanish
    Ko,     // Force Korean
    ZhHans, // Force Simplified Chinese
    ZhHant, // Force Traditional Chinese
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct Config {
    pub config_version: String,
    pub theme: ThemeMode,
    pub executor_profile: ExecutorProfileId,
    pub disclaimer_acknowledged: bool,
    pub onboarding_acknowledged: bool,
    pub github_login_acknowledged: bool,
    pub telemetry_acknowledged: bool,
    pub notifications: NotificationConfig,
    pub editor: EditorConfig,
    pub github: GitHubConfig,
    pub analytics_enabled: Option<bool>,
    pub workspace_dir: Option<String>,
    pub last_app_version: Option<String>,
    pub show_release_notes: bool,
    #[serde(default)]
    pub language: UiLanguage,
}

/// Convert a legacy v5 `ProfileVariantLabel` (kebab-case profile name, e.g.
/// "claude-code") into an `ExecutorProfileId` (SCREAMING_SNAKE_CASE executor
/// plus canonical variant). Unknown executor names fall back to ClaudeCode.
fn executor_profile_from_v5(profile: &ProfileVariantLabel) -> ExecutorProfileId {
    let normalized = profile.profile.replace('-', "_").to_uppercase();
    let base_coding_agent = BaseCodingAgent::from_str(&normalized).unwrap_or_else(|_| {
        tracing::warn!(
            "Unknown executor '{}' in v5 config, falling back to CLAUDE_CODE",
            profile.profile
        );
        BaseCodingAgent::ClaudeCode
    });
    match &profile.variant {
        Some(variant) => {
            ExecutorProfileId::with_variant(base_coding_agent, canonical_variant_key(variant))
        }
        None => ExecutorProfileId::new(base_coding_agent),
    }
}

impl Config {
    pub fn from_previous_version(raw_config: &str) -> Result<Self, Error> {
        let old_config = match serde_json::from_str::<v5::Config>(raw_config) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::error!("❌ Failed to parse config: {}", e);
                tracing::error!("   at line {}, column {}", e.line(), e.column());
                return Err(e.into());
            }
        };

        // Backup custom profiles.json if it exists (v6 migration may break compatibility)
        let profiles_path = utils::assets::profiles_path();
        if profiles_path.exists() {
            let backup_name = format!(
                "profiles_v5_backup_{}.json",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            );
            let backup_path = profiles_path.parent().unwrap().join(backup_name);

            if let Err(e) = std::fs::rename(&profiles_path, &backup_path) {
                tracing::warn!("Failed to backup profiles.json: {}", e);
            } else {
                tracing::info!("Custom profiles.json backed up to {:?}", backup_path);
                tracing::info!("Please review your custom profiles after migration to v6");
            }
        }

        // Validate and convert ProfileVariantLabel
        let executor_profile = executor_profile_from_v5(&old_config.profile);

        Ok(Self {
            config_version: "v6".to_string(),
            theme: old_config.theme,
            executor_profile,
            disclaimer_acknowledged: old_config.disclaimer_acknowledged,
            onboarding_acknowledged: old_config.onboarding_acknowledged,
            github_login_acknowledged: old_config.github_login_acknowledged,
            telemetry_acknowledged: old_config.telemetry_acknowledged,
            notifications: old_config.notifications,
            editor: old_config.editor,
            github: old_config.github,
            analytics_enabled: old_config.analytics_enabled,
            workspace_dir: old_config.workspace_dir,
            last_app_version: old_config.last_app_version,
            show_release_notes: old_config.show_release_notes,
            language: UiLanguage::default(),
        })
    }
}

impl From<String> for Config {
    fn from(raw_config: String) -> Self {
        if let Ok(config) = serde_json::from_str::<Config>(&raw_config)
            && config.config_version == "v6"
        {
            return config;
        }

        match Self::from_previous_version(&raw_config) {
            Ok(config) => {
                tracing::info!("Config upgraded to v6");
                config
            }
            Err(e) => {
                tracing::warn!("Config migration failed: {}, using default", e);
                Self::default()
            }
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: "v6".to_string(),
            theme: ThemeMode::System,
            executor_profile: ExecutorProfileId::new(BaseCodingAgent::ClaudeCode),
            disclaimer_acknowledged: false,
            onboarding_acknowledged: false,
            github_login_acknowledged: false,
            telemetry_acknowledged: false,
            notifications: NotificationConfig::default(),
            editor: EditorConfig::default(),
            github: GitHubConfig::default(),
            analytics_enabled: None,
            workspace_dir: None,
            last_app_version: None,
            show_release_notes: false,
            language: UiLanguage::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_multi_word_kebab_case_profile_name() {
        // "qwen-code".to_uppercase() is "QWEN-CODE", which does not match the
        // SCREAMING_SNAKE_CASE enum representation; the old code silently fell
        // back to ClaudeCode for any multi-word profile name.
        let profile = ProfileVariantLabel::default("qwen-code".to_string());
        let id = executor_profile_from_v5(&profile);
        assert_eq!(id.executor, BaseCodingAgent::QwenCode);
        assert_eq!(id.variant, None);
    }

    #[test]
    fn migrates_single_word_profile_name() {
        let profile = ProfileVariantLabel::default("claude-code".to_string());
        let id = executor_profile_from_v5(&profile);
        assert_eq!(id.executor, BaseCodingAgent::ClaudeCode);
        assert_eq!(id.variant, None);
    }

    #[test]
    fn preserves_variant_during_migration() {
        let profile =
            ProfileVariantLabel::with_variant("claude-code".to_string(), "plan".to_string());
        let id = executor_profile_from_v5(&profile);
        assert_eq!(id.executor, BaseCodingAgent::ClaudeCode);
        assert_eq!(id.variant.as_deref(), Some("PLAN"));
    }

    #[test]
    fn unknown_profile_falls_back_to_claude_code() {
        let profile = ProfileVariantLabel::default("not-a-real-agent".to_string());
        let id = executor_profile_from_v5(&profile);
        assert_eq!(id.executor, BaseCodingAgent::ClaudeCode);
        assert_eq!(id.variant, None);
    }
}
