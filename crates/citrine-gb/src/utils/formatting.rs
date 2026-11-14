pub fn format_byte_size(bytes: usize) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let (value, unit) = if bytes < 1024 * 1024 {
        (bytes as f64 / 1024.0, "KB")
    } else {
        (bytes as f64 / (1024.0 * 1024.0), "MB")
    };
    if value.fract() == 0.0 {
        format!("{value} {unit}")
    } else {
        format!("{value:.1} {unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_byte_size;

    #[test]
    fn sizes_read_as_cartridges_are_sold() {
        assert_eq!(format_byte_size(0), "0 B");
        assert_eq!(format_byte_size(2 * 1024), "2 KB");
        assert_eq!(format_byte_size(8 * 1024), "8 KB");
        assert_eq!(format_byte_size(256 * 1024), "256 KB");
        assert_eq!(format_byte_size(1024 * 1024), "1 MB");
        assert_eq!(format_byte_size(72 * 16 * 1024), "1.1 MB");
        assert_eq!(format_byte_size(8 * 1024 * 1024), "8 MB");
    }
}
