use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;
use swofty_core::support::{Goal, Supporter, tier_name};
use swofty_i18n::tr;

use crate::pages::GUTTER;
use crate::scale::px;
use crate::shell::Shell;
use crate::theme::{Palette, ScrollArea};
use crate::time_trial::Load;

const LIST_WIDTH: f32 = 470.;
const PINK: u32 = 0xff5fa2;
const HOT_PINK: u32 = 0xff2d95;
const PURPLE: u32 = 0x9b30ff;
const PLUS_NAME: u32 = 0xd45af0;

fn gradient() -> Background {
    linear_gradient(
        90.,
        linear_color_stop(rgb(HOT_PINK), 0.),
        linear_color_stop(rgb(PURPLE), 1.),
    )
}

fn money(cents: u64) -> String {
    match cents % 100 {
        0 => format!("${}", cents / 100),
        c => format!("${}.{c:02}", cents / 100),
    }
}

fn heading(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

fn muted(text: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .text_size(px(12.))
        .text_color(p.text_muted)
        .child(text.into())
}

/// A soft white streak that sweeps left to right across its parent, which must be relative and clip.
fn glint(id: impl Into<ElementId>, every: Duration) -> impl IntoElement {
    let half = |from: u32, to: u32| {
        div().h_full().w(px(9.)).bg(linear_gradient(
            90.,
            linear_color_stop(rgba(from), 0.),
            linear_color_stop(rgba(to), 1.),
        ))
    };
    h_flex()
        .absolute()
        .top_0()
        .bottom_0()
        .child(half(0xffffff00, 0xffffff99))
        .child(half(0xffffff99, 0xffffff00))
        .with_animation(id, Animation::new(every).repeat(), |el, t| {
            el.left(relative(t * 2.4 - 0.4))
        })
}

fn badge(id: impl Into<ElementId>, plus: bool) -> Div {
    div()
        .relative()
        .overflow_hidden()
        .flex_shrink_0()
        .px(px(8.))
        .py(px(2.))
        .rounded_full()
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(0xffffff))
        .map(|d| match plus {
            true => d.bg(gradient()).child(glint(id, Duration::from_millis(3200))),
            false => d.bg(rgb(PINK)),
        })
        .child(tier_name(if plus { "supporter_plus" } else { "supporter" }))
}

impl Shell {
    pub(crate) fn support_page(&mut self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let refresh = Button::new("support-refresh")
            .ghost()
            .icon(IconName::ArrowRightLeft)
            .on_click(cx.listener(|this, _, _, cx| this.support_reload(cx)));
        let list = self.support_list(p);
        let main = self.support_main(p);
        let header = self.header(
            tr("Support"),
            tr("Keep ERC Launcher free and open source").to_owned(),
            h_flex().gap(px(8.)).items_center().child(refresh),
            p,
        );
        v_flex()
            .relative()
            .size_full()
            .child(header)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(24.))
                    .px(px(GUTTER))
                    .pb(px(16.))
                    .child(list)
                    .child(main),
            )
            .into_any_element()
    }

    fn support_list(&self, p: Palette) -> AnyElement {
        let frame = || v_flex().w(px(LIST_WIDTH)).flex_shrink_0().h_full().min_h_0();
        let note = |text: String| {
            frame()
                .text_size(px(13.))
                .text_color(p.text_muted)
                .child(text)
                .into_any_element()
        };
        let supporters = match &self.support.supporters {
            Load::Idle | Load::Loading => return note(tr("Loading supporters…").to_owned()),
            Load::Failed(e) => return note(e.clone()),
            Load::Ready(s) if s.is_empty() => return note(tr("No supporters yet. Be the first.").to_owned()),
            Load::Ready(s) => s,
        };
        let rows: Vec<AnyElement> = supporters
            .iter()
            .enumerate()
            .map(|(i, s)| self.supporter_row(i, s, p))
            .collect();
        frame()
            .child(
                h_flex()
                    .pb(px(6.))
                    .gap(px(8.))
                    .items_center()
                    .child(heading(tr("Supporters")))
                    .child(muted(supporters.len().to_string(), p)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .scroll_y("support-list")
                    .child(h_flex().flex_wrap().items_start().pr(px(8.)).children(rows)),
            )
            .into_any_element()
    }

    fn supporter_row(&self, i: usize, s: &Supporter, p: Palette) -> AnyElement {
        let name = match (&s.display.name, s.display.anonymous) {
            (Some(n), false) => n.clone(),
            _ => tr("Anonymous").to_owned(),
        };
        let face = match (&s.display.name, s.display.anonymous) {
            (Some(n), false) => self.face(n, s.display.avatar_url.as_deref(), 36., p),
            _ => div()
                .size(px(36.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(p.surface)
                .child(Icon::new(IconName::User).size(px(18.)).text_color(p.text_muted)),
        };
        let plus = s.plus();
        let face = face.when(plus, |d| d.border_2().border_color(rgb(PURPLE)));
        let bio = s.bio.clone().filter(|b| !b.trim().is_empty());
        h_flex()
            .id(SharedString::from(format!("supporter-{i}")))
            .w(relative(0.5))
            .py(px(8.))
            .pr(px(8.))
            .gap(px(10.))
            .items_center()
            .child(face)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(3.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(if plus { PLUS_NAME } else { PINK }))
                                    .child(name),
                            )
                            .child(badge(SharedString::from(format!("supporter-glint-{i}")), plus)),
                    )
                    .children(bio.map(|b| muted(b, p))),
            )
            .into_any_element()
    }

    fn support_main(&mut self, p: Palette) -> AnyElement {
        let body: AnyElement = match &self.support.goal {
            Load::Idle | Load::Loading => muted(tr("Loading the goal…"), p).into_any_element(),
            Load::Failed(e) => muted(e.clone(), p).into_any_element(),
            Load::Ready(g) => {
                let donate = g
                    .donate_url
                    .clone()
                    .filter(|u| u.starts_with("https://"));
                v_flex()
                    .items_center()
                    .gap(px(18.))
                    .child(goal_body(g, p))
                    .children(donate.map(|url| self.donate_button(&url, p)))
                    .into_any_element()
            }
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .min_h_0()
            .scroll_y("support-main")
            .child(v_flex().gap(px(26.)).pr(px(4.)).child(body))
            .into_any_element()
    }
}

