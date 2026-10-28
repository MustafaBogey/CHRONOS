use crate::models::{Category, Rule};
use rusqlite::Connection;

/// Classification decision with grounded confidence, named source, and the matched
/// evidence used to reach it (shown in the activity-details interface).
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub cat: Category,
    pub source: String,
    pub confidence: f64,
    /// True when the decision is too uncertain to resolve without review.
    pub needs_review: bool,
    /// The keyword/rule/domain that matched, for transparency.
    pub matched: Option<String>,
}

impl Decision {
    fn abstain() -> Decision {
        Decision { cat: Category::Unclassified, source: "Low confidence".into(), confidence: 0.4, needs_review: true, matched: None }
    }
}

/* ---------------- keyword evidence (curated, local, deterministic) ---------------- */

/// Short tokens require word boundaries to avoid substring false hits (api in "napkin").
fn token_matches(haystack: &str, token: &str) -> bool {
    let h = haystack.to_lowercase();
    if token.chars().count() <= 4 {
        let mut start = 0;
        while let Some(pos) = h[start..].find(token) {
            let at = start + pos;
            let before_ok = at == 0 || !h.as_bytes()[at - 1].is_ascii_alphanumeric();
            let end = at + token.len();
            let after_ok = end >= h.len() || !h.as_bytes()[end].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
            start = at + 1;
        }
        false
    } else {
        h.contains(token)
    }
}

/// Development, engineering, and documentation context — strong work signals.
const WORK_KEYWORDS: &[&'static str] = &[
    "cargo", "rustc", "npm", "pnpm", "webpack", "vite", "typescript", "javascript", "python",
    "git", "github", "gitlab", "docker", "kubernetes", "kubectl", "vim", "neovim", "nvim",
    "vscode", "visual studio", "intellij", "pycharm", "webstorm", "rustrover", "opencode",
    "localhost", "127.0.0.1", "bash", "zsh", "powershell", "ssh", "mysql", "postgres", "sqlite",
    "redis", "compiler", "debugger", "refactor", "stack overflow", "stackoverflow", "docs",
    "documentation", "mdn", "readthedocs", "api reference", "sdk", "latex", "overleaf",
    "figma", "jira", "linear", "trello", "asana", "blender", "unity", "unreal", "autocad",
    "solidworks", "tutorial", "documentation", "man page", "manpage", "test suite", "unit test",
    "ci/cd", "jenkins", "makefile", "cmake", "gradle", "maven",
    "chatgpt", "claude.ai", "gemini", "perplexity", "notion.so", "notion", "smartsheet",
];

/// Entertainment, social platforms, games, and shopping — strong distraction signals.
/// Note: YouTube and Steam are deliberately NOT here — the approved Recognition Catalog
/// v1 marks both Neutral by default (Steam gameplay is detected separately).
const DISTRACT_KEYWORDS: &[&'static str] = &[
    "twitch", "netflix", "hulu", "disney+", "prime video", "tiktok", "instagram",
    "facebook", "twitter", "reddit", "9gag", "imgur", "epic games", "battle.net",
    "riot games", "roblox", "minecraft", "amazon.com", "ebay", "aliexpress", "booking.com",
    "airbnb", "onlyfans", "porn", "xhamster", "tube", "meme", "funny cats",
];

