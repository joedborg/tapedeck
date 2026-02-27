/// Wrapper around the `get_iplayer` CLI.
///
/// Spawns subprocesses using Tokio and parses the stdout output for
/// progress information. Supports both TV and radio programmes.
use anyhow::{Context, bail};
use regex::Regex;
use tokio::{io::AsyncReadExt, process::Command, sync::mpsc as tmpsc};

use crate::models::SearchResult;

// ── Progress parsing ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ProgressUpdate {
    pub percent: f64,
    pub speed: Option<String>,
    pub eta: Option<String>,
    #[allow(dead_code)]
    pub size: Option<String>,
}

fn parse_progress_line(line: &str) -> Option<ProgressUpdate> {
    // ── Format 1: get_iplayer HLS progress line ────────────────────────────
    // Actual format (observed):
    //   5.4% of ~2442.31 MB @  97.8 Mb/s ETA: 00:03:09 (hlshd1/cf) [audio+video]
    // Older/alternate formats:
    //   23.4%  12.34 MiB  3.45 MiB/s  ETA 00:00:15
    static RE_PERCENT: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"(\d+\.?\d*)%").unwrap());
    // Speed: matches "97.8 Mb/s", "3.45 MiB/s", "12.3 KB/s", etc.
    static RE_SPEED_BW: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"([\d.]+\s*(?:[KMGT]i?[Bb])/s)").unwrap());
    // ETA: matches both "ETA: 00:03:09" and "ETA 00:00:15"
    static RE_ETA: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"ETA:?\s+([\d:]+)").unwrap());
    // Size: matches "~2442.31 MB", "12.34 MiB", "512 KB", etc.
    static RE_SIZE_BW: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"~?([\d.]+\s*(?:[KMGT]i?[Bb]))\b").unwrap());

    if let Some(pct) = RE_PERCENT.captures(line) {
        if let Ok(percent) = pct[1].parse::<f64>() {
            return Some(ProgressUpdate {
                percent,
                speed: RE_SPEED_BW.captures(line).map(|c| c[1].to_string()),
                eta: RE_ETA.captures(line).map(|c| c[1].to_string()),
                size: RE_SIZE_BW.captures(line).map(|c| c[1].to_string()),
            });
        }
    }

    // ── Format 2: ffmpeg stats line (DASH downloads) ──────────────────────
    //   frame=  123 fps= 25 q=28.0 size=    512kB time=00:00:12.00 bitrate= 350kbps speed=1.2x
    static RE_FFMPEG: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"frame=\s*\d+.*?size=\s*(\d+)kB\s+time=([\d:\.]+).*?speed=\s*([\d.Na/]+)x?")
            .unwrap()
    });

    if let Some(caps) = RE_FFMPEG.captures(line) {
        let size_kb: f64 = caps[1].parse().unwrap_or(0.0);
        let size_str = if size_kb >= 1024.0 * 1024.0 {
            format!("{:.2} GB", size_kb / 1_048_576.0)
        } else if size_kb >= 1024.0 {
            format!("{:.1} MB", size_kb / 1024.0)
        } else {
            format!("{:.0} kB", size_kb)
        };
        let elapsed = caps[2].to_string();
        let speed = caps[3].to_string();
        return Some(ProgressUpdate {
            percent: 0.0,
            speed: Some(format!("{speed}x")),
            eta: Some(elapsed),
            size: Some(size_str),
        });
    }

    None
}

// ── Download ───────────────────────────────────────────────────────────────────

pub struct DownloadOptions<'a> {
    pub pid: &'a str,
    pub media_type: &'a str, // "tv" or "radio"
    pub quality: &'a str,
    pub subtitles: bool,
    pub output_dir: &'a str,
    pub get_iplayer_path: &'a str,
    pub ffmpeg_path: &'a str,
    pub cache_dir: &'a str,
    pub proxy: Option<&'a str>,
}

