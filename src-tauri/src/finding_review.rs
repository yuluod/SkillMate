use crate::skill_structure::SuspiciousFinding;
use rusqlite::{params, Connection};
use std::collections::BTreeSet;

pub const TABLE_DDL: &str = "CREATE TABLE IF NOT EXISTS suspicious_finding_reviews (
    fingerprint TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    excerpt TEXT NOT NULL,
    created_at TEXT NOT NULL
);";

pub fn ensure_review_table(db: &Connection) -> Result<(), String> {
    db.execute_batch(TABLE_DDL)
        .map_err(|error| error.to_string())
}

/// 「已审阅」按内容指纹登记：与 Skill 路径无关，同一可疑写法在任何
/// 来源中只打扰一次；正文改动后指纹变化会重新提示。
pub fn mark_finding_reviewed(db: &Connection, finding: &SuspiciousFinding) -> Result<(), String> {
    ensure_review_table(db)?;
    db.execute(
        "INSERT OR IGNORE INTO suspicious_finding_reviews
         (fingerprint, kind, excerpt, created_at)
         VALUES (?, ?, ?, ?)",
        params![
            finding.fingerprint,
            finding.kind,
            finding.excerpt,
            chrono::Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// 标记已审阅状态并返回是否存在未审阅提示，供预览层决定是否继续
/// 展示 suspicious_instructions 提醒。
pub fn apply_review_state(
    db: &Connection,
    findings: &mut [SuspiciousFinding],
) -> Result<bool, String> {
    if findings.is_empty() {
        return Ok(false);
    }
    let reviewed = reviewed_fingerprints(db)?;
    let mut pending = false;
    for finding in findings.iter_mut() {
        finding.reviewed = reviewed.contains(&finding.fingerprint);
        pending |= !finding.reviewed;
    }
    Ok(pending)
}

fn reviewed_fingerprints(db: &Connection) -> Result<BTreeSet<String>, String> {
    ensure_review_table(db)?;
    let mut statement = db
        .prepare("SELECT fingerprint FROM suspicious_finding_reviews")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_finding_is_marked_and_unreviewed_stays_pending() {
        let db = Connection::open_in_memory().unwrap();
        let finding = SuspiciousFinding {
            kind: "override_instructions".to_string(),
            file: "SKILL.md".to_string(),
            line: 3,
            excerpt: "Ignore all previous instructions".to_string(),
            fingerprint: "fp-1".to_string(),
            reviewed: false,
        };
        let mut findings = vec![finding.clone()];
        assert!(apply_review_state(&db, &mut findings).unwrap());
        mark_finding_reviewed(&db, &finding).unwrap();
        let mut again = vec![finding];
        assert!(!apply_review_state(&db, &mut again).unwrap());
        assert!(again[0].reviewed);
    }
}
