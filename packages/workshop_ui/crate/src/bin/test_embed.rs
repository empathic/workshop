use workshop_ui::Assets;

fn main() {
    // v2
    println!("Testing embedded assets...\n");

    // List all embedded files
    println!("Embedded files:");
    for file in Assets::iter() {
        println!("  - {}", file);
    }
    println!();
}
