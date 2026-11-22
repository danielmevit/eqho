//! Build-script helpers for the damt kits: a pure-Rust Windows resource writer.
//!
//! [`ResourceBuilder`] collects Windows resources (an application manifest, the images of an
//! `.ico` file, and a `VERSIONINFO`) and emits them as a COFF object with a single `.rsrc`
//! section, ready to be passed to the linker with `cargo:rustc-link-arg-bins`. No external
//! resource compiler (`rc.exe`, `llvm-rc`, `mt.exe`, `clang-cl`) is needed, so the same path
//! works for native builds and cross-compilation from Linux.
//!
//! ```no_run
//! # use std::path::Path;
//! # fn demo(out_dir: &Path, target: &str) -> std::io::Result<()> {
//! let resources = damt_build::ResourceBuilder::new()
//!     .manifest("<?xml version=\"1.0\"?><assembly/>")
//!     .icon("assets/app.ico")
//!     .write_coff(out_dir, "app-resources", target)?;
//! for arg in &resources.link_args {
//!     println!("cargo:rustc-link-arg-bins={arg}");
//! }
//! # Ok(())
//! # }
//! ```

#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// `RT_ICON`: a single image from an `.ico` file.
pub const RT_ICON: u32 = 3;
/// `RT_GROUP_ICON`: the `GRPICONDIR` directory that ties the `RT_ICON` images together.
pub const RT_GROUP_ICON: u32 = 14;
/// `RT_VERSION`: the `VERSIONINFO` block with product and file metadata.
pub const RT_VERSION: u32 = 16;
/// `RT_MANIFEST`: the application manifest (XML).
pub const RT_MANIFEST: u32 = 24;

/// The `VERSIONINFO` string table values written into the `RT_VERSION` resource.
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// `CompanyName` string value.
    pub company_name: String,
    /// `ProductName` string value.
    pub product_name: String,
    /// `FileVersion` string value (usually the package version).
    pub file_version: String,
    /// `ProductVersion` string value (usually the package version).
    pub product_version: String,
    /// `LegalCopyright` string value.
    pub legal_copyright: String,
}

/// The COFF object written by [`ResourceBuilder::write_coff`].
#[derive(Debug)]
pub struct CoffResources {
    /// Path of the written object file.
    pub object_path: PathBuf,
    /// Linker arguments to pass via `cargo:rustc-link-arg-bins` (the object path, and
    /// `/MANIFEST:NO` when a manifest is embedded so the linker does not try to merge
    /// another manifest with `mt.exe`).
    pub link_args: Vec<String>,
}

#[derive(Debug)]
struct ResourceEntry {
    kind: u32,
    id: u32,
    lang: u32,
    data: Vec<u8>,
}

/// Collects Windows resources and writes them as a pure-Rust COFF `.rsrc` object.
///
/// See the [crate documentation](crate) for an example.
#[derive(Debug, Default)]
pub struct ResourceBuilder {
    entries: Vec<ResourceEntry>,
    icon_paths: Vec<PathBuf>,
    has_manifest: bool,
}

impl ResourceBuilder {
    /// Creates an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `RT_MANIFEST` resource id 1 (language `0x0409`) with the given manifest XML.
    pub fn manifest(mut self, xml: &str) -> Self {
        self.entries.push(ResourceEntry {
            kind: RT_MANIFEST,
            id: 1,
            lang: 0x0409,
            data: xml.as_bytes().to_vec(),
        });
        self.has_manifest = true;
        self
    }

    /// Adds one `RT_ICON` resource per image in the `.ico` file (ids 1..=n, language
    /// neutral) plus one `RT_GROUP_ICON` resource id 1 (language neutral) whose
    /// `GRPICONDIR` entries reference the icon ids. The file is read when
    /// [`write_coff`](Self::write_coff) runs.
    pub fn icon(mut self, path: impl Into<PathBuf>) -> Self {
        self.icon_paths.push(path.into());
        self
    }

    /// Adds `RT_VERSION` resource id 1 (language `0x0409`) with the given metadata.
    pub fn version_info(mut self, info: VersionInfo) -> Self {
        self.entries.push(ResourceEntry {
            kind: RT_VERSION,
            id: 1,
            lang: 0x0409,
            data: build_version_info(&info),
        });
        self
    }

