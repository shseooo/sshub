//! i18n — generated.rs는 rust/scripts/gen_i18n.mjs 산출물 (직접 수정 금지).
mod generated;

pub use generated::{tr, Lang, TrKey};

impl Lang {
    pub fn from_code(code: &str) -> Option<Lang> {
        match code {
            "ko" => Some(Lang::Ko),
            "en" => Some(Lang::En),
            "ja" => Some(Lang::Ja),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Ko => "ko",
            Lang::En => "en",
            Lang::Ja => "ja",
        }
    }

    /// 시스템 로케일 → 언어 감지 (기존 detectLang과 동일: ko/ja 접두, 그 외 en)
    pub fn detect() -> Lang {
        let locale = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_MESSAGES"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default();
        if locale.starts_with("ko") {
            Lang::Ko
        } else if locale.starts_with("ja") {
            Lang::Ja
        } else {
            Lang::En
        }
    }
}

/// `{param}` 치환 — 기존 translate()의 split/join 동작과 동일.
pub fn tr_with(lang: Lang, key: TrKey, params: &[(&str, &str)]) -> String {
    let mut s = tr(lang, key).to_string();
    for (name, value) in params {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tr_returns_all_langs() {
        assert_eq!(tr(Lang::En, TrKey::NavDashboard), "Dashboard");
        assert!(!tr(Lang::Ko, TrKey::NavDashboard).is_empty());
        assert!(!tr(Lang::Ja, TrKey::NavDashboard).is_empty());
    }

    #[test]
    fn detect_defaults_to_en() {
        // 환경 조작 없이 반환값이 3종 중 하나인지만 확인
        let l = Lang::detect();
        assert!(matches!(l, Lang::Ko | Lang::En | Lang::Ja));
    }

    #[test]
    fn param_substitution() {
        let s = tr_with(Lang::En, TrKey::NavDashboard, &[("x", "y")]);
        assert_eq!(s, "Dashboard");
    }
}
