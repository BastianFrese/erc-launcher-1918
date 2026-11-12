use gpui_kit::component::scroll::{Scrollable, ScrollableElement, Scrollbar, ScrollbarHandle, ScrollbarMode};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{Anchor, App, ElementId, Global, Hsla, hsla, px, rgb};

#[derive(Clone, Copy)]
pub struct Palette {
    pub chrome: Hsla,
    pub background: Hsla,
    pub surface: Hsla,
    pub hover: Hsla,
    pub elevated: Hsla,
    pub selected: Hsla,
    pub border_variant: Hsla,
    pub border: Hsla,
    pub switch_off: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_placeholder: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub on_accent: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub error: Hsla,
    pub tab_home: Hsla,
    pub tab_library: Hsla,
    pub tab_discover: Hsla,
    pub tab_trial: Hsla,
    pub tab_support: Hsla,
}

impl Global for Palette {}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            // Die ERC-Farben aus `ui/tokens.css` des eigenen Launchers:
            // Tarmac, Kerb, Ink und der eine Akzent (Flaggenrot).
            chrome: hex(0x0b0c0e),
            background: hex(0x14171b),
            surface: hex(0x191d23),
            hover: hex(0x202630),
            elevated: hex(0x232830),
            selected: hex(0x291619),
            border_variant: hex(0x22262d),
            border: hex(0x2e343d),
            switch_off: hex(0x3a4048),
            text: hex(0xf2f4f7),
            text_muted: hex(0xa9adb3),
            text_placeholder: hex(0x6a6d72),
            accent: hex(0xe10600),
            accent_hover: hex(0xff2b1f),
            on_accent: hex(0xffffff),
            success: hex(0x00d26a),
            warning: hex(0xffd100),
            error: hex(0xff5a4d),
            tab_home: hex(0xe10600),
            tab_library: hex(0xd9dde3),
            tab_discover: hex(0x00b3a4),
            tab_trial: hex(0xffc247),
            tab_support: hex(0x8b93a1),
        }
    }
}

pub fn palette(cx: &App) -> Palette {
    *cx.global::<Palette>()
}

pub fn init(cx: &mut App) {
    let p = Palette::dark();
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |t| {
        t.font_size = px(14.);
        t.radius = px(2.);
        t.radius_lg = px(3.);
        t.shadow = false;
        t.background = p.background;
        t.foreground = p.text;
        t.border = p.border;
        t.input = p.border;
        t.ring = p.accent;
        t.caret = p.accent;
        t.selection = p.selected;
        t.muted = p.elevated;
        t.muted_foreground = p.text_muted;
        t.accent = p.hover;
        t.accent_foreground = p.text;
        t.primary = p.accent;
        t.primary_hover = p.accent_hover;
        t.primary_active = p.accent;
        t.primary_foreground = p.on_accent;
        t.secondary = p.elevated;
        t.secondary_hover = p.selected;
        t.secondary_active = p.selected;
        t.secondary_foreground = p.text;
        t.button = p.elevated;
        t.button_hover = p.selected;
        t.button_active = p.selected;
        t.button_foreground = p.text;
        t.button_primary = p.accent;
        t.button_primary_hover = p.accent_hover;
        t.button_primary_active = p.accent;
        t.button_primary_foreground = p.on_accent;
        t.link = p.text;
        t.link_hover = p.accent;
        t.link_active = p.accent;
        t.popover = p.elevated;
        t.popover_foreground = p.text;
        t.colors.list = p.background;
        t.list_hover = p.hover;
        t.list_active = p.selected;
        t.list_active_border = p.border;
        t.sidebar = p.chrome;
        t.sidebar_border = p.border_variant;
        t.sidebar_foreground = p.text;
        t.title_bar = p.chrome;
        t.title_bar_border = p.chrome;
        t.status_bar = p.chrome;
        t.status_bar_border = p.border_variant;
        t.window_border = p.border;
        t.switch = p.switch_off;
        t.switch_thumb = p.text;
        t.scrollbar_mode = ScrollbarMode::Always;
        t.scrollbar = hsla(0., 0., 0., 0.);
        t.scrollbar_thumb = p.switch_off;
        t.scrollbar_thumb_hover = p.text_placeholder;
        t.success = p.success;
        t.warning = p.warning;
        t.danger = p.error;
        t.overlay = hsla(0., 0., 0., 0.55);
        t.notification.placement = Anchor::BottomRight;
        t.notification.margins.bottom = px(crate::play_bar::HEIGHT + 16.);
    });
    cx.set_global(p);
    cx.set_global(crate::notices::Notices::default());
}

/// Room a column keeps clear of the scrollbar thumb at its right edge.
pub const SCROLLBAR_GAP: f32 = 14.;

/// Scrolls an area that has no scroll handle of its own, with the launcher's
/// scrollbar. `id` keys the scroll position.
pub trait ScrollArea: ScrollableElement + Sized {
    fn scroll_y(self, id: impl Into<ElementId>) -> Scrollable<Self> {
        self.overflow_y_scrollbar().id(id)
    }
}

impl<E: ScrollableElement> ScrollArea for E {}

#[track_caller]
pub fn scrollbar_x(handle: &(impl ScrollbarHandle + Clone)) -> Scrollbar {
    Scrollbar::horizontal(handle)
}
