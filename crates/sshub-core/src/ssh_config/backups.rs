//! config 백업 프루닝 (configBackups.ts 직역). 매 동기화마다
//! `config.bak.<ts>`가 쌓이므로 최신 `max`개만 남긴다. 타임스탬프 접미사는
//! ISO 유래(`:`/`.` → `-`)라 문자열 정렬이 곧 시간순이다.

pub fn backups_to_prune(filenames: &[String], max: usize) -> Vec<String> {
    let mut baks: Vec<String> = filenames
        .iter()
        .filter(|f| f.starts_with("config.bak."))
        .cloned()
        .collect();
    baks.sort();
    let cut = baks.len().saturating_sub(max);
    baks.truncate(cut);
    baks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("config.bak.2024-01-{i:02}")).collect()
    }

    #[test]
    fn keeps_the_newest_max_and_returns_the_oldest_for_deletion() {
        let files = mk(13);
        let del = backups_to_prune(&files, 10);
        assert_eq!(del.len(), 3);
        assert_eq!(del, files[..3].to_vec()); // 오름차순 문자열 정렬 == 시간순
    }

    #[test]
    fn deletes_nothing_when_at_or_under_the_cap() {
        assert!(backups_to_prune(&mk(10), 10).is_empty());
        assert!(backups_to_prune(&mk(4), 10).is_empty());
    }

    #[test]
    fn ignores_files_that_are_not_config_backups() {
        let files: Vec<String> = ["config", "known_hosts", "config.bak.a", "config.bak.b", "id_rsa"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(backups_to_prune(&files, 1), vec!["config.bak.a"]);
    }

    #[test]
    fn is_order_independent_sorts_before_slicing() {
        let files: Vec<String> = ["config.bak.c", "config.bak.a", "config.bak.b"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(backups_to_prune(&files, 1), vec!["config.bak.a", "config.bak.b"]);
    }
}