/// Runs `get_iplayer` to download a single PID. Calls `on_progress` with each
/// progress update parsed from stdout/stderr.
pub async fn download<F>(opts: DownloadOptions<'_>, mut on_progress: F) -> anyhow::Result<String>
where
    F: FnMut(ProgressUpdate) + Send,
{
    let mut cmd = Command::new(opts.get_iplayer_path);

    cmd.arg("--profile-dir")
        .arg(opts.cache_dir)
        .arg("--pid")
        .arg(opts.pid)
        .arg("--output")
        .arg(opts.output_dir)
        .arg("--ffmpeg")
        .arg(opts.ffmpeg_path)
        .arg("--force")
        .arg("--overwrite")
        .arg("--nocopyright")
        // Force progress output even when stdout is not a TTY (piped).
        // Without this flag, get_iplayer suppresses all progress display.
        .arg("--log-progress");

    match opts.media_type {
        "radio" => {
            cmd.arg("--type").arg("radio");
        }
        _ => {
            cmd.arg("--type").arg("tv");
        }
    }

    let quality_flag = if opts.media_type == "radio" {
        "--radio-quality"
    } else {
        "--tv-quality"
    };
    // Map friendly quality names to get_iplayer's accepted values.
    // TV valid values:    fhd, hd, sd, web, mobile, 1080p, 720p, 540p, 396p, 288p, default
    // Radio valid values: high, standard, low
    // Comma-separated lists are tried in order so a missing quality level falls back gracefully
    // rather than aborting with "No specified recording quality available".
    let quality_val = if opts.media_type == "radio" {
        match opts.quality {
            "best" => "high,standard,low",
            "good" => "standard,low",
            "worst" => "low",
            other => other,
        }
    } else {
        match opts.quality {
            "best" => "fhd,hd,sd,web,mobile",
            "good" => "hd,sd,web,mobile",
            "worst" => "mobile,web",
            other => other,
        }
    };
    cmd.arg(quality_flag).arg(quality_val);

    if opts.subtitles {
        cmd.arg("--subtitles");
    }

    if let Some(proxy) = opts.proxy {
        if !proxy.is_empty() {
            cmd.arg("--proxy").arg(proxy);
        }
    }

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().context("spawn get_iplayer")?;

    // get_iplayer writes everything (INFO lines, progress lines) to stdout.
    // Progress lines use \r (not \n) for in-place updates, so BufReader::lines()
    // misses them entirely.  Use the same raw-byte splitter for both stdout and
    // stderr so we never miss a \r-terminated progress tick.
    fn spawn_line_reader(
        reader: impl tokio::io::AsyncRead + Send + Unpin + 'static,
    ) -> tmpsc::UnboundedReceiver<String> {
        let (tx, rx) = tmpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            let mut reader = reader;
            let mut chunk = vec![0u8; 4096];
            let mut buf: Vec<u8> = Vec::with_capacity(256);
            loop {
                match reader.read(&mut chunk).await {
                    Ok(0) => break,
                    Ok(n) => {
                        for &b in &chunk[..n] {
                            if b == b'\r' || b == b'\n' {
                                if !buf.is_empty() {
                                    let s = String::from_utf8_lossy(&buf).trim().to_string();
                                    if !s.is_empty() {
                                        let _ = tx.send(s);
                                    }
                                    buf.clear();
                                }
                            } else {
                                buf.push(b);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            if !buf.is_empty() {
                let s = String::from_utf8_lossy(&buf).trim().to_string();
                if !s.is_empty() {
                    let _ = tx.send(s);
                }
            }
        });
        rx
    }

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let mut stdout_rx = spawn_line_reader(stdout);
    let mut stderr_rx = spawn_line_reader(stderr);

    let mut output_path = String::new();
    let mut stderr_buf: Vec<String> = Vec::new();
    let mut stderr_done = false;
    let mut stdout_done = false;

    loop {
        if stderr_done && stdout_done {
            break;
        }
        tokio::select! {
            msg = stdout_rx.recv(), if !stdout_done => {
                match msg {
                    Some(l) => {
                        if let Some(progress) = parse_progress_line(&l) {
                            tracing::info!(
                                "[get_iplayer] progress: {:.1}% speed={} eta={}",
                                progress.percent,
                                progress.speed.as_deref().unwrap_or("-"),
                                progress.eta.as_deref().unwrap_or("-"),
                            );
                            on_progress(progress);
                        } else {
                            tracing::info!("[get_iplayer] {l}");
                            // get_iplayer prints e.g. "INFO: Recorded /downloads/Episode.mp4"
                            if let Some(path) = extract_output_path(&l) {
                                output_path = path;
                            }
                            stderr_buf.push(l);
                            if stderr_buf.len() > 50 {
                                stderr_buf.remove(0);
                            }
                        }
                    }
                    None => { stdout_done = true; }
                }
            }
            msg = stderr_rx.recv(), if !stderr_done => {
                match msg {
                    Some(l) => {
                        if let Some(progress) = parse_progress_line(&l) {
                            tracing::info!(
                                "[get_iplayer] progress: {:.1}% speed={} eta={}",
                                progress.percent,
                                progress.speed.as_deref().unwrap_or("-"),
                                progress.eta.as_deref().unwrap_or("-"),
                            );
                            on_progress(progress);
                        } else {
                            tracing::info!("[get_iplayer stderr] {l}");
                            stderr_buf.push(l);
                            if stderr_buf.len() > 50 {
                                stderr_buf.remove(0);
                            }
                        }
                    }
                    None => { stderr_done = true; }
                }
            }
        }
    }

    let status = child.wait().await.context("wait for get_iplayer")?;
    if !status.success() {
        let detail = stderr_buf
            .iter()
            .filter(|l| {
                // Surface only lines that look like real errors/warnings.
                // The licence advisory is harmless and should not appear as a failure reason.
                let u = l.to_uppercase();
                if u.contains("UK TV LICENCE") || u.contains("TV LICENCE IS REQUIRED") {
                    return false;
                }
                u.contains("ERROR")
                    || u.contains("WARNING")
                    || u.contains("FAILED")
                    || u.contains("ABORT")
            })
            .cloned()
            .collect::<Vec<_>>();
        let detail_str = if detail.is_empty() {
            // Fall back to last 5 lines of stderr so there's always something useful
            stderr_buf
                .iter()
                .rev()
                .take(5)
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            detail.join("\n")
        };
        bail!(
            "get_iplayer exited with status {} for PID {}\n{}",
            status.code().unwrap_or(-1),
            opts.pid,
            detail_str,
        );
    }

    Ok(output_path)
}

fn extract_output_path(line: &str) -> Option<String> {
    static RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"(?:INFO:|Recorded)\s+(.+\.(?:mp4|m4v|mp3|m4a|aac|ts))").unwrap()
    });
    RE.captures(line).map(|c| c[1].trim().to_string())
}

// ── Search ────────────────────────────────────────────────────────────────────

pub struct SearchOptions<'a> {
    pub query: &'a str,
    pub media_type: &'a str,
    pub get_iplayer_path: &'a str,
    pub cache_dir: &'a str,
    pub proxy: Option<&'a str>,
}

