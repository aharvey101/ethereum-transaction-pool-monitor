use std::sync::{Arc, Mutex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = rusqlite::Connection::open("dex_pools.db")?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pools",
        [],
        |row| row.get(0),
    )?;
    println!("Pool count: {}", count);
    Ok(())
}
