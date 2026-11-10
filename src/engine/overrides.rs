//! Attribute overrides as refine sees them: the adapted data with the overridden base values
//! written into the items, so every item of the type starts from them, in this fit and in every
//! projected or command fit, the way pyfa swaps the original value for the override.
//!
//! refine has no API to change a base value, so a cacher that loads the cached adapted data and
//! patches it stands in for the usual one. It also adds the scaled burst charges generic command
//! links fly with.

use rc::{
    Src,
    ad::{
        AAttrId, AData, AFingerprint, AItem, AItemAttr, AItemAttrs, AValue, AdaptedDataCacherCore,
        err::AdaptedDataCacherError,
    },
};
use serde::{Serialize, de::DeserializeOwned};

use super::source::{EngineSource, load_src};
use crate::overrides::Values;

/// The cacher refine reads its data through, with the overrides applied on load.
#[derive(Debug)]
pub(super) struct Patched {
    inner: Box<dyn AdaptedDataCacherCore>,
    values: Values,
}

impl Patched {
    /// `inner`'s data with `values` written into it.
    pub(super) fn new(inner: Box<dyn AdaptedDataCacherCore>, values: Values) -> Self {
        Self { inner, values }
    }
}

impl AdaptedDataCacherCore for Patched {
    fn get_cache_fingerprint(&self) -> Result<AFingerprint, AdaptedDataCacherError> {
        self.inner.get_cache_fingerprint()
    }

    fn load_from_cache(&self) -> Result<AData, AdaptedDataCacherError> {
        let mut data = self.inner.load_from_cache()?;
        patch(&mut data, &self.values).map_err(AdaptedDataCacherError::new)?;
        super::links::add_scaled_charges(&mut data).map_err(AdaptedDataCacherError::new)?;
        Ok(data)
    }

    fn write_cache(
        &self,
        data: &AData,
        fingerprint: AFingerprint,
    ) -> Result<(), AdaptedDataCacherError> {
        self.inner.write_cache(data, fingerprint)
    }

    fn get_cacher_version(&self) -> String {
        self.inner.get_cacher_version()
    }
}

/// The engine's data with attribute overrides applied, built away from the UI thread and handed
/// to [`super::Engine::set_overrides`].
pub struct Overridden {
    pub(super) src: Src,
    pub(super) values: Values,
}

impl Overridden {
    /// Load the data from `source` with `values` applied.
    pub fn load(source: &EngineSource, values: &Values) -> Result<Self, String> {
        Ok(Self {
            src: load_src(source, values)?,
            values: values.clone(),
        })
    }
}

/// Write the overridden values into the items they belong to. Attributes an item does not have
/// are left alone, as pyfa only offers the ones it has.
fn patch(data: &mut AData, values: &Values) -> Result<(), postcard::Error> {
    let mut patched = Vec::new();
    for item in data.items.iter() {
        let Some(attrs) = values.get(&item.id.into_i32()) else {
            continue;
        };
        let new_attrs: AItemAttrs = item
            .attrs
            .iter()
            .map(|a| {
                let value = match a.id {
                    AAttrId::Eve(id) => attrs.get(&id.into_i32()).copied(),
                    AAttrId::Custom(_) => None,
                };
                AItemAttr {
                    id: a.id,
                    value: value.map_or(a.value, AValue::from_f64),
                }
            })
            .collect();
        patched.push(with_attrs(item, new_attrs)?);
    }
    for item in patched {
        data.items.insert(item);
    }
    Ok(())
}

/// A copy of `item` with other attributes. Adapted items cannot be cloned or changed in place,
/// so the parts that are not plain values are copied through their serialized form.
pub(super) fn with_attrs(item: &AItem, attrs: AItemAttrs) -> Result<AItem, postcard::Error> {
    Ok(AItem {
        id: item.id,
        grp_id: item.grp_id,
        cat_id: item.cat_id,
        attrs,
        effects: copy(&item.effects)?,
        defeff_id: item.defeff_id,
        abil_ids: copy(&item.abil_ids)?,
        srqs: copy(&item.srqs)?,
        max_state: item.max_state,
        proj_buff_item_list_ids: copy(&item.proj_buff_item_list_ids)?,
        fleet_buff_item_list_ids: copy(&item.fleet_buff_item_list_ids)?,
        val_fitted_group_id: item.val_fitted_group_id,
        val_online_group_id: item.val_online_group_id,
        val_active_group_id: item.val_active_group_id,
        is_cloak: item.is_cloak,
        is_ice_harvester: item.is_ice_harvester,
        disallowed_in_wspace: item.disallowed_in_wspace,
        enables_portal: item.enables_portal,
    })
}

fn copy<T: Serialize + DeserializeOwned>(value: &T) -> Result<T, postcard::Error> {
    postcard::from_bytes(&postcard::to_allocvec(value)?)
}