pub struct EpisodesOptions<'a> {
    pub pid: &'a str,
    pub media_type: &'a str,
    pub get_iplayer_path: &'a str,
    pub cache_dir: &'a str,
    pub proxy: Option<&'a str>,
}

/// Enumerate all episodes for a brand/series PID using get_iplayer's
/// `--pid-recursive --pid-recursive-list` mode.  get_iplayer scrapes the BBC
/// programmes website and prints one line per episode to stderr:
///   `<name> - <episode>, <channel>, <pid>`
pub async fn list_episodes(opts: EpisodesOptions<'_>) -> anyhow::Result<Vec<SearchResult>> {
    let mut cmd = Command::new(opts.get_iplayer_path);
    cmd.arg("--profile-dir")
        .arg(opts.cache_dir)
        .arg("--type")
        .arg(opts.media_type)
        .arg("--pid")
        .arg(opts.pid)
        .arg("--pid-recursive")
        .arg("--pid-recursive-list")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    if let Some(p) = opts.proxy {
        if !p.is_empty() {
            cmd.arg("--proxy").arg(p);
        }
    }

    let out = cmd
        .output()
        .await
        .context("spawn get_iplayer --pid-recursive-list")?;
    parse_pid_recursive_output(&out.stdout, &out.stderr, opts.media_type)
}

