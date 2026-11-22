#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Round-trip tests: build a COFF `.rsrc` object with [`damt_build::ResourceBuilder`],
//! parse the resource directory back out of the raw bytes, and assert the tree contents.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use damt_build::{
    CoffResources, RT_GROUP_ICON, RT_ICON, RT_MANIFEST, RT_VERSION, ResourceBuilder, VersionInfo,
};

const TARGET_X64: &str = "x86_64-pc-windows-msvc";

fn temp_dir(tag: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "damt-build-test-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn real_eqho_ico() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/eqho.ico")
}

fn le16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn le32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[derive(Debug, Clone, PartialEq)]
struct Resource {
    kind: u32,
    id: u32,
    lang: u32,
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
struct Reloc {
    offset: u32,
    symbol: u16,
    kind: u16,
}

/// Parses the `.rsrc` section of the produced COFF object back into resources.
fn parse_coff_rsrc(bytes: &[u8]) -> (Vec<Resource>, Vec<Reloc>, u16) {
    assert_eq!(le16(bytes, 2), 1, "NumberOfSections");
    let machine = le16(bytes, 0);
    assert_eq!(le32(bytes, 12), 2, "NumberOfSymbols (section symbol + aux)");

    let section = 20;
    assert_eq!(&bytes[section..section + 5], b".rsrc", "section name");
    let raw_size = le32(bytes, section + 16) as usize;
    let raw_ptr = le32(bytes, section + 20) as usize;
    let reloc_ptr = le32(bytes, section + 24) as usize;
    let nreloc = le16(bytes, section + 32);

    let raw = &bytes[raw_ptr..raw_ptr + raw_size];
    let mut resources = Vec::new();
    walk_dir(raw, 0, &mut Vec::new(), &mut resources);

    let mut relocations = Vec::new();
    for index in 0..usize::from(nreloc) {
        let entry = reloc_ptr + index * 10;
        relocations.push(Reloc {
            offset: le32(bytes, entry),
            symbol: le16(bytes, entry + 6),
            kind: le16(bytes, entry + 8),
        });
    }
    (resources, relocations, machine)
}

fn walk_dir(raw: &[u8], offset: usize, path: &mut Vec<u32>, out: &mut Vec<Resource>) {
    let named = le16(raw, offset + 12);
    let ids = le16(raw, offset + 14);
    for index in 0..usize::from(named + ids) {
        let entry = offset + 16 + index * 8;
        let id = le32(raw, entry);
        let target = le32(raw, entry + 4);
        path.push(id);
        if target & 0x8000_0000 != 0 {
            walk_dir(raw, (target & 0x7FFF_FFFF) as usize, path, out);
        } else {
            assert_eq!(path.len(), 3, "type -> id -> language levels");
            let data_entry = target as usize;
            let rva = le32(raw, data_entry) as usize;
            let size = le32(raw, data_entry + 4) as usize;
            assert!(rva + size <= raw.len(), "data entry inside the section");
            assert_eq!(rva % 8, 0, "resource data is 8-byte aligned");
            out.push(Resource {
                kind: path[0],
                id: path[1],
                lang: path[2],
                data: raw[rva..rva + size].to_vec(),
            });
        }
        path.pop();
    }
}

fn resources_of_kind(resources: &[Resource], kind: u32) -> Vec<Resource> {
    resources
        .iter()
        .filter(|r| r.kind == kind)
        .cloned()
        .collect()
}

/// A `GRPICONDIR` + `GRPICONDIRENTRY` view over a `RT_GROUP_ICON` blob.
fn parse_group_icon(data: &[u8]) -> Vec<(u8, u8, u32, u16)> {
    assert_eq!(le16(data, 0), 0, "reserved");
    assert_eq!(le16(data, 2), 1, "type: icon");
    let count = le16(data, 4);
    let mut entries = Vec::new();
    for index in 0..usize::from(count) {
        let entry = 6 + index * 14;
        entries.push((
            data[entry],
            data[entry + 1],
            le32(data, entry + 8),
            le16(data, entry + 12),
        ));
    }
    entries
}

fn synthetic_ico(images: &[(u8, u8, &[u8])]) -> Vec<u8> {
    let count = images.len();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0]); // reserved
    out.extend_from_slice(&[1, 0]); // type: icon
    out.extend_from_slice(&(count as u16).to_le_bytes());
    let mut offset = 6 + 16 * count;
    for (width, height, data) in images {
        out.push(*width);
        out.push(*height);
        out.push(0); // color count
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bit count
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += data.len();
    }
    for (_, _, data) in images {
        out.extend_from_slice(data);
    }
    out
}

