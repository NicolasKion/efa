//! The data pack: everything efa needs from a deimos dump, prepared ahead of time and embedded in
//! the binary, so a release reads no JSON and downloads no EVE data.
//!
//! A pack is one zstd-compressed postcard [`PackFile`]: its format, the deimos release it was
//! built from, the catalog and item data as their caches store them, and refine's adapted data
//! with the fingerprint refine checks it by. `efa --build-pack` (scripts/build-pack.sh) writes
//! it, and build.rs embeds it when it is there. Builds without one read a dump instead, the way
//! development always has.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::{
    data::Catalog,
    engine::{EngineSource, PackedEngine},
    item_data::ItemData,
};

/// What build.rs embeds by default, relative to the crate.
pub const DEFAULT_PATH: &str = "assets/data/efa-data.pack";
/// Bump whenever [`PackFile`] changes.
const FORMAT: u32 = 1;
/// zstd level. The pack is built once per data release, so it may take its time.
const LEVEL: i32 = 19;

#[cfg(efa_pack)]
static EMBEDDED: &[u8] = include_bytes!(env!("EFA_PACK_FILE"));

/// The pack as stored, after decompression. `format` comes first so a pack of another format is
/// told apart before the rest is read.
#[derive(Serialize, Deserialize)]
struct PackFile {
    format: u32,
    version: String,
    catalog_format: u32,
    #[serde(with = "serde_bytes")]
    catalog: Vec<u8>,
    item_data_format: u32,
    #[serde(with = "serde_bytes")]
    item_data: Vec<u8>,
    engine: PackedEngine,
}

/// A data pack, unpacked as far as needed: the catalog and item data are read when asked for.
pub struct Pack {
    version: String,
    /// Format and bytes, as [`Catalog::to_bytes`] gives them.
    catalog: (u32, Vec<u8>),
    item_data: (u32, Vec<u8>),
    engine: Arc<PackedEngine>,
}

impl std::fmt::Debug for Pack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pack")
            .field("version", &self.version)
            .field("engine", &self.engine)
            .finish_non_exhaustive()
    }
}

impl Pack {
    /// The pack built into this binary, if it was built with one.
    pub fn embedded() -> Option<&'static [u8]> {
        #[cfg(efa_pack)]
        return Some(EMBEDDED);
        #[cfg(not(efa_pack))]
        None
    }

    /// Build a pack from the deimos dump in `dump`: the same loading a development start does,
    /// written out instead of cached. The same dump gives the same bytes.
    pub fn build(dump: &Path) -> Result<Vec<u8>, String> {
        let version = std::fs::read_to_string(dump.join("VERSION"))
            .map_err(|e| format!("{}: {e}", dump.join("VERSION").display()))?
            .trim()
            .to_owned();
        let catalog = Catalog::load(dump)?;
        let wanted = catalog.types.keys().copied().collect();
        let item_data = ItemData::load(dump, &wanted)?;
        let (catalog_format, catalog) = catalog.to_bytes()?;
        let (item_data_format, item_data) = item_data.to_bytes()?;
        let file = PackFile {
            format: FORMAT,
            version,
            catalog_format,
            catalog,
            item_data_format,
            item_data,
            engine: PackedEngine::adapt(dump)?,
        };
        let raw = postcard::to_stdvec(&file).map_err(|e| format!("writing the pack: {e}"))?;
        zstd::bulk::compress(&raw, LEVEL).map_err(|e| format!("compressing the pack: {e}"))
    }

    /// Build a pack from `dump` and write it to `out`.
    pub fn write(dump: &Path, out: &Path) -> Result<usize, String> {
        let bytes = Self::build(dump)?;
        if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let tmp = out.with_extension("pack.tmp");
        std::fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, out).map_err(|e| format!("{}: {e}", out.display()))?;
        Ok(bytes.len())
    }

    /// Unpack a pack [`Pack::build`] made.
    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        let raw = zstd::stream::decode_all(bytes)
            .map_err(|e| format!("decompressing the data pack: {e}"))?;
        let (format, _) = postcard::take_from_bytes::<u32>(&raw)
            .map_err(|e| format!("reading the data pack: {e}"))?;
        if format != FORMAT {
            return Err(format!(
                "the data pack is in format {format}, this build reads {FORMAT}"
            ));
        }
        let PackFile {
            format: _,
            version,
            catalog_format,
            catalog,
            item_data_format,
            item_data,
            engine,
        } = postcard::from_bytes(&raw).map_err(|e| format!("reading the data pack: {e}"))?;
        Ok(Self {
            version,
            catalog: (catalog_format, catalog),
            item_data: (item_data_format, item_data),
            engine: Arc::new(engine),
        })
    }

    /// The deimos release the pack was built from.
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn catalog(&self) -> Result<Catalog, String> {
        Catalog::from_bytes(self.catalog.0, &self.catalog.1)
    }

    pub fn item_data(&self) -> Result<ItemData, String> {
        ItemData::from_bytes(self.item_data.0, &self.item_data.1)
    }

    pub fn engine(&self) -> EngineSource {
        EngineSource::Packed(Arc::clone(&self.engine))
    }
}

