use gpui_kit::*;
use swofty_core::online::server::api::{Api, ApiError};
use swofty_core::online::server::sign_in::Session;
use swofty_core::online::server::support::{Goal, Me, Supporter};

use crate::features::time_trial::Load;
use crate::shell::Shell;
use crate::shell::host::{Host, Hosted};

/// Der Spendenstand kommt von unserem Raumserver, nicht vom Katalog.
pub(crate) const SUPPORT_SERVER: &str = "https://raum.erdi-erc.de";

pub(crate) struct StorePage {
    pub goal: Load<Goal>,
    pub supporters: Load<Vec<Supporter>>,
    /// Wer gerade angemeldet ist: entscheidet ueber die Werbung.
    pub me: Load<Me>,
}

impl Default for StorePage {
    fn default() -> Self {
        Self {
            goal: Load::Idle,
            supporters: Load::Idle,
            me: Load::Idle,
        }
    }
}

/// What the support page borrows from the shell: who is signed in.
pub(crate) struct StoreNeeds<'a> {
    pub(crate) session: Option<&'a Session>,
}

impl StoreNeeds<'_> {
    fn api(&self) -> Api {
        Api::with_server(
            SUPPORT_SERVER,
            self.session.map(|s| s.token.as_str()).unwrap_or_default(),
        )
    }

    fn signed_in(&self) -> bool {
        self.session.is_some()
    }
}

/// What the shell follows when the support page loads.
pub(crate) enum StoreChange {
    /// The supporters' avatars to fetch.
    Images(Vec<String>),
}

impl Hosted for StorePage {
    type Needs<'a> = StoreNeeds<'a>;
    type Change = StoreChange;
}

impl StorePage {
    /// Loads the page when it opens, unless what it shows is already loaded or loading.
    pub(crate) fn open<H: Host<Self>>(host: &mut H, cx: &mut Context<H>) {
        let (page, needs) = host.split();
        if page.needs_load(needs.signed_in()) {
            Self::reload(host, cx);
        }
    }

    /// Fetches the signed-in user's support once per sign-in, which decides their ads.
    pub(crate) fn sync<H: Host<Self>>(host: &mut H, cx: &mut Context<H>) {
        let (page, needs) = host.split();
        if page.follow_sign_in(needs.signed_in()) {
            Self::fetch_me(host, cx);
        }
    }

