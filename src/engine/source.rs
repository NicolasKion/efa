//! Where refine's data comes from: a deimos dump, adapted on the first start and cached on disk,
//! or the adapted data of the embedded data pack, which needs no EVE data at all.
//!
//! refine checks a fingerprint of the EVE data version and the cacher before it trusts cached
//! data, and generates fresh data from the EVE data when they differ. The pack has no EVE data
//! to generate from, so its handler only knows the version and turns every read into an error
//! saying the pack does not belong to this build.

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use rc::{
    Src,
    ad::{AData, AFingerprint, AdaptedDataCacherCore, err::AdaptedDataCacherError},
    ed::{EData, EveDataHandlerCore, err::EveDataHandlerError},
    src::SrcOrigin,
};
use serde::{Deserialize, Serialize};

use super::overrides::Patched;
use crate::overrides::Values;

/// The cacher version in the pack's fingerprint. It names the refine commit efa builds against
/// (see build.rs), since refine's own version does not move with its commits, and the adapted
/// data's layout does.
const PACK_CACHER: &str = concat!("efa-pack-refine-", env!("EFA_REFINE_REV"));

/// What refine's data is loaded from.
#[derive(Debug, Clone)]
pub enum EngineSource {
    /// A Phobos-layout dump, with the adapted data cached in `cache_dir`.
    Dump {
        data_dir: PathBuf,
        cache_dir: PathBuf,
    },
    /// Adapted data that came with the data pack.
    Packed(Arc<PackedEngine>),
}

/// refine's adapted data as the data pack carries it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackedEngine {
    /// The EVE data version refine read off the dump (the client build).
    pub data_version: String,
    /// What refine asked of the cache when the pack was built.
    pub fingerprint: String,
    /// The adapted data, postcard.
    #[serde(with = "serde_bytes")]
    pub adapted: Vec<u8>,
}

impl fmt::Debug for PackedEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PackedEngine")
            .field("data_version", &self.data_version)
            .field("fingerprint", &self.fingerprint)
            .field("adapted", &format_args!("{} bytes", self.adapted.len()))
            .finish()
    }
}

impl PackedEngine {
    /// Adapt the dump in `data_dir` for refine, the way a first start does, and keep the result
    /// instead of writing it to a cache.
    pub fn adapt(data_dir: &Path) -> Result<Self, String> {
        let edh = redh::PhbFsEdh::new(data_dir);
        let data_version = edh
            .get_data_version()
            .map_err(|e| format!("reading the EVE data version: {e}"))?;
        let capture = Capture::default();
        Src::new(&edh.into(), Some(&capture.clone().into()))
            .map_err(|e| format!("adapting the engine data: {e}"))?;
        let (adapted, fingerprint) = capture
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or("refine did not hand over the adapted data")?;
        Ok(Self {
            data_version,
            fingerprint,
            adapted,
        })
    }
}

/// refine's data source with `values` as the base values of their types, and the charges generic
/// links need.
pub fn load_src(source: &EngineSource, values: &Values) -> Result<Src, String> {
    match source {
        EngineSource::Dump {
            data_dir,
            cache_dir,
        } => {
            let edh = redh::PhbFsEdh::new(data_dir).into();
            let plain = || radc::PostcardZfsAdc::new(cache_dir, "tq");
            Src::new(&edh, Some(&plain().into()))
                .map_err(|e| format!("loading engine data: {e}"))?;
            // The plain load brought the cache up to date, so the patched one reads it rather
            // than generating fresh, unpatched data.
            let patched = Patched::new(Box::new(plain()), values.clone());
            Src::new(&edh, Some(&patched.into())).map_err(|e| format!("loading engine data: {e}"))
        }
        EngineSource::Packed(packed) => {
            let edh = PackEdh(Arc::clone(packed)).into();
            let adc = PackAdc(Arc::clone(packed));
            // refine only fails here when it wanted EVE data, after a fingerprint mismatch or
            // unreadable adapted data; its error does not carry the handler's message.
            let patched = Patched::new(Box::new(adc), values.clone());
            let src = Src::new(&edh, Some(&patched.into()))
                .map_err(|e| format!("{} (refine: {e})", mismatch(packed)))?;
            match &src.get_info().origin {
                SrcOrigin::Cached(_) => Ok(src),
                SrcOrigin::Generated(_) => Err(mismatch(packed)),
            }
        }
    }
}

fn mismatch(packed: &PackedEngine) -> String {
    format!(
        "The data pack's engine data does not fit this build of efa, or is damaged (its \
         fingerprint is {}, this build uses cacher {PACK_CACHER} and refine {}). Rebuild the pack \
         with scripts/build-pack.sh, or point EFA_DATA at a deimos dump.",
        packed.fingerprint,
        rc::VERSION,
    )
}

/// The pack's stand-in for EVE data: a version and nothing else.
#[derive(Debug)]
struct PackEdh(Arc<PackedEngine>);

impl EveDataHandlerCore for PackEdh {
    fn get_data(&self) -> Result<EData, EveDataHandlerError> {
        // refine only asks for the data when the fingerprint did not match.
        Err(EveDataHandlerError::new(std::io::Error::other(mismatch(
            &self.0,
        ))))
    }

    fn get_data_version(&self) -> Result<String, EveDataHandlerError> {
        Ok(self.0.data_version.clone())
    }
}

/// The pack's adapted data as a read-only cache.
#[derive(Debug)]
struct PackAdc(Arc<PackedEngine>);

impl AdaptedDataCacherCore for PackAdc {
    fn get_cache_fingerprint(&self) -> Result<AFingerprint, AdaptedDataCacherError> {
        Ok(AFingerprint::from_string(self.0.fingerprint.clone()))
    }

    fn load_from_cache(&self) -> Result<AData, AdaptedDataCacherError> {
        postcard::from_bytes(&self.0.adapted).map_err(AdaptedDataCacherError::new)
    }

    fn write_cache(&self, _: &AData, _: AFingerprint) -> Result<(), AdaptedDataCacherError> {
        Err(AdaptedDataCacherError::new(std::io::Error::other(
            "the data pack is read only",
        )))
    }

    fn get_cacher_version(&self) -> String {
        PACK_CACHER.to_owned()
    }
}

/// A cacher with nothing cached, which keeps what refine writes to it: the pack builder's way to
/// get the adapted data and its fingerprint.
#[derive(Debug, Default, Clone)]
struct Capture(Arc<Mutex<Option<Captured>>>);

/// The adapted data, postcard, and its fingerprint.
type Captured = (Vec<u8>, String);

impl AdaptedDataCacherCore for Capture {
    fn get_cache_fingerprint(&self) -> Result<AFingerprint, AdaptedDataCacherError> {
        Err(AdaptedDataCacherError::new(std::io::Error::other(
            "nothing cached yet",
        )))
    }

    fn load_from_cache(&self) -> Result<AData, AdaptedDataCacherError> {
        Err(AdaptedDataCacherError::new(std::io::Error::other(
            "nothing cached yet",
        )))
    }

    fn write_cache(
        &self,
        data: &AData,
        fingerprint: AFingerprint,
    ) -> Result<(), AdaptedDataCacherError> {
        let bytes = postcard::to_stdvec(data).map_err(AdaptedDataCacherError::new)?;
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((bytes, fingerprint.into_string()));
        Ok(())
    }

    fn get_cacher_version(&self) -> String {
        PACK_CACHER.to_owned()
    }
}
