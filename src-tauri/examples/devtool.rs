//! Development verification tool (not shipped): inspect and steer the local CHRONOS database.
//!
//! Usage:
//!   cargo run --example devtool -- status
//!   cargo run --example devtool -- query "SELECT COUNT(*) FROM activity_intervals"
//!   cargo run --example devtool -- sql "UPDATE settings SET value='1' WHERE key='onboarded'"

use chrono::Local;
use rusqlite::Connection;
use std::path::PathBuf;

fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("CHRONOS_DB") {
        return PathBuf::from(p);
    }
    let base = std::env::var("APP_DATA_DIR").unwrap_or_else(|_| {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        format!("{appdata}/app.chronos.desktop/chronos")
    });
    PathBuf::from(base).join("chronos.db")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = db_path();
    println!("db: {}", path.display());
    let conn = match Connection::open(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("open failed: {e}");
            std::process::exit(1);
        }
    };
    match args.first().map(|s| s.as_str()) {
        Some("status") => {
            let intervals: i64 = conn.query_row("SELECT COUNT(*) FROM activity_intervals", [], |r| r.get(0)).unwrap_or(-1);
            let open_rows: i64 = conn.query_row("SELECT COUNT(*) FROM activity_intervals WHERE open = 1", [], |r| r.get(0)).unwrap_or(-1);
            let tracking: String = conn.query_row("SELECT state FROM tracking_state WHERE id = 1", [], |r| r.get(0)).unwrap_or_else(|_| "none".into());
            let onboarded: String = conn.query_row("SELECT value FROM settings WHERE key='onboarded'", [], |r| r.get(0)).unwrap_or_else(|_| "0".into());
            let clean: String = conn.query_row("SELECT value FROM meta WHERE key='clean_shutdown'", [], |r| r.get(0)).unwrap_or_else(|_| "?".into());
            let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM focus_sessions", [], |r| r.get(0)).unwrap_or(-1);
            println!("intervals={intervals} open={open_rows} tracking={tracking} onboarded={onboarded} clean_shutdown={clean} sessions={sessions} now={}", Local::now());
        }
        Some("query") => {
            let sql = args.get(1).expect("sql required");
            let mut stmt = conn.prepare(sql).expect("prepare");
            let cols = stmt.column_count();
            let mut rows = stmt.query([]).expect("query");
            while let Some(row) = rows.next().expect("row") {
                let mut out = Vec::new();
                for i in 0..cols {
                    let v: String = match row.get_ref(i) {
                        Ok(rusqlite::types::ValueRef::Null) => "NULL".into(),
                        Ok(rusqlite::types::ValueRef::Integer(n)) => n.to_string(),
                        Ok(rusqlite::types::ValueRef::Real(f)) => f.to_string(),
                        Ok(rusqlite::types::ValueRef::Text(t)) => String::from_utf8_lossy(t).to_string(),
                        Ok(rusqlite::types::ValueRef::Blob(b)) => format!("<{} bytes>", b.len()),
                        Err(_) => "?".into(),
                    };
                    out.push(v);
                }
                println!("{}", out.join(" | "));
            }
        }
        Some("sql") => {
            let sql = args.get(1).expect("sql required");
            let n = conn.execute(sql, []).expect("execute");
            println!("{n} rows affected");
        }
        _ => {
            eprintln!("commands: status | query <sql> | sql <statement>");
            std::process::exit(2);
        }
    }
}
