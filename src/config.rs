//! Per-workspace settings file (bajan-bku): a strict, minimal
//! `bajan.toml` in the working directory. The format is deliberately a
//! flat subset — `key = <non-negative integer>` lines and `#` comments
//! only. Unknown keys, table headers, quoted values, and anything
//! non-integer are refused with machine-readable errors: a typo must
//! not silently disable tuning, and the strict key set means no
//! secret-bearing key can ever be added (anti-goal: API keys stay
//! env-var-NAME based). Config is never store state — dump-recreate is
//! unaffected.
//!
//! Resolution order (bajan-bku): CLI flag > config file > built-in
//! default. A missing file is byte-identical current behavior.

pub const CONFIG_FILE: &str = "bajan.toml";

/// The workspace tuning surface (bajan-bku). All fields are `None` when
/// unset — handlers fall through to their built-in defaults.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BajanConfig {
    /// `qt_ranking` staleness demotion window in days (was the
    /// hardcoded STALENESS_WINDOW_DAYS 365).
    pub staleness_window_days: Option<i64>,
    /// Default bounded-traversal budget for query/contradict/er/scan/
    /// propose when no CLI flag overrides it (was hardcoded 256).
    pub default_budget: Option<usize>,
    /// LLM extractor retry budget: total attempts per episode per pass
    /// (was hardcoded LLM_DEFAULT_MAX_ATTEMPTS 3).
    pub max_attempts: Option<u32>,
}

/// Machine-readable config errors (bajan-bku): every message names the
/// file and the offending content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "unknown config key '{key}' in bajan.toml (allowed: staleness_window_days, default_budget, max_attempts)"
    )]
    UnknownKey { key: String },
    #[error("malformed line in bajan.toml: '{line}' (expected 'key = <non-negative integer>')")]
    MalformedLine { line: String },
    #[error(
        "invalid value for '{key}' in bajan.toml: '{value}' (a non-negative integer is required)"
    )]
    BadValue { key: String, value: String },
    #[error("cannot read bajan.toml: {message}")]
    Io { message: String },
}

impl BajanConfig {
    /// Parse a config body (pure — the binary wraps `load`). Strict:
    /// unknown keys, table headers, quoted values, negative or
    /// non-integer values are all errors.
    pub fn parse(body: &str) -> Result<Self, ConfigError> {
        let mut cfg = Self::default();
        for raw_line in body.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                return Err(ConfigError::MalformedLine {
                    line: line.to_string(),
                });
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(ConfigError::MalformedLine {
                    line: line.to_string(),
                });
            };
            let key = key.trim().to_string();
            let value = value.trim();
            match key.as_str() {
                "staleness_window_days" => {
                    cfg.staleness_window_days = Some(parse_int::<i64>(value, &key)?);
                }
                "default_budget" => {
                    cfg.default_budget = Some(parse_int::<usize>(value, &key)?);
                }
                "max_attempts" => {
                    cfg.max_attempts =
                        Some(u32::try_from(parse_int::<i128>(value, &key)?).map_err(|_| {
                            ConfigError::BadValue {
                                key: key.clone(),
                                value: value.to_string(),
                            }
                        })?);
                }
                other => {
                    return Err(ConfigError::UnknownKey {
                        key: other.to_string(),
                    });
                }
            }
        }
        Ok(cfg)
    }

    /// Load `bajan.toml` from the working directory. A missing file is
    /// `Ok(None)` — byte-identical defaults; an unreadable present file
    /// is an error (a config that exists but cannot be read must not be
    /// silently ignored).
    pub fn load() -> Result<Option<Self>, ConfigError> {
        match std::fs::read_to_string(CONFIG_FILE) {
            Ok(body) => Ok(Some(Self::parse(&body)?)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(ConfigError::Io {
                message: err.to_string(),
            }),
        }
    }
}

/// Strict integer value: the trimmed text must be a bare non-negative
/// integer — quotes, signs, or anything else are refused.
fn parse_int<T>(value: &str, key: &str) -> Result<T, ConfigError>
where
    T: TryFrom<i128>,
{
    if value.starts_with('"') || value.starts_with('\'') {
        return Err(ConfigError::BadValue {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    match value.parse::<i128>() {
        Ok(n) if n >= 0 => T::try_from(n).map_err(|_| ConfigError::BadValue {
            key: key.to_string(),
            value: value.to_string(),
        }),
        _ => Err(ConfigError::BadValue {
            key: key.to_string(),
            value: value.to_string(),
        }),
    }
}