    /// Writes the COFF object `out_dir/<stem>.o` for `target` (a Rust target triple) and
    /// returns its path plus the linker arguments to pass to `rustc`.
    pub fn write_coff(self, out_dir: &Path, stem: &str, target: &str) -> io::Result<CoffResources> {
        let mut entries = self.entries;
        for path in &self.icon_paths {
            let bytes = fs::read(path)?;
            let images =
                parse_ico(&bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            if images.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("ico file `{}` holds no images", path.display()),
                ));
            }
            let group = build_group_icon(&images);
            for (index, image) in images.iter().enumerate() {
                let id = u32::try_from(index + 1).unwrap_or(u32::MAX);
                entries.push(ResourceEntry {
                    kind: RT_ICON,
                    id,
                    lang: 0,
                    data: image.data.clone(),
                });
            }
            entries.push(ResourceEntry {
                kind: RT_GROUP_ICON,
                id: 1,
                lang: 0,
                data: group,
            });
        }

        let (raw, reloc_offsets) = build_resource_section(&entries);
        let object = build_coff_object(&raw, &reloc_offsets, target);

        let object_path = out_dir.join(format!("{stem}.o"));
        fs::write(&object_path, object)?;

        let mut link_args = Vec::with_capacity(2);
        if self.has_manifest {
            link_args.push("/MANIFEST:NO".to_owned());
        }
        link_args.push(object_path.to_string_lossy().into_owned());
        Ok(CoffResources {
            object_path,
            link_args,
        })
    }
}

// ---------------------------------------------------------------------------
// .ico parsing and GRPICONDIR
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct IcoImage {
    width: u8,
    height: u8,
    colors: u8,
    reserved: u8,
    planes: u16,
    bit_count: u16,
    data: Vec<u8>,
}

/// Parses an `.ico` file into its images, validating the `ICONDIR` header.
fn parse_ico(bytes: &[u8]) -> Result<Vec<IcoImage>, String> {
    const HEADER: usize = 6;
    const ENTRY: usize = 16;

    if bytes.len() < HEADER {
        return Err("ico file is shorter than the 6-byte ICONDIR header".to_owned());
    }
    let reserved = u16::from_le_bytes([bytes[0], bytes[1]]);
    let kind = u16::from_le_bytes([bytes[2], bytes[3]]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]);
    if reserved != 0 {
        return Err(format!("ico reserved field is {reserved}, expected 0"));
    }
    if kind != 1 {
        return Err(format!("ico type field is {kind}, expected 1 (icon)"));
    }

    let mut images = Vec::new();
    for index in 0..usize::from(count) {
        let entry = HEADER + index * ENTRY;
        if bytes.len() < entry + ENTRY {
            return Err(format!("ico entry {index} is truncated"));
        }
        let b = &bytes[entry..entry + ENTRY];
        let size = u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize;
        let offset = u32::from_le_bytes([b[12], b[13], b[14], b[15]]) as usize;
        if bytes.len() < offset || bytes.len() - offset < size {
            return Err(format!("ico image {index} data is truncated"));
        }
        images.push(IcoImage {
            width: b[0],
            height: b[1],
            colors: b[2],
            reserved: b[3],
            planes: u16::from_le_bytes([b[4], b[5]]),
            bit_count: u16::from_le_bytes([b[6], b[7]]),
            data: bytes[offset..offset + size].to_vec(),
        });
    }
    Ok(images)
}

/// Builds the `RT_GROUP_ICON` directory (`GRPICONDIR` + `GRPICONDIRENTRY` array) that
/// references icon ids 1..=n in the same order as the images.
fn build_group_icon(images: &[IcoImage]) -> Vec<u8> {
    let count = u16::try_from(images.len()).unwrap_or(u16::MAX);
    let mut out = Vec::with_capacity(6 + images.len() * 14);
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    out.extend_from_slice(&count.to_le_bytes());
    for (index, image) in images.iter().enumerate() {
        let id = u16::try_from(index + 1).unwrap_or(u16::MAX);
        out.push(image.width);
        out.push(image.height);
        out.push(image.colors);
        out.push(image.reserved);
        out.extend_from_slice(&image.planes.to_le_bytes());
        out.extend_from_slice(&image.bit_count.to_le_bytes());
        let size = u32::try_from(image.data.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&size.to_le_bytes()); // dwBytesInRes
        out.extend_from_slice(&id.to_le_bytes()); // nID
    }
    out
}

// ---------------------------------------------------------------------------
// VERSIONINFO
// ---------------------------------------------------------------------------

fn push_u16(buf: &mut Vec<u8>, value: u16) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn push_utf16(buf: &mut Vec<u8>, text: &str) {
    for unit in text.encode_utf16() {
        push_u16(buf, unit);
    }
    push_u16(buf, 0); // terminating NUL
}

fn pad32(buf: &mut Vec<u8>) {
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
}

