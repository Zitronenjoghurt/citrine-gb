use super::sound::channels_value;

#[test]
fn channels_name_the_ones_still_playing() {
    assert_eq!(channels_value(&[true; 4]), "All");
    assert_eq!(channels_value(&[false; 4]), "None");
    assert_eq!(channels_value(&[true, false, true, true]), "1, 3, 4");
}
