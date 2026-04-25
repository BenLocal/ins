use anyhow::{Result, anyhow};

pub const DEFAULT_NAMESPACE: &str = "default";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyRef {
    pub namespace: String,
    pub service: String,
    /// `true` only when the raw entry was `<ns>:<service>` with a non-empty
    /// `<ns>`. Bare `service` and `:service` both produce `false` so the
    /// hybrid env-var keying rule (§5.2 of the spec) can distinguish "user
    /// did not write a namespace" from "user wrote `default:`".
    pub explicit_namespace: bool,
}

pub fn parse_dependency(raw: &str) -> Result<DependencyRef> {
    if raw.is_empty() {
        return Err(anyhow!("invalid dependency '': non-empty value required"));
    }

    let mut parts = raw.splitn(3, ':');
    let first = parts.next().unwrap_or("");
    let second = parts.next();
    let third = parts.next();

    if third.is_some() {
        return Err(anyhow!(
            "invalid dependency '{raw}': at most one ':' separator allowed"
        ));
    }

    let (namespace_raw, service) = match second {
        Some(svc) => (first, svc),
        None => ("", first),
    };

    if service.is_empty() {
        return Err(anyhow!(
            "invalid dependency '{raw}': service name required after ':'"
        ));
    }

    let (namespace, explicit) = if namespace_raw.is_empty() {
        (DEFAULT_NAMESPACE.to_string(), false)
    } else {
        validate_namespace_name(namespace_raw)
            .map_err(|e| anyhow!("invalid dependency '{raw}': {e}"))?;
        (namespace_raw.to_string(), true)
    };

    Ok(DependencyRef {
        namespace,
        service: service.to_string(),
        explicit_namespace: explicit,
    })
}

/// Allowed namespace shape: `^[a-z0-9][a-z0-9_-]{0,63}$`. The constraint
/// flows from the env-var key shape — namespace text gets uppercased and
/// concatenated into `INS_SERVICE_<NS>_<SVC>_*`, so we forbid characters
/// that would either round-trip lossily or produce ambiguous keys.
pub fn validate_namespace_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("namespace name cannot be empty"));
    }
    if name.chars().count() > 64 {
        return Err(anyhow!(
            "namespace name '{name}' exceeds 64-character limit"
        ));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(anyhow!("namespace name '{name}' must start with [a-z0-9]"));
    }
    for ch in chars {
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-') {
            return Err(anyhow!(
                "namespace name '{name}' contains invalid character '{ch}'; \
                 only [a-z0-9_-] allowed after the first character"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "dependency_test.rs"]
mod dependency_test;
