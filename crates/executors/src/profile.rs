use std::{
    collections::HashMap,
    fs,
    str::FromStr,
    sync::{LazyLock, RwLock},
};

use convert_case::{Case, Casing};
use serde::{Deserialize, Deserializer, Serialize, de::Error as DeError};
use thiserror::Error;
use ts_rs::TS;

use crate::executors::{
    AvailabilityInfo, BaseCodingAgent, CodingAgent, StandardCodingAgentExecutor,
};

/// Return the canonical form for variant keys.
/// – "DEFAULT" is kept as-is  
/// – everything else is converted to SCREAMING_SNAKE_CASE
pub fn canonical_variant_key<S: AsRef<str>>(raw: S) -> String {
    let key = raw.as_ref();
    if key.eq_ignore_ascii_case("DEFAULT") {
        "DEFAULT".to_string()
    } else {
        // Convert to SCREAMING_SNAKE_CASE by first going to snake_case then uppercase
        key.to_case(Case::Snake).to_case(Case::ScreamingSnake)
    }
}

#[derive(Error, Debug)]
pub enum ProfileError {
    #[error("Built-in executor '{executor}' cannot be deleted")]
    CannotDeleteExecutor { executor: BaseCodingAgent },

    #[error("Built-in configuration '{executor}:{variant}' cannot be deleted")]
    CannotDeleteBuiltInConfig {
        executor: BaseCodingAgent,
        variant: String,
    },

    #[error("Validation error: {0}")]
    Validation(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Serde(#[from] serde_json::Error),

    #[error("No available executor profile")]
    NoAvailableExecutorProfile,
}

static EXECUTOR_PROFILES_CACHE: LazyLock<RwLock<ExecutorConfigs>> =
    LazyLock::new(|| RwLock::new(ExecutorConfigs::load()));

// New format default profiles (v3 - flattened)
const DEFAULT_PROFILES_JSON: &str = include_str!("../default_profiles.json");

// Executor-centric profile identifier
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS, Hash, Eq)]
pub struct ExecutorProfileId {
    /// The executor type (e.g., "CLAUDE_CODE", "AMP")
    #[serde(alias = "profile", deserialize_with = "de_base_coding_agent_kebab")]
    // Backwards compatibility with ProfileVariantIds, esp stored in DB under ExecutorAction
    pub executor: BaseCodingAgent,
    /// Optional variant name (e.g., "PLAN", "ROUTER")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

// Convert legacy profile/executor names from kebab-case to SCREAMING_SNAKE_CASE.
// Kept for backwards compatibility with values stored by older versions (e.g. legacy CURSOR naming).
fn de_base_coding_agent_kebab<'de, D>(de: D) -> Result<BaseCodingAgent, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(de)?;
    // kebab-case -> SCREAMING_SNAKE_CASE
    let norm = raw.replace('-', "_").to_ascii_uppercase();
    BaseCodingAgent::from_str(&norm)
        .map_err(|_| D::Error::custom(format!("unknown executor '{raw}' (normalized to '{norm}')")))
}

impl ExecutorProfileId {
    /// Create a new executor profile ID with default variant
    pub fn new(executor: BaseCodingAgent) -> Self {
        Self {
            executor,
            variant: None,
        }
    }

    /// Create a new executor profile ID with specific variant
    pub fn with_variant(executor: BaseCodingAgent, variant: String) -> Self {
        Self {
            executor,
            variant: Some(variant),
        }
    }

    /// Get cache key for this executor profile
    pub fn cache_key(&self) -> String {
        match &self.variant {
            Some(variant) => format!("{}:{}", self.executor, variant),
            None => self.executor.clone().to_string(),
        }
    }
}

impl std::fmt::Display for ExecutorProfileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.variant {
            Some(variant) => write!(f, "{}:{}", self.executor, variant),
            None => write!(f, "{}", self.executor),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
pub struct ExecutorConfig {
    #[serde(flatten)]
    pub configurations: HashMap<String, CodingAgent>,
}

impl ExecutorConfig {
    /// Get variant configuration by name, or None if not found
    pub fn get_variant(&self, variant: &str) -> Option<&CodingAgent> {
        self.configurations.get(variant)
    }

    /// Get the default configuration for this executor
    pub fn get_default(&self) -> Option<&CodingAgent> {
        self.configurations.get("DEFAULT")
    }

    /// Create a new executor profile with just a default configuration
    pub fn new_with_default(default_config: CodingAgent) -> Self {
        let mut configurations = HashMap::new();
        configurations.insert("DEFAULT".to_string(), default_config);
        Self { configurations }
    }

