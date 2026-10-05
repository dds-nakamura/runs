#![forbid(unsafe_code)]

fn main() {
    println!("runs {}", env!("CARGO_PKG_VERSION"));
}