/// Builds one `VERSIONINFO` block: a header (`wLength`, `wValueLength`, `wType`), the
/// UTF-16 `key`, 32-bit padding, and the `body`. `wLength` is patched in at the end and
/// includes the trailing padding, matching what `rc.exe` writes.
fn version_block(wtype: u16, value_length: u16, key: &str, body: &[u8]) -> Vec<u8> {
    let mut block = Vec::new();
    push_u16(&mut block, 0); // wLength, patched below
    push_u16(&mut block, value_length);
    push_u16(&mut block, wtype);
    push_utf16(&mut block, key);
    pad32(&mut block);
    block.extend_from_slice(body);
    pad32(&mut block);
    let length = u16::try_from(block.len()).unwrap_or(u16::MAX);
    block[0..2].copy_from_slice(&length.to_le_bytes());
    block
}

/// Builds the `VS_VERSIONINFO` resource: a `VS_FIXEDFILEINFO` plus a `StringFileInfo`
/// block (language `0x0409`, code page `0x04B0` UTF-16) and a `VarFileInfo` block.
fn build_version_info(info: &VersionInfo) -> Vec<u8> {
    let strings: [(&str, &str); 5] = [
        ("CompanyName", info.company_name.as_str()),
        ("FileVersion", info.file_version.as_str()),
        ("LegalCopyright", info.legal_copyright.as_str()),
        ("ProductName", info.product_name.as_str()),
        ("ProductVersion", info.product_version.as_str()),
    ];

    // String entries, in the conventional alphabetical order.
    let mut entries = Vec::new();
    for (key, value) in strings {
        let value_chars = value.chars().count() + 1;
        let mut value_bytes = Vec::new();
        push_utf16(&mut value_bytes, value);
        entries.extend_from_slice(&version_block(
            1,
            u16::try_from(value_chars).unwrap_or(u16::MAX),
            key,
            &value_bytes,
        ));
    }

    let table = version_block(1, 0, "040904b0", &entries);
    let string_file_info = version_block(1, 0, "StringFileInfo", &table);

    let mut translation = Vec::new();
    push_u32(&mut translation, 0x04B0_0409); // lang 0x0409, code page 1200 (UTF-16)
    let var = version_block(0, 4, "Translation", &translation);
    let var_file_info = version_block(1, 0, "VarFileInfo", &var);

    // VS_FIXEDFILEINFO (52 bytes).
    let (ms, ls) = version_words(&info.file_version);
    let (pms, pls) = version_words(&info.product_version);
    let mut fixed = Vec::with_capacity(52);
    push_u32(&mut fixed, 0xFEEF_04BD); // dwSignature
    push_u32(&mut fixed, 0x0001_0000); // dwStrucVersion
    push_u32(&mut fixed, ms); // dwFileVersionMS
    push_u32(&mut fixed, ls); // dwFileVersionLS
    push_u32(&mut fixed, pms); // dwProductVersionMS
    push_u32(&mut fixed, pls); // dwProductVersionLS
    push_u32(&mut fixed, 0x0000_003F); // dwFileFlagsMask
    push_u32(&mut fixed, 0); // dwFileFlags
    push_u32(&mut fixed, 0x0004_0004); // dwFileOS: VOS_NT_WINDOWS32
    push_u32(&mut fixed, 0x0000_0001); // dwFileType: VFT_APP
    push_u32(&mut fixed, 0); // dwFileSubtype
    push_u32(&mut fixed, 0); // dwFileDateMS
    push_u32(&mut fixed, 0); // dwFileDateLS

    let mut body = fixed;
    pad32(&mut body);
    body.extend_from_slice(&string_file_info);
    pad32(&mut body);
    body.extend_from_slice(&var_file_info);
    version_block(0, 52, "VS_VERSION_INFO", &body)
}

/// Splits a version string like `0.2.0-dev` into the `(MS, LS)` DWORD pair used by
/// `VS_FIXEDFILEINFO`; non-numeric parts are ignored and missing parts default to 0.
fn version_words(version: &str) -> (u32, u32) {
    let mut parts = [0u32; 4];
    for (slot, part) in version.split('.').enumerate() {
        if slot >= parts.len() {
            break;
        }
        if let Ok(value) = part.parse::<u32>() {
            parts[slot] = value;
        }
    }
    (
        (parts[0] << 16) | (parts[1] & 0xFFFF),
        (parts[2] << 16) | (parts[3] & 0xFFFF),
    )
}

// ---------------------------------------------------------------------------
// Resource directory (.rsrc section data)
// ---------------------------------------------------------------------------

