use chronos_lib::classifier;
fn main() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    chronos_lib::db::migrate(&conn).unwrap();
    for title in ["ChatGPT", "Rust help - ChatGPT", "chatgpt.com", "New Tab", "YouTube", "cat videos - YouTube", "ChatGPT - OpenAI"] {
        let d = classifier::classify(&conn, "chrome", "Google Chrome", title, None, &[]);
        let detail = classifier::privacy_safe_detail("chrome", "Google Chrome", title, None);
        println!("{:32} -> {:?} conf {:.2} matched {:?} detail '{}'", title, d.cat, d.confidence, d.matched, detail);
    }
}