fn version_info() -> VersionInfo {
    VersionInfo {
        company_name: "damt.xyz".to_owned(),
        product_name: "Ocy".to_owned(),
        file_version: "0.2.0-dev".to_owned(),
        product_version: "0.2.0-dev".to_owned(),
        legal_copyright: "Copyright (C) 2026 Daniel Mitev (Daniel Mevit)".to_owned(),
    }
}

fn utf16(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

#[test]
fn manifest_icon_and_version_round_trip() {
    let dir = temp_dir("round-trip");
    let ico_path = dir.join("synthetic.ico");
    let png_a: &[u8] = b"\x89PNG\r\n\x1a\nfake-16px";
    let png_b: &[u8] = b"\x89PNG\r\n\x1a\nfake-32px-payload";
    fs::write(
        &ico_path,
        synthetic_ico(&[(16, 16, png_a), (32, 32, png_b)]),
    )
    .expect("write ico");

    let manifest_xml =
        "<?xml version=\"1.0\"?><assembly xmlns=\"urn:schemas-microsoft-com:asm.v1\"/>";
    let resources = ResourceBuilder::new()
        .manifest(manifest_xml)
        .icon(&ico_path)
        .version_info(version_info())
        .write_coff(&dir, "app", TARGET_X64)
        .expect("write coff");

    let bytes = fs::read(&resources.object_path).expect("read object");
    let (resources_out, relocations, machine) = parse_coff_rsrc(&bytes);

    assert_eq!(machine, 0x8664, "AMD64 machine");
    assert_eq!(
        resources.link_args,
        vec![
            "/MANIFEST:NO".to_owned(),
            resources.object_path.to_string_lossy().into_owned()
        ]
    );

    // RT_MANIFEST x 1: id 1, en-US, exact XML bytes.
    let manifests = resources_of_kind(&resources_out, RT_MANIFEST);
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].id, 1);
    assert_eq!(manifests[0].lang, 0x0409);
    assert_eq!(manifests[0].data, manifest_xml.as_bytes());

    // RT_ICON x 2: ids 1 and 2, language neutral, byte-identical payloads.
    let icons = resources_of_kind(&resources_out, RT_ICON);
    assert_eq!(icons.len(), 2);
    assert_eq!(icons[0].id, 1);
    assert_eq!(icons[1].id, 2);
    for icon in &icons {
        assert_eq!(icon.lang, 0, "language neutral");
    }
    assert_eq!(icons[0].data, png_a);
    assert_eq!(icons[1].data, png_b);

    // RT_GROUP_ICON x 1: id 1, language neutral, referencing icon ids 1 and 2.
    let groups = resources_of_kind(&resources_out, RT_GROUP_ICON);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, 1);
    assert_eq!(groups[0].lang, 0, "language neutral");
    assert_eq!(
        parse_group_icon(&groups[0].data),
        vec![
            (16, 16, png_a.len() as u32, 1),
            (32, 32, png_b.len() as u32, 2)
        ]
    );

    // RT_VERSION x 1: id 1, en-US, with the fixed signature, numeric version, and strings.
    let versions = resources_of_kind(&resources_out, RT_VERSION);
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].id, 1);
    assert_eq!(versions[0].lang, 0x0409);
    let version = &versions[0].data;
    let root_length = le16(version, 0);
    assert_eq!(
        root_length as usize,
        version.len(),
        "wLength covers the block"
    );
    assert_eq!(
        le16(version, 2),
        52,
        "wValueLength = sizeof(VS_FIXEDFILEINFO)"
    );
    // Header (6) + "VS_VERSION_INFO\0" (32) padded to 40 -> VS_FIXEDFILEINFO at 40.
    assert_eq!(le32(version, 40), 0xFEEF_04BD, "dwSignature");
    assert_eq!(le32(version, 48), 0x0000_0002, "dwFileVersionMS = 0.2");
    assert_eq!(le32(version, 52), 0, "dwFileVersionLS = 0.0");
    for text in [
        "StringFileInfo",
        "040904b0",
        "CompanyName",
        "damt.xyz",
        "FileVersion",
        "0.2.0-dev",
        "LegalCopyright",
        "Copyright (C) 2026 Daniel Mitev (Daniel Mevit)",
        "ProductName",
        "Ocy",
        "ProductVersion",
        "VarFileInfo",
        "Translation",
    ] {
        assert!(
            contains_subslice(version, &utf16(text)),
            "version info must contain {text}"
        );
    }
    assert_eq!(le32(version, version.len() - 4), 0x04B0_0409, "Translation");

    // One ADDR32NB relocation per data entry (manifest + 2 icons + group + version = 5).
    assert_eq!(resources_out.len(), 5);
    assert_eq!(relocations.len(), 5);
    for reloc in &relocations {
        assert_eq!(reloc.symbol, 0, "against the .rsrc section symbol");
        assert_eq!(reloc.kind, 3, "IMAGE_REL_AMD64_ADDR32NB");
    }
    let mut offsets: Vec<u32> = relocations.iter().map(|r| r.offset).collect();
    offsets.sort_unstable();
    offsets.dedup();
    assert_eq!(offsets.len(), relocations.len(), "distinct RVA fields");
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn real_eqho_ico_embeds_all_entries() {
    let ico_path = real_eqho_ico();
    let ico = fs::read(&ico_path).expect("read eqho.ico");
    assert_eq!(le16(&ico, 0), 0, "reserved");
    assert_eq!(le16(&ico, 2), 1, "type: icon");
    let count = usize::from(le16(&ico, 4));
    assert!(count >= 1, "at least one icon image");

    let expected: Vec<(u8, u8, u32)> = (0..count)
        .map(|index| {
            let entry = 6 + index * 16;
            (ico[entry], ico[entry + 1], le32(&ico, entry + 8))
        })
        .collect();
    let expected_sizes: Vec<u32> = expected.iter().map(|(_, _, size)| *size).collect();

    let dir = temp_dir("real-ico");
    let resources = ResourceBuilder::new()
        .icon(&ico_path)
        .write_coff(&dir, "eqho", TARGET_X64)
        .expect("write coff");
    assert_eq!(
        resources.link_args.len(),
        1,
        "no /MANIFEST:NO without a manifest"
    );

    let bytes = fs::read(&resources.object_path).expect("read object");
    let (resources_out, relocations, machine) = parse_coff_rsrc(&bytes);
    assert_eq!(machine, 0x8664);

    let icons = resources_of_kind(&resources_out, RT_ICON);
    assert_eq!(icons.len(), count, "RT_ICON count matches ico");
    for (index, icon) in icons.iter().enumerate() {
        assert_eq!(icon.id, index as u32 + 1);
        assert_eq!(icon.lang, 0);
        assert_eq!(icon.data.len() as u32, expected_sizes[index]);
    }

    let groups = resources_of_kind(&resources_out, RT_GROUP_ICON);
    assert_eq!(groups.len(), 1, "RT_GROUP_ICON x 1");
    assert_eq!(groups[0].id, 1);
    assert_eq!(groups[0].lang, 0);
    let entries = parse_group_icon(&groups[0].data);
    assert_eq!(entries.len(), count);
    for (index, (width, height, size, id)) in entries.iter().enumerate() {
        assert_eq!(*id, index as u16 + 1, "group references icon ids in order");
        assert_eq!((*width, *height, *size), expected[index]);
    }

    assert_eq!(relocations.len(), count + 1, "one relocation per data entry");
}

#[test]
fn invalid_ico_is_rejected() {
    let dir = temp_dir("bad-ico");
    let ico_path = dir.join("bad.ico");
    fs::write(&ico_path, b"not an icon at all").expect("write ico");

    let result = ResourceBuilder::new()
        .icon(&ico_path)
        .write_coff(&dir, "bad", TARGET_X64);
    assert!(result.is_err(), "a non-ico file must be rejected");
}

#[test]
fn empty_builder_writes_a_valid_empty_directory() {
    let dir = temp_dir("empty");
    let resources: CoffResources = ResourceBuilder::new()
        .write_coff(&dir, "empty", TARGET_X64)
        .expect("write coff");
    let bytes = fs::read(&resources.object_path).expect("read object");
    let (resources_out, relocations, machine) = parse_coff_rsrc(&bytes);
    assert!(resources_out.is_empty());
    assert!(relocations.is_empty());
    assert_eq!(machine, 0x8664);
    assert_eq!(resources.link_args.len(), 1);
}
