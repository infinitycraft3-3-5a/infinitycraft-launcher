use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::AppState;

const DEFAULT_URL: &str = "http://localhost/wp-json/wp/v2/ic_forum_topic";
const DEFAULT_BOARD: &str = "events";

#[derive(Debug, Clone, Serialize)]
pub struct Announcement {
    pub id: u64,
    pub title: String,
    pub link: String,
    pub date: String,
    pub excerpt: String,
}

#[derive(Default, Deserialize)]
struct RawText {
    #[serde(default)]
    rendered: String,
}

#[derive(Deserialize)]
struct RawPost {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    link: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    title: RawText,
    #[serde(default)]
    excerpt: RawText,
    #[serde(default)]
    meta: HashMap<String, Value>,
}

fn effective_date(p: &RawPost) -> &str {
    p.meta
        .get("_ic_event_date")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(&p.date)
}

fn decode_entities(s: &str) -> String {
    let pairs = [
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#8217;", "\u{2019}"),
        ("&#039;", "'"),
        ("&#8216;", "\u{2018}"),
        ("&#8220;", "\u{201C}"),
        ("&#8221;", "\u{201D}"),
        ("&#8230;", "\u{2026}"),
        ("&#8211;", "\u{2013}"),
        ("&#8212;", "\u{2014}"),
        ("&nbsp;", " "),
    ];
    let mut out = s.to_string();
    for (k, v) in pairs {
        out = out.replace(k, v);
    }
    out
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ => {
                if depth == 0 {
                    out.push(c);
                }
            }
        }
    }
    let cleaned = decode_entities(&out);
    let collapsed: String = {
        let mut s = String::with_capacity(cleaned.len());
        let mut prev_ws = false;
        for c in cleaned.chars() {
            let ws = c.is_whitespace();
            if ws && prev_ws {
                continue;
            }
            prev_ws = ws;
            s.push(c);
        }
        s
    };
    collapsed.trim().to_string()
}

fn resolve_url(state: &AppState) -> String {
    let u = state.settings().announce_url.trim().to_string();
    if u.is_empty() {
        DEFAULT_URL.to_string()
    } else {
        u
    }
}

fn host_of(url: &str) -> String {
    let after = url.split_once("://").map(|x| x.1).unwrap_or(url);
    after
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

fn resolve_board_slug(state: &AppState) -> String {
    let b = state.settings().announce_board.trim().to_string();
    if b.is_empty() {
        DEFAULT_BOARD.to_string()
    } else {
        b
    }
}

fn board_url(base: &str) -> String {
    if let Some(stripped) = base.strip_suffix("ic_forum_topic") {
        format!("{}ic_forum_board", stripped)
    } else if let Some(prefix) = base.split("/wp/v2/").next() {
        format!("{}/wp/v2/ic_forum_board", prefix.trim_end_matches('/'))
    } else {
        format!("{}/wp/v2/ic_forum_board", base.trim_end_matches('/'))
    }
}

async fn resolve_board_id(client: &reqwest::Client, base: &str, slug: &str) -> Option<u64> {
    let url = format!(
        "{}?slug={}&_fields=id",
        board_url(base),
        urlencoding::encode(slug)
    );
    let res = match client.get(&url).header("User-Agent", "InfinityCraft-Launcher").send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            crate::log_line("ANNOUNCE", &format!("board http status: {}", r.status()));
            return None;
        }
        Err(e) => {
            crate::log_line("ANNOUNCE", &format!("board fetch error: {e}"));
            return None;
        }
    };
    #[derive(Deserialize)]
    struct Term {
        id: u64,
    }
    let terms: Vec<Term> = res.json().await.ok()?;
    terms.into_iter().next().map(|t| t.id)
}