/// Builds the `.rsrc` section data (a three-level resource directory: type -> id ->
/// language -> data entry) and returns the raw bytes plus the section-relative offsets
/// of every data-entry RVA field, which need `ADDR32NB` relocations.
fn build_resource_section(entries: &[ResourceEntry]) -> (Vec<u8>, Vec<u32>) {
    use std::collections::BTreeMap;

    let mut sorted: Vec<&ResourceEntry> = entries.iter().collect();
    sorted.sort_by_key(|entry| (entry.kind, entry.id, entry.lang));

    // Resource directory tree: kind -> id -> language -> index into `sorted`.
    // `BTreeMap` keeps every level in the ascending order the format requires.
    let mut tree: BTreeMap<u32, BTreeMap<u32, BTreeMap<u32, usize>>> = BTreeMap::new();
    for (index, entry) in sorted.iter().enumerate() {
        tree.entry(entry.kind)
            .or_default()
            .entry(entry.id)
            .or_default()
            .insert(entry.lang, index);
    }

    // Pass 1: compute the offset of the root table, every type/id table, every data
    // entry, and every data blob (blobs are 8-byte aligned).
    let mut offset = 0u32;
    let root_offset = offset;
    offset += 16 + 8 * tree.len() as u32;

    let mut kind_table_offsets = BTreeMap::new();
    for (kind, ids) in &tree {
        kind_table_offsets.insert(*kind, offset);
        offset += 16 + 8 * ids.len() as u32;
    }

    let mut id_table_offsets = BTreeMap::new();
    for (kind, ids) in &tree {
        for (id, langs) in ids {
            id_table_offsets.insert((*kind, *id), offset);
            offset += 16 + 8 * langs.len() as u32;
        }
    }

    let mut data_entry_offsets = Vec::with_capacity(sorted.len());
    for _ in &sorted {
        data_entry_offsets.push(offset);
        offset += 16;
    }

    let mut blob_offsets = Vec::with_capacity(sorted.len());
    for entry in &sorted {
        offset = offset.div_ceil(8) * 8;
        blob_offsets.push(offset);
        offset += u32::try_from(entry.data.len()).unwrap_or(u32::MAX);
    }

    // Pass 2: serialize the directory tables, data entries, and blobs.
    let mut raw = Vec::new();
    let mut reloc_offsets = Vec::with_capacity(sorted.len());

    debug_assert_eq!(raw.len() as u32, root_offset);
    push_dir_table(&mut raw, tree.len());
    for (kind, table_offset) in &kind_table_offsets {
        push_dir_entry(&mut raw, *kind, *table_offset, true);
    }

    for (kind, ids) in &tree {
        let table_offset = kind_table_offsets[kind];
        debug_assert_eq!(raw.len() as u32, table_offset);
        push_dir_table(&mut raw, ids.len());
        for id in ids.keys() {
            push_dir_entry(&mut raw, *id, id_table_offsets[&(*kind, *id)], true);
        }
    }

    for (kind, ids) in &tree {
        for (id, langs) in ids {
            let table_offset = id_table_offsets[&(*kind, *id)];
            debug_assert_eq!(raw.len() as u32, table_offset);
            push_dir_table(&mut raw, langs.len());
            for (lang, index) in langs {
                push_dir_entry(&mut raw, *lang, data_entry_offsets[*index], false);
            }
        }
    }

    for (index, entry) in sorted.iter().enumerate() {
        debug_assert_eq!(raw.len() as u32, data_entry_offsets[index]);
        reloc_offsets.push(raw.len() as u32);
        push_data_entry(&mut raw, blob_offsets[index], entry.data.len());
    }

    for (index, entry) in sorted.iter().enumerate() {
        while raw.len() < blob_offsets[index] as usize {
            raw.push(0);
        }
        raw.extend_from_slice(&entry.data);
    }
    while raw.len() % 8 != 0 {
        raw.push(0);
    }

    (raw, reloc_offsets)
}

fn push_dir_table(raw: &mut Vec<u8>, id_count: usize) {
    raw.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    raw.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
    raw.extend_from_slice(&4u16.to_le_bytes()); // MajorVersion
    raw.extend_from_slice(&0u16.to_le_bytes()); // MinorVersion
    raw.extend_from_slice(&0u16.to_le_bytes()); // NumberOfNamedEntries
    raw.extend_from_slice(&(id_count as u16).to_le_bytes()); // NumberOfIdEntries
}

fn push_dir_entry(raw: &mut Vec<u8>, id: u32, offset: u32, subdirectory: bool) {
    raw.extend_from_slice(&id.to_le_bytes());
    let flag = if subdirectory { 0x8000_0000 } else { 0 };
    raw.extend_from_slice(&((offset & 0x7FFF_FFFF) | flag).to_le_bytes());
}

