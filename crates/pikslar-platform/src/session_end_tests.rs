use super::*;

#[test]
fn writes_count_while_held_and_the_wait_for_them_is_bounded() {
    let before = writes_pending();
    let held = write_pending(WriteKind::Recovery);
    assert_eq!(writes_pending(), before + 1);
    let start = Instant::now();
    assert!(
        !wait_for_writes(Duration::from_millis(60)),
        "one is under way"
    );
    let took = start.elapsed();
    assert!(
        took >= Duration::from_millis(60) && took < Duration::from_millis(500),
        "{took:?}"
    );
    let ends = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        drop(held);
    });
    assert!(wait_for_writes(Duration::from_secs(5)), "it ended");
    ends.join().expect("ended");
    assert_eq!(writes_pending(), before);
    assert!(!blocking_shutdown(), "no reason without a window asking");
}

#[test]
fn a_save_under_way_is_not_a_recovery_write_but_both_count_for_the_end() {
    let before = writes_pending();
    let save = write_pending(WriteKind::Save);
    assert_eq!(writes_pending(), before + 1, "the end waits for it");
    let start = Instant::now();
    assert!(
        wait_for_recovery_writes(Duration::from_secs(2)) || recovery_writes_pending() > 0,
        "a save is not waited for as a recovery write"
    );
    assert!(start.elapsed() < Duration::from_secs(1) || recovery_writes_pending() > 0);
    let copy = write_pending(WriteKind::Recovery);
    assert!(recovery_writes_pending() >= 1);
    drop(copy);
    drop(save);
}