    /// Add or update a variant configuration
    pub fn set_variant(
        &mut self,
        variant_name: String,
        config: CodingAgent,
    ) -> Result<(), &'static str> {
        let key = canonical_variant_key(&variant_name);
        if key == "DEFAULT" {
            return Err(
                "Cannot override 'DEFAULT' variant using set_variant, use set_default instead",
            );
        }
        self.configurations.insert(key, config);
        Ok(())
    }

    /// Set the default configuration
    pub fn set_default(&mut self, config: CodingAgent) {
        self.configurations.insert("DEFAULT".to_string(), config);
    }

    /// Get all variant names (excluding "DEFAULT")
    pub fn variant_names(&self) -> Vec<&String> {
        self.configurations
            .keys()
            .filter(|k| *k != "DEFAULT")
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
pub struct ExecutorConfigs {
    pub executors: HashMap<BaseCodingAgent, ExecutorConfig>,
}

impl ExecutorConfigs {
    /// Normalise all variant keys in-place
    fn canonicalise(&mut self) {
        for profile in self.executors.values_mut() {
            let mut replacements = Vec::new();
            for key in profile.configurations.keys().cloned().collect::<Vec<_>>() {
                let canon = canonical_variant_key(&key);
                if canon != key {
                    replacements.push((key, canon));
                }
            }
            for (old, new) in replacements {
                if let Some(cfg) = profile.configurations.remove(&old) {
                    // If both lowercase and canonical forms existed, keep canonical one
                    profile.configurations.entry(new).or_insert(cfg);
                }
            }
        }
    }

    /// Get cached executor profiles
    pub fn get_cached() -> ExecutorConfigs {
        EXECUTOR_PROFILES_CACHE.read().unwrap().clone()
    }

    /// Reload executor profiles cache
    pub fn reload() {
        let mut cache = EXECUTOR_PROFILES_CACHE.write().unwrap();
        *cache = Self::load();
    }

    /// Load executor profiles from file or defaults
    pub fn load() -> Self {
        let profiles_path = workspace_utils::assets::profiles_path();

        // Load defaults first
        let mut defaults = Self::from_defaults();
        defaults.canonicalise();

        // Try to load user overrides
        let content = match fs::read_to_string(&profiles_path) {
            Ok(content) => content,
            Err(_) => {
                tracing::info!("No user profiles.json found, using defaults only");
                return defaults;
            }
        };

        // Parse user overrides
        match serde_json::from_str::<Self>(&content) {
            Ok(mut user_overrides) => {
                tracing::info!("Loaded user profile overrides from profiles.json");
                user_overrides.canonicalise();
                Self::merge_with_defaults_validated(defaults, user_overrides)
            }
            Err(e) => {
                tracing::error!(
                    "Failed to parse user profiles.json: {}, using defaults only",
                    e
                );
                defaults
            }
        }
    }

    /// Merge defaults with user overrides, validating the result.
    /// If the merged configuration is invalid (e.g. a hand-edited profiles.json
    /// left an executor without a DEFAULT configuration), log a warning and
    /// fall back to the built-in defaults instead of keeping a config that
    /// would panic or misbehave at query time.
    fn merge_with_defaults_validated(defaults: Self, overrides: Self) -> Self {
        let merged = Self::merge_with_defaults(defaults.clone(), overrides);
        match Self::validate_merged(&merged) {
            Ok(()) => merged,
            Err(e) => {
                tracing::warn!(
                    "Merged executor profiles are invalid ({e}), falling back to built-in defaults"
                );
                defaults
            }
        }
    }

    /// Save user profile overrides to file (only saves what differs from defaults)
    pub fn save_overrides(&self) -> Result<(), ProfileError> {
        let profiles_path = workspace_utils::assets::profiles_path();
        let mut defaults = Self::from_defaults();
        defaults.canonicalise();

        // Canonicalise current config before computing overrides
        let mut self_clone = self.clone();
        self_clone.canonicalise();

        // Compute differences from defaults
        let overrides = Self::compute_overrides(&defaults, &self_clone)?;

        // Validate the merged result would be valid
        let merged = Self::merge_with_defaults(defaults, overrides.clone());
        Self::validate_merged(&merged)?;

        // Write overrides directly to file
        let content = serde_json::to_string_pretty(&overrides)?;
        fs::write(&profiles_path, content)?;

        tracing::info!("Saved profile overrides to {:?}", profiles_path);
        Ok(())
    }

    /// Deep merge defaults with user overrides
    fn merge_with_defaults(mut defaults: Self, overrides: Self) -> Self {
        for (executor_key, override_profile) in overrides.executors {
            match defaults.executors.get_mut(&executor_key) {
                Some(default_profile) => {
                    // Merge configurations (user configs override defaults, new ones are added)
                    for (config_name, config) in override_profile.configurations {
                        default_profile.configurations.insert(config_name, config);
                    }
                }
                None => {
                    // New executor, add completely
                    defaults.executors.insert(executor_key, override_profile);
                }
            }
        }
        defaults
    }

    /// Compute what overrides are needed to transform defaults into current config
    fn compute_overrides(defaults: &Self, current: &Self) -> Result<Self, ProfileError> {
        let mut overrides = Self {
            executors: HashMap::new(),
        };

        // Fast scan for any illegal deletions BEFORE allocating/cloning
        for (executor_key, default_profile) in &defaults.executors {
            // Check if executor was removed entirely
            if !current.executors.contains_key(executor_key) {
                return Err(ProfileError::CannotDeleteExecutor {
                    executor: *executor_key,
                });
            }

            let current_profile = &current.executors[executor_key];

            // Check if ANY built-in configuration was removed
            for config_name in default_profile.configurations.keys() {
                if !current_profile.configurations.contains_key(config_name) {
                    return Err(ProfileError::CannotDeleteBuiltInConfig {
                        executor: *executor_key,
                        variant: config_name.clone(),
                    });
                }
            }
        }

        for (executor_key, current_profile) in &current.executors {
            if let Some(default_profile) = defaults.executors.get(executor_key) {
                let mut override_configurations = HashMap::new();

                // Check each configuration in current profile
                for (config_name, current_config) in &current_profile.configurations {
                    if let Some(default_config) = default_profile.configurations.get(config_name) {
                        // Only include if different from default
                        if current_config != default_config {
                            override_configurations
                                .insert(config_name.clone(), current_config.clone());
                        }
                    } else {
                        // New configuration, always include
                        override_configurations.insert(config_name.clone(), current_config.clone());
                    }
                }

                // Only include executor if there are actual differences
                if !override_configurations.is_empty() {
                    overrides.executors.insert(
                        *executor_key,
                        ExecutorConfig {
                            configurations: override_configurations,
                        },
                    );
                }
            } else {
                // New executor, include completely
                overrides
                    .executors
                    .insert(*executor_key, current_profile.clone());
            }
        }

        Ok(overrides)
    }

    /// Validate that merged profiles are consistent and valid
    fn validate_merged(merged: &Self) -> Result<(), ProfileError> {
        for (executor_key, profile) in &merged.executors {
            // Ensure default configuration exists
            let default_config = profile.configurations.get("DEFAULT").ok_or_else(|| {
                ProfileError::Validation(format!(
                    "Executor '{executor_key}' is missing required 'DEFAULT' configuration"
                ))
            })?;

            // Validate that the default agent type matches the executor key
            if BaseCodingAgent::from(default_config) != *executor_key {
                return Err(ProfileError::Validation(format!(
                    "Executor key '{executor_key}' does not match the agent variant '{default_config}'"
                )));
            }

            // Ensure configuration names don't conflict with reserved words
            for config_name in profile.configurations.keys() {
                if config_name.starts_with("__") {
                    return Err(ProfileError::Validation(format!(
                        "Configuration name '{config_name}' is reserved (starts with '__')"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Load from the new v3 defaults
    pub fn from_defaults() -> Self {
        serde_json::from_str(DEFAULT_PROFILES_JSON).unwrap_or_else(|e| {
            tracing::error!("Failed to parse embedded default_profiles.json: {}", e);
            panic!("Default profiles v3 JSON is invalid")
        })
    }

    pub fn get_coding_agent(&self, executor_profile_id: &ExecutorProfileId) -> Option<CodingAgent> {
        // Normalise the variant key the same way configuration keys are stored,
        // so legacy lowercase/kebab-case variants from the DB still resolve.
        let variant_key = executor_profile_id
            .variant
            .as_deref()
            .map(canonical_variant_key)
            .unwrap_or_else(|| "DEFAULT".to_string());
        self.executors
            .get(&executor_profile_id.executor)
            .and_then(|executor| executor.get_variant(&variant_key))
            .cloned()
    }

    pub fn get_coding_agent_or_default(
        &self,
        executor_profile_id: &ExecutorProfileId,
    ) -> CodingAgent {
        if let Some(agent) = self.get_coding_agent(executor_profile_id) {
            return agent;
        }

        if let Some(variant) = &executor_profile_id.variant {
            tracing::warn!(
                "Variant '{variant}' not found for executor '{}', falling back to DEFAULT",
                executor_profile_id.executor
            );
        }

        let default_id =
            ExecutorProfileId::with_variant(executor_profile_id.executor, "DEFAULT".to_string());
        if let Some(agent) = self.get_coding_agent(&default_id) {
            return agent;
        }

        // The loaded profiles are missing a DEFAULT configuration for this
        // executor (e.g. a hand-edited profiles.json). Recover from the
        // embedded defaults instead of panicking.
        tracing::warn!(
            "No DEFAULT configuration found for executor '{}', falling back to built-in defaults",
            executor_profile_id.executor
        );
        Self::from_defaults()
            .get_coding_agent(&default_id)
            .unwrap_or_else(|| {
                tracing::warn!(
                    "Executor '{}' is not in the built-in defaults, falling back to CLAUDE_CODE",
                    executor_profile_id.executor
                );
                Self::from_defaults()
                    .get_coding_agent(&ExecutorProfileId::new(BaseCodingAgent::ClaudeCode))
                    .expect(
                        "Built-in defaults must always contain a CLAUDE_CODE DEFAULT configuration",
                    )
            })
    }
    pub async fn get_recommended_executor_profile(
        &self,
    ) -> Result<ExecutorProfileId, ProfileError> {
        let mut agents_with_info: Vec<(BaseCodingAgent, AvailabilityInfo)> = Vec::new();

        for &base_agent in self.executors.keys() {
            let profile_id = ExecutorProfileId::new(base_agent);
            if let Some(coding_agent) = self.get_coding_agent(&profile_id) {
                let info = coding_agent.get_availability_info();
                if info.is_available() {
                    agents_with_info.push((base_agent, info));
                }
            }
        }

        if agents_with_info.is_empty() {
            return Err(ProfileError::NoAvailableExecutorProfile);
        }

        agents_with_info.sort_by(|a, b| {
            use crate::executors::AvailabilityInfo;
            (match (&a.1, &b.1) {
                // Both have login detected - compare timestamps (most recent first)
                (
                    AvailabilityInfo::LoginDetected {
                        last_auth_timestamp: time_a,
                    },
                    AvailabilityInfo::LoginDetected {
                        last_auth_timestamp: time_b,
                    },
                ) => time_b.cmp(time_a),
                // LoginDetected > InstallationFound
                (AvailabilityInfo::LoginDetected { .. }, AvailabilityInfo::InstallationFound) => {
                    std::cmp::Ordering::Less
                }
                (AvailabilityInfo::InstallationFound, AvailabilityInfo::LoginDetected { .. }) => {
                    std::cmp::Ordering::Greater
                }
                // LoginDetected > NotFound
                (AvailabilityInfo::LoginDetected { .. }, AvailabilityInfo::NotFound) => {
                    std::cmp::Ordering::Less
                }
                (AvailabilityInfo::NotFound, AvailabilityInfo::LoginDetected { .. }) => {
                    std::cmp::Ordering::Greater
                }
                // InstallationFound > NotFound
                (AvailabilityInfo::InstallationFound, AvailabilityInfo::NotFound) => {
                    std::cmp::Ordering::Less
                }
                (AvailabilityInfo::NotFound, AvailabilityInfo::InstallationFound) => {
                    std::cmp::Ordering::Greater
                }
                // Same state - equal
                _ => std::cmp::Ordering::Equal,
            })
            // Deterministic tie-break: HashMap iteration order is random, so
            // without this equally-available agents would be recommended at random.
            .then_with(|| a.0.to_string().cmp(&b.0.to_string()))
        });

        let selected = agents_with_info[0].0;
        tracing::info!("Recommended executor: {}", selected);
        Ok(ExecutorProfileId::new(selected))
    }
}

pub fn to_default_variant(id: &ExecutorProfileId) -> ExecutorProfileId {
    ExecutorProfileId {
        executor: id.executor,
        variant: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configs_from_json(json: &str) -> ExecutorConfigs {
        serde_json::from_str(json).expect("test config JSON should parse")
    }

    #[test]
    fn get_coding_agent_normalises_variant_key() {
        let configs = configs_from_json(
            r#"{"executors":{"CLAUDE_CODE":{
                "DEFAULT":{"CLAUDE_CODE":{}},
                "PLAN":{"CLAUDE_CODE":{"plan":true}}
            }}}"#,
        );

        // Legacy lowercase / mixed-case variant names must resolve to the
        // canonical SCREAMING_SNAKE_CASE configuration.
        for raw in ["plan", "Plan", "PLAN"] {
            let agent = configs
                .get_coding_agent(&ExecutorProfileId::with_variant(
                    BaseCodingAgent::ClaudeCode,
                    raw.to_string(),
                ))
                .unwrap_or_else(|| panic!("variant '{raw}' should resolve"));
            match agent {
                CodingAgent::ClaudeCode(claude) => {
                    assert_eq!(
                        claude.plan,
                        Some(true),
                        "variant '{raw}' should map to PLAN"
                    )
                }
                other => panic!("expected ClaudeCode config, got {other:?}"),
            }
        }
    }

    #[test]
    fn missing_variant_falls_back_to_default_config() {
        let configs = configs_from_json(
            r#"{"executors":{"CLAUDE_CODE":{
                "DEFAULT":{"CLAUDE_CODE":{"model":"opus"}}
            }}}"#,
        );

        let agent = configs.get_coding_agent_or_default(&ExecutorProfileId::with_variant(
            BaseCodingAgent::ClaudeCode,
            "DOES_NOT_EXIST".to_string(),
        ));
        match agent {
            CodingAgent::ClaudeCode(claude) => assert_eq!(claude.model.as_deref(), Some("opus")),
            other => panic!("expected ClaudeCode DEFAULT config, got {other:?}"),
        }
    }

    #[test]
    fn missing_default_config_recovers_from_builtin_defaults() {
        // A hand-edited profiles.json could leave an executor without a DEFAULT
        // configuration; querying it must not panic.
        let configs = configs_from_json(
            r#"{"executors":{"QWEN_CODE":{
                "FOO":{"QWEN_CODE":{}}
            }}}"#,
        );

        let agent =
            configs.get_coding_agent_or_default(&ExecutorProfileId::new(BaseCodingAgent::QwenCode));
        let expected = ExecutorConfigs::from_defaults()
            .get_coding_agent(&ExecutorProfileId::new(BaseCodingAgent::QwenCode))
            .expect("built-in defaults must contain QWEN_CODE");
        assert_eq!(agent, expected);
    }

    #[test]
    fn invalid_merged_profiles_fall_back_to_defaults() {
        let mut defaults = ExecutorConfigs::from_defaults();
        defaults.canonicalise();

        // Override that replaces CLAUDE_CODE's DEFAULT with a mismatched agent
        // type, which fails validation.
        let invalid_overrides = configs_from_json(
            r#"{"executors":{"CLAUDE_CODE":{
                "DEFAULT":{"AMP":{}}
            }}}"#,
        );

        let merged =
            ExecutorConfigs::merge_with_defaults_validated(defaults.clone(), invalid_overrides);
        assert_eq!(merged, defaults);
    }

    #[test]
    fn valid_overrides_still_merge() {
        let mut defaults = ExecutorConfigs::from_defaults();
        defaults.canonicalise();

        let overrides = configs_from_json(
            r#"{"executors":{"CLAUDE_CODE":{
                "MY_VARIANT":{"CLAUDE_CODE":{"model":"sonnet"}}
            }}}"#,
        );

        let merged = ExecutorConfigs::merge_with_defaults_validated(defaults, overrides);
        let agent = merged
            .get_coding_agent(&ExecutorProfileId::with_variant(
                BaseCodingAgent::ClaudeCode,
                "my-variant".to_string(),
            ))
            .expect("custom variant should be merged in");
        match agent {
            CodingAgent::ClaudeCode(claude) => assert_eq!(claude.model.as_deref(), Some("sonnet")),
            other => panic!("expected ClaudeCode config, got {other:?}"),
        }
    }

    #[test]
    fn builtin_defaults_have_default_config_for_every_executor() {
        // Guards the final fallback in get_coding_agent_or_default.
        let defaults = ExecutorConfigs::from_defaults();
        for (executor, profile) in &defaults.executors {
            assert!(
                profile.get_default().is_some(),
                "built-in defaults must contain a DEFAULT configuration for {executor}"
            );
        }
        assert!(
            defaults
                .get_coding_agent(&ExecutorProfileId::new(BaseCodingAgent::ClaudeCode))
                .is_some()
        );
    }
}
