use serde::Deserialize;
use serde_yml::Value;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

pub type ConfigResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub core: CoreConfig,
    #[serde(default)]
    pub plugins: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreConfig {
    pub database: PathBuf,
    pub dry_run: bool,
    pub import: ImportConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportConfig {
    pub max_depth: usize,
    pub follow_symlinks: bool,
    pub analysis: String,
}

/// Only values explicitly provided by the CLI should be `Some`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigOverrides {
    pub database: Option<PathBuf>,
    pub dry_run: Option<bool>,
    pub import_max_depth: Option<usize>,
    pub import_follow_symlinks: Option<bool>,
    pub import_analysis: Option<String>,
    pub plugins: BTreeMap<String, Value>,
}

impl Config {
    /// Loads repository defaults, then local values, then CLI values.
    pub fn load(
        default_path: impl AsRef<Path>,
        local_path: Option<&Path>,
        cli: &ConfigOverrides,
    ) -> ConfigResult<Self> {
        let mut values = read_yaml(default_path.as_ref())?;

        if let Some(path) = local_path {
            merge(&mut values, read_yaml(path)?);
        }

        let mut config: Self = serde_yml::from_value(values)?;
        config.apply(cli);
        Ok(config)
    }

    pub fn plugin(&self, name: &str) -> Option<&Value> {
        self.plugins.get(name)
    }

    fn apply(&mut self, cli: &ConfigOverrides) {
        if let Some(value) = &cli.database {
            self.core.database = value.clone();
        }
        if let Some(value) = cli.dry_run {
            self.core.dry_run = value;
        }
        if let Some(value) = cli.import_max_depth {
            self.core.import.max_depth = value;
        }
        if let Some(value) = cli.import_follow_symlinks {
            self.core.import.follow_symlinks = value;
        }
        if let Some(value) = &cli.import_analysis {
            self.core.import.analysis = value.clone();
        }

        for (name, value) in &cli.plugins {
            match self.plugins.get_mut(name) {
                Some(current) => merge(current, value.clone()),
                None => {
                    self.plugins.insert(name.clone(), value.clone());
                }
            }
        }
    }
}

fn read_yaml(path: &Path) -> ConfigResult<Value> {
    Ok(serde_yml::from_str(&fs::read_to_string(path)?)?)
}

/// Maps merge recursively. Lists and scalar values are replaced.
fn merge(current: &mut Value, new: Value) {
    match (current, new) {
        (Value::Mapping(current), Value::Mapping(new)) => {
            for (key, value) in new {
                match current.get_mut(&key) {
                    Some(current) => merge(current, value),
                    None => {
                        current.insert(key, value);
                    }
                }
            }
        }
        (current, new) => *current = new,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_values_replace_earlier_values() {
        let mut values: Value = serde_yml::from_str(
            "core:\n  database: radish.db\n  dry_run: false\nplugins:\n  tagger:\n    enabled: false\n",
        )
        .unwrap();
        let local =
            serde_yml::from_str("core:\n  dry_run: true\nplugins:\n  tagger:\n    enabled: true\n")
                .unwrap();

        merge(&mut values, local);

        assert_eq!(values["core"]["database"], "radish.db");
        assert_eq!(values["core"]["dry_run"], true);
        assert_eq!(values["plugins"]["tagger"]["enabled"], true);
    }
}
