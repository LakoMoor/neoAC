//! `dump_acd <car folder> <file>`: prints a file out of a car's data.acd.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let folder = std::path::Path::new(&a[1]);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    let acd = rustyac_content::acd::Acd::open_as(&folder.join("data.acd"), &name).expect("open");
    match acd.get(&a[2]) {
        Some(b) => println!("{}", String::from_utf8_lossy(b)),
        None => println!("no such file; files: {:?}", acd.names().collect::<Vec<_>>()),
    }
}