/// Parse the fixed-format output from `--pid-recursive-list`.
///
/// get_iplayer prints each episode to stderr as:
///   `<name> - <episode>, <channel>, <pid>`
/// We extract PID (always last, 8 alphanum chars) and the two preceding
/// comma-separated tokens as channel and "name - episode".
fn parse_pid_recursive_output(
    stdout: &[u8],
    stderr: &[u8],
    media_type: &str,
) -> anyhow::Result<Vec<SearchResult>> {
    // Regex: greedily captures name+episode, then channel, then the 8-char BBC PID at end.
    static RE_LINE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"^(.+),\s+([^,]+),\s+([bpm][0-9a-z]{7})\s*$").unwrap()
    });

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );

    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in combined.lines() {
        // Skip header / info lines
        if line.is_empty()
            || line.starts_with("INFO:")
            || line.starts_with("WARNING:")
            || line.starts_with("ERROR:")
            || line.starts_with("Episodes:")
            || line.starts_with("get_iplayer")
        {
            continue;
        }

        let Some(caps) = RE_LINE.captures(line) else {
            continue;
        };

        let pid = caps[3].trim().to_string();
        if seen.contains(&pid) {
            continue;
        }
        seen.insert(pid.clone());

        let name_episode = caps[1].trim();
        let channel = caps[2].trim().to_string();

        // BBC / get_iplayer uses several formats:
        //   "Show Title: Series N - Episode Title"   (most common)
        //   "Show Title - Series N - Episode Title"  (some shows, second dash)
        //   "Show Title - Episode Title"             (specials / no-series episodes)
        //
        // We try the colon split first; if there is no ": " we fall back to " - ".
        static RE_SER: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
            Regex::new(r"^((?:Series|Season)\s+\d+)\s*[-:]\s*(.+)$").unwrap()
        });

        let (title, series, episode) = if let Some(colon_idx) = name_episode.find(": ") {
            let show = name_episode[..colon_idx].trim().to_string();
            let rest = name_episode[colon_idx + 2..].trim(); // after ": "

            // rest is now "Series N - Episode Title" or just "Episode Title"
            if let Some(sc) = RE_SER.captures(rest) {
                (
                    show,
                    Some(sc[1].trim().to_string()),
                    Some(sc[2].trim().to_string()),
                )
            } else {
                // No recognisable series label – whole rest is the episode title
                (show, None, Some(rest.to_string()))
            }
        } else if let Some(dash_idx) = name_episode.find(" - ") {
            // "Show Title - …"
            let show = name_episode[..dash_idx].trim().to_string();
            let rest = name_episode[dash_idx + 3..].trim(); // after " - "

            // rest may be "Series N - Episode Title" (second dash) or just "Episode Title"
            if let Some(sc) = RE_SER.captures(rest) {
                (
                    show,
                    Some(sc[1].trim().to_string()),
                    Some(sc[2].trim().to_string()),
                )
            } else {
                (show, None, Some(rest.to_string()))
            }
        } else {
            (name_episode.to_string(), None, None)
        };

        tracing::debug!(
            "pid-recursive line: raw={name_episode:?} → title={title:?} series={series:?} episode={episode:?}"
        );

        results.push(SearchResult {
            pid,
            title,
            series,
            episode,
            channel: Some(channel),
            media_type: media_type.to_string(),
            ..Default::default()
        });
    }

    Ok(results)
}

/// BBC PIDs are 8 chars: one letter (usually b or p) followed by 7 lowercase alphanumerics.
fn extract_pid(input: &str) -> Option<String> {
    static RE_PID: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"(?:/|^)([bpm][0-9a-z]{7})(?:[/?#]|$)").unwrap());
    RE_PID.captures(input).map(|c| c[1].to_string())
}

// ── BBC iPlayer / Sounds search page scraper ─────────────────────────────────