#[tauri::command]
pub async fn fetch_announcement(state: State<'_, AppState>) -> Result<Option<Announcement>, String> {
    let base = resolve_url(&state);
    let slug = resolve_board_slug(&state);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let board_id = resolve_board_id(&client, &base, &slug).await;
    let filter = match board_id {
        Some(id) => format!("&ic_forum_board={id}"),
        None => String::new(),
    };
    let url = format!(
        "{}?per_page=50&orderby=date&order=desc&_fields=id,link,date,title,excerpt,meta{}",
        base, filter
    );

    let res = match client
        .get(&url)
        .header("User-Agent", "InfinityCraft-Launcher")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            crate::log_line("ANNOUNCE", &format!("fetch error: {e}"));
            return Ok(None);
        }
    };
    if !res.status().is_success() {
        crate::log_line("ANNOUNCE", &format!("http status: {}", res.status()));
        return Ok(None);
    }

    let posts: Vec<RawPost> = match res.json().await {
        Ok(p) => p,
        Err(e) => {
            crate::log_line("ANNOUNCE", &format!("parse error: {e}"));
            return Ok(None);
        }
    };

    Ok(posts
        .into_iter()
        .max_by(|a, b| (effective_date(a), a.id).cmp(&(effective_date(b), b.id)))
        .map(|p| {
            let date = effective_date(&p).to_string();
            Announcement {
                id: p.id,
                title: strip_html(&p.title.rendered),
                link: p.link,
                date,
                excerpt: strip_html(&p.excerpt.rendered),
            }
        }))
}

#[tauri::command]
pub fn open_announcement(state: State<'_, AppState>, url: String) -> Result<(), String> {
    let u = url.trim().to_string();
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return Err("URL invalide".into());
    }
    let want = host_of(&resolve_url(&state));
    let got = host_of(&u);
    if !want.is_empty() && got != want {
        return Err(format!("hote non autorise: {got}"));
    }
    open::that(&u).map_err(|e| format!("open failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_removes_tags_and_decodes() {
        let raw = "<p>Hello &#8217;World&#8217; &amp; co&#8230;</p>";
        assert_eq!(strip_html(raw), "Hello \u{2019}World\u{2019} & co\u{2026}");
    }

    #[test]
    fn board_url_derivation() {
        assert_eq!(
            board_url("http://localhost/wp-json/wp/v2/ic_forum_topic"),
            "http://localhost/wp-json/wp/v2/ic_forum_board"
        );
        assert_eq!(
            board_url("https://exemple.com/blog/wp-json/wp/v2/ic_forum_topic"),
            "https://exemple.com/blog/wp-json/wp/v2/ic_forum_board"
        );
    }

    #[test]
    fn host_of_extracts_host() {
        assert_eq!(host_of("http://localhost/wp-json/x"), "localhost");
        assert_eq!(host_of("https://site.org:8443/a"), "site.org");
    }

    #[tokio::test]
    async fn live_wp_roundtrip() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .unwrap();
        let base = "http://localhost/wp-json/wp/v2/ic_forum_topic";
        let id = match resolve_board_id(&client, base, "events").await {
            Some(id) => id,
            None => return,
        };
        let url = format!(
            "{}?per_page=50&orderby=date&order=desc&_fields=id,link,date,title,excerpt,meta&ic_forum_board={id}",
            base
        );
        let res = client
            .get(&url)
            .header("User-Agent", "test")
            .send()
            .await
            .expect("request failed");
        assert!(res.status().is_success());
        let posts: Vec<RawPost> = res.json().await.expect("json parse");
        assert!(!posts.is_empty());
        let chosen = posts
            .iter()
            .max_by(|a, b| (effective_date(a), a.id).cmp(&(effective_date(b), b.id)))
            .unwrap();
        for p in &posts {
            assert!((effective_date(chosen), chosen.id) >= (effective_date(p), p.id));
        }
        assert!(!chosen.link.is_empty());
        if let Some(d) = posts[0].meta.get("_ic_event_date").and_then(|v| v.as_str()) {
            assert!(!d.is_empty());
        }
    }
}
