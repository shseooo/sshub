//! 관리 화면들 (Electron `src/pages/*`의 GPUI 포팅).
//!
//! 각 뷰는 독립 엔티티이고, 화면 전환·접속 요청은 **자기가 처리하지 않고**
//! `ViewEvent`로 올려보낸다 — 라우팅과 PTY 소유는 워크스페이스 몫이라
//! 뷰가 재생성돼도 세션이 살아남아야 하기 때문(DESIGN-ui.md §3).
pub mod dashboard;
pub mod key_manager;
pub mod server_edit;
pub mod server_list;
pub mod settings_page;
pub mod sidebar;

use gpui::App;

use crate::i18n::Lang;
use crate::state::app_state;

/// 라우팅 대상. `ServerEdit { id: None }`은 신규 작성.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Servers,
    ServerEdit { id: Option<i64> },
    Terminal,
    Keys,
    Settings,
}

impl Page {
    /// `settings.start_page` 문자열 ↔ 페이지. 알 수 없는 값은 대시보드로
    /// (구버전 설정 파일이 사라진 페이지 이름을 들고 있어도 앱은 떠야 한다).
    pub fn from_start_page(value: &str) -> Page {
        match value {
            "servers" => Page::Servers,
            "terminal" => Page::Terminal,
            "keys" => Page::Keys,
            "settings" => Page::Settings,
            _ => Page::Dashboard,
        }
    }

    pub fn as_start_page(self) -> &'static str {
        match self {
            Page::Servers | Page::ServerEdit { .. } => "servers",
            Page::Terminal => "terminal",
            Page::Keys => "keys",
            Page::Settings => "settings",
            Page::Dashboard => "dashboard",
        }
    }
}

/// 뷰 → 워크스페이스 상향 이벤트.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewEvent {
    Navigate(Page),
    /// 해당 서버로 접속(새 탭). 서버 id.
    Connect(i64),
}

/// 현재 언어 — 설정에 없으면 시스템 로케일 감지.
pub fn current_lang(cx: &App) -> Lang {
    let state = app_state(cx);
    let settings = &state.read(cx).settings;
    settings
        .language
        .as_deref()
        .and_then(Lang::from_code)
        .unwrap_or_else(Lang::detect)
}

/// 빈 문자열 → `None`. 폼 제출 시 "지우기"와 "안 건드림"을 구분하기 위한 변환.
pub fn blank_to_none(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_page_roundtrips() {
        for page in [Page::Dashboard, Page::Servers, Page::Terminal, Page::Keys, Page::Settings] {
            assert_eq!(Page::from_start_page(page.as_start_page()), page);
        }
    }

    #[test]
    fn unknown_start_page_falls_back_to_dashboard() {
        assert_eq!(Page::from_start_page("/nope"), Page::Dashboard);
        assert_eq!(Page::from_start_page(""), Page::Dashboard);
    }

    #[test]
    fn server_edit_maps_onto_servers_start_page() {
        // 편집 화면은 시작 페이지로 저장될 수 없다 — 목록으로 접는다.
        assert_eq!(Page::ServerEdit { id: Some(3) }.as_start_page(), "servers");
    }

    #[test]
    fn blank_to_none_trims() {
        assert_eq!(blank_to_none("  "), None);
        assert_eq!(blank_to_none(""), None);
        assert_eq!(blank_to_none("  x "), Some("x".to_string()));
    }
}
