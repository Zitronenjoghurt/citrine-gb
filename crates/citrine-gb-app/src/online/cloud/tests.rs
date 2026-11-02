use super::device::browser_on;
use super::*;

fn entry(version: u64, hash: u8, mark: Mark) -> Entry {
    Entry {
        version,
        hash: [hash; 32],
        mark,
    }
}

#[test]
fn equal_copies_are_adopted() {
    assert_eq!(
        decide(Some([1; 32]), Some((3, [1; 32])), None, false, false),
        Step::Adopt
    );
}

#[test]
fn first_meeting_of_two_different_copies_is_a_conflict() {
    assert_eq!(
        decide(Some([1; 32]), Some((3, [2; 32])), None, false, false),
        Step::Conflict
    );
    assert_eq!(
        decide(Some([1; 32]), Some((3, [2; 32])), None, true, false),
        Step::Upload(3)
    );
}

#[test]
fn one_side_changed_moves_that_way() {
    let synced = entry(3, 1, Mark::Synced);
    assert_eq!(
        decide(
            Some([2; 32]),
            Some((3, [1; 32])),
            Some(&synced),
            false,
            false
        ),
        Step::Upload(3)
    );
    assert_eq!(
        decide(
            Some([1; 32]),
            Some((4, [5; 32])),
            Some(&synced),
            false,
            false
        ),
        Step::Fetch
    );
    assert_eq!(
        decide(
            Some([1; 32]),
            Some((4, [5; 32])),
            Some(&synced),
            false,
            true
        ),
        Step::Conflict
    );
    assert_eq!(
        decide(
            Some([2; 32]),
            Some((4, [5; 32])),
            Some(&synced),
            false,
            false
        ),
        Step::Conflict
    );
}

#[test]
fn new_saves_upload_and_missing_ones_come_down() {
    assert_eq!(
        decide(Some([1; 32]), None, None, false, false),
        Step::Upload(0)
    );
    assert_eq!(
        decide(None, Some((2, [1; 32])), None, false, false),
        Step::Fetch
    );
    assert_eq!(
        decide(None, Some((2, [1; 32])), None, false, true),
        Step::Conflict
    );
}

#[test]
fn deletes_follow_the_ledger() {
    let synced = entry(3, 1, Mark::Synced);
    assert_eq!(
        decide(Some([1; 32]), None, Some(&synced), false, false),
        Step::CloudGone
    );
    let gone = entry(3, 1, Mark::CloudGone);
    assert_eq!(
        decide(Some([1; 32]), None, Some(&gone), false, false),
        Step::Keep
    );
    assert_eq!(
        decide(Some([2; 32]), None, Some(&gone), false, false),
        Step::Upload(0)
    );
    let both = entry(3, 1, Mark::DeletedHere { cloud_too: true });
    assert_eq!(
        decide(None, Some((3, [1; 32])), Some(&both), false, false),
        Step::DeleteCloud(3)
    );
    assert_eq!(
        decide(None, Some((4, [1; 32])), Some(&both), false, false),
        Step::Fetch
    );
    let here = entry(3, 1, Mark::DeletedHere { cloud_too: false });
    assert_eq!(
        decide(None, Some((3, [1; 32])), Some(&here), false, false),
        Step::Keep
    );
    assert_eq!(
        decide(None, Some((3, [1; 32])), Some(&synced), true, false),
        Step::DeleteCloud(3)
    );
    assert_eq!(
        decide(None, Some((3, [1; 32])), Some(&synced), false, false),
        Step::Fetch
    );
    assert_eq!(
        decide(None, None, Some(&synced), false, false),
        Step::Forget
    );
}

#[test]
fn devices_read_as_browser_and_system() {
    let firefox = "Mozilla/5.0 (Android 14; Mobile; rv:131.0) Gecko/131.0 Firefox/131.0";
    assert_eq!(browser_on(firefox), "Firefox on Android");
    let chrome = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";
    assert_eq!(browser_on(chrome), "Chrome on Mac");
}

#[test]
fn copies_are_named_after_their_device() {
    assert_eq!(copy_name("", "slot-2", "Mac"), "Slot 3 from Mac");
    assert_eq!(copy_name("Boss", "battery", "Mac"), "Boss from Mac");
}