/// Strip HTML tags, collapsing whitespace.
fn strip_html_tags(s: &str) -> String {
    static RE_TAG: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
    RE_TAG
        .replace_all(s, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extract a map of programme PID → thumbnail URL from the
/// `window.__IPLAYER_REDUX_STATE__` JSON blob embedded in BBC iPlayer HTML.
///
/// The BBC search page embeds all result data as a Redux state object.
/// Image URLs inside it use `{recipe}` as a size placeholder which we replace
/// with `480x270` (a reliable 16:9 thumbnail size).
fn extract_redux_image_map(html: &str) -> std::collections::HashMap<String, String> {
    const PREFIX: &str = "window.__IPLAYER_REDUX_STATE__ =";
    let start = match html.find(PREFIX) {
        Some(i) => i + PREFIX.len(),
        None => return Default::default(),
    };
    let tail = html[start..].trim_start();

    // Walk the raw bytes counting braces to extract just the top-level JSON object
    let mut depth = 0i32;
    let mut end = 0;
    let bytes = tail.as_bytes();
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        match b {
            b'\\' if in_string => escape = true,
            b'"' => in_string = !in_string,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if end == 0 {
        return Default::default();
    }

    let state: serde_json::Value = match serde_json::from_str(&tail[..end]) {
        Ok(v) => v,
        Err(_) => return Default::default(),
    };

    let mut map = std::collections::HashMap::new();
    collect_redux_images(&state, &mut map);
    map
}

/// Recursively walk a JSON value, collecting every object that has both an
/// `"id"` string field and an `"images"` object field into `map`.
fn collect_redux_images(
    v: &serde_json::Value,
    map: &mut std::collections::HashMap<String, String>,
) {
    match v {
        serde_json::Value::Object(obj) => {
            if let (Some(id_val), Some(images_val)) = (obj.get("id"), obj.get("images")) {
                if let (Some(pid), Some(imgs)) = (id_val.as_str(), images_val.as_object()) {
                    // Prefer "standard"; fall back to any image field
                    let url = imgs
                        .get("standard")
                        .or_else(|| imgs.get("promotional_with_logo"))
                        .or_else(|| imgs.values().next())
                        .and_then(|u| u.as_str());
                    if let Some(raw) = url {
                        map.insert(pid.to_string(), raw.replace("{recipe}", "480x270"));
                    }
                }
            }
            for val in obj.values() {
                collect_redux_images(val, map);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_redux_images(item, map);
            }
        }
        _ => {}
    }
}

/// Parse the BBC iPlayer (or Sounds) search results page HTML.
///
/// Extracts PID, title, description and duration from the anchor tags that
/// link to episode/series pages.
fn parse_bbc_search_html(html: &str, media_type: &str) -> Vec<SearchResult> {
    // ── Regexes for anchor tags containing iPlayer / Sounds links ──────────
    static RE_TV: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(
            r#"(?s)<a\s[^>]*href="(?:https://www\.bbc\.co\.uk)?/iplayer/episodes?/([a-z0-9]{8,})[^"]*"[^>]*>(.*?)</a>"#,
        )
        .unwrap()
    });
    static RE_RADIO: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(
            r#"(?s)<a\s[^>]*href="(?:https://www\.bbc\.co\.uk)?/sounds/(?:series|play)/([a-z0-9]{8,})[^"]*"[^>]*>(.*?)</a>"#,
        )
        .unwrap()
    });
    static RE_DURATION: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"Duration:\s*([^.<]+)").unwrap());

    // Build pid → thumbnail URL from the embedded Redux state blob
    let image_map = extract_redux_image_map(html);

    let re = if media_type == "radio" {
        &*RE_RADIO
    } else {
        &*RE_TV
    };

    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for cap in re.captures_iter(html) {
        let pid = cap[1].to_string();
        if seen.contains(&pid) {
            continue;
        }
        seen.insert(pid.clone());

        let inner = &cap[2];
        let text = strip_html_tags(inner);

        // Extract optional duration
        let duration = RE_DURATION.captures(&text).map(|d| d[1].trim().to_string());

        // Remove duration token from the text before splitting title/desc
        let clean = RE_DURATION.replace(&text, "").to_string();
        let clean = clean.replace(" . ", ". ");
        let clean = clean.trim().to_string();

        // Split "Title. Description text." or "Title Description: …"
        let (title, description) = if let Some(idx) = clean.find(" Description: ") {
            let (t, d) = clean.split_at(idx);
            (
                t.trim_end_matches('.').trim().to_string(),
                Some(d.trim_start_matches(" Description: ").trim().to_string()),
            )
        } else if let Some(idx) = clean.find(". ") {
            let (t, d) = clean.split_at(idx + 2);
            (
                t.trim_end_matches('.').trim().to_string(),
                Some(d.trim().to_string()),
            )
        } else {
            (clean.trim().to_string(), None)
        };

        if title.is_empty() {
            continue;
        }

        let thumbnail_url = image_map.get(&pid).cloned();

        results.push(SearchResult {
            pid,
            title,
            episode: None,
            series: None,
            channel: None,
            thumbnail_url,
            duration,
            description,
            media_type: media_type.to_string(),
            ..Default::default()
        });
    }

    results
}

