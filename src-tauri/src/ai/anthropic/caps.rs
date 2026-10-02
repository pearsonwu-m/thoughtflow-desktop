//! Per-model request capabilities.
//!
//! Claude models differ in which request parameters they accept (for example,
//! newer models reject `temperature` and older ones reject `effort`). The
//! model is user-configurable, so the request builder consults this table
//! instead of hard-coding one model's shape. Unknown models get the most
//! conservative request.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Opus,
    Sonnet,
    Haiku,
    Fable,
    Mythos,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCaps {
    /// Accepts `output_config.effort`.
    pub effort: bool,
    /// Accepts sampling parameters such as `temperature`.
    pub sampling: bool,
    /// Accepts `{"role": "system"}` messages inside `messages`.
    pub system_messages: bool,
    /// Supports the server-side `fallbacks: "default"` refusal fallback.
    pub server_fallbacks: bool,
    /// Supports `output_config.format` structured outputs.
    pub structured_outputs: bool,
}

/// Parses ids such as `claude-opus-5-5`, `claude-haiku-4-5-20251001`, or
/// `claude-3-7-sonnet-20250219` into a family and (major, minor) version.
pub fn parse_model(id: &str) -> (Family, (u32, u32)) {
    let id = id.trim().to_ascii_lowercase();
    let rest = id.strip_prefix("claude-").unwrap_or(&id);
    let tokens: Vec<&str> = rest.split(['-', '.', '@']).collect();

    let family_of = |t: &str| match t {
        "opus" => Some(Family::Opus),
        "sonnet" => Some(Family::Sonnet),
        "haiku" => Some(Family::Haiku),
        "fable" => Some(Family::Fable),
        "mythos" => Some(Family::Mythos),
        _ => None,
    };
    // Version components are short numbers; 8-digit snapshot dates are not.
    let version_part = |t: &&&str| t.len() <= 2 && t.chars().all(|c| c.is_ascii_digit());

    let family = tokens
        .iter()
        .find_map(|t| family_of(t))
        .unwrap_or(Family::Unknown);
    let numbers: Vec<u32> = if tokens.first().is_some_and(|t| family_of(t).is_some()) {
        tokens
            .iter()
            .skip(1)
            .take_while(version_part)
            .filter_map(|t| t.parse().ok())
            .collect()
    } else {
        tokens
            .iter()
            .take_while(version_part)
            .filter_map(|t| t.parse().ok())
            .collect()
    };
    let major = numbers.first().copied().unwrap_or(0);
    let minor = numbers.get(1).copied().unwrap_or(0);
    (family, (major, minor))
}

impl ModelCaps {
    pub fn for_model(id: &str) -> ModelCaps {
        let (family, v) = parse_model(id);
        match family {
            Family::Opus => ModelCaps {
                effort: v >= (4, 5),
                sampling: v < (4, 7),
                system_messages: v >= (4, 8),
                server_fallbacks: v >= (5, 0),
                structured_outputs: v >= (4, 8) || v == (4, 5) || v == (4, 1),
            },
            Family::Sonnet => ModelCaps {
                effort: v >= (4, 6),
                sampling: v < (5, 0),
                system_messages: v >= (5, 5),
                server_fallbacks: v >= (5, 5),
                structured_outputs: v >= (5, 0) || v == (4, 5),
            },
            Family::Haiku => ModelCaps {
                effort: false,
                sampling: true,
                system_messages: false,
                server_fallbacks: false,
                structured_outputs: v >= (4, 5),
            },
            Family::Fable => ModelCaps {
                effort: true,
                sampling: false,
                system_messages: true,
                server_fallbacks: v >= (5, 1),
                structured_outputs: true,
            },
            Family::Mythos => ModelCaps {
                effort: true,
                sampling: false,
                system_messages: true,
                server_fallbacks: false,
                structured_outputs: true,
            },
            Family::Unknown => ModelCaps {
                effort: false,
                sampling: false,
                system_messages: false,
                server_fallbacks: false,
                structured_outputs: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_and_legacy_ids() {
        assert_eq!(parse_model("claude-opus-5-5"), (Family::Opus, (5, 5)));
        assert_eq!(parse_model("claude-opus-5"), (Family::Opus, (5, 0)));
        assert_eq!(
            parse_model("claude-haiku-4-5-20251001"),
            (Family::Haiku, (4, 5))
        );
        assert_eq!(
            parse_model("claude-3-7-sonnet-20250219"),
            (Family::Sonnet, (3, 7))
        );
        assert_eq!(
            parse_model("claude-sonnet-4-20250514"),
            (Family::Sonnet, (4, 0))
        );
        assert_eq!(parse_model("something-else"), (Family::Unknown, (0, 0)));
    }

    #[test]
    fn opus_5_5_gets_effort_and_fallbacks_but_no_temperature() {
        let caps = ModelCaps::for_model("claude-opus-5-5");
        assert!(
            caps.effort && caps.system_messages && caps.server_fallbacks && caps.structured_outputs
        );
        assert!(!caps.sampling);
    }

    #[test]
    fn haiku_gets_temperature_but_no_effort() {
        let caps = ModelCaps::for_model("claude-haiku-4-5");
        assert!(caps.sampling && caps.structured_outputs);
        assert!(!caps.effort && !caps.system_messages && !caps.server_fallbacks);
    }

    #[test]
    fn sonnet_5_has_no_mid_conversation_system_messages() {
        assert!(!ModelCaps::for_model("claude-sonnet-5").system_messages);
        assert!(ModelCaps::for_model("claude-sonnet-5-5").system_messages);
    }

    #[test]
    fn unknown_models_get_the_plainest_request() {
        let caps = ModelCaps::for_model("my-proxy-model");
        assert_eq!(
            caps,
            ModelCaps {
                effort: false,
                sampling: false,
                system_messages: false,
                server_fallbacks: false,
                structured_outputs: false
            }
        );
    }
}
