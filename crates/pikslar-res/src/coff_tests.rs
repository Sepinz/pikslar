use super::*;
use crate::read;

fn res(ty: u16, id: u16, lang: u16, data: &[u8]) -> Resource {
    Resource {
        ty,
        id,
        lang,
        data: data.to_vec(),
    }
}

fn sample() -> Vec<Resource> {
    vec![
        res(24, 1, 0x409, b"<assembly/>"),
        res(3, 2, 0x409, &[7; 13]),
        res(3, 1, 0x409, &[5; 40]),
        res(14, 1, 0x409, &[1, 2, 3]),
        res(16, 1, 0x409, &[9; 4]),
        res(16, 1, 0x809, &[8; 6]),
    ]
}

/// The bytes a leaf's relocated address points to, given the section starts at address 0.
fn leaf_data(section: &read::Section, leaf: &read::Leaf) -> Vec<u8> {
    let at = leaf.address as usize;
    section.data[at..at + leaf.size as usize].to_vec()
}

#[test]
fn the_object_is_an_amd64_coff_file_with_one_rsrc_section_and_no_time_stamp() {
    let o = read::coff(&object(&sample()).expect("ok")).expect("ok");
    assert_eq!(o.machine, MACHINE_AMD64);
    assert_eq!(o.timestamp, 0);
    assert_eq!(o.sections.len(), 1);
    assert_eq!(o.sections[0].name, SECTION);
    // Initialised data, readable and writable, as windres marks it.
    assert_eq!(o.sections[0].characteristics, 0xC000_0040);
}

#[test]
fn the_tree_lists_types_ids_and_languages_in_ascending_order() {
    let o = read::coff(&object(&sample()).expect("ok")).expect("ok");
    let keys: Vec<(u16, u16, u16)> = read::leaves(&o.sections[0].data)
        .expect("ok")
        .iter()
        .map(|l| (l.ty, l.id, l.lang))
        .collect();
    let want = [
        (3, 1, 0x409),
        (3, 2, 0x409),
        (14, 1, 0x409),
        (16, 1, 0x409),
        (16, 1, 0x809),
        (24, 1, 0x409),
    ];
    assert_eq!(keys, want);
}

#[test]
fn every_data_entry_has_an_addr32nb_relocation_against_the_section_symbol() {
    let o = read::coff(&object(&sample()).expect("ok")).expect("ok");
    let s = &o.sections[0];
    let leaves = read::leaves(&s.data).expect("ok");
    assert_eq!(s.relocs.len(), leaves.len());
    for leaf in &leaves {
        let r = s
            .relocs
            .iter()
            .find(|r| r.offset == leaf.entry)
            .expect("relocation");
        assert_eq!(r.kind, REL_ADDR32NB);
        let sym = o.symbols[r.symbol as usize].as_ref().expect("symbol");
        assert_eq!(
            (sym.name, sym.section, sym.value, sym.class),
            (SECTION, 1, 0, 3)
        );
    }
}

#[test]
fn each_entry_addresses_its_own_bytes_eight_byte_aligned() {
    let input = sample();
    let o = read::coff(&object(&input).expect("ok")).expect("ok");
    let s = &o.sections[0];
    for leaf in read::leaves(&s.data).expect("ok") {
        let want = input
            .iter()
            .find(|r| (r.ty, r.id, r.lang) == (leaf.ty, leaf.id, leaf.lang));
        assert_eq!(Some(leaf_data(s, &leaf)), want.map(|r| r.data.clone()));
        assert_eq!(leaf.address % 8, 0);
    }
}

#[test]
fn identical_input_gives_identical_bytes_whatever_the_order() {
    let mut shuffled = sample();
    shuffled.reverse();
    assert_eq!(
        object(&sample()).expect("ok"),
        object(&shuffled).expect("ok")
    );
    assert_eq!(
        object(&sample()).expect("ok"),
        object(&sample()).expect("ok")
    );
}

#[test]
fn a_duplicate_resource_is_refused() {
    let mut input = sample();
    input.push(res(24, 1, 0x409, b"again"));
    assert!(object(&input).expect_err("refused").contains("duplicate"));
}

#[test]
fn an_empty_list_gives_a_valid_object_with_an_empty_tree() {
    let o = read::coff(&object(&[]).expect("ok")).expect("ok");
    assert!(read::leaves(&o.sections[0].data).expect("ok").is_empty());
}

#[test]
fn the_empty_object_has_no_sections_or_symbols() {
    let o = read::coff(&crate::empty_object()).expect("ok");
    assert_eq!(o.machine, MACHINE_AMD64);
    assert!(o.sections.is_empty() && o.symbols.is_empty());
}
