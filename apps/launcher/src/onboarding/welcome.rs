use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use swofty_i18n::tr;

use super::Step;
use crate::scale::px;
use crate::theme::Palette;

const INTRO: Duration = Duration::from_millis(1500);

fn within(t: f32, start: f32, end: f32) -> f32 {
    ((t - start) / (end - start)).clamp(0., 1.)
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(4)
}

/// Der Hintergrund: Tarmac-Schwarz mit einem roten Schein von oben, wie die Seite.
pub(super) fn backdrop(_step: Step, p: Palette) -> impl IntoElement {
    // Ein Schein von oben ueber die ganze Hoehe — kein eigener Abschnitt, sonst
    // steht eine sichtbare Naht im Bild.
    let glow = div().absolute().size_full().bg(linear_gradient(
        180.,
        linear_color_stop(p.accent.opacity(0.20), 0.),
        linear_color_stop(p.accent.opacity(0.), 1.),
    ));
    div().absolute().size_full().child(glow)
}

/// Die Wortmarke vom Ladebildschirm: weit gesperrtes `ERC`, darunter
/// `L A U N C H E R` zwischen zwei Haarlinien.
fn wordmark(p: Palette) -> Div {
    let letters = div()
        .text_size(px(96.))
        .line_height(px(100.))
        .font_weight(FontWeight::BOLD)
        .text_color(p.text)
        .child("E R C");
    let rule = |width: f32| {
        div()
            .h(px(1.))
            .w(px(width))
            .bg(p.accent.opacity(0.6))
    };
    let tag = h_flex()
        .gap(px(14.))
        .items_center()
        .child(rule(64.))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.accent)
                .child("L A U N C H E R"),
        )
        .child(rule(64.));
    v_flex().items_center().gap(px(12.)).child(letters).child(tag)
}

fn reveal(id: &'static str, start: f32, rise: f32, line: Div) -> impl IntoElement {
    line.with_animation(id, Animation::new(INTRO), move |el, t| {
        let local = ease_out(within(t, start, (start + 0.4).min(1.)));
        el.opacity(local).relative().top(px((1. - local) * rise))
    })
}

pub(super) fn view(p: Palette) -> Div {
    let tagline = div()
        .max_w(px(460.))
        .text_center()
        .text_size(px(15.))
        .text_color(p.text_muted)
        .child(tr("Addons, mods and tracks for F1 25, in one place."));
    v_flex()
        .items_center()
        .gap(px(6.))
        .child(reveal("onboarding-mark", 0.05, 22., wordmark(p)))
        .child(div().h(px(22.)))
        .child(reveal("onboarding-tagline", 0.45, 14., tagline))
}
