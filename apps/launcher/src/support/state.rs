use gpui_kit::*;
use swofty_core::support::{Goal, Supporter};
use swofty_core::timetrial::api::Api;

use crate::shell::Shell;
use crate::time_trial::Load;

/// Der Spendenstand kommt von unserem Raumserver, nicht vom Katalog.
pub(crate) const SUPPORT_SERVER: &str = "https://raum.erdi-erc.de";

pub(crate) struct SupportPage {
    pub goal: Load<Goal>,
    pub supporters: Load<Vec<Supporter>>,
}

impl Default for SupportPage {
    fn default() -> Self {
        Self {
            goal: Load::Idle,
            supporters: Load::Idle,
        }
    }
}

impl Shell {
    fn support_api(&self) -> Api {
        Api::with_server(SUPPORT_SERVER, "")
    }

    pub(crate) fn support_open(&mut self, cx: &mut Context<Self>) {
        if matches!(self.support.goal, Load::Idle | Load::Failed(_)) {
            self.support_reload(cx);
        }
    }

    pub(super) fn support_reload(&mut self, cx: &mut Context<Self>) {
        let api = self.support_api();
        self.support.goal = Load::Loading;
        self.support.supporters = Load::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (goal, supporters) = cx
                .background_executor()
                .spawn(async move { (api.support_goal(), api.support_supporters()) })
                .await;
            this.update(cx, |s, cx| {
                let faces: Vec<String> = supporters
                    .iter()
                    .flatten()
                    .filter_map(|x| x.display.avatar_url.clone())
                    .collect();
                s.support.goal = load(goal);
                s.support.supporters = load(supporters);
                s.load_faces(faces, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn load<T>(got: Result<T, swofty_core::timetrial::api::ApiError>) -> Load<T> {
    match got {
        Ok(v) => Load::Ready(v),
        Err(e) => Load::Failed(e.message),
    }
}