/// Scrape the BBC iPlayer or BBC Sounds search results page.
///
/// BBC iPlayer search (`/iplayer/search?q=…`) is server-side rendered, so a
/// plain HTTP GET returns fully-populated HTML with the complete catalogue —
/// not just the 30-day schedule window in the local get_iplayer cache.
async fn bbc_web_search(
    query: &str,
    media_type: &str,
    proxy: Option<&str>,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        // Use a real browser UA — the BBC returns a cookie wall for bots
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/124.0.0.0 Safari/537.36",
        );

    if let Some(p) = proxy {
        if !p.is_empty() {
            builder = builder.proxy(reqwest::Proxy::all(p)?);
        }
    }

    let client = builder.build()?;

    // BBC Sounds for radio; iPlayer for TV
    let search_url = if media_type == "radio" {
        "https://www.bbc.co.uk/sounds/search"
    } else {
        "https://www.bbc.co.uk/iplayer/search"
    };

    let html = client
        .get(search_url)
        .query(&[("q", query)])
        .send()
        .await
        .context("BBC search HTTP request")?
        .text()
        .await
        .context("BBC search response body")?;

    tracing::debug!("BBC {} search HTML: {} chars", search_url, html.len());

    let results = parse_bbc_search_html(&html, media_type);
    tracing::debug!(
        "BBC web search returned {} results for {:?}",
        results.len(),
        query
    );
    Ok(results)
}

/// Runs search. For PID/URL queries, uses the BBC Programmes API first (fast, no local cache).
/// For text queries calls the BBC iPlayer search page directly — full history, no local cache needed.
pub async fn search(opts: SearchOptions<'_>) -> anyhow::Result<Vec<SearchResult>> {
    // PID or BBC URL → try BBC Programmes API first (instant, no TTY/cache required)
    if let Some(pid) = extract_pid(opts.query) {
        tracing::info!("Detected PID {pid}, looking up via BBC Programmes API");
        match lookup_pid_api(&pid, opts.media_type, opts.proxy).await {
            Ok(results) if !results.is_empty() => {
                tracing::info!(
                    "BBC Programmes API returned {} result(s) for PID {pid}",
                    results.len()
                );
                return Ok(results);
            }
            Ok(_empty) => {
                // Series/brand PID — fall through to list_episodes below
                tracing::info!("PID {pid} is a series/brand, listing episodes via get_iplayer");
            }
            Err(e) => {
                tracing::warn!(
                    "BBC Programmes API failed for PID {pid} ({e:#}), falling back to get_iplayer --pid"
                );
                // Try get_iplayer local cache as a secondary fallback
                let results = search_by_pid(
                    &pid,
                    opts.media_type,
                    opts.get_iplayer_path,
                    opts.cache_dir,
                    opts.proxy,
                )
                .await?;
                if !results.is_empty() {
                    return Ok(results);
                }
            }
        }

        // Series/brand PID (or API + cache both missed) — list all episodes.
        // Cap at 90 s so a slow series doesn't hang the UI indefinitely.
        tracing::info!(
            "PID {pid} returned 0 episode results, trying --pid-recursive-list (90 s timeout)"
        );
        let episode_opts = EpisodesOptions {
            pid: &pid,
            media_type: opts.media_type,
            get_iplayer_path: opts.get_iplayer_path,
            cache_dir: opts.cache_dir,
            proxy: opts.proxy,
        };
        // Look up the series label (e.g. "Series 12") in parallel with listing episodes
        let series_label_fut = get_series_label(&pid, opts.proxy);
        let list_fut = tokio::time::timeout(
            std::time::Duration::from_secs(90),
            list_episodes(episode_opts),
        );

        let (series_info, list_result) = tokio::join!(series_label_fut, list_fut);
        let mut episodes = list_result.unwrap_or_else(|_| {
            tracing::warn!("list_episodes for PID {pid} timed out after 90 s");
            Ok(vec![])
        })?;

        // Stamp series label and thumbnail onto every episode that lacks them
        let SeriesInfo {
            label: series_label,
            thumbnail_url: series_thumb,
        } = series_info;
        if series_label.is_some() || series_thumb.is_some() {
            tracing::debug!(
                "Stamping series info (label={series_label:?}) onto {} episode(s)",
                episodes.len()
            );
            for ep in &mut episodes {
                if ep.series.is_none() {
                    ep.series = series_label.clone();
                }
                if ep.thumbnail_url.is_none() {
                    ep.thumbnail_url = series_thumb.clone();
                }
            }
        }

        return Ok(episodes);
    }

    // Text query → scrape BBC iPlayer / Sounds search page (full catalogue),
    // fall back to local get_iplayer cache on error or empty results.
    match bbc_web_search(opts.query, opts.media_type, opts.proxy).await {
        Ok(results) if !results.is_empty() => {
            tracing::debug!(
                "BBC web search returned {} results for {:?}",
                results.len(),
                opts.query
            );
            Ok(results)
        }
        Ok(_empty) => {
            tracing::warn!(
                "BBC web search returned 0 results for {:?}, falling back to local cache",
                opts.query
            );
            search_local_cache(opts).await
        }
        Err(e) => {
            tracing::warn!(
                "BBC web search failed ({e:#}), falling back to local get_iplayer cache"
            );
            search_local_cache(opts).await
        }
    }
}

