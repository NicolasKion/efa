//! The extra item data (descriptions, traits, attribute names, effects) the stats windows and
//! hover texts read: loaded once, away from the UI thread, the first time something asks.

use std::{collections::HashSet, rc::Rc};

use gpui_kit::{App, AppContext, Context, Global, SharedString, Subscription};

use super::Loaded;
use crate::{data::TypeId, item_data::ItemData};

/// What waits for the data to come in.
type Waiter = Box<dyn FnOnce(Rc<ItemData>, &mut App)>;

enum DataState {
    /// On its way, with what runs once it is in.
    Loading(Vec<Waiter>),
    Ready(Rc<ItemData>),
    Failed(SharedString),
}

struct ItemDataGlobal(DataState);

impl Global for ItemDataGlobal {}

/// Start loading the data unless it is loaded or on its way.
pub fn ensure_data(loaded: &Rc<Loaded>, cx: &mut App) {
    if cx.has_global::<ItemDataGlobal>() {
        return;
    }
    cx.set_global(ItemDataGlobal(DataState::Loading(Vec::new())));
    let source = loaded.source.clone();
    let wanted: HashSet<TypeId> = loaded.catalog.types.keys().copied().collect();
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move { source.item_data(&wanted) })
            .await;
        cx.update(|cx| {
            let (state, ready) = match result {
                Ok(data) => {
                    let data = Rc::new(data);
                    (DataState::Ready(Rc::clone(&data)), Some(data))
                }
                Err(e) => (DataState::Failed(e.into()), None),
            };
            // Changing the global tells its observers.
            let previous = std::mem::replace(&mut cx.global_mut::<ItemDataGlobal>().0, state);
            if let (DataState::Loading(waiters), Some(data)) = (previous, ready) {
                for waiter in waiters {
                    waiter(Rc::clone(&data), cx);
                }
            }
        });
    })
    .detach();
}

/// The data for hover text, such as descriptions and traits. Starts loading it the first time;
/// None until it is in.
pub fn hover_data(loaded: &Rc<Loaded>, cx: &mut App) -> Option<Rc<ItemData>> {
    ensure_data(loaded, cx);
    match &cx.try_global::<ItemDataGlobal>()?.0 {
        DataState::Ready(data) => Some(Rc::clone(data)),
        DataState::Loading(_) | DataState::Failed(_) => None,
    }
}

/// The data as far as it is loaded: None while loading.
pub fn data(cx: &App) -> Option<Result<Rc<ItemData>, SharedString>> {
    match &cx.try_global::<ItemDataGlobal>()?.0 {
        DataState::Loading(_) => None,
        DataState::Ready(data) => Some(Ok(Rc::clone(data))),
        DataState::Failed(e) => Some(Err(e.clone())),
    }
}

/// Run `f` with the data once it is in: right away when it is, later when it lands, never when
/// it fails to load. Starts loading it the first time.
pub fn when_ready(
    loaded: &Rc<Loaded>,
    cx: &mut App,
    f: impl FnOnce(Rc<ItemData>, &mut App) + 'static,
) {
    match data(cx) {
        Some(Ok(data)) => f(data, cx),
        Some(Err(_)) => {}
        None => {
            ensure_data(loaded, cx);
            if let DataState::Loading(waiters) = &mut cx.global_mut::<ItemDataGlobal>().0 {
                waiters.push(Box::new(f));
            }
        }
    }
}

/// Redraw a view once the data is in.
pub fn observe_data<V: 'static>(cx: &mut Context<V>) -> Subscription {
    cx.observe_global::<ItemDataGlobal>(|_, cx| cx.notify())
}