impl Shell {
    fn donate_button(&self, url: &str, p: Palette) -> AnyElement {
        let url = url.to_owned();
        v_flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .id("support-donate")
                    .relative()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(52.))
                    .px(px(44.))
                    .rounded_full()
                    .text_size(px(16.))
                    .font_weight(FontWeight::EXTRA_BOLD)
                    .text_color(rgb(0xffffff))
                    .bg(gradient())
                    .child(glint("support-donate-glint", Duration::from_millis(2600)))
                    .cursor_pointer()
                    .hover(|d| d.opacity(0.9))
                    .child(tr("DONATE"))
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            )
            .child(muted(tr("Opens in your browser"), p))
            .into_any_element()
    }
}

fn goal_body(g: &Goal, p: Palette) -> Div {
    let filled = g.fraction();
    // Thin lines that drift up and fade above the filled part of the bar.
    let floats = (0..9usize).map(|i| {
        let at = filled * ((i as f32 * 0.137 + 0.05) % 1.);
        let wide = 10. + (i % 4) as f32 * 7.;
        let period = Duration::from_millis(2200 + (i as u64 * 430) % 1900);
        div()
            .absolute()
            .left(relative(at))
            .h(px(2.))
            .w(px(wide))
            .rounded_full()
            .bg(rgb(if i % 2 == 0 { HOT_PINK } else { PURPLE }))
            .with_animation(
                SharedString::from(format!("goal-float-{i}")),
                Animation::new(period).repeat(),
                move |el, t| {
                    let t = (t + i as f32 * 0.21) % 1.;
                    el.top(px(30. - 30. * t))
                        .ml(px(18. * t))
                        .opacity((t * std::f32::consts::PI).sin() * 0.8)
                },
            )
    });
    // Bright sparks that run along the filled part.
    let sparks = (0..3usize).map(|i| {
        let half = |from: u32, to: u32| {
            div().h_full().w(px(22.)).bg(linear_gradient(
                90.,
                linear_color_stop(rgba(from), 0.),
                linear_color_stop(rgba(to), 1.),
            ))
        };
        h_flex()
            .absolute()
            .top_0()
            .bottom_0()
            .child(half(0xffffff00, 0xffffffcc))
            .child(half(0xffffffcc, 0xffffff00))
            .with_animation(
                SharedString::from(format!("goal-spark-{i}")),
                Animation::new(Duration::from_millis(1700 + i as u64 * 650)).repeat(),
                move |el, t| el.left(relative(((t + i as f32 * 0.37) % 1.) * 1.3 - 0.2)),
            )
    });
    v_flex()
        .gap(px(6.))
        .when(g.goal_cents > 0, |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(PINK))
                    .child(tr("Monthly goal").to_uppercase()),
            )
            .child(
                h_flex()
                    .items_end()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(44.))
                            .line_height(px(46.))
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .child(money(g.raised_cents)),
                    )
                    .child(
                        div()
                            .pb(px(6.))
                            .text_size(px(16.))
                            .text_color(p.text_muted)
                            .child(format!("{} {} {}", tr("of"), money(g.goal_cents), tr("/ month"))),
                    ),
            )
            .child(div().relative().h(px(32.)).w_full().children(floats))
            .child(
                div()
                    .h(px(20.))
                    .w_full()
                    .rounded_full()
                    .bg(p.border_variant)
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .overflow_hidden()
                            .h_full()
                            .w(relative(filled))
                            .rounded_full()
                            .bg(gradient())
                            .children(sparks),
                    ),
            )
        })
        .when(!g.costs.is_empty(), |d| {
            d.child(
                h_flex()
                    .w_full()
                    .pt(px(12.))
                    .justify_center()
                    .flex_wrap()
                    .gap_x(px(28.))
                    .gap_y(px(6.))
                    .children(g.costs.iter().map(|c| {
                        h_flex()
                            .gap(px(10.))
                            .items_center()
                            .text_size(px(15.))
                            .child(div().size(px(6.)).rounded_full().bg(rgb(PINK)))
                            .child(c.clone())
                    })),
            )
        })
}
