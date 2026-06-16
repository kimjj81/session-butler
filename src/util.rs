//! 공용 유틸리티

use unicode_normalization::UnicodeNormalization;

/// 정수를 천 단위 쉼표로 포맷 (예: 1234567 → "1,234,567").
/// indicatif::HumanCount 를 사용해 일관된 그룹 구분자를 제공한다.
pub fn fmt_int(n: i64) -> String {
    use indicatif::HumanCount;
    if n < 0 {
        format!("-{}", HumanCount((-(n as i128)) as u64))
    } else {
        format!("{}", HumanCount(n as u64))
    }
}

/// 문자열을 NFC(정준 결합형)로 정규화한다.
///
/// macOS(APFS/HFS+)는 파일명/경로를 NFD(분해형)로 저장한다.
/// 한글 등 조합 문자가 분해된 채로 DB/JSON에 저장되면 표시가 깨지고
/// 시스템 간 비교가 어려워지므로, 파일명·경로 출력 시에는 NFC로 통일한다.
/// ASCII에는 영향이 없으므로 경로 전체에 안전하게 적용할 수 있다.
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nfc_korean() {
        // NFD: '가' = U+1100 U+1161 (분해형)
        let nfd = "\u{1100}\u{1161}\u{BC14}";
        let normalized = nfc(nfd);
        // NFC: '가' = U+AC00 (결합형)
        assert_eq!(normalized, "가바");
    }

    #[test]
    fn test_nfc_ascii_unchanged() {
        assert_eq!(nfc("/Users/kim/rollout-2026.jsonl"), "/Users/kim/rollout-2026.jsonl");
    }

    #[test]
    fn test_fmt_int() {
        assert_eq!(fmt_int(0), "0");
        assert_eq!(fmt_int(999), "999");
        assert_eq!(fmt_int(1_000), "1,000");
        assert_eq!(fmt_int(1_234_567), "1,234,567");
        assert_eq!(fmt_int(16_236_291_196), "16,236,291,196");
    }

    #[test]
    fn test_nfc_idempotent() {
        let s = "프로젝트/코드.py";
        assert_eq!(nfc(&nfc(s)), s);
    }
}
