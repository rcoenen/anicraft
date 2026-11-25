//! The compound binary container (Microsoft [MS-CFB], the OLE2 compound file format), read by
//! the shared `craft-cfb` reader from craft-libs.
//!
//! This module keeps only the importer's own policy: the caps of the hardened zip reader's
//! profile (ADR 0054: 1 GiB per file, 256 MiB per stream), stream lookup by full path, and one
//! sentence per failure that is safe to show, and bounds on the full paths it builds (a storage
//! depth, and a budget for all paths together). Every chain walk, cycle guard and directory
//! bound is `craft-cfb`'s. Nothing is extracted to disk, and only stream contents are copied.

use craft_cfb::{CompoundFile, EntryKind, Limits};
use std::collections::HashMap;
use std::fmt;

/// Hard caps, mirroring the hardened zip reader's profile (ADR 0054): 1 GiB per file and
/// 256 MiB per stream.
pub const MAX_FILE: u64 = 1 << 30;
const MAX_STREAM: u64 = 256 << 20;
/// The most directory entries a container may hold (`craft-cfb`'s own default).
const MAX_ENTRIES: usize = 1 << 20;
/// The deepest storage nesting read. Flash keeps its streams at the top level; `craft-cfb`
/// doesn't bound the depth, and a full path costs its depth in names.
const MAX_TREE_DEPTH: usize = 64;
/// The most bytes all entries' full paths may take together, so a small file of long, deeply
/// nested names can't make the path list many times its own size.
const MAX_PATH_BYTES: usize = 64 << 20;

/// Why a container couldn't be read: one sentence each, safe to show.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("not a legacy Flash document: its first bytes are not the compound binary signature")]
    NotCompound,
    #[error("this legacy Flash document is larger than AniCraft reads (over 1 GiB)")]
    TooLarge,
    /// The container's structure is broken: why, as `craft-cfb` words it.
    #[error("this legacy Flash document is damaged: {0}")]
    Damaged(String),
    #[error("this legacy Flash document claims a stream far larger than the file could hold")]
    StreamTooLarge,
    #[error("this legacy Flash document asks for more than AniCraft reads: {0}")]
    OverLimit(String),
    #[error("no stream named {0:?} in this legacy Flash document")]
    NotFound(String),
}

impl From<craft_cfb::Error> for Error {
    fn from(e: craft_cfb::Error) -> Self {
        match e {
            // The signature was already checked, so what `craft-cfb` refuses here is the rest of
            // the header's frame.
            craft_cfb::Error::NotCfb => Error::Damaged("its header is cut short or isn't little-endian".to_string()),
            craft_cfb::Error::Invalid(why) => Error::Damaged(why),
            craft_cfb::Error::LimitExceeded(why) => Error::OverLimit(why),
            craft_cfb::Error::NotFound(path) => Error::NotFound(path),
            other => Error::Damaged(other.to_string()),
        }
    }
}

/// What a directory entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A stream: its bytes can be read.
    Stream,
    /// A storage: it holds other entries.
    Storage,
}

/// One entry of the container's directory, with its full path (`"S 49 1724827326"`).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: String,
    pub kind: Kind,
    /// The stream's size in bytes (0 for storages).
    pub size: u64,
    /// The stream's first sector (its chain's start; debug/forensics, not needed to read).
    pub start: u32,
    /// The entry's index in the container's directory.
    id: usize,
}

/// An open compound file over the caller's bytes.
pub struct Compound<'a> {
    file: CompoundFile<'a>,
    /// The file's length in bytes.
    len: u64,
    /// Directory entries with paths, root first, then the tree in walk order.
    entries: Vec<Entry>,
    /// Each stream path's first entry in `entries` (a damaged file may list a path twice).
    streams: HashMap<String, usize>,
}