struct SeriesInfo {
    label: Option<String>,
    thumbnail_url: Option<String>,
}

/// Fetch label + thumbnail for a series-type PID from the BBC Programmes API.
/// Returns `SeriesInfo` with `None` fields if the PID is not a `series` or on any error.
async fn get_series_label(pid: &str, proxy: Option<&str>) -> SeriesInfo {
    let info = async {
        let mut builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/124.0.0.0 Safari/537.36",
            );
        if let Some(p) = proxy {
            if !p.is_empty() {
                builder = builder.proxy(reqwest::Proxy::all(p).ok()?);
            }
        }
        let client = builder.build().ok()?;
        let url = format!("https://www.bbc.co.uk/programmes/{pid}.json");
        let json: serde_json::Value = client.get(&url).send().await.ok()?.json().await.ok()?;
        let prog = &json["programme"];
        if prog["type"].as_str() != Some("series") {
            return None;
        }
        // Prefer explicit title like "Series 12"; fall back to constructing from position
        let label = prog["title"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| prog["position"].as_u64().map(|n| format!("Series {n}")));
        // Use the series image; if absent, walk up to the parent brand image
        let image_pid = prog["image"]["pid"]
            .as_str()
            .or_else(|| prog["parent"]["programme"]["image"]["pid"].as_str());
        let thumbnail_url =
            image_pid.map(|ip| format!("https://ichef.bbci.co.uk/images/ic/640x360/{ip}.jpg"));
        Some((label, thumbnail_url))
    }
    .await;
    match info {
        Some((label, thumbnail_url)) => SeriesInfo {
            label,
            thumbnail_url,
        },
        None => SeriesInfo {
            label: None,
            thumbnail_url: None,
        },
    }
}

/// Look up a single PID via the BBC Programmes JSON API.
///
/// This is the fastest way to resolve a known PID — no local cache required,
/// returns in < 1 s for episode PIDs.  Returns an empty vec for series/brand
/// PIDs so the caller can fall through to `list_episodes`.
async fn lookup_pid_api(
    pid: &str,
    media_type: &str,
    proxy: Option<&str>,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/124.0.0.0 Safari/537.36",
        );
    if let Some(p) = proxy {
        if !p.is_empty() {
            builder = builder.proxy(reqwest::Proxy::all(p)?);
        }
    }
    let client = builder.build()?;

    let url = format!("https://www.bbc.co.uk/programmes/{pid}.json");
    let resp = client
        .get(&url)
        .send()
        .await
        .context("BBC Programmes API request")?;

    if !resp.status().is_success() {
        anyhow::bail!("BBC Programmes API returned HTTP {}", resp.status());
    }

    let json: serde_json::Value = resp.json().await.context("BBC Programmes API JSON parse")?;

    let prog = &json["programme"];
    let prog_type = prog["type"].as_str().unwrap_or("");

    // Only return a result for episode/clip PIDs; series/brand need list_episodes
    if prog_type != "episode" && prog_type != "clip" {
        tracing::debug!("PID {pid} has type '{prog_type}', deferring to list_episodes");
        return Ok(vec![]);
    }

    let title = prog["display_title"]["title"]
        .as_str()
        .or_else(|| prog["title"].as_str())
        .unwrap_or("")
        .to_string();

    let episode = prog["display_title"]["subtitle"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let description = prog["short_synopsis"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let channel = prog["ownership"]["service"]["title"]
        .as_str()
        .map(|s| s.to_string());

    // Duration is in seconds; convert to H:MM:SS / M:SS string
    let duration = prog["duration"].as_u64().map(|d| {
        let h = d / 3600;
        let m = (d % 3600) / 60;
        let s = d % 60;
        if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m}:{s:02}")
        }
    });

    // BBC image URL pattern
    let thumbnail_url = prog["image"]["pid"]
        .as_str()
        .map(|ip| format!("https://ichef.bbci.co.uk/images/ic/640x360/{ip}.jpg"));

    // Use parent series position as the series number
    let series = prog["parent"]["programme"]["position"]
        .as_u64()
        .map(|n| n.to_string());

    Ok(vec![SearchResult {
        pid: pid.to_string(),
        title,
        episode,
        series,
        channel,
        thumbnail_url,
        duration,
        description,
        media_type: media_type.to_string(),
        ..Default::default()
    }])
}

