fn main() {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["status"] {
        std::process::exit(2);
    }
    println!("{{\"mode\":\"disconnected\",\"profiles\":[]}}");
}