/// Catalog services approved as Neutral by default (recognized, not inherently
/// productive or distracting): YouTube and the Steam client.
const NEUTRAL_SERVICE_KEYWORDS: &[&'static str] = &["youtube", "steam"];

/// Communication and scheduling — recognized but not inherently work output.
const COMMS_KEYWORDS: &[&'static str] = &[
    "gmail", "outlook", "- mail", "inbox", "calendar", "slack", "microsoft teams", "zoom",
    "meet.google", "discord", "telegram", "whatsapp", "signal messenger", "protonmail",
];

/// Reference/reading — knowledge context, neutral by default.
const KNOWLEDGE_KEYWORDS: &[&'static str] = &[
    "wikipedia", "wiktionary", "britannica", "news", "the guardian", "nytimes", "bbc",
    "reuters", "bloomberg", "weather",
];

fn first_match(haystack: &str, keywords: &[&'static str]) -> Option<&'static str> {
    keywords.iter().find(|k| token_matches(haystack, k)).map(|k| *k)
}

/* ---------------- application classes ---------------- */

fn app_class(exe: &str, display: &str) -> &'static str {
    let both = format!("{exe} {display}").to_lowercase();
    const GENERIC_EDITORS: &[&str] = &["notepad", "textedit", "gedit"];
    const TERMINALS: &[&str] = &[
        "wezterm", "windowsterminal", "alacritty", "kitty", "foot", "cmd", "powershell", "pwsh",
        "conhost", "mintty", "gnome-terminal", "konsole", "xterm", "st ", "tabby", "hyper",
    ];
    const BROWSERS: &[&str] = &[
        "chrome", "msedge", "firefox", "brave", "zen", "chromium", "opera", "vivaldi", "arc",
        "safari", "librewolf", "waterfox",
    ];
    const EDITORS: &[&str] = &[
        "code", "cursor", "opencode", "idea", "pycharm", "webstorm", "rustrover", "devenv",
        "neovide", "nvim-qt", "sublime", "notepad++", "emacs", "textmate", "zed",
    ];
    // Order matters: Notepad++ is a real code editor; plain Notepad/TextEdit/Gedit are
    // generic editors whose meaning comes from the document (title context).
    if both.contains("notepad++") {
        return "editor";
    }
    for g in GENERIC_EDITORS {
        if both.contains(g) {
            return "generic_editor";
        }
    }
    for t in TERMINALS {
        if both.contains(t) {
            return "terminal";
        }
    }
    for b in BROWSERS {
        if both.contains(b) {
            return "browser";
        }
    }
    for e in EDITORS {
        if both.contains(e) {
            return "editor";
        }
    }
    "unknown"
}

/// Curated built-in identities (conf 0.7). Context-dependent classes (terminal/browser)
/// are deliberately absent here and resolved by title context instead.
fn builtin(exe: &str, display: &str) -> Option<(&'static str, Category)> {
    let exe_l = exe.to_lowercase();
    let display_l = display.to_lowercase();
    let key = if exe_l.is_empty() { display_l.as_str() } else { exe_l.as_str() };
    const MAP: &[(&str, &str, Category)] = &[
        ("code", "Visual Studio Code", Category::Productive),
        ("code - insiders", "Visual Studio Code", Category::Productive),
        ("cursor", "Cursor", Category::Productive),
        ("idea64", "IntelliJ IDEA", Category::Productive),
        ("rustrover64", "RustRover", Category::Productive),
        ("pycharm64", "PyCharm", Category::Productive),
        ("webstorm64", "WebStorm", Category::Productive),
        ("devenv", "Visual Studio", Category::Productive),
        ("neovide", "Neovim", Category::Productive),
        ("nvim-qt", "Neovim", Category::Productive),
        ("sublime_text", "Sublime Text", Category::Productive),
        ("figma", "Figma", Category::Productive),
        ("photoshop", "Photoshop", Category::Productive),
        ("illustrator", "Illustrator", Category::Productive),
        ("inkscape", "Inkscape", Category::Productive),
        ("gimp", "GIMP", Category::Productive),
        ("blender", "Blender", Category::Productive),
        ("obs64", "OBS Studio", Category::Productive),
        ("winword", "Microsoft Word", Category::Productive),
        ("excel", "Microsoft Excel", Category::Productive),
        ("powerpnt", "Microsoft PowerPoint", Category::Productive),
        ("onenote", "OneNote", Category::Productive),
        ("windowsterminal", "Terminal", Category::Neutral), // overridden by title context
        ("wezterm-gui", "WezTerm", Category::Neutral),     // overridden by title context
        ("alacritty", "Alacritty", Category::Neutral),     // overridden by title context
        ("kitty", "Kitty", Category::Neutral),
        ("foot", "Foot Terminal", Category::Neutral),
        ("explorer", "File Explorer", Category::Neutral),
        ("systemsettings", "System Settings", Category::Neutral),
        ("settings", "System Settings", Category::Neutral),
        ("calc", "Calculator", Category::Neutral),
        ("snippingtool", "Snipping Tool", Category::Neutral),
        ("screenclip", "Snipping Tool", Category::Neutral),
        ("notepad", "Notepad", Category::Neutral),
        ("slack", "Slack", Category::Neutral),
        ("teams", "Microsoft Teams", Category::Neutral),
        ("msteams", "Microsoft Teams", Category::Neutral),
        ("outlook", "Outlook", Category::Neutral),
        ("thunderbird", "Thunderbird", Category::Neutral),
        ("telegram", "Telegram", Category::Neutral),
        ("discord", "Discord", Category::Neutral),
        ("spotify", "Spotify", Category::Neutral),
        ("obsidian", "Obsidian", Category::Productive),
        ("notion", "Notion", Category::Productive),
        // Gaming & entertainment (Recognition Catalog v1): the Steam *client* is Neutral;
        // actual gameplay is detected separately via steamapps evidence.
        ("steam", "Steam", Category::Neutral),
        ("steamwebhelper", "Steam", Category::Neutral),
        ("epicgameslauncher", "Epic Games Launcher", Category::Distracting),
        ("galaxyclient", "GOG Galaxy", Category::Distracting),
        ("galaxyclient64", "GOG Galaxy", Category::Distracting),
        ("xbox", "Xbox", Category::Distracting),
        ("battle.net", "Battle.net", Category::Distracting),
        ("battle.net.exe", "Battle.net", Category::Distracting),
        ("netflix", "Netflix", Category::Distracting),
        // Media players and general utilities.
        ("vlc", "VLC", Category::Neutral),
        ("mpc-hc64", "MPC-HC", Category::Neutral),
        ("mpc-hc", "MPC-HC", Category::Neutral),
        ("mpcbe64", "MPC-HC", Category::Neutral),
        ("pwsh", "PowerShell", Category::Neutral), // resolved via terminal context normally
        ("calc", "Calculator", Category::Neutral),
    ];
    // Terminals and generic editors resolve via context; skip their builtin entries here.
    let class = app_class(exe, display);
    if class == "terminal" || class == "generic_editor" {
        return None;
    }
    MAP.iter().find(|(exe_key, _, _)| *exe_key == key).map(|(_, name, cat)| (*name, *cat))
}

/* ---------------- Built-in Recognition Catalog v1 (websites) ---------------- */

/// Approved domain catalog: exact or bounded (sub)domain match → friendly label and
/// default category. These are defaults only — user rules, learned patterns, and
/// reliable work-context still take precedence.
const DOMAIN_CATALOG: &[(&str, &str, Category)] = &[
    // Development, AI & productivity
    ("chatgpt.com", "ChatGPT", Category::Productive),
    ("claude.ai", "Claude", Category::Productive),
    ("gemini.google.com", "Gemini", Category::Productive),
    ("perplexity.ai", "Perplexity", Category::Productive),
    ("chat.openai.com", "ChatGPT", Category::Productive),
    ("github.com", "GitHub", Category::Productive),
    ("gitlab.com", "GitLab", Category::Productive),
    ("stackoverflow.com", "Stack Overflow", Category::Productive),
    ("docs.google.com", "Google Docs", Category::Productive),
    ("sheets.google.com", "Google Sheets", Category::Productive),
    ("slides.google.com", "Google Slides", Category::Productive),
    ("notion.so", "Notion", Category::Productive),
    ("smartsheet.com", "Smartsheet", Category::Productive),
    ("figma.com", "Figma", Category::Productive),
    // Gaming & entertainment (Steam and YouTube deliberately Neutral)
    ("store.steampowered.com", "Steam", Category::Neutral),
    ("youtube.com", "YouTube", Category::Neutral),
    ("youtu.be", "YouTube", Category::Neutral),
    ("twitch.tv", "Twitch", Category::Distracting),
    ("netflix.com", "Netflix", Category::Distracting),
    ("disneyplus.com", "Disney+", Category::Distracting),
    ("crunchyroll.com", "Crunchyroll", Category::Distracting),
    ("reddit.com", "Reddit", Category::Distracting),
    ("tiktok.com", "TikTok", Category::Distracting),
    ("instagram.com", "Instagram", Category::Distracting),
    ("facebook.com", "Facebook", Category::Distracting),
    ("x.com", "X", Category::Distracting),
    ("twitter.com", "X", Category::Distracting),
    // Communication
    ("slack.com", "Slack", Category::Neutral),
    ("teams.microsoft.com", "Microsoft Teams", Category::Neutral),
    ("zoom.us", "Zoom", Category::Neutral),
    ("web.telegram.org", "Telegram", Category::Neutral),
    ("web.whatsapp.com", "WhatsApp", Category::Neutral),
    ("outlook.office.com", "Outlook", Category::Neutral),
    ("mail.google.com", "Gmail", Category::Neutral),
    ("open.spotify.com", "Spotify", Category::Neutral),
];

/// Bounded domain-catalog lookup: exact host match or subdomain of a catalog entry.
pub fn domain_catalog_lookup(domain: &str) -> Option<(&'static str, &'static str, Category)> {
    let host = domain.trim_start_matches("www.").to_lowercase();
    DOMAIN_CATALOG.iter().find(|(d, _, _)| host == *d || host.ends_with(&format!(".{d}"))).map(|(d, l, c)| (*d, *l, *c))
}

/// Catalog services whose usage intent genuinely varies between users and
/// sessions. Their catalog category stays as a starting point, but records are
/// marked for review instead of silently judged — corrections settle them.
const AMBIGUOUS_SERVICES: &[&str] = &[
    "chatgpt.com",
    "chat.openai.com",
    "claude.ai",
    "gemini.google.com",
    "perplexity.ai",
    "reddit.com",
    "x.com",
    "twitter.com",
];

/// Friendly service label for a domain, when the catalog knows it reliably.
pub fn domain_label(domain: &str) -> Option<&'static str> {
    domain_catalog_lookup(domain).map(|(_, l, _)| l)
}

/// Title-evidence analysis shared by terminals, browsers, and unknown applications.
struct TitleEvidence {
    work: Option<&'static str>,
    distract: Option<&'static str>,
    neutral_service: Option<&'static str>,
    comms: Option<&'static str>,
    knowledge: Option<&'static str>,
}

fn scan_title(title: &str) -> TitleEvidence {
    TitleEvidence {
        work: first_match(title, WORK_KEYWORDS),
        distract: first_match(title, DISTRACT_KEYWORDS),
        neutral_service: first_match(title, NEUTRAL_SERVICE_KEYWORDS),
        comms: first_match(title, COMMS_KEYWORDS),
        knowledge: first_match(title, KNOWLEDGE_KEYWORDS),
    }
}


fn decision_from_title(ev: &TitleEvidence, base_conf: f64) -> Decision {
    if let (Some(d), Some(_w)) = (ev.distract, ev.work) {
        // Entertainment platform used in a work/learning context (e.g., a Rust tutorial on
        // YouTube): genuinely ambiguous — Neutral, never silently Distracting.
        return Decision {
            cat: Category::Neutral,
            source: "Conflicting context signals".into(),
            confidence: 0.5,
            needs_review: false,
            matched: Some(format!("work + {d}")),
        };
    }
    if let Some(d) = ev.distract {
        return Decision { cat: Category::Distracting, source: "Context · title keyword".into(), confidence: base_conf + 0.05, needs_review: false, matched: Some(d.into()) };
    }
    if let Some(w) = ev.work {
        return Decision { cat: Category::Productive, source: "Context · title keyword".into(), confidence: base_conf, needs_review: false, matched: Some(w.into()) };
    }
    // Catalog services that are Neutral by default (YouTube, Steam client).
    if let Some(s) = ev.neutral_service {
        return Decision { cat: Category::Neutral, source: "Recognized service · catalog default".into(), confidence: 0.75, needs_review: false, matched: Some(s.into()) };
    }
    if let Some(c) = ev.comms {
        return Decision { cat: Category::Neutral, source: "Context · communication".into(), confidence: base_conf, needs_review: false, matched: Some(c.into()) };
    }
    if let Some(k) = ev.knowledge {
        return Decision { cat: Category::Neutral, source: "Context · reading".into(), confidence: base_conf - 0.05, needs_review: false, matched: Some(k.into()) };
    }
    Decision::abstain()
}

/// Learned evidence: majority category among corrected records for one key.
fn learned_majority_category(conn: &Connection, key: &str) -> Option<(Category, i64, i64)> {
    let rows: Vec<(String, i64)> = conn
        .prepare(
            "SELECT category, COUNT(*) FROM activity_intervals
             WHERE corrected = 1 AND LOWER(app) = ?1 AND category IN ('productive','distracting','neutral')
             GROUP BY category ORDER BY COUNT(*) DESC",
        )
        .ok()?
        .query_map([key.to_lowercase()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .ok()?
        .filter_map(|x| x.ok())
        .collect();
    decide_majority(rows)
}

/// Learned website evidence: majority category among corrected website records for a domain.
fn learned_majority_domain(conn: &Connection, domain: &str) -> Option<(Category, i64, i64)> {
    let rows: Vec<(String, i64)> = conn
        .prepare(
            "SELECT category, COUNT(*) FROM activity_intervals
             WHERE corrected = 1 AND kind = 'website' AND LOWER(detail) = ?1
               AND category IN ('productive','distracting','neutral')
             GROUP BY category ORDER BY COUNT(*) DESC",
        )
        .ok()?
        .query_map([domain.to_lowercase()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .ok()?
        .filter_map(|x| x.ok())
        .collect();
    decide_majority(rows)
}

fn decide_majority(rows: Vec<(String, i64)>) -> Option<(Category, i64, i64)> {
    if rows.is_empty() {
        return None;
    }
    let top = &rows[0];
    if top.1 < 2 {
        return None; // one correction is not enough evidence
    }
    // A tie between the top two categories means genuine ambiguity → no learned decision.
    if rows.len() > 1 && rows[1].1 == top.1 {
        return None;
    }
    // Runner-up count feeds conflict-aware confidence: consistent corrections
    // earn more; conflicting corrections reduce (but do not veto) the decision.
    let runner_up = rows.get(1).map(|r| r.1).unwrap_or(0);
    Category::parse(&top.0).map(|c| (c, top.1, runner_up))
}

/// Learned-pattern confidence: grows with consistent evidence, shrinks with
/// conflicting corrections. Always within [0.55, 0.85] — learning never claims
/// the authority of an explicit rule (0.99) or correction.
fn learned_confidence(n: i64, runner_up: i64) -> f64 {
    (0.55 + 0.1 * n as f64 - 0.1 * runner_up as f64).clamp(0.55, 0.85)
}

/// Classify one observation.
///
/// Precedence (documented decision matrix):
///   1. explicit user corrections        — stored on records, never re-derived
///   2. explicit user rules              — domain match wins over app match; newest rule wins
///   3. learned patterns from corrections — domain-level majority first, then app-level (≥2, no tie, conflict-aware confidence)
///   4. built-in curated identities       — conf 0.7; terminals excluded (context-dependent)
///   4b. catalog domains                  — firm defaults; intent-variable services (AI assistants,
///                                          discussion platforms) are marked for review instead of judged
///   5. contextual title analysis         — terminals/browsers/unknown apps; keyword evidence
///   6. abstention                        — Unclassified, conf 0.4, Review Queue
pub fn classify(conn: &Connection, exe: &str, display: &str, title: &str, domain: Option<&str>, rules: &[Rule]) -> Decision {
    classify_inner(conn, exe, display, title, domain, rules, None)
}

/// Full entry point with a Steam-gameplay hint (set when the platform adapter has
/// reliable local evidence that the foreground executable is a Steam game).
pub fn classify_with_steam_hint(conn: &Connection, exe: &str, display: &str, title: &str, domain: Option<&str>, rules: &[Rule], steam_game: Option<&str>) -> Decision {
    classify_inner(conn, exe, display, title, domain, rules, steam_game)
}

fn classify_inner(conn: &Connection, exe: &str, display: &str, title: &str, domain: Option<&str>, rules: &[Rule], steam_game: Option<&str>) -> Decision {
    let exe_l = exe.to_lowercase();
    let display_l = display.to_lowercase();
    let dom_l = domain.map(|d| d.to_lowercase());

    // --- 2. explicit user rules (domain rules are the more specific context) ---
    let mut app_rule: Option<&Rule> = None;
    for r in rules.iter() {
        if r.app.to_lowercase() == display_l || r.app.to_lowercase() == exe_l {
            app_rule = Some(r); // iterate ascending; last (newest) wins
        }
    }
    let mut domain_rule: Option<&Rule> = None;
    if let Some(d) = &dom_l {
        for r in rules.iter() {
            if r.app.to_lowercase() == *d {
                domain_rule = Some(r);
            }
        }
    }
    if let Some(r) = domain_rule.or(app_rule) {
        let source = if domain_rule.is_some() { "User rule · website" } else { "User-defined rule" };
        return Decision {
            cat: r.cat,
            source: source.into(),
            confidence: 0.99,
            needs_review: false,
            matched: Some(r.app.clone()),
        };
    }

    // --- 3. learned patterns ---
    // With a domain present (browser attribution), domain-level learning is the more
    // specific signal and is consulted before app-level learning.
    if let Some(d) = &dom_l {
        if let Some((cat, n, runner_up)) = learned_majority_domain(conn, d) {
            return Decision { cat, source: "Learned from your corrections · website".into(), confidence: learned_confidence(n, runner_up), needs_review: false, matched: Some(d.clone()) };
        }
    }
    if let Some((cat, n, runner_up)) = learned_majority_category(conn, display) {
        return Decision { cat, source: "Learned from your corrections".into(), confidence: learned_confidence(n, runner_up), needs_review: false, matched: Some(display.to_string()) };
    }
    if exe_l != display_l && !exe_l.is_empty() {
        if let Some((cat, n, runner_up)) = learned_majority_category(conn, exe) {
            return Decision { cat, source: "Learned from your corrections".into(), confidence: learned_confidence(n, runner_up), needs_review: false, matched: Some(exe.to_string()) };
        }
    }

    // --- 4. curated built-ins ---
    if let Some((_name, cat)) = builtin(exe, display) {
        return Decision { cat, source: "Built-in default".into(), confidence: 0.7, needs_review: false, matched: Some(display.to_string()) };
    }

    // --- 4b. catalog domain recognition (browser attribution) ---
    // A reliably identified domain carries its approved default and friendly label.
    // Services whose usage intent genuinely varies (AI assistants, discussion
    // platforms) keep the default category but are marked for review instead of
    // asserting a firm judgment — the user's first corrections settle them.
    if let Some(d) = &dom_l {
        if let Some((dom, label, cat)) = domain_catalog_lookup(d) {
            if AMBIGUOUS_SERVICES.contains(&dom) {
                return Decision {
                    cat,
                    source: "Recognized service · ambiguous use".into(),
                    confidence: 0.5,
                    needs_review: true,
                    matched: Some(label.to_string()),
                };
            }
            return Decision { cat, source: "Recognized service · catalog default".into(), confidence: 0.75, needs_review: false, matched: Some(label.to_string()) };
        }
    }

    // --- 4c. Steam gameplay evidence (foreground executable under steamapps\common) ---
    if let Some(game) = steam_game {
        return Decision { cat: Category::Distracting, source: "Steam gameplay".into(), confidence: 0.8, needs_review: false, matched: Some(game.to_string()) };
    }

    // --- 5. contextual title analysis ---
    let class = app_class(exe, display);
    let ev = scan_title(title);
    // For browsers with a domain (extension attribution), the domain is the stronger signal.
    if class == "browser" {
        if let Some(d) = &dom_l {
            let domain_ev = scan_title(d);
            let merged = TitleEvidence {
                work: ev.work.or(domain_ev.work),
                distract: ev.distract.or(domain_ev.distract),
                neutral_service: ev.neutral_service.or(domain_ev.neutral_service),
                comms: ev.comms.or(domain_ev.comms),
                knowledge: ev.knowledge.or(domain_ev.knowledge),
            };
            return decision_from_title(&merged, 0.72);
        }
    }
    match class {
        "terminal" => {
            let d = decision_from_title(&ev, 0.75);
            if d.needs_review {
                // Recognized tool with unknown use: Neutral fallback (documented), not review.
                return Decision {
                    cat: Category::Neutral,
                    source: "Recognized tool · ambiguous use".into(),
                    confidence: 0.55,
                    needs_review: false,
                    matched: Some(display.to_string()),
                };
            }
            return d;
        }
        "browser" => {
            let d = decision_from_title(&ev, 0.72);
            if d.needs_review {
                // Browsers are the most ambiguous surface: abstain for review.
                return Decision::abstain();
            }
            return d;
        }
        "generic_editor" => {
            // A generic text editor's meaning comes from the document: title decides.
            let d = decision_from_title(&ev, 0.75);
            if d.needs_review {
                return Decision {
                    cat: Category::Neutral,
                    source: "Recognized tool · ambiguous use".into(),
                    confidence: 0.55,
                    needs_review: false,
                    matched: Some(display.to_string()),
                };
            }
            return d;
        }
        "editor" => {
            // Recognized work tool; title rarely adds signal. Distract keywords still win.
            if let Some(dk) = ev.distract {
                return Decision { cat: Category::Distracting, source: "Context · title keyword".into(), confidence: 0.7, needs_review: false, matched: Some(dk.into()) };
            }
            return Decision { cat: Category::Productive, source: "Built-in default".into(), confidence: 0.65, needs_review: false, matched: Some(display.to_string()) };
        }
        _ => {
            // Unknown application: try title evidence at a lower confidence, else abstain.
            let d = decision_from_title(&ev, 0.6);
            if d.needs_review {
                return Decision::abstain();
            }
            return d;
        }
    }
}

/* ---------------- project / client attribution ---------------- */

/// Attribution decision for an observation. `source` explains the reason and is stored
/// on the interval ('rule' | 'learned' | '' for unassigned).
pub fn attribute_project(conn: &Connection, exe: &str, display: &str, domain: Option<&str>, rules: &[Rule]) -> (String, &'static str) {
    let exe_l = exe.to_lowercase();
    let display_l = display.to_lowercase();
    let dom_l = domain.map(|d| d.to_lowercase());

    // 1. Explicit assignment rules (domain rule wins, then newest app rule with a project).
    if let Some(d) = &dom_l {
        let mut found: Option<&Rule> = None;
        for r in rules.iter() {
            if !r.project.is_empty() && r.app.to_lowercase() == *d {
                found = Some(r);
            }
        }
        if let Some(r) = found {
            return (r.project.clone(), "rule");
        }
    }
    let mut app_found: Option<&Rule> = None;
    for r in rules.iter() {
        if !r.project.is_empty() && (r.app.to_lowercase() == display_l || r.app.to_lowercase() == exe_l) {
            app_found = Some(r);
        }
    }
    if let Some(r) = app_found {
        return (r.project.clone(), "rule");
    }

    // 2. Learned assignment: majority project among corrected records for this app.
    //    Ties stay unassigned (never silently guessed), and conflicting usage
    //    (runner_up within 2× of the top) also stays unassigned — multipurpose
    //    applications are never forced into a single project.
    let learned = |key: &str| -> Option<String> {
        let rows: Vec<(String, i64)> = conn
            .prepare(
                "SELECT project, COUNT(*) FROM activity_intervals
                 WHERE corrected = 1 AND LOWER(app) = ?1 AND project != ''
                 GROUP BY project ORDER BY COUNT(*) DESC",
            )
            .ok()?
            .query_map([key.to_lowercase()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .ok()?
            .filter_map(|x| x.ok())
            .collect();
        if rows.is_empty() || rows[0].1 < 2 {
            return None;
        }
        if rows.len() > 1 {
            if rows[1].1 == rows[0].1 {
                return None; // tie between projects → unassigned
            }
            if rows[0].1 < 2 * rows[1].1 {
                return None; // genuinely split usage → unassigned
            }
        }
        Some(rows[0].0.clone())
    };
    if let Some(p) = learned(display) {
        return (p, "learned");
    }
    if !exe_l.is_empty() && exe_l != display_l {
        if let Some(p) = learned(exe) {
            return (p, "learned");
        }
    }
    (String::new(), "")
}

/* ---------------- rules / exclusions loading ---------------- */

pub fn load_rules(conn: &Connection) -> Vec<Rule> {
    let Ok(mut stmt) = conn.prepare("SELECT app, category, project FROM rules ORDER BY id") else { return vec![] };
    stmt.query_map([], |r| {
        let app: String = r.get(0)?;
        let cat: String = r.get(1)?;
        let project: String = r.get(2)?;
        Ok(Rule { app, cat: Category::parse(&cat).unwrap_or(Category::Unclassified), project })
    })
    .map(|it| it.filter_map(|x| x.ok()).collect())
    .unwrap_or_default()
}

pub fn load_exclusions(conn: &Connection) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare("SELECT label FROM exclusions ORDER BY id") else { return vec![] };
    stmt.query_map([], |r| r.get::<_, String>(0))
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
}

/// Canonical identity for a user-entered exclusion label.
///
/// Domain-ish input (anything with a scheme, a leading `www.`, a path, or a
/// port) is reduced to its bare host without the leading `www.` — so
/// `https://www.youtube.com/watch?v=x`, `www.youtube.com`, and `youtube.com/`
/// all normalize to `youtube.com`. Plain application names pass through
/// trimmed. Labels are never invented: a string that is not a plausible host
/// or app name is returned as-is (trimmed) and simply will not match anything.
pub fn normalize_exclusion_label(label: &str) -> String {
    let mut s = label.trim();
    if let Some(rest) = s.strip_prefix("http://").or_else(|| s.strip_prefix("https://")) {
        s = rest;
    }
    // Cut path, query, fragment, and port.
    let host_port = s.split(['/', '?', '#']).next().unwrap_or("");
    let host = host_port.rsplit_once(':').map(|(h, _)| h).unwrap_or(host_port);
    let host = host.trim().to_lowercase();
    if host.is_empty() {
        return label.trim().to_string();
    }
    // A plausible host is at least two dot-separated alphanumeric/hyphen parts,
    // or a localhost-style single label when the input looked like a URL.
    let looks_like_host = host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':')
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.');
    if !looks_like_host {
        // Plain application name (e.g. "Steam", "Microsoft Teams") — keep as-is.
        return label.trim().to_string();
    }
    host.strip_prefix("www.").unwrap_or(&host).to_string()
}

/// True when the observation must not be collected at all.
///
/// Matching semantics: an exclusion label matches either the application
/// identity (display name or executable, case-insensitive) or the attributed
/// website domain. Domain labels match the domain itself and all of its
/// subdomains (`youtube.com` matches `m.youtube.com`) but never sibling
/// domains (`notyoutube.com` is untouched). Labels that were stored before
/// normalization existed are normalized at match time, so entries like
/// `https://www.youtube.com/` still take effect.
pub fn is_excluded(app: &str, domain: Option<&str>, exclusions: &[String]) -> bool {
    let app_lower = app.to_lowercase();
    let domain_lower = domain.map(|d| {
        let d = d.trim_start_matches("https://").trim_start_matches("http://");
        let d = d.split(['/', '?', '#']).next().unwrap_or(d);
        d.to_lowercase()
    });
    exclusions.iter().any(|e| {
        let normalized = normalize_exclusion_label(e);
        let e_lower = normalized.to_lowercase();
        if e_lower == app_lower {
            return true;
        }
        if let Some(d) = &domain_lower {
            let d = d.strip_prefix("www.").unwrap_or(d);
            if e_lower == *d || d.ends_with(&format!(".{e_lower}")) {
                return true;
            }
        }
        false
    })
}

/* ---------------- activity-detail privacy (Discord, File Explorer, browsers) ---------------- */

/// Privacy classes whose window titles are considered personal content. These apps keep
/// full usage tracking and classification, but only the application identity (or a
/// privacy-safe service label) may be persisted or displayed — never the raw title.
pub fn privacy_class(exe: &str, display: &str) -> &'static str {
    let exe_l = exe.to_lowercase();
    let display_l = display.to_lowercase();
    if exe_l.contains("discord") || display_l.contains("discord") {
        return "discord";
    }
    if exe_l == "explorer" || exe_l == "explorer.exe" || display_l == "file explorer" {
        return "explorer";
    }
    if app_class(exe, display) == "browser" {
        return "browser";
    }
    "other"
}

/// Pretty labels for well-known services on the curated keyword allowlist. Only these
/// curated tokens may ever surface from a browser title — never page content.
fn service_label(token: &str) -> Option<String> {
    let pretty = match token {
        "chatgpt" => "ChatGPT",
        "claude.ai" => "Claude",
        "gemini" => "Gemini",
        "perplexity" => "Perplexity",
        "notion.so" | "notion" => "Notion",
        "smartsheet" => "Smartsheet",
        "youtube" => "YouTube",
        "github" => "GitHub",
        "gitlab" => "GitLab",
        "netflix" => "Netflix",
        "twitch" => "Twitch",
        "tiktok" => "TikTok",
        "instagram" => "Instagram",
        "facebook" => "Facebook",
        "twitter" => "Twitter",
        "reddit" => "Reddit",
        "imgur" => "Imgur",
        "9gag" => "9GAG",
        "steam" => "Steam",
        "epic games" => "Epic Games",
        "roblox" => "Roblox",
        "minecraft" => "Minecraft",
        "amazon.com" => "Amazon",
        "ebay" => "eBay",
        "aliexpress" => "AliExpress",
        "booking.com" => "Booking.com",
        "airbnb" => "Airbnb",
        "stack overflow" | "stackoverflow" => "Stack Overflow",
        "mdn" => "MDN",
        "readthedocs" => "Read the Docs",
        "docs" | "documentation" => "Documentation",
        "npm" => "npm",
        "cargo" => "Cargo",
        "rustc" => "rustc",
        "docker" => "Docker",
        "kubernetes" | "kubectl" => "Kubernetes",
        "figma" => "Figma",
        "jira" => "Jira",
        "linear" => "Linear",
        "trello" => "Trello",
        "asana" => "Asana",
        "blender" => "Blender",
        "unity" => "Unity",
        "unreal" => "Unreal",
        "latex" => "LaTeX",
        "overleaf" => "Overleaf",
        "localhost" | "127.0.0.1" => "Localhost",
        "postgresql" => "PostgreSQL",
        "sqlite" => "SQLite",
        "redis" => "Redis",
        "gmail" => "Gmail",
        "outlook" => "Outlook",
        "calendar" => "Calendar",
        "slack" => "Slack",
        "microsoft teams" => "Microsoft Teams",
        "zoom" => "Zoom",
        "meet.google" => "Google Meet",
        "telegram" => "Telegram",
        "whatsapp" => "WhatsApp",
        "wikipedia" => "Wikipedia",
        "bbc" => "BBC",
        "reuters" => "Reuters",
        "bloomberg" => "Bloomberg",
        "disney+" => "Disney+",
        "prime video" => "Prime Video",
        "hulu" => "Hulu",
        _ => return None,
    };
    Some(pretty.to_string())
}

/// Privacy-safe service name for a browser title: only a curated allowlist keyword may
/// surface (e.g. "YouTube" from any video page). Everything else — page titles, video
/// names, search queries — returns None and the browser falls back to its app identity.
pub fn service_from_title(title: &str) -> Option<String> {
    for list in [DISTRACT_KEYWORDS, WORK_KEYWORDS, NEUTRAL_SERVICE_KEYWORDS, COMMS_KEYWORDS, KNOWLEDGE_KEYWORDS] {
        if let Some(token) = first_match(title, list) {
            if let Some(label) = service_label(token) {
                return Some(label);
            }
        }
    }
    None
}

/// Write-time detail sanitizer. Applied BEFORE persistence so restricted content never
/// reaches SQLite. Development tools (terminals, editors, IDEs) keep their full title
/// context, which their classification depends on.
pub fn privacy_safe_detail(exe: &str, display: &str, title: &str, domain: Option<&str>) -> String {
    match privacy_class(exe, display) {
        "discord" | "explorer" => display.to_string(), // application identity only
        "browser" => {
            if let Some(d) = domain {
                return d.to_string(); // domain from the optional extension is already privacy-safe
            }
            service_from_title(title).unwrap_or_default() // curated service label or nothing
        }
        _ => title.to_string(),
    }
}

/// Sanitize previously stored sensitive details in place. Idempotent; preserves
/// timestamps, durations, classifications, project assignments, and interval identity.
/// Also run after restores, which may reintroduce older titles.
pub fn sanitize_stored_details(conn: &Connection) {
    // Discord and File Explorer: application identity only.
    let _ = conn.execute(
        "UPDATE activity_intervals SET detail = app WHERE LOWER(app) LIKE '%discord%' OR LOWER(app) IN ('file explorer','explorer')",
        [],
    );
    // Browsers: keep domain rows (kind='website') as-is; sanitize raw-title rows.
    let browsers = ["Google Chrome", "Microsoft Edge", "Firefox", "Brave", "Zen Browser", "Chromium", "Opera", "Vivaldi", "Arc", "LibreWolf", "Waterfox"];
    for b in browsers {
        let rows: Vec<(i64, String)> = match conn
            .prepare("SELECT id, detail FROM activity_intervals WHERE app = ?1 AND (kind IS NULL OR kind != 'website') AND detail != ''")
            .and_then(|mut s| {
                s.query_map([b], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                    .map(|it| it.filter_map(|x| x.ok()).collect())
            }) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for (id, detail) in rows {
            let safe = service_from_title(&detail).unwrap_or_default();
            let _ = conn.execute("UPDATE activity_intervals SET detail = ?2 WHERE id = ?1", rusqlite::params![id, safe]);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn exclusion_labels_normalize_urls_to_bare_domains() {
        assert_eq!(normalize_exclusion_label("https://www.youtube.com/"), "youtube.com");
        assert_eq!(normalize_exclusion_label("https://www.youtube.com"), "youtube.com");
        assert_eq!(normalize_exclusion_label("http://youtube.com/watch?v=x"), "youtube.com");
        assert_eq!(normalize_exclusion_label("www.youtube.com"), "youtube.com");
        assert_eq!(normalize_exclusion_label("youtube.com/"), "youtube.com");
        assert_eq!(normalize_exclusion_label("  YouTube.COM  "), "youtube.com");
        assert_eq!(normalize_exclusion_label("m.example.org:8080/path"), "m.example.org");
        // Plain application names pass through untouched.
        assert_eq!(normalize_exclusion_label("Steam"), "Steam");
        assert_eq!(normalize_exclusion_label("  Microsoft Teams  "), "Microsoft Teams");
        assert_eq!(normalize_exclusion_label(""), "");
    }

    #[test]
    fn exclusion_matches_domains_subdomains_and_apps() {
        let ex = vec!["youtube.com".to_string(), "Steam".to_string()];
        // Domain match, with and without normalization of the stored label.
        assert!(is_excluded("Google Chrome", Some("youtube.com"), &ex));
        assert!(is_excluded("Google Chrome", Some("www.youtube.com"), &ex));
        assert!(is_excluded("Google Chrome", Some("m.youtube.com"), &ex), "subdomains are included");
        assert!(!is_excluded("Google Chrome", Some("notyoutube.com"), &ex), "sibling domains never match");
        assert!(!is_excluded("Google Chrome", Some("youtu.be"), &ex));
        // Application identity match (case-insensitive, both directions).
        assert!(is_excluded("Steam", None, &ex));
        assert!(is_excluded("steam.exe", None, &ex) == false, "exe names match only by exact identity");
        assert!(is_excluded("steam", None, &ex));
        // A URL-shaped stored label (pre-normalization entries) still matches.
        let legacy = vec!["https://www.youtube.com/".to_string()];
        assert!(is_excluded("Google Chrome", Some("youtube.com"), &legacy), "legacy raw-URL exclusion now matches");
    }

    #[test]
    fn exclusions_load_and_remove_round_trip() {
        let conn = mem();
        conn.execute("INSERT INTO exclusions (label, created_at) VALUES ('youtube.com', 0)", []).unwrap();
        conn.execute("INSERT INTO exclusions (label, created_at) VALUES ('Steam', 0)", []).unwrap();
        assert_eq!(load_exclusions(&conn), vec!["youtube.com".to_string(), "Steam".to_string()]);
        let removed = conn.execute("DELETE FROM exclusions WHERE label = 'youtube.com'", []).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(load_exclusions(&conn), vec!["Steam".to_string()]);
    }

    fn corrected(conn: &Connection, app: &str, cat: &str, project: &str) {
        conn.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, project, corrected)
             VALUES (0, 1, '2026-10-24', ?1, '', ?2, ?3, 1)",
            rusqlite::params![app, cat, project],
        )
        .unwrap();
    }

    /* ---- precedence: rules ---- */

    #[test]
    fn domain_rule_wins_for_browsers() {
        let conn = mem();
        let rules = vec![
            Rule { app: "Chrome".into(), cat: Category::Distracting, project: String::new() },
            Rule { app: "docs.rust-lang.org".into(), cat: Category::Productive, project: String::new() },
        ];
        let d = classify(&conn, "chrome", "Google Chrome", "docs.rust-lang.org — Rust", Some("docs.rust-lang.org"), &rules);
        assert_eq!(d.cat, Category::Productive);
        assert_eq!(d.source, "User rule · website");
    }

    #[test]
    fn newest_rule_wins_between_conflicting_app_rules() {
        let conn = mem();
        conn.execute("INSERT INTO rules (app, category, project, created_at) VALUES ('Spotify','productive','','1')", []).unwrap();
        conn.execute("INSERT INTO rules (app, category, project, created_at) VALUES ('Spotify','distracting','','2')", []).unwrap();
        let rules = load_rules(&conn);
        let d = classify(&conn, "spotify", "Spotify", "", None, &rules);
        assert_eq!(d.cat, Category::Distracting);
    }

    /* ---- learned patterns ---- */

    #[test]
    fn learned_majority_requires_two_and_rejects_ties() {
        let conn = mem();
        corrected(&conn, "WeirdTool", "productive", "");
        // one correction: not enough
        let d = classify(&conn, "weirdtool", "WeirdTool", "", None, &[]);
        assert!(d.needs_review);
        corrected(&conn, "WeirdTool", "productive", "");
        let d = classify(&conn, "weirdtool", "WeirdTool", "", None, &[]);
        assert_eq!(d.cat, Category::Productive);
        assert_eq!(d.source, "Learned from your corrections");
        // tie → falls through to abstention (unknown app)
        corrected(&conn, "WeirdTool", "distracting", "");
        corrected(&conn, "WeirdTool", "distracting", "");
        let d = classify(&conn, "weirdtool", "WeirdTool", "", None, &[]);
        assert!(d.needs_review, "tied evidence must not decide");
    }

    #[test]
    fn learned_domain_for_websites() {
        let conn = mem();
        conn.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, kind, corrected)
             VALUES (0,1,'2026-10-24','Google Chrome','news.example','productive','website',1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, kind, corrected)
             VALUES (2,3,'2026-10-24','Google Chrome','news.example','productive','website',1)",
            [],
        )
        .unwrap();
        let d = classify(&conn, "chrome", "Google Chrome", "news.example — headlines", Some("news.example"), &[]);
        assert_eq!(d.cat, Category::Productive);
        assert_eq!(d.source, "Learned from your corrections · website");
    }

    /* ---- terminals: context matters ---- */

    #[test]
    fn terminal_with_dev_title_is_productive() {
        let conn = mem();
        let d = classify(&conn, "wezterm-gui", "WezTerm", "opencode — cargo build - Chronos", None, &[]);
        assert_eq!(d.cat, Category::Productive);
        assert_eq!(d.source, "Context · title keyword");
        assert!(d.matched.is_some());
    }

    #[test]
    fn plain_terminal_is_neutral_not_productive() {
        let conn = mem();
        let d = classify(&conn, "wezterm-gui", "WezTerm", "~", None, &[]);
        assert_eq!(d.cat, Category::Neutral);
        assert_eq!(d.source, "Recognized tool · ambiguous use");
        assert!(!d.needs_review);
    }

    #[test]
    fn terminal_with_entertainment_title_is_distracting() {
        let conn = mem();
        let d = classify(&conn, "windowsterminal", "Terminal", "twitch - live stream", None, &[]);
        assert_eq!(d.cat, Category::Distracting);
        // YouTube in a terminal title is Neutral by the approved catalog.
        let d = classify(&conn, "windowsterminal", "Terminal", "youtube - music playlist", None, &[]);
        assert_eq!(d.cat, Category::Neutral);
    }

    /* ---- browsers ---- */

    #[test]
    fn browser_without_context_abstains_for_review() {
        let conn = mem();
        let d = classify(&conn, "chrome", "Google Chrome", "Untitled — Google Chrome", None, &[]);
        assert_eq!(d.cat, Category::Unclassified);
        assert!(d.needs_review);
        assert_eq!(d.confidence, 0.4);
    }

    #[test]
    fn browser_entertainment_is_distracting_and_work_context_wins_conflicts() {
        let conn = mem();
        // Work + non-catalog entertainment → Neutral conflict resolution.
        let d = classify(&conn, "chrome", "Google Chrome", "Rust tutorial - Twitch", None, &[]);
        assert_eq!(d.cat, Category::Neutral, "work + entertainment = neutral, never silently distracting");
        let d = classify(&conn, "chrome", "Google Chrome", "cat videos - Twitch", None, &[]);
        assert_eq!(d.cat, Category::Distracting);
        // YouTube is catalog-Neutral: work context on YouTube is Productive, plain YouTube Neutral.
        let d = classify(&conn, "chrome", "Google Chrome", "Rust tutorial - YouTube", None, &[]);
        assert_eq!(d.cat, Category::Productive, "reliable work context beats the neutral default");
        let d = classify(&conn, "chrome", "Google Chrome", "cat videos - YouTube", None, &[]);
        assert_eq!(d.cat, Category::Neutral);
    }

    #[test]
    fn browser_domain_evidence_from_extension() {
        let conn = mem();
        let d = classify(&conn, "chrome", "Google Chrome", "docs.rust-lang.org", Some("docs.rust-lang.org"), &[]);
        assert_eq!(d.cat, Category::Productive);
    }

    /* ---- unknown apps ---- */

    #[test]
    fn unknown_app_uses_title_evidence_then_abstains() {
        let conn = mem();
        let d = classify(&conn, "sometool", "SomeTool", "github pull requests", None, &[]);
        assert_eq!(d.cat, Category::Productive);
        assert!((d.confidence - 0.6).abs() < 1e-9);
        let d = classify(&conn, "sometool", "SomeTool", "hello world", None, &[]);
        assert!(d.needs_review);
    }

    #[test]
    fn builtin_identity_still_wins_over_title() {
        let conn = mem();
        // Netflix is curated distracting even if the title mentions work-ish words.
        let d = classify(&conn, "netflix", "Netflix", "github", None, &[]);
        assert_eq!(d.cat, Category::Distracting);
        assert_eq!(d.source, "Built-in default");
    }

    #[test]
    fn short_keyword_boundaries_prevent_substring_hits() {
        assert!(token_matches("npm run dev", "npm"));
        assert!(!token_matches("napkin", "npm"));
        assert!(token_matches("git status", "git"));
        assert!(!token_matches("legitimate", "git"));
    }

    /* ---- project attribution ---- */

    #[test]
    fn attribution_rule_beats_learned() {
        let conn = mem();
        conn.execute("INSERT INTO rules (app, category, project, created_at) VALUES ('WezTerm','productive','p1','1')", []).unwrap();
        corrected(&conn, "WezTerm", "productive", "p2");
        corrected(&conn, "WezTerm", "productive", "p2");
        let rules = load_rules(&conn);
        let (project, source) = attribute_project(&conn, "wezterm-gui", "WezTerm", None, &rules);
        assert_eq!((project.as_str(), source), ("p1", "rule"));
    }

    #[test]
    fn attribution_learned_majority_and_tie_stays_unassigned() {
        let conn = mem();
        corrected(&conn, "WezTerm", "productive", "p1");
        // one assignment is not enough
        let (p, s) = attribute_project(&conn, "wezterm-gui", "WezTerm", None, &[]);
        assert_eq!((p.as_str(), s), ("", ""));
        corrected(&conn, "WezTerm", "productive", "p1");
        let (p, s) = attribute_project(&conn, "wezterm-gui", "WezTerm", None, &[]);
        assert_eq!((p.as_str(), s), ("p1", "learned"));
        // tie → unassigned
        corrected(&conn, "WezTerm", "productive", "p2");
        corrected(&conn, "WezTerm", "productive", "p2");
        let (p, s) = attribute_project(&conn, "wezterm-gui", "WezTerm", None, &[]);
        assert_eq!((p.as_str(), s), ("", ""), "project ties are never guessed");
    }

    #[test]
    fn generic_editor_title_decides_and_plain_is_neutral() {
        let conn = mem();
        // The exact Windows acceptance scenario: real work in a generic editor.
        let d = classify(&conn, "notepad", "Notepad", "cargo-build-opencode-work.txt - Notepad", None, &[]);
        assert_eq!(d.cat, Category::Productive, "dev-document title must classify Productive");
        assert_eq!(d.source, "Context · title keyword");
        // A shopping list is not work, but it is not distraction either.
        let d = classify(&conn, "notepad", "Notepad", "groceries.txt - Notepad", None, &[]);
        assert_eq!(d.cat, Category::Neutral);
        assert_eq!(d.source, "Recognized tool · ambiguous use");
        // Notepad++ is a real editor: productive by identity.
        let d = classify(&conn, "notepad++", "Notepad++", "anything", None, &[]);
        assert_eq!(d.cat, Category::Productive);
    }

    #[test]
    fn exclusions_match_app_and_domain() {
        let ex = vec!["private.example".to_string(), "1password".to_string()];
        assert!(is_excluded("1Password", None, &ex));
        assert!(is_excluded("Firefox", Some("private.example"), &ex));
        assert!(is_excluded("Firefox", Some("sub.private.example"), &ex));
        assert!(!is_excluded("Firefox", Some("example.com"), &ex));
    }

    /* ---- Recognition Catalog v1 ---- */

    #[test]
    fn youtube_and_steam_are_neutral_by_catalog() {
        let conn = mem();
        // Domain attribution (extension).
        let d = classify(&conn, "chrome", "Google Chrome", "whatever page", Some("youtube.com"), &[]);
        assert_eq!(d.cat, Category::Neutral);
        assert_eq!(d.matched.as_deref(), Some("YouTube"));
        let d = classify(&conn, "chrome", "Google Chrome", "x", Some("music.youtube.com"), &[]);
        assert_eq!(d.cat, Category::Neutral, "subdomain bounded match");
        // Title evidence without extension.
        let d = classify(&conn, "chrome", "Google Chrome", "cat videos - YouTube", None, &[]);
        assert_eq!(d.cat, Category::Neutral, "YouTube title token is Neutral, not Distracting");
        // Steam client executables.
        let d = classify(&conn, "steam", "Steam", "Steam Store", None, &[]);
        assert_eq!(d.cat, Category::Neutral);
        let d = classify(&conn, "steamwebhelper", "Steam", "store.steampowered.com", None, &[]);
        assert_eq!(d.cat, Category::Neutral);
        // Steam store domain.
        let d = classify(&conn, "msedge", "Microsoft Edge", "x", Some("store.steampowered.com"), &[]);
        assert_eq!(d.cat, Category::Neutral);
    }

    #[test]
    fn ai_and_dev_domains_are_productive_with_labels() {
        let conn = mem();
        for (dom, label) in [
            ("chatgpt.com", "ChatGPT"),
            ("claude.ai", "Claude"),
            ("gemini.google.com", "Gemini"),
            ("perplexity.ai", "Perplexity"),
            ("github.com", "GitHub"),
            ("stackoverflow.com", "Stack Overflow"),
            ("docs.google.com", "Google Docs"),
            ("figma.com", "Figma"),
            ("smartsheet.com", "Smartsheet"),
        ] {
            let d = classify(&conn, "chrome", "Google Chrome", "page", Some(dom), &[]);
            assert_eq!(d.cat, Category::Productive, "{dom}");
            assert_eq!(d.matched.as_deref(), Some(label), "{dom} label");
        }
    }

    #[test]
    fn entertainment_and_comms_catalog_domains() {
        let conn = mem();
        for dom in ["twitch.tv", "netflix.com", "reddit.com", "tiktok.com", "instagram.com", "x.com", "crunchyroll.com", "disneyplus.com"] {
            let d = classify(&conn, "chrome", "Google Chrome", "p", Some(dom), &[]);
            assert_eq!(d.cat, Category::Distracting, "{dom}");
        }
        for dom in ["slack.com", "teams.microsoft.com", "zoom.us", "web.whatsapp.com", "mail.google.com", "outlook.office.com", "open.spotify.com"] {
            let d = classify(&conn, "chrome", "Google Chrome", "p", Some(dom), &[]);
            assert_eq!(d.cat, Category::Neutral, "{dom}");
        }
        // Gaming desktop identities.
        for exe in ["epicgameslauncher", "galaxyclient", "galaxyclient64", "xbox", "battle.net"] {
            let d = classify(&conn, exe, exe, "", None, &[]);
            assert_eq!(d.cat, Category::Distracting, "{exe}");
        }
        // Media players / utilities.
        for exe in ["vlc", "mpc-hc64", "spotify"] {
            let d = classify(&conn, exe, exe, "", None, &[]);
            assert_eq!(d.cat, Category::Neutral, "{exe}");
        }
    }

    #[test]
    fn user_rules_and_learning_override_catalog() {
        let conn = mem();
        conn.execute("INSERT INTO rules (app, category, project, created_at) VALUES ('youtube.com','distracting','','1')", []).unwrap();
        let rules = load_rules(&conn);
        let d = classify(&conn, "chrome", "Google Chrome", "p", Some("youtube.com"), &rules);
        assert_eq!(d.cat, Category::Distracting, "user rule beats catalog default");
        drop(rules);
        // Learned corrections (≥2, majority) beat the catalog too.
        for _ in 0..2 {
            conn.execute(
                "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, kind, corrected)
                 VALUES (0,1,'2026-10-24','Google Chrome','YouTube','productive','website',1)",
                [],
            )
            .unwrap();
        }
        let d = classify(&conn, "chrome", "Google Chrome", "p", Some("youtube.com"), &[]);
        assert_eq!(d.cat, Category::Productive, "learned beats catalog");
    }

    #[test]
    fn steam_gameplay_hint_is_distracting_but_rules_win() {
        let conn = mem();
        let d = classify_with_steam_hint(&conn, "eldenring", "eldenring", "Elden Ring", None, &[], Some("Elden Ring"));
        assert_eq!(d.cat, Category::Distracting);
        assert_eq!(d.source, "Steam gameplay");
        assert_eq!(d.matched.as_deref(), Some("Elden Ring"));
        // Steam client executables never reach the hint (built-in Neutral first).
        let d = classify_with_steam_hint(&conn, "steam", "Steam", "", None, &[], Some("whatever"));
        assert_eq!(d.cat, Category::Neutral);
        // A user rule for the game overrides the gameplay default.
        conn.execute("INSERT INTO rules (app, category, project, created_at) VALUES ('eldenring','neutral','','1')", []).unwrap();
        let rules = load_rules(&conn);
        let d = classify_with_steam_hint(&conn, "eldenring", "eldenring", "", None, &rules, Some("Elden Ring"));
        assert_eq!(d.cat, Category::Neutral, "user rule beats gameplay default");
    }

    #[test]
    fn unknown_game_and_unknown_domain_stay_unclassified() {
        let conn = mem();
        // Unverified game executable (no steamapps evidence) stays unclassified.
        let d = classify(&conn, "somegame", "somegame", "", None, &[]);
        assert!(d.needs_review);
        // Unknown domain: no catalog guess — falls to domain/title evidence or abstains.
        let d = classify(&conn, "chrome", "Google Chrome", "p", Some("random-shop.example"), &[]);
        assert!(d.needs_review, "unknown domains are not guessed");
    }

    #[test]
    fn catalog_labels_for_detail_display() {
        assert_eq!(domain_label("chatgpt.com"), Some("ChatGPT"));
        assert_eq!(domain_label("www.youtube.com"), Some("YouTube"));
        assert_eq!(domain_label("unknown.example"), None);
    }

    /* ---- activity-detail privacy ---- */

    #[test]
    fn discord_and_explorer_reduce_to_app_identity() {
        assert_eq!(privacy_safe_detail("discord", "Discord", "#main-chat | Die A lot Platoon - Discord", None), "Discord");
        assert_eq!(privacy_safe_detail("Discord.exe", "Discord", "@Gothic - Discord", None), "Discord");
        assert_eq!(privacy_safe_detail("explorer", "File Explorer", "BBSC-0.0.2-BBport and 5 more tabs - File Explorer", None), "File Explorer");
        assert_eq!(privacy_safe_detail("explorer", "File Explorer", "Program Manager", None), "File Explorer");
    }

    #[test]
    fn browser_detail_is_service_label_or_nothing() {
        // Specific video name must never persist; the service label may.
        assert_eq!(
            privacy_safe_detail("chrome", "Google Chrome", "(31) Bloodborne - Female Vileblood OC creation - YouTube - Google Chrome", None),
            "YouTube"
        );
        // Page titles / search queries with no curated service → no detail at all.
        assert_eq!(privacy_safe_detail("chrome", "Google Chrome", "Check Bloodborne Release - Google Chrome", None), "");
        assert_eq!(privacy_safe_detail("msedge", "Microsoft Edge", "New Tab - Microsoft Edge", None), "");
        // A domain from the optional extension remains the preferred privacy-safe detail.
        assert_eq!(privacy_safe_detail("chrome", "Google Chrome", "whatever page", Some("docs.rust-lang.org")), "docs.rust-lang.org");
    }

    #[test]
    fn development_tools_keep_title_context() {
        assert_eq!(
            privacy_safe_detail("wezterm-gui", "wezterm-gui", "opencode — cargo build - Chronos", None),
            "opencode — cargo build - Chronos"
        );
        assert_eq!(privacy_safe_detail("code", "Visual Studio Code", "main.rs - chronos", None), "main.rs - chronos");
    }

    #[test]
    fn stored_sensitive_titles_are_sanitized_in_place() {
        let conn = mem();
        let seed = |app: &str, detail: &str, kind: &str, cat: &str| {
            conn.execute(
                "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, kind, project, focus_minutes)
                 VALUES (0, 60000, '2026-10-24', ?1, ?2, ?3, ?4, 'p9', 1.5)",
                rusqlite::params![app, detail, cat, kind],
            )
            .unwrap();
        };
        seed("Discord", "#main-chat | Die A lot Platoon - Discord", "app", "neutral");
        seed("Discord", "@Gothic - Discord", "app", "neutral");
        seed("File Explorer", "BBSC-0.0.2 and 5 more tabs - File Explorer", "app", "neutral");
        seed("Google Chrome", "(31) Bloodborne OC creation - YouTube - Google Chrome", "app", "distracting");
        seed("Google Chrome", "Check Bloodborne Release - Google Chrome", "app", "unclassified");
        seed("Google Chrome", "docs.rust-lang.org", "website", "productive");
        seed("wezterm-gui", "opencode — cargo build", "app", "productive");

        sanitize_stored_details(&conn);
        sanitize_stored_details(&conn); // idempotent

        let rows: Vec<(String, String, String, String, f64)> = conn
            .prepare("SELECT app, detail, category, project, focus_minutes FROM activity_intervals ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .unwrap()
            .filter_map(|x| x.ok())
            .collect();
        assert_eq!(rows[0], ("Discord".into(), "Discord".into(), "neutral".into(), "p9".into(), 1.5));
        assert_eq!(rows[1], ("Discord".into(), "Discord".into(), "neutral".into(), "p9".into(), 1.5));
        assert_eq!(rows[2], ("File Explorer".into(), "File Explorer".into(), "neutral".into(), "p9".into(), 1.5));
        // YouTube stays identifiable for classification; the video name is gone.
        assert_eq!(rows[3], ("Google Chrome".into(), "YouTube".into(), "distracting".into(), "p9".into(), 1.5));
        assert_eq!(rows[4], ("Google Chrome".into(), "".into(), "unclassified".into(), "p9".into(), 1.5));
        // Domain rows (extension attribution) are untouched.
        assert_eq!(rows[5], ("Google Chrome".into(), "docs.rust-lang.org".into(), "productive".into(), "p9".into(), 1.5));
        // Development context untouched.
        assert_eq!(rows[6], ("wezterm-gui".into(), "opencode — cargo build".into(), "productive".into(), "p9".into(), 1.5));
        // Durations/timestamps preserved for every row.
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM activity_intervals WHERE ended_at - started_at = 60000", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 7);
    }
}
