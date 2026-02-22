use std::io::Cursor;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "PXL_20251216_122821334.jpg".to_string());
    let bytes = std::fs::read(&path).expect("Failed to read file");
    println!("File: {path}, size: {} bytes", bytes.len());

    let mime_type = "image/jpeg";
    let reader = c2pa::Reader::from_stream(mime_type, Cursor::new(&bytes))
        .expect("from_stream failed");

    println!("validation_state: {:?}", reader.validation_state());
    println!("active_label: {:?}", reader.active_label());
    println!("active_manifest: {:?}", reader.active_manifest().is_some());

    // Try to get manifest by label
    if let Some(label) = reader.active_label() {
        println!("Trying get_manifest({:?})...", label);
        let manifest = reader.get_manifest(label);
        println!("get_manifest result: {:?}", manifest.is_some());
        if let Some(m) = manifest {
            println!("  title: {:?}", m.title());
            println!("  format: {:?}", m.format());
            println!("  label: {:?}", m.label());
        }
    }

    // iter_manifests
    println!("\niter_manifests:");
    for m in reader.iter_manifests() {
        println!("  manifest: label={:?}, title={:?}", m.label(), m.title());
    }

    // Try reader JSON
    let json = reader.to_string();
    println!("\nReader JSON (first 500 chars): {}", &json[..std::cmp::min(500, json.len())]);
}
