use std::io::Cursor;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "PXL_20251216_122821334.jpg".to_string());
    let bytes = std::fs::read(&path).expect("Failed to read file");
    println!("File: {path}, size: {} bytes", bytes.len());

    match c2pa::Reader::from_stream("image/jpeg", Cursor::new(&bytes)) {
        Ok(reader) => {
            println!("Reader OK");
            println!("Active label: {:?}", reader.active_label());
            println!("Validation state: {:?}", reader.validation_state());
            for m in reader.iter_manifests() {
                println!("  Manifest: label={:?}, title={:?}", m.label(), m.title());
            }
        }
        Err(e) => {
            println!("Reader error: {e}");
            println!("Debug: {e:?}");
        }
    }
}