impl<'a> Compound<'a> {
    /// Opens a compound file over `data`, validating its header, FAT and directory.
    pub fn new(data: &'a [u8]) -> Result<Compound<'a>, Error> {
        Self::open_within(data, MAX_PATH_BYTES)
    }

    /// [`Compound::new`] with a budget for the full paths' bytes (a test seam).
    pub(crate) fn open_within(data: &'a [u8], max_path_bytes: usize) -> Result<Compound<'a>, Error> {
        if !crate::is_legacy(data) {
            return Err(Error::NotCompound);
        }
        if data.len() as u64 > MAX_FILE {
            return Err(Error::TooLarge);
        }
        let file = CompoundFile::open_with(data, Limits { max_stream_size: MAX_STREAM, max_dir_entries: MAX_ENTRIES })?;
        let root = file.root();
        let mut entries = vec![Entry { path: String::new(), kind: Kind::Storage, size: root.size, start: root.start, id: 0 }];
        // Each storage's depth and full path length, so both are checked before a path is built.
        // `walk` lists parents before their children.
        let mut reached: HashMap<usize, (usize, usize)> = HashMap::from([(0, (0, 0))]);
        let mut path_bytes = 0usize;
        for id in file.walk() {
            let Some(e) = file.entry(id) else { continue };
            // Unallocated entries and unknown types never hold document streams.
            let kind = match e.kind {
                EntryKind::Stream => Kind::Stream,
                EntryKind::Storage | EntryKind::Root => Kind::Storage,
                EntryKind::Unknown => continue,
            };
            let Some(&(parent_depth, parent_len)) = e.parent.and_then(|p| reached.get(&p)) else { continue };
            let depth = parent_depth + 1;
            if depth > MAX_TREE_DEPTH {
                return Err(Error::OverLimit(format!("its storages are nested more than {MAX_TREE_DEPTH} deep")));
            }
            let len = if parent_len == 0 { e.name.len() } else { parent_len + 1 + e.name.len() };
            path_bytes = path_bytes.saturating_add(len);
            if path_bytes > max_path_bytes {
                return Err(Error::OverLimit("its entries' names add up to more than AniCraft reads".to_string()));
            }
            if kind == Kind::Storage {
                reached.insert(id, (depth, len));
            }
            let Some(path) = file.path(id) else { continue };
            let size = if kind == Kind::Stream { e.size } else { 0 };
            entries.push(Entry { path, kind, size, start: e.start, id });
        }
        let mut streams = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            if e.kind == Kind::Stream {
                streams.entry(e.path.clone()).or_insert(i);
            }
        }
        Ok(Compound { file, len: data.len() as u64, entries, streams })
    }

    /// Every entry, root first, then the tree in walk order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Reads a stream by its full path. Storages have nothing to read.
    pub fn stream(&self, path: &str) -> Result<Vec<u8>, Error> {
        let entry = self.streams.get(path).and_then(|&i| self.entries.get(i)).ok_or_else(|| Error::NotFound(path.to_string()))?;
        self.read_entry(entry)
    }

    /// The size of the stream at a full path, without reading it.
    pub fn stream_size(&self, path: &str) -> Option<u64> {
        self.streams.get(path).and_then(|&i| self.entries.get(i)).map(|e| e.size)
    }

    /// Reads the stream one of [`Compound::entries`] is: its own bytes, even when another
    /// entry of a damaged file has the same path.
    pub fn read_entry(&self, entry: &Entry) -> Result<Vec<u8>, Error> {
        if entry.kind != Kind::Stream {
            return Err(Error::NotFound(entry.path.clone()));
        }
        // Refused before anything is reserved for it: no stream holds more than the file does.
        if entry.size > MAX_STREAM || entry.size > self.len {
            return Err(Error::StreamTooLarge);
        }
        Ok(self.file.read(entry.id)?)
    }
}

impl fmt::Debug for Compound<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Compound").field("sector", &self.file.sector_size()).field("entries", &self.entries.len()).finish()
    }
}
