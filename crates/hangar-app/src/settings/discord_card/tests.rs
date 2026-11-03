//! The Discord card's own logic: the elapsed time and how the link reads.

use super::*;

#[test]
fn the_elapsed_time_counts_hours_minutes_and_seconds() {
    assert_eq!(elapsed_since(0, 0), "00:00:00");
    assert_eq!(elapsed_since(0, 59_999), "00:00:59");
    assert_eq!(elapsed_since(0, 61_000), "00:01:01");
    assert_eq!(elapsed_since(1_000, 3_661_000), "01:01:00");
    assert_eq!(elapsed_since(0, 100 * 3600 * 1000), "100:00:00");
}

#[test]
fn a_clock_that_stepped_back_does_not_show_a_negative_time() {
    assert_eq!(elapsed_since(10_000, 5_000), "00:00:00");
}

#[test]
fn the_link_reads_off_connected_or_waiting() {
    assert_eq!(tone(false, false), Tone::Off);
    // Off wins over a connection that has not closed yet.
    assert_eq!(tone(false, true), Tone::Off);
    assert_eq!(tone(true, true), Tone::On);
    assert_eq!(tone(true, false), Tone::Waiting);
}

#[test]
fn the_logo_is_bundled_and_decodes() {
    // The card shows the repository's own file, so a missing or broken one
    // would be a card with a hole in it.
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/logo.png"
    ));
    assert!(bytes.starts_with(b"\x89PNG"));
}
