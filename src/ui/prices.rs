//! Fetching prices for the window: where from, when they are stale, and work waiting on them.

use std::{
    collections::HashSet,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::{AppContext, Context, WeakEntity};

use super::Efa;
use crate::data::TypeId;

/// How long to wait before asking ESI again after a failed price fetch.
const PRICE_RETRY: Duration = Duration::from_secs(600);

/// What runs once prices for some types are in, or could not be had.
pub(super) type PriceWaiter = Box<dyn FnOnce(&mut Efa, &mut Context<Efa>)>;

impl Efa {
    /// The source and hub the preferences ask prices from.
    fn price_origin(&self) -> crate::prices::Origin {
        let market = &self.saved.settings.preferences.market;
        crate::prices::Origin {
            source: market.price_source,
            system: market.price_system,
        }
    }

    pub(super) fn prices_online(&self) -> bool {
        let network = &self.saved.settings.preferences.network;
        network.enabled && network.prices
    }

    /// Every type the saved fits use, what the price sources are asked about. Gathered again
    /// only after the fits changed.
    fn saved_fit_types(&mut self) -> Rc<HashSet<TypeId>> {
        if let Some((generation, types)) = &self.fit_types
            && *generation == self.calc_generation
        {
            return Rc::clone(types);
        }
        let types: Rc<HashSet<TypeId>> = Rc::new(
            self.saved
                .fits
                .iter()
                .flat_map(|fit| crate::port::fit_types(fit))
                .collect(),
        );
        self.fit_types = Some((self.calc_generation, Rc::clone(&types)));
        types
    }

    /// Fetch newer prices in the background when the ones we have are stale. After a failure it
    /// waits a while before trying again, so being offline does not mean a request every poll.
    pub(super) fn refresh_prices(&mut self, cx: &mut Context<Self>) {
        if !self.prices_online() {
            return;
        }
        let waiting = self.prices_retry.is_some_and(|at| Instant::now() < at);
        if self.prices_fetch.is_some() || waiting {
            return;
        }
        let types = self.saved_fit_types();
        if self.prices.is_stale_for(self.price_origin(), &types) {
            self.start_price_fetch((*types).clone(), cx);
        }
    }

    /// Run `then` once there are prices for `types`, fetching them first when they are stale
    /// and the network allows. Without them it runs with what there is.
    pub(super) fn with_prices_for(
        &mut self,
        types: HashSet<TypeId>,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) {
        if !self.prices_online() || !self.prices.is_stale_for(self.price_origin(), &types) {
            then(self, cx);
            return;
        }
        cx.notify();
        let mut all = (*self.saved_fit_types()).clone();
        all.extend(&types);
        self.price_waiters.push((types, Box::new(then)));
        if self.prices_fetch.is_none() {
            self.start_price_fetch(all, cx);
        }
    }

    fn start_price_fetch(&mut self, types: HashSet<TypeId>, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let origin = self.price_origin();
        let prices = (*self.prices).clone();
        let cache_dir = loaded.cache_dir.clone();
        let proxy = self.saved.settings.preferences.network.proxy.clone();
        self.prices_fetch = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = cx
                .background_spawn(async move {
                    let http = proxy.client()?;
                    prices.refresh_from(&cache_dir, origin, &types, &http)
                })
                .await;
            let _ = this.update(cx, |g, cx| {
                g.prices_fetch = None;
                let fetched = result.is_ok();
                match result {
                    Ok(prices) => {
                        let prices = Rc::new(prices);
                        g.prices = Rc::clone(&prices);
                        g.prices_retry = None;
                        g.hub.update(cx, |hub, cx| hub.set_prices(prices, cx));
                    }
                    Err(e) => {
                        eprintln!("prices: {e}");
                        g.prices_retry = Some(Instant::now() + PRICE_RETRY);
                        if !g.price_waiters.is_empty() {
                            g.set_error(format!("Could not fetch prices: {e}"));
                        }
                    }
                }
                // Waiters whose types this fetch did not cover start another; after a failure
                // they make do with what there is.
                for (types, then) in std::mem::take(&mut g.price_waiters) {
                    if fetched {
                        g.with_prices_for(types, cx, then);
                    } else {
                        then(g, cx);
                    }
                }
                cx.notify();
            });
        }));
    }
}
