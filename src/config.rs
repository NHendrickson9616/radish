use serde::Deserialize;
use serde_yml::Value;
use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DEFAULT_CONFIG: &str = include_str!("../config.yml");
pub const CONFIG_PATH: &str = "~/.config/radishes/config.yml";

pub type ConfigResult<T> = Result<T, Box<dyn Error>>;

pub trait ConfigApply {
    type CliArgs;

    fn apply_cli(&mut self, args: Self::CliArgs);
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Config {
    pub version: u32,
    pub database: PathBuf,
    pub library: PathBuf,
    pub dry_run: bool,
    pub import: ImportConfig,
    #[serde(flatten)]
    pub plugins: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportConfig {
    pub max_depth: usize,
    pub follow_symlinks: bool,
}

impl ConfigApply for ImportConfig {
    type CliArgs = (Option<usize>, Option<bool>);

    fn apply_cli(&mut self, (max_depth, follow_symlinks): Self::CliArgs) {
        if let Some(value) = max_depth {
            self.max_depth = value;
        }
        if let Some(value) = follow_symlinks {
            self.follow_symlinks = value;
        }
    }
}

impl Config {
    /// Loads compiled defaults, then values from the optional user config.
    pub fn load(user_path: Option<&Path>) -> ConfigResult<Self> {
        let mut values = serde_yml::from_str(DEFAULT_CONFIG)?;

        if let Some(path) = user_path {
            merge(&mut values, read_yaml(path)?);
        }

        let mut config: Self = serde_yml::from_value(values)?;
        config.database = expand_home(config.database)?;
        config.library = expand_home(config.library)?;
        Ok(config)
    }

    pub fn plugin(&self, name: &str) -> Option<&Value> {
        self.plugins.get(name)
    }
}

impl ConfigApply for Config {
    type CliArgs = (Option<PathBuf>, Option<PathBuf>, Option<bool>);

    fn apply_cli(&mut self, (database, library, dry_run): Self::CliArgs) {
        if let Some(value) = database {
            self.database = value;
        }
        if let Some(value) = library {
            self.library = value;
        }
        if let Some(value) = dry_run {
            self.dry_run = value;
        }
    }
}

pub fn config_path() -> ConfigResult<PathBuf> {
    expand_home(PathBuf::from(CONFIG_PATH))
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

fn expand_home(path: PathBuf) -> ConfigResult<PathBuf> {
    let Some(path_text) = path.to_str() else {
        return Ok(path);
    };
    let Some(relative) = path_text.strip_prefix("~/") else {
        return Ok(path);
    };
    let home = env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;

    Ok(PathBuf::from(home).join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_defaults_load_without_a_user_file() {
        let config = Config::load(None).unwrap();

        assert_eq!(config.version, 1);
    }

    #[test]
    fn later_values_replace_earlier_values() {
        let mut values: Value = serde_yml::from_str(
            "database: radish.db\nlibrary: ~/Music\ndry_run: false\nautotag:\n  enabled: false\n",
        )
        .unwrap();
        let user = serde_yml::from_str("dry_run: true\nautotag:\n  enabled: true\n").unwrap();

        merge(&mut values, user);

        assert_eq!(values["database"], "radish.db");
        assert_eq!(values["dry_run"], true);
        assert_eq!(values["autotag"]["enabled"], true);
    }

    #[test]
    fn top_level_plugin_sections_are_collected() {
        let config: Config = serde_yml::from_str(
            "version: 1\ndatabase: radish.db\nlibrary: ~/Music\ndry_run: false\nimport:\n  max_depth: 10\n  follow_symlinks: false\nautotag:\n  enabled: false\n",
        )
        .unwrap();

        assert_eq!(config.database, PathBuf::from("radish.db"));
        assert_eq!(config.library, PathBuf::from("~/Music"));
        assert_eq!(config.import.max_depth, 10);
        assert_eq!(config.plugin("autotag").unwrap()["enabled"], false);
    }

    #[test]
    fn cli_values_change_only_the_supplied_settings() {
        let mut config: Config = serde_yml::from_str(
            "version: 1\ndatabase: radish.db\nlibrary: ~/Music\ndry_run: false\nimport:\n  max_depth: 10\n  follow_symlinks: false\n",
        )
        .unwrap();

        config.apply_cli((None, None, Some(true)));
        config.import.apply_cli((Some(20), None));

        assert_eq!(config.database, PathBuf::from("radish.db"));
        assert_eq!(config.library, PathBuf::from("~/Music"));
        assert!(config.dry_run);
        assert_eq!(config.import.max_depth, 20);
        assert!(!config.import.follow_symlinks);
    }
}
