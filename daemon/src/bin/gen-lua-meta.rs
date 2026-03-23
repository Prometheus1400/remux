#[path = "../lua/meta.rs"]
mod meta;

fn main() {
    if let Err(err) = meta::write_generated_meta_file() {
        eprintln!("failed to write Lua metadata: {err}");
        std::process::exit(1);
    }
}