    fn fetch_me<H: Host<Self>>(host: &mut H, cx: &mut Context<H>) {
        let (page, needs) = host.split();
        let api = needs.api();
        page.me = Load::Loading;
        cx.spawn(async move |this, cx| {
            let me = cx
                .background_executor()
                .spawn(async move { api.support_me() })
                .await;
            this.update(cx, |host, cx| {
                let (page, needs) = host.split();
                if needs.signed_in() {
                    page.me = load(me);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn reload<H: Host<Self>>(host: &mut H, cx: &mut Context<H>) {
        let (page, needs) = host.split();
        let api = needs.api();
        let signed_in = needs.signed_in();
        page.reloading(signed_in);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (goal, supporters, me) = cx
                .background_executor()
                .spawn(async move {
                    (
                        api.support_goal(),
                        api.support_supporters(),
                        signed_in.then(|| api.support_me()),
                    )
                })
                .await;
            this.update(cx, |host, cx| {
                let images = host.split().0.take_reload(goal, supporters, me);
                host.changed(StoreChange::Images(images), cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn needs_load(&self, signed_in: bool) -> bool {
        matches!(self.goal, Load::Idle | Load::Failed(_))
            || matches!(self.supporters, Load::Idle | Load::Failed(_))
            || (signed_in && matches!(self.me, Load::Idle | Load::Failed(_)))
    }

    /// Forgets the support status of someone who signed out. True when a signed-in user's status
    /// still has to be fetched.
    fn follow_sign_in(&mut self, signed_in: bool) -> bool {
        if !signed_in {
            self.me = Load::Idle;
            return false;
        }
        matches!(self.me, Load::Idle | Load::Failed(_))
    }

    fn reloading(&mut self, signed_in: bool) {
        self.goal = Load::Loading;
        self.supporters = Load::Loading;
        self.me = if signed_in { Load::Loading } else { Load::Idle };
    }

    /// Takes a reload's results, and gives back the supporters' avatar links to fetch.
    fn take_reload(
        &mut self,
        goal: Result<Goal, ApiError>,
        supporters: Result<Vec<Supporter>, ApiError>,
        me: Option<Result<Me, ApiError>>,
    ) -> Vec<String> {
        let supporters = supporters.map(|mut all| {
            all.sort_by_key(|x| !x.plus());
            all
        });
        let images = supporters
            .iter()
            .flatten()
            .filter_map(|x| x.display.avatar_url.clone())
            .collect();
        self.goal = load(goal);
        self.supporters = load(supporters);
        self.me = me.map(load).unwrap_or(Load::Idle);
        images
    }
}

impl Host<StorePage> for Shell {
    fn split(&mut self) -> (&mut StorePage, StoreNeeds<'_>) {
        (
            &mut self.store_page,
            StoreNeeds {
                session: self.account.session.as_ref(),
            },
        )
    }

    fn changed(&mut self, change: StoreChange, cx: &mut Context<Self>) {
        match change {
            StoreChange::Images(images) => {
                crate::features::multiplayer::Multiplayer::load_faces(self, images, cx)
            }
        }
    }
}

fn load<T>(got: Result<T, ApiError>) -> Load<T> {
    match got {
        Ok(v) => Load::Ready(v),
        Err(e) => Load::Failed(e.message),
    }
}

#[cfg(test)]
mod tests {
    use swofty_core::online::server::api::ApiError;
    use swofty_core::online::server::support::{Goal, Supporter};

    use super::{Load, StorePage};

    fn error(message: &str) -> ApiError {
        ApiError {
            status: 500,
            message: message.to_owned(),
            retry_after: None,
        }
    }

    fn goal(cents: u64) -> Goal {
        Goal {
            goal_cents: cents,
            raised_cents: cents / 2,
            since: String::new(),
            costs: Vec::new(),
            donate_url: Some("https://donate.example/".to_owned()),
        }
    }

    fn supporter(avatar: Option<&str>) -> Supporter {
        serde_json::from_value(serde_json::json!({
            "tier": "plus",
            "display": { "avatar_url": avatar },
        }))
        .unwrap()
    }

    fn member(tier: &str, name: &str) -> Supporter {
        serde_json::from_value(serde_json::json!({
            "tier": tier,
            "display": { "name": name },
        }))
        .unwrap()
    }

    #[test]
    fn the_page_loads_until_its_goal_supporters_and_the_users_status_are_in() {
        let mut page = StorePage::default();
        assert!(page.needs_load(false));
        page.reloading(false);
        assert!(!page.needs_load(false));
        assert!(matches!(page.me, Load::Idle));
        assert!(page.needs_load(true));
        page.reloading(true);
        assert!(!page.needs_load(true));
        page.goal = Load::Failed("down".to_owned());
        assert!(page.needs_load(false));
    }

    #[test]
    fn signing_out_forgets_the_status_and_signing_in_fetches_it_once() {
        let mut page = StorePage::default();
        assert!(page.follow_sign_in(true));
        page.me = Load::Loading;
        assert!(!page.follow_sign_in(true));
        assert!(!page.follow_sign_in(false));
        assert!(matches!(page.me, Load::Idle));
        page.me = Load::Failed("down".to_owned());
        assert!(page.follow_sign_in(true));
    }

    #[test]
    fn a_reload_shows_its_results_and_hands_back_the_pictures_to_fetch() {
        let mut page = StorePage::default();
        let images = page.take_reload(
            Ok(goal(5_000)),
            Ok(vec![supporter(Some("https://a/1.png")), supporter(None)]),
            None,
        );
        assert_eq!(images, ["https://a/1.png"]);
        assert!(matches!(&page.goal, Load::Ready(g) if g.goal_cents == 5_000));
        assert!(matches!(&page.supporters, Load::Ready(s) if s.len() == 2));
        assert!(matches!(page.me, Load::Idle));
        page.take_reload(Err(error("goal down")), Err(error("supporters down")), None);
        assert!(matches!(&page.goal, Load::Failed(m) if m == "goal down"));
        assert!(matches!(&page.supporters, Load::Failed(m) if m == "supporters down"));
    }

    #[test]
    fn supporter_plus_members_are_listed_first_in_the_order_the_server_sent() {
        let mut page = StorePage::default();
        page.take_reload(
            Ok(goal(1_000)),
            Ok(vec![
                member("supporter", "Ann"),
                member("supporter_plus", "Bo"),
                member("supporter", "Cy"),
                member("supporter_plus", "Di"),
            ]),
            None,
        );
        let Load::Ready(all) = &page.supporters else {
            panic!("the supporters did not load");
        };
        let names: Vec<_> = all.iter().filter_map(|x| x.display.name.as_deref()).collect();
        assert_eq!(names, ["Bo", "Di", "Ann", "Cy"]);
    }
}