fn push_data_entry(raw: &mut Vec<u8>, rva: u32, size: usize) {
    raw.extend_from_slice(&rva.to_le_bytes()); // OffsetToData (relocated)
    raw.extend_from_slice(&(size as u32).to_le_bytes()); // Size
    raw.extend_from_slice(&0u32.to_le_bytes()); // CodePage
    raw.extend_from_slice(&0u32.to_le_bytes()); // Reserved
}

// ---------------------------------------------------------------------------
// COFF object wrapper
// ---------------------------------------------------------------------------

/// Wraps the `.rsrc` section data in a COFF object file for `target` (a Rust target
/// triple), with one `ADDR32NB` relocation per data-entry RVA field.
fn build_coff_object(raw: &[u8], reloc_offsets: &[u32], target: &str) -> Vec<u8> {
    let (machine, reloc_type): (u16, u16) = if target.starts_with("i686-") {
        (0x014c, 7) // IMAGE_REL_I386_DIR32NB
    } else if target.starts_with("aarch64-") {
        (0xaa64, 2) // IMAGE_REL_ARM64_ADDR32NB
    } else {
        (0x8664, 3) // IMAGE_REL_AMD64_ADDR32NB
    };

    let raw_len = raw.len() as u32;
    let ptr_to_raw = 60u32; // COFF header (20) + one section header (40)
    let ptr_to_relocs = ptr_to_raw + raw_len;
    let ptr_to_syms = ptr_to_relocs + 10 * (reloc_offsets.len() as u32);

    let mut out = Vec::with_capacity(ptr_to_syms as usize + 40);
    // COFF File Header (20 bytes)
    out.extend_from_slice(&machine.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // NumberOfSections
    out.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
    out.extend_from_slice(&ptr_to_syms.to_le_bytes()); // PointerToSymbolTable
    out.extend_from_slice(&2u32.to_le_bytes()); // NumberOfSymbols
    out.extend_from_slice(&0u16.to_le_bytes()); // SizeOfOptionalHeader
    out.extend_from_slice(&0u16.to_le_bytes()); // Characteristics

    // Section Header `.rsrc` (40 bytes)
    out.extend_from_slice(b".rsrc\0\0\0");
    out.extend_from_slice(&0u32.to_le_bytes()); // VirtualSize
    out.extend_from_slice(&0u32.to_le_bytes()); // VirtualAddress
    out.extend_from_slice(&raw_len.to_le_bytes()); // SizeOfRawData
    out.extend_from_slice(&ptr_to_raw.to_le_bytes()); // PointerToRawData
    out.extend_from_slice(&ptr_to_relocs.to_le_bytes()); // PointerToRelocations
    out.extend_from_slice(&0u32.to_le_bytes()); // PointerToLinenumbers
    out.extend_from_slice(&(reloc_offsets.len() as u16).to_le_bytes()); // NumberOfRelocations
    out.extend_from_slice(&0u16.to_le_bytes()); // NumberOfLinenumbers
    out.extend_from_slice(&0xC030_0040u32.to_le_bytes()); // INITIALIZED_DATA | ALIGN_4 | READ | WRITE

    // Raw section data
    out.extend_from_slice(raw);

    // Relocations (10 bytes each): RVA fields against the `.rsrc` section symbol.
    for offset in reloc_offsets {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // symbol table index 0
        out.extend_from_slice(&reloc_type.to_le_bytes());
    }

    // Symbol 0: `.rsrc` section static symbol + 1 aux symbol (18 + 18 bytes).
    out.extend_from_slice(b".rsrc\0\0\0");
    out.extend_from_slice(&0u32.to_le_bytes()); // Value
    out.extend_from_slice(&1u16.to_le_bytes()); // SectionNumber = 1
    out.extend_from_slice(&0u16.to_le_bytes()); // Type = 0
    out.extend_from_slice(&[3u8, 1u8]); // IMAGE_SYM_CLASS_STATIC, NumberOfAuxSymbols = 1
    out.extend_from_slice(&raw_len.to_le_bytes()); // aux: section length
    out.extend_from_slice(&(reloc_offsets.len() as u16).to_le_bytes()); // aux: relocation count
    out.extend_from_slice(&0u16.to_le_bytes()); // aux: line number count
    out.extend_from_slice(&0u32.to_le_bytes()); // aux: checksum
    out.extend_from_slice(&0u16.to_le_bytes()); // aux: number (high)
    out.extend_from_slice(&0u16.to_le_bytes()); // aux: selection/padding
    out.extend_from_slice(&0u32.to_le_bytes()); // aux: padding

    // Empty string table (4 bytes)
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}
