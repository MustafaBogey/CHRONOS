use chronos_lib::classifier;
fn main() {
    // Uses the same environment-based path resolution as devtool.rs — never a
    // machine-specific absolute path.
    let base = std::env::var("APP_DATA_DIR").unwrap_or_else(|_| {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        format!("{appdata}/app.chronos.desktop/chronos")
    });
    let db = std::path::Path::new(&base).join("chronos.db");
    let conn = rusqlite::Connection::open(&db).unwrap_or_else(|e| {
        eprintln!("could not open {}: {e} (set APP_DATA_DIR to override)", db.display());
        std::process::exit(1);
    });
    let rules = classifier::load_rules(&conn);
    println!("rules: {:?}", rules);
    let (p, s) = classifier::attribute_project(&conn, "wezterm-gui", "wezterm-gui", None, &rules);
    println!("attribution: '{p}' source='{s}'");
}