async fn search_by_pid(
    pid: &str,
    media_type: &str,
    get_iplayer_path: &str,
    cache_dir: &str,
    proxy: Option<&str>,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut cmd = Command::new(get_iplayer_path);
    cmd.arg("--profile-dir")
        .arg(cache_dir)
        .arg("--listformat")
        .arg("<pid>|<name>|<episode>|<seriesnum>|<channel>|<thumbnail>|<duration>|<desc>")
        .arg("--type")
        .arg(media_type)
        .arg("--pid")
        .arg(pid)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(p) = proxy {
        if !p.is_empty() {
            cmd.arg("--proxy").arg(p);
        }
    }
    let out = cmd.output().await.context("spawn get_iplayer --pid")?;
    parse_get_iplayer_output(&out.stdout, &out.stderr, media_type)
}

async fn search_local_cache(opts: SearchOptions<'_>) -> anyhow::Result<Vec<SearchResult>> {
    let mut cmd = Command::new(opts.get_iplayer_path);
    cmd.arg("--profile-dir")
        .arg(opts.cache_dir)
        .arg("--listformat")
        .arg("<pid>|<name>|<episode>|<seriesnum>|<channel>|<thumbnail>|<duration>|<desc>")
        .arg("--type")
        .arg(opts.media_type)
        // Also search episode names and descriptions, not just programme titles
        .arg("--long")
        .arg(opts.query)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(p) = opts.proxy {
        if !p.is_empty() {
            cmd.arg("--proxy").arg(p);
        }
    }
    let out = cmd.output().await.context("spawn get_iplayer search")?;
    parse_get_iplayer_output(&out.stdout, &out.stderr, opts.media_type)
}

fn parse_get_iplayer_output(
    stdout: &[u8],
    stderr: &[u8],
    media_type: &str,
) -> anyhow::Result<Vec<SearchResult>> {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let combined = format!("{stdout}{stderr}");
    let mut results = Vec::new();
    for line in combined.lines() {
        if line.starts_with("INFO:")
            || line.starts_with("WARNING:")
            || line.starts_with("get_iplayer")
            || line.is_empty()
        {
            continue;
        }
        let parts: Vec<&str> = line.splitn(8, '|').collect();
        if parts.len() < 2 {
            continue;
        }
        let pid = parts.first().unwrap_or(&"").trim();
        if pid.is_empty() {
            continue;
        }
        results.push(SearchResult {
            pid: pid.to_string(),
            title: parts.get(1).unwrap_or(&"").trim().to_string(),
            episode: parts
                .get(2)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string()),
            series: parts
                .get(3)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string()),
            channel: parts
                .get(4)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string()),
            thumbnail_url: parts
                .get(5)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string()),
            duration: parts
                .get(6)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string()),
            description: parts
                .get(7)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string()),
            media_type: media_type.to_string(),
            ..Default::default()
        });
    }
    Ok(results)
}

/// Run `get_iplayer --refresh` to update the programme cache.
pub async fn refresh_cache(
    get_iplayer_path: &str,
    media_type: &str,
    cache_dir: &str,
) -> anyhow::Result<()> {
    let status = Command::new(get_iplayer_path)
        .arg("--profile-dir")
        .arg(cache_dir)
        .arg("--refresh")
        .arg("--type")
        .arg(media_type)
        // Include upcoming/future schedule feeds (many more programmes)
        .arg("--refresh-future")
        // Re-fetch even recently-cached feeds
        .arg("--force")
        .status()
        .await
        .context("run get_iplayer --refresh")?;

    if !status.success() {
        bail!(
            "get_iplayer --refresh failed with status {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}
