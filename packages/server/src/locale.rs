//! Workspaces need a UTF-8 locale. The image and many hosts start with none (LANG unset or `C`), and then
//! bash's readline prints typed Cyrillic, CJK or emoji as octal bytes (`\320\265`) and tmux draws non-ASCII
//! as underscores. We only step in when the inherited locale is not UTF-8; a UTF-8 one is left alone.

fn is_utf8(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    v.contains("utf-8") || v.contains("utf8")
}

/// Whether the locale given by these variables (LC_ALL wins over LC_CTYPE, which wins over LANG) is not UTF-8.
pub fn needs_utf8(lc_all: Option<&str>, lc_ctype: Option<&str>, lang: Option<&str>) -> bool {
    match [lc_all, lc_ctype, lang]
        .into_iter()
        .flatten()
        .find(|v| !v.is_empty())
    {
        Some(v) => !is_utf8(v),
        None => true,
    }
}

/// Environment to add to every workspace command: empty when the inherited locale already is UTF-8.
pub fn utf8_env() -> Vec<(&'static str, &'static str)> {
    let get = |k: &str| std::env::var(k).ok();
    if needs_utf8(
        get("LC_ALL").as_deref(),
        get("LC_CTYPE").as_deref(),
        get("LANG").as_deref(),
    ) {
        vec![("LANG", "C.UTF-8"), ("LC_ALL", "C.UTF-8")]
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_missing_or_non_utf8_locale_is_replaced() {
        assert!(needs_utf8(None, None, None));
        assert!(needs_utf8(None, None, Some("C")));
        assert!(needs_utf8(None, None, Some("POSIX")));
        assert!(
            needs_utf8(Some("C"), None, Some("en_US.UTF-8")),
            "LC_ALL wins"
        );
        assert!(!needs_utf8(None, None, Some("en_US.UTF-8")));
        assert!(!needs_utf8(None, None, Some("ru_RU.utf8")));
        assert!(
            !needs_utf8(None, Some("C.UTF-8"), Some("C")),
            "LC_CTYPE wins over LANG"
        );
        assert!(
            needs_utf8(Some(""), Some(""), Some("")),
            "empty means unset"
        );
    }
}
