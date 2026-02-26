pub fn format_byte_size(bytes: usize) -> String {
    if bytes < 1000 {
        format!("{} B", bytes)
    } else if bytes < 1_000_000 {
        format!("{:.2} KB", bytes as f64 / 1000.0)
    } else {
        format!("{:.2} MB", bytes as f64 / 1000000.0)
    }
}
