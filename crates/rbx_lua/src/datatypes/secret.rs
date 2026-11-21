//! `Secret`: what `HttpService:GetSecret` hands a script. Its value never
//! reads back (it prints as `Secret`); only HttpService puts it into a
//! request's URL or headers, and only for its domain.

use mlua::{MetaMethod, UserData, UserDataMethods};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LuaSecret {
    pub(crate) value: String,
    /// The hosts it may go to: `*`, a host, or `*.example.com`.
    pub(crate) domain: String,
}

impl LuaSecret {
    /// Whether a request to `host` may carry it.
    pub(crate) fn allows(&self, host: &str) -> bool {
        let domain = self.domain.trim().to_ascii_lowercase();
        match domain.strip_prefix("*.") {
            _ if domain == "*" => true,
            Some(base) => host == base || host.ends_with(&format!(".{base}")),
            None => host == domain,
        }
    }
}

impl UserData for LuaSecret {
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Secret");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("AddPrefix", |_, this, prefix: String| {
            Ok(LuaSecret {
                value: prefix + &this.value,
                domain: this.domain.clone(),
            })
        });
        methods.add_method("AddSuffix", |_, this, suffix: String| {
            Ok(LuaSecret {
                value: this.value.clone() + &suffix,
                domain: this.domain.clone(),
            })
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("Secret"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_goes_only_to_its_domain() {
        let secret = |domain: &str| LuaSecret {
            value: "v".into(),
            domain: domain.into(),
        };
        assert!(secret("*").allows("api.example.com"));
        assert!(secret("*.example.com").allows("api.example.com"));
        assert!(secret("*.example.com").allows("example.com"));
        assert!(!secret("*.example.com").allows("example.org"));
        assert!(secret("api.example.com").allows("api.example.com"));
        assert!(!secret("api.example.com").allows("x.api.example.com"));
    }
}
