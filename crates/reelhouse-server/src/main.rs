//! The Reelhouse media server.

mod db;
mod scan;

fn main() {
    println!("reelhouse-server {}", env!("CARGO_PKG_VERSION"));
}
