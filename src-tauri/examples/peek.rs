use rusqlite::Connection;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let c = Connection::open(&args[0]).unwrap();
    let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    println!("journal_mode: {mode}");
}
