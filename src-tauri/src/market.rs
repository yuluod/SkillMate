use serde::{Deserialize, Serialize};
use std::time::Duration;

const MARKET_RESULT_LIMIT: usize = 24;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketSkill {
    pub id: String,
    pub source: String,
    pub name: String,
    pub description: String,
    pub repository: String,
    pub skill_id: String,
    pub installs: u64,
    pub stars: u64,
    pub url: String,
    pub install_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketSearchResponse {
    pub source: String,
    pub total: u64,
    pub items: Vec<MarketSkill>,
}

#[derive(Debug, Deserialize)]
struct SkillsShResponse {
    #[serde(default)]
    skills: Vec<SkillsShItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillsShItem {
    id: String,
    skill_id: String,
    name: String,
    #[serde(default)]
    installs: u64,
    source: String,
}

#[derive(Debug, Deserialize)]
struct GithubResponse {
    #[serde(default)]
    total_count: u64,
    #[serde(default)]
    items: Vec<GithubItem>,
}

#[derive(Debug, Deserialize)]
struct GithubItem {
    full_name: String,
    name: String,
    description: Option<String>,
    #[serde(default)]
    stargazers_count: u64,
    html_url: String,
}

pub fn search_market(source: &str, query: &str) -> Result<MarketSearchResponse, String> {
    let query = query.trim();
    if query.chars().count() < 2 {
        return Ok(MarketSearchResponse {
            source: source.to_string(),
            total: 0,
            items: Vec::new(),
        });
    }
    match source {
        "skills-sh" => search_skills_sh(query),
        "github" => search_github(query),
        _ => Err("市场来源仅支持 skills-sh 或 github".to_string()),
    }
}

fn search_skills_sh(query: &str) -> Result<MarketSearchResponse, String> {
    let url = format!("https://skills.sh/api/search?q={}", encode_query(query));
    let response: SkillsShResponse = request_json(&url, false)?;
    let total = response.skills.len() as u64;
    let items = response
        .skills
        .into_iter()
        .take(MARKET_RESULT_LIMIT)
        .map(|item| MarketSkill {
            id: format!("skills-sh:{}", item.id),
            source: "skills-sh".to_string(),
            description: item.name.clone(),
            name: item.name,
            repository: item.source.clone(),
            skill_id: item.skill_id.clone(),
            installs: item.installs,
            stars: 0,
            url: format!("https://skills.sh/{}/{}", item.source, item.skill_id),
            install_source: format!("https://github.com/{}.git", item.source),
        })
        .collect();
    Ok(MarketSearchResponse {
        source: "skills-sh".to_string(),
        total,
        items,
    })
}

fn search_github(query: &str) -> Result<MarketSearchResponse, String> {
    let expression = format!("{} skill in:name,description,readme", query);
    let url = format!(
        "https://api.github.com/search/repositories?q={}&per_page={}",
        encode_query(&expression),
        MARKET_RESULT_LIMIT
    );
    let response: GithubResponse = request_json(&url, true)?;
    let items = response
        .items
        .into_iter()
        .map(|item| MarketSkill {
            id: format!("github:{}", item.full_name),
            source: "github".to_string(),
            name: item.name,
            description: item.description.unwrap_or_default(),
            repository: item.full_name.clone(),
            skill_id: String::new(),
            installs: 0,
            stars: item.stargazers_count,
            url: item.html_url,
            install_source: format!("https://github.com/{}.git", item.full_name),
        })
        .collect();
    Ok(MarketSearchResponse {
        source: "github".to_string(),
        total: response.total_count.min(1_000),
        items,
    })
}

fn request_json<T: for<'de> Deserialize<'de>>(url: &str, github: bool) -> Result<T, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_send_request(Some(Duration::from_secs(8)))
        .timeout_recv_response(Some(Duration::from_secs(12)))
        .timeout_recv_body(Some(Duration::from_secs(12)))
        .build()
        .into();
    let accept = if github {
        "application/vnd.github+json"
    } else {
        "application/json"
    };
    let mut request = agent
        .get(url)
        .header("Accept", accept)
        .header("User-Agent", "SkillMate");
    if github {
        request = request.header("X-GitHub-Api-Version", "2022-11-28");
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.trim().is_empty() {
                request = request.header("Authorization", format!("Bearer {}", token.trim()));
            }
        }
    }
    let mut response = request.call().map_err(|error| match error {
        ureq::Error::StatusCode(403) if github => {
            "GitHub 搜索已达到匿名请求限额，可设置 GITHUB_TOKEN 后重试".to_string()
        }
        ureq::Error::StatusCode(code) => format!("市场请求失败（HTTP {code}）"),
        _ => format!("市场连接失败: {error}"),
    })?;
    response
        .body_mut()
        .read_json::<T>()
        .map_err(|error| format!("市场响应格式无效: {error}"))
}

fn encode_query(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_encoding_is_url_safe() {
        assert_eq!(
            encode_query("pdf skill/中文"),
            "pdf%20skill%2F%E4%B8%AD%E6%96%87"
        );
    }

    #[test]
    fn short_query_does_not_access_network() {
        let result = search_market("skills-sh", "a").unwrap();
        assert!(result.items.is_empty());
        assert_eq!(result.total, 0);
    }

    #[test]
    fn unknown_market_is_rejected() {
        assert!(search_market("unknown", "writer").is_err());
    }
}
