//! Path routes that leave the URL fragment alone.
//!
//! The fragment (`#…`) of an Oxfer URL is a share or room capability, or QA flags such as
//! `#dev` and `sink=fsa|sw|mem`. This router never parses, logs or rewrites it: it reads
//! and writes only `location.pathname`, and every `pushState` / `replaceState` it makes
//! carries the current `search + hash` through verbatim. The session core is the only
//! code that reads the fragment (and scrubs the capability from history).
//!
//! The route follows the core's [`Activity`] and the other way round: opening a share
//! link moves the page to `/receive`, and Back from `/receive` to `/` goes home. See
//! [`use_route_sync`].

use dioxus::prelude::*;
use p2p_transfer::sessions::{Activity, CopiesIntent, Intent};
use wasm_bindgen::{closure::Closure, JsCast as _, JsValue};

use crate::core::Core;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Home,
    Send,
    Receive,
    Chat,
    Copies,
    Diags,
    NotFound,
}

impl Route {
    pub const fn path(self) -> &'static str {
        match self {
            Self::Home | Self::NotFound => "/",
            Self::Send => "/send",
            Self::Receive => "/receive",
            Self::Chat => "/chat",
            Self::Copies => "/copies",
            Self::Diags => "/diags",
        }
    }

    pub fn from_path(path: &str) -> Self {
        match path.trim_end_matches('/') {
            "" | "/index.html" => Self::Home,
            "/send" => Self::Send,
            "/receive" => Self::Receive,
            "/chat" => Self::Chat,
            "/copies" => Self::Copies,
            "/diags" => Self::Diags,
            _ => Self::NotFound,
        }
    }

    /// The route that shows `activity`.
    pub const fn of(activity: Activity) -> Self {
        match activity {
            Activity::Home => Self::Home,
            Activity::Send => Self::Send,
            Activity::Receive => Self::Receive,
            Activity::Chat => Self::Chat,
            Activity::Diagnostics => Self::Diags,
        }
    }

    /// Views that are not an activity of their own: they can show over any activity
    /// (local copies) or over Home (the chat start view, not found).
    const fn shows_over(self, activity: Activity) -> bool {
        match self {
            Self::Copies => true,
            Self::Chat | Self::NotFound => matches!(activity, Activity::Home),
            _ => false,
        }
    }
}

fn location_path() -> String {
    web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .unwrap_or_default()
}

/// `path` plus the current query and fragment, untouched.
fn url_with_current_tail(path: &str) -> String {
    let Some(location) = web_sys::window().map(|w| w.location()) else {
        return path.to_string();
    };
    let search = location.search().unwrap_or_default();
    let hash = location.hash().unwrap_or_default();
    format!("{path}{search}{hash}")
}

fn write_history(path: &str, replace: bool) {
    let Some(history) = web_sys::window().and_then(|w| w.history().ok()) else {
        return;
    };
    let url = url_with_current_tail(path);
    let _ = if replace {
        history.replace_state_with_url(&JsValue::NULL, "", Some(&url))
    } else {
        history.push_state_with_url(&JsValue::NULL, "", Some(&url))
    };
}

/// The current route, as a signal every component can read.
#[derive(Clone, Copy)]
pub struct Router {
    route: Signal<Route>,
}

impl Router {
    pub fn current(&self) -> Route {
        (self.route)()
    }

    /// Go to `route` because the user asked to (a nav link). The core follows the route.
    pub fn go(&self, route: Route) {
        self.set(route, false);
    }

    fn set(&self, route: Route, replace: bool) {
        let mut signal = self.route;
        if *signal.peek() == route && Route::from_path(&location_path()) == route {
            return;
        }
        write_history(route.path(), replace);
        signal.set(route);
    }
}

pub fn use_router() -> Router {
    use_context::<Router>()
}

/// Provide the router, listen for Back/Forward, and keep route and activity in step.
pub fn use_route_sync(core: Core) -> Router {
    let router = use_context_provider(|| Router {
        route: Signal::new(Route::from_path(&location_path())),
    });

    // Back and Forward change the path without telling anyone else.
    use_hook(move || {
        let mut route = router.route;
        let on_pop = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            route.set(Route::from_path(&location_path()));
        });
        if let Some(window) = web_sys::window() {
            let _ = window
                .add_event_listener_with_callback("popstate", on_pop.as_ref().unchecked_ref());
        }
        on_pop.forget();
    });

    // Route → core: entering a route asks the core for the matching activity. The core
    // ignores what does not apply (Home while a file is still preparing), and the
    // activity effect below then puts the route back.
    let route = router.route;
    let mut first_run = use_hook(|| CopyValue::new(true));
    use_effect(move || {
        let route = route();
        let snap = core.snap();
        let activity = snap.peek().activity;
        if std::mem::replace(&mut *first_run.write(), false) && activity != Activity::Home {
            // The page loaded with a share or room link: the core already opened it, and
            // the path it loaded on (usually `/`) must not cancel that.
            router.set(Route::of(activity), true);
            return;
        }
        if route.shows_over(activity) || Route::of(activity) == route {
            if route == Route::Copies {
                core.dispatch(Intent::Copies(CopiesIntent::Refresh));
            }
            return;
        }
        let intent = match route {
            Route::Home => Intent::GoHome,
            Route::Send => Intent::ReturnToSharing,
            Route::Receive => Intent::OpenReceive,
            Route::Diags => Intent::OpenDiagnostics,
            // A room is started or joined from its view; leaving one goes Home.
            Route::Chat | Route::NotFound => Intent::GoHome,
            Route::Copies => return,
        };
        core.dispatch(intent);
        let now = core.snap().peek().activity;
        if !route.shows_over(now) && Route::of(now) != route {
            router.set(Route::of(now), true);
        }
    });

    // Core → route: a new activity (a link opened, a pick started a share, a room
    // closed) moves the page to its route, as a new history entry so Back returns.
    let activity = use_memo(move || core.snap().read().activity);
    use_effect(move || {
        let activity = activity();
        let current = *router.route.peek();
        if Route::of(activity) != current && !current.shows_over(activity) {
            router.set(Route::of(activity), false);
        }
    });

    router
}
