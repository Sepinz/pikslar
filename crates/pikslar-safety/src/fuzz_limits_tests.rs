use super::*;
use pikslar_format::inflate::STOP_EVERY_BYTES;

#[test]
fn every_seed_project_opens_at_its_charge_and_not_below() {
    let seeds = crate::fuzz::Seeds::load().expect("seeds");
    for bytes in seeds.projects.iter().chain(&seeds.tiled[0]) {
        for case in (0..64).step_by(CHARGE_ONE_IN as usize) {
            assert_eq!(charge_holds(bytes, case), Ok(true), "case {case}");
        }
        assert_eq!(charge_holds(bytes, 1), Ok(false));
    }
}

#[test]
fn every_seed_project_is_refused_at_its_first_charge_with_no_room() {
    let seeds = crate::fuzz::Seeds::load().expect("seeds");
    for bytes in seeds.projects.iter().chain(&seeds.tiled[0]) {
        assert!(piks::charges(bytes).expect("opens") > 0);
        assert!(matches!(
            piks::load_with_room(bytes, 0),
            Err(FormatError::FileTooLarge)
        ));
    }
}

#[test]
fn the_rooms_below_a_charge_reach_every_scale() {
    let charge = 50_000_000;
    let rooms: Vec<usize> = (0..4_000)
        .step_by(CHARGE_ONE_IN as usize)
        .map(|case| room_below(charge, case))
        .collect();
    assert!(rooms.iter().all(|&r| r < charge));
    assert!(rooms.contains(&0), "the first charge is met");
    for (low, high) in [(1, 1_000), (1_000, 1_000_000), (charge / 2, charge)] {
        assert!(
            rooms.iter().any(|r| (low..high).contains(r)),
            "none from {low} to {high}"
        );
    }
    assert_eq!(room_below(1, 8), 0);
}

#[test]
fn a_stop_ends_a_stream_or_lets_it_end_as_before() {
    let raw: Vec<u8> = (0..300_000u32).map(|i| (i / 3) as u8).collect();
    let packed = pikslar_format::deflate::deflate(&raw);
    for after in 0..4 {
        assert_eq!(stop_holds(&packed, raw.len(), &raw, after), Ok(()));
    }
    let wrong = vec![0u8; 3];
    assert!(stop_holds(&packed, raw.len(), &wrong, 1000).is_err());
}

#[test]
fn a_stop_can_end_a_stream_inside_a_block() {
    // One block of more than two mebibytes: asked as it starts, and at each mebibyte made.
    let raw: Vec<u8> = (0..5 * STOP_EVERY_BYTES / 2)
        .map(|i| (i / 5) as u8)
        .collect();
    let packed = pikslar_format::deflate::deflate(&raw);
    let asked = std::cell::Cell::new(0usize);
    let count = || {
        asked.set(asked.get() + 1);
        false
    };
    pikslar_format::inflate::inflate_until(&packed, raw.len(), &count).expect("valid");
    assert_eq!(asked.get(), 3, "once before the block and twice inside it");
    for after in 0..4 {
        assert_eq!(stop_holds(&packed, raw.len(), &raw, after), Ok(()));
    }
}

#[test]
fn claimed_projects_of_every_version_are_refused_by_the_limit() {
    for version in 1..=3 {
        let claim = project_claim(version, "small", (4, 8));
        assert!(claim.is_ok(), "version {version}: {:?}", claim.err());
        let bytes = claimed_project(version, 4, 8, 32);
        assert!(
            matches!(piks::load(&bytes), Err(FormatError::Corrupt(_))),
            "with the whole limit, cut short"
        );
    }
}

#[test]
fn a_claim_refused_otherwise_or_opened_fails() {
    let bytes = claimed_project(2, 4, 8, 32);
    let too_large = |e: &FormatError| matches!(e, FormatError::FileTooLarge);
    let whole = |b: &[u8]| piks::load(b).map(|_| ());
    let wanted = (CLAIM_PEAK_LIMIT, Ending::Refused(too_large));
    assert!(held_to("cut short", &bytes, wanted, whole).is_err());
    let ase = claimed_ase(3, 2);
    assert!(import_ase(&ase).is_ok());
    let read = |b: &[u8]| import_ase(b).map(|_| ());
    let refused = (ASE_CLAIM_PEAK_LIMIT, Ending::Refused(too_large));
    assert!(held_to("ase", &ase, refused, read).is_err(), "it opened");
    assert!(held_to("ase", &ase, (ASE_CLAIM_PEAK_LIMIT, Ending::Opens), read).is_ok());
    assert!(held_to("ase", &ase, (ASE_CLAIM_PEAK_LIMIT, Ending::Either), read).is_ok());
    assert!(
        held_to("ase", &ase, (0, Ending::Opens), read).is_err(),
        "held too much"
    );
}

#[test]
fn the_claimed_tilemap_file_opens_with_every_layer_over_one_tileset() {
    let bytes = claimed_ase_tilemaps(5, 64);
    let (doc, _) = import_ase(&bytes).expect("opens");
    assert_eq!(doc.layers().len(), 6);
    assert_eq!(doc.tilesets.len(), 1);
    assert_eq!(doc.blending, pikslar_core::Blending::Pikslar, "all opaque");
    let read = |b: &[u8]| import_ase(b).map(|_| ());
    let limit = (ASE_TILES_CLAIM_PEAK_LIMIT, Ending::Opens);
    assert!(held_to("tilemaps", &bytes, limit, read).is_ok());
}
