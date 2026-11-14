pub(crate) const HEIGHT: usize = 5;
const WIDTH: usize = 3;
const GAP: usize = 1;

fn glyph(c: char) -> Option<[u8; HEIGHT]> {
    Some(match c.to_ascii_uppercase() {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 3, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' | 'É' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '!' => [2, 2, 2, 0, 2],
        '?' => [6, 1, 2, 0, 2],
        '.' => [0, 0, 0, 0, 2],
        ',' => [0, 0, 0, 2, 4],
        ':' => [0, 2, 0, 2, 0],
        '-' => [0, 0, 7, 0, 0],
        '/' => [1, 1, 2, 4, 4],
        '\'' => [2, 2, 0, 0, 0],
        ' ' => [0; HEIGHT],
        _ => return None,
    })
}

pub(crate) fn width(text: &str) -> usize {
    let count = text.chars().filter(|c| glyph(*c).is_some()).count();
    (count * (WIDTH + GAP)).saturating_sub(GAP)
}

pub(crate) fn draw(text: &str, mut x: usize, y: usize, mut put: impl FnMut(usize, usize)) {
    for rows in text.chars().filter_map(glyph) {
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..WIDTH {
                if bits & (4 >> col) != 0 {
                    put(x + col, y + row);
                }
            }
        }
        x += WIDTH + GAP;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(text: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        draw(text, 0, 0, |x, y| out.push((x, y)));
        out
    }

    #[test]
    fn width_counts_glyphs_and_the_gaps_between_them() {
        assert_eq!(width(""), 0);
        assert_eq!(width("1"), 3);
        assert_eq!(width("NEED WATTS"), 39);
    }

    #[test]
    fn lowercase_draws_as_uppercase() {
        assert_eq!(pixels("watts"), pixels("WATTS"));
    }

    #[test]
    fn unknown_characters_take_no_room() {
        assert_eq!(width("A~B"), width("AB"));
        assert_eq!(pixels("A~B"), pixels("AB"));
    }

    #[test]
    fn one_is_drawn_from_its_rows() {
        let one = pixels("1");
        assert!(one.contains(&(1, 0)));
        assert!(one.contains(&(0, 1)));
        assert!(!one.contains(&(0, 0)));
        assert_eq!(one.iter().filter(|(_, y)| *y == 4).count(), 3);
    }
}