/// Where the EVE data comes from this run.
#[derive(Debug, Clone)]
pub enum Source {
    /// The data pack built into the binary.
    Pack(Arc<Pack>),
    /// A deimos dump, with what is built from it cached in `cache_dir`.
    Dump { dir: PathBuf, cache_dir: PathBuf },
}

impl Source {
    /// `EFA_DATA` when it is set, otherwise the embedded pack, otherwise `data/eve`.
    pub fn detect() -> Result<Self, String> {
        if !crate::paths::dump_requested()
            && let Some(bytes) = Pack::embedded()
        {
            return Pack::read(bytes).map(|pack| Self::Pack(Arc::new(pack))).map_err(|e| {
                format!(
                    "{e}.\nRebuild the data pack with scripts/build-pack.sh, or point EFA_DATA at \
                     a deimos dump."
                )
            });
        }
        Self::dump(crate::paths::dump_dir(), crate::paths::dump_cache_dir())
    }

    /// The dump in `dir`, if there is one.
    pub fn dump(dir: PathBuf, cache_dir: PathBuf) -> Result<Self, String> {
        if !dir.join("phobos/metadata.json").exists() {
            return Err(format!(
                "No EVE data in {}.\nRun scripts/fetch-data.sh, or point EFA_DATA at a deimos dump.",
                dir.display()
            ));
        }
        Ok(Self::Dump { dir, cache_dir })
    }

    pub fn catalog(&self) -> Result<Catalog, String> {
        match self {
            Self::Pack(pack) => pack.catalog(),
            Self::Dump { dir, cache_dir } => Catalog::load_cached(dir, cache_dir),
        }
    }

    /// The item stats window's data; a dump reads it for the types in `wanted`.
    pub fn item_data(
        &self,
        wanted: &std::collections::HashSet<crate::data::TypeId>,
    ) -> Result<ItemData, String> {
        match self {
            Self::Pack(pack) => pack.item_data(),
            Self::Dump { dir, cache_dir } => {
                let key = crate::data::cache_key(dir);
                ItemData::load_cached(dir, cache_dir, key.as_deref(), wanted)
            }
        }
    }

    pub fn engine(&self) -> EngineSource {
        match self {
            Self::Pack(pack) => pack.engine(),
            Self::Dump { dir, cache_dir } => EngineSource::Dump {
                data_dir: dir.clone(),
                cache_dir: cache_dir.clone(),
            },
        }
    }

    /// The dump's folder, for the attribute and market group icons it carries. The pack has
    /// none; those icons come from the CDN like item icons.
    pub fn dump_dir(&self) -> Option<&Path> {
        match self {
            Self::Pack(_) => None,
            Self::Dump { dir, .. } => Some(dir),
        }
    }

    /// For the log: which data this is.
    /// The deimos release the data came from, `sde-<build>-client-<build>`, when it says.
    pub fn version(&self) -> Option<String> {
        match self {
            Self::Pack(pack) => Some(pack.version().to_owned()),
            Self::Dump { dir, .. } => std::fs::read_to_string(dir.join("VERSION"))
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty()),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Pack(pack) => format!("the embedded data pack ({})", pack.version()),
            Self::Dump { dir, .. } => format!("the dump in {}", dir.display()),
        }
    }
}
