use crate::paths;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DistillationMode {
    Debug,
    #[serde(alias = "conservative")]
    Conservative,
    #[default]
    Balanced,
    Efficient,
    #[serde(alias = "aggressive")]
    Aggressive,
    Auto,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct AgentConfig {
    #[serde(alias = "aggressiveness")]
    pub mode: Option<DistillationMode>,
    pub enable_readfile_distillation: Option<bool>,
    pub enable_grep_distillation: Option<bool>,
    pub enable_webfetch_distillation: Option<bool>,
    pub pinned_files: Option<Vec<String>>,
}

impl AgentConfig {
    pub fn route_thresholds(&self) -> (f32, f32) {
        let mode = self.mode.clone().unwrap_or(DistillationMode::Balanced);
        match mode {
            DistillationMode::Debug => (0.90, 0.50),
            DistillationMode::Conservative => (0.75, 0.40),
            DistillationMode::Efficient | DistillationMode::Aggressive => (0.60, 0.20),
            DistillationMode::Balanced | DistillationMode::Auto => (0.70, 0.30),
        }
    }

    pub fn readfile_enabled(&self) -> bool {
        self.enable_readfile_distillation.unwrap_or(true)
    }

    pub fn grep_enabled(&self) -> bool {
        self.enable_grep_distillation.unwrap_or(true)
    }

    pub fn webfetch_enabled(&self) -> bool {
        self.enable_webfetch_distillation.unwrap_or(true)
    }

    pub fn pinned_files(&self) -> Vec<String> {
        self.pinned_files.clone().unwrap_or_else(|| {
            vec![
                "AGENTS.md".to_string(),
                "CLAUDE.md".to_string(),
                ".cursorrules".to_string(),
                ".veng/CONTEXT.md".to_string(),
            ]
        })
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct VengConfig {
    pub global: Option<AgentConfig>,
    pub agents: Option<std::collections::HashMap<String, AgentConfig>>,
}

impl VengConfig {
    pub fn for_agent(&self, agent_id: &str) -> AgentConfig {
        // Priority: explicit per-agent config > Hermes defaults > global > generic defaults
        if let Some(agent_cfg) = self.agents.as_ref().and_then(|a| a.get(agent_id)) {
            return agent_cfg.clone();
        }

        // Hermes-optimized defaults when no explicit config exists
        if crate::agents::hermes::is_hermes_agent(agent_id) {
            return self
                .global
                .clone()
                .unwrap_or_else(crate::agents::hermes::hermes_default_config);
        }

        self.global.clone().unwrap_or_default()
    }
}

pub fn load_config() -> VengConfig {
    let path = paths::veng_home().join("config.toml");

    if !path.exists() {
        return VengConfig::default();
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return VengConfig::default(),
    };

    toml::from_str(&content).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_distillation_mode_with_backward_compatibility() {
        // Test new exact key
        let toml_str = r#"
            [global]
            mode = "efficient"
        "#;
        let config: VengConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.global.unwrap().mode,
            Some(DistillationMode::Efficient)
        );

        // Test old alias functionality
        let toml_str2 = r#"
            [global]
            aggressiveness = "conservative"
        "#;
        let config2: VengConfig = toml::from_str(toml_str2).unwrap();
        assert_eq!(
            config2.global.unwrap().mode,
            Some(DistillationMode::Conservative)
        );

        let toml_str3 = r#"
            [global]
            aggressiveness = "aggressive"
        "#;
        let config3: VengConfig = toml::from_str(toml_str3).unwrap();
        assert_eq!(
            config3.global.unwrap().mode,
            Some(DistillationMode::Aggressive)
        );
    }

    #[test]
    fn computes_correct_route_thresholds() {
        let mut cfg = AgentConfig {
            mode: Some(DistillationMode::Debug),
            ..Default::default()
        };
        assert_eq!(cfg.route_thresholds(), (0.90, 0.50));

        cfg.mode = Some(DistillationMode::Aggressive);
        assert_eq!(cfg.route_thresholds(), (0.60, 0.20));

        cfg.mode = Some(DistillationMode::Efficient);
        assert_eq!(cfg.route_thresholds(), (0.60, 0.20));

        cfg.mode = None; // fallback
        assert_eq!(cfg.route_thresholds(), (0.70, 0.30));
    }
}
