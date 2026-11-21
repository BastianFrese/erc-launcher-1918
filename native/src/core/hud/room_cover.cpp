#include "core/runtime/runtime.hpp"
// ERC-BEGIN netplay-cover
#include "core/netplay/netplay_room.hpp"
// ERC-END netplay-cover

#include "core/hud/room_cover.hpp"
#include "core/hud/room_cover_logic.hpp"

#include <swofty/mixin.hpp>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <mutex>

namespace hud::room_cover {

namespace {

constexpr uint32_t BG_TOP = 0x0a0b0dffu;
constexpr uint32_t BG_BOTTOM = 0x16181cffu;
constexpr uint32_t TEXT = 0xf2f4f7ffu;
constexpr uint32_t MUTED = 0xb6babdffu;
constexpr uint32_t FAINT = 0x737988ffu;
constexpr uint32_t ACCENT = 0xe10600ffu;
constexpr uint32_t ACCENT_HI = 0xff2b1fffu;
constexpr uint32_t TRACK = 0x22262dffu;
/// Startampel: dunkel, solange das Licht aus ist; gruen, wenn es losgeht.
constexpr uint32_t LIGHT_OFF = 0x3a1418ffu;
constexpr uint32_t READY = 0x00d26affu;

uint32_t with_alpha(uint32_t rgba, float k)
{
    uint32_t a = uint32_t(std::clamp(float(rgba & 0xff) * k, 0.0f, 255.0f));
    return (rgba & 0xffffff00u) | a;
}

hud::drawlist::TextStyle style(float px, uint32_t rgba, float tracking = 0)
{
    hud::drawlist::TextStyle s;
    s.px = px;
    s.rgba = rgba;
    s.tracking = px * tracking;
    return s;
}

float text_width(const hud::drawlist::TextStyle &st, const std::string &t)
{
    float w = 0, h = 0;
    hud::drawlist::measure(st, t, &w, &h);
    return w;
}

class CoverScreen {
public:
    void step(const char *what, float fraction, float ceiling);
    Snapshot snapshot();
    void set_subtitle(const char *text);
    bool setup();
    bool active() { return armed.load(); }
    void paint(hud::drawlist::Frame &f, uint64_t now);

private:
    void draw(hud::drawlist::Frame &f, uint64_t now, float alpha);

    std::mutex lock;
    Cover cover;
    Kind kind = Kind::none;
    Progress progress;
    std::string subtitle = "Sitzung wird gestartet";
    std::atomic<bool> armed{false};
    uint64_t polled_at = 0;
    Signal last = Signal::waiting;
};

constinit CoverScreen screen;

void CoverScreen::draw(hud::drawlist::Frame &f, uint64_t now, float alpha)
{
    hud::drawlist::reset_state(f);
    f.opacity = alpha;
    float W = f.width, H = f.height, u = H / 1080.0f, cx = W * 0.5f;
    // Hintergrund: das ERC-Bild aus der Boxengasse (aus der 1.4.4), sonst der Verlauf.
    static int bg_slot = -1;
    static bool bg_tried = false;
    static uint32_t bg_w = 0, bg_h = 0;
    if (!bg_tried) {
        bg_tried = true;
        bg_slot = runtime::overlay_image("core.room_cover\nbg", "swofty:addons/core/cover.png", &bg_w, &bg_h);
    }
    if (bg_slot >= 0 && bg_w && bg_h) {
        float s = std::max(W / float(bg_w), H / float(bg_h));
        float dw = bg_w * s, dh = bg_h * s;
        hud::drawlist::image(f, bg_slot, (W - dw) * 0.5f, (H - dh) * 0.5f, dw, dh, 0xffffffffu);
    } else {
        hud::drawlist::rect_gradient(f, 0, 0, W, H, 0, BG_TOP, BG_BOTTOM, 0);
    }
    // Abdunkeln, damit Text und Instrumente lesbar bleiben.
    hud::drawlist::rect_gradient(f, 0, 0, W, H * 0.42f, 0, 0x000000a8, 0x00000000, 0);
    hud::drawlist::rect_gradient(f, 0, H * 0.52f, W, H * 0.48f, 0, 0x00000000, 0x000000c4, 0);

    bool ready = progress.completed();
    int pct = progress.percent(now);
    float shown = progress.shown(now);

    // Wortmarke oben in der Mitte.
    hud::drawlist::TextStyle word = style(34 * u, TEXT, 0.34f);
    std::string name = "ERC";
    float ww = text_width(word, name);
    hud::drawlist::text(f, cx - ww * 0.5f + word.tracking * 0.5f, 44 * u, word, name);
    hud::drawlist::TextStyle tag = style(13 * u, MUTED, 0.55f);
    std::string tagline = "LAUNCHER";
    float tw = text_width(tag, tagline);
    hud::drawlist::text(f, cx - tw * 0.5f + tag.tracking * 0.5f, 88 * u, tag, tagline);

    // Vier Instrumente wie auf dem Armaturenbrett.
    float pw2 = 300 * u, ph = 104 * u, pad = 54 * u;
    auto panel = [&](float x, float y) {
        hud::drawlist::rect_rounded(f, x, y, pw2, ph, 6 * u, 0x101216cc);
        hud::drawlist::rect_rounded(f, x, y, pw2, 2 * u, u, with_alpha(ACCENT, 0.45f));
    };
    auto heading = [&](float x, float y, const char *t) {
        hud::drawlist::TextStyle h = style(13 * u, MUTED, 0.16f);
        hud::drawlist::text(f, x + 14 * u, y + 12 * u, h, t);
    };

    float top_y = 130 * u, bot_y = H - 130 * u - ph;
    panel(pad, top_y);
    heading(pad, top_y, "SYSTEM CHECK");
    {
        hud::drawlist::TextStyle v = style(15 * u, ready ? READY : TEXT, 0.10f);
        hud::drawlist::text(f, pad + 14 * u, top_y + 40 * u, v, ready ? "PASS" : "LAEUFT");
        for (int i = 0; i < 5; ++i) {
            bool on = ready || i < std::clamp(pct / 20, 0, 5);
            hud::drawlist::rect_rounded(f, pad + 14 * u + i * 26 * u, top_y + 72 * u, 18 * u, 18 * u, 3 * u,
                                        on ? READY : 0x2a2f37ffu);
        }
    }
    panel(W - pad - pw2, top_y);
    heading(W - pad - pw2, top_y, "STRECKE");
    {
        hud::drawlist::TextStyle v = style(15 * u, TEXT, 0.08f);
        hud::drawlist::text(f, W - pad - pw2 + 14 * u, top_y + 40 * u, v, subtitle);
        // Ein Streckenumriss als Platzhalter, bis echtes Kartenmaterial da ist.
        const float pts[][2] = {{0.05f, 0.62f}, {0.18f, 0.34f}, {0.38f, 0.30f}, {0.52f, 0.46f}, {0.44f, 0.66f},
                                {0.60f, 0.78f}, {0.84f, 0.72f}, {0.90f, 0.52f}, {0.74f, 0.40f}};
        for (size_t i = 0; i + 1 < sizeof pts / sizeof pts[0]; ++i)
            hud::drawlist::line(f, W - pad - pw2 + 14 * u + pts[i][0] * 270 * u, top_y + 52 * u + pts[i][1] * 40 * u,
                                W - pad - pw2 + 14 * u + pts[i + 1][0] * 270 * u,
                                top_y + 52 * u + pts[i + 1][1] * 40 * u, 2 * u, with_alpha(ACCENT_HI, 0.75f));
    }
    panel(pad, bot_y);
    heading(pad, bot_y, "ANTRIEB");
    {
        for (int i = 0; i < 9; ++i) {
            float hgt = (0.25f + 0.75f * std::fabs(std::sin(float(i) * 1.7f))) * 54 * u;
            hud::drawlist::rect_rounded(f, pad + 14 * u + i * 30 * u, bot_y + ph - 16 * u - hgt, 18 * u, hgt, 2 * u,
                                        i * 11 < pct ? ACCENT : 0x2a2f37ffu);
        }
    }
    panel(W - pad - pw2, bot_y);
    heading(W - pad - pw2, bot_y, "FUNK");
    {
        hud::drawlist::TextStyle v = style(15 * u, READY, 0.10f);
        hud::drawlist::text(f, W - pad - pw2 + 14 * u, bot_y + 40 * u, v, "ONLINE");
        for (int i = 0; i < 22; ++i) {
            float x = W - pad - pw2 + 14 * u + i * 12 * u;
            float a = std::sin(float(now % 2400) / 2400.0f * 6.283f + i * 0.7f);
            float hgt = (0.2f + 0.8f * std::fabs(a)) * 28 * u;
            hud::drawlist::rect_rounded(f, x, bot_y + ph - 14 * u - hgt, 3 * u, hgt, 1.5f * u, with_alpha(READY, 0.7f));
        }
    }

    // In der Mitte: der Ring mit dem Fortschritt.
    float r = 150 * u, gy = H * 0.52f;
    constexpr float PI_ = 3.14159265f, start = -0.75f * PI_, span = 1.5f * PI_;
    hud::drawlist::arc(f, cx, gy, r, start, start + span, 8 * u, TRACK);
    float to = start + span * shown;
    if (shown > 0.001f) {
        hud::drawlist::arc(f, cx, gy, r, start, to, 22 * u, with_alpha(ACCENT, 0.16f));
        hud::drawlist::arc(f, cx, gy, r, start, to, 8 * u, ready ? READY : ACCENT);
    }
    hud::drawlist::TextStyle big = style(64 * u, TEXT, 0.02f);
    std::string pt = std::to_string(pct) + "%";
    float pw3 = text_width(big, pt);
    hud::drawlist::text(f, cx - pw3 * 0.5f, gy - 40 * u, big, pt);
    hud::drawlist::TextStyle rdy = style(16 * u, ready ? READY : MUTED, 0.22f);
    std::string label = ready ? "BEREIT" : "LAEDT";
    float lw = text_width(rdy, label);
    hud::drawlist::text(f, cx - lw * 0.5f, gy + 40 * u, rdy, label);

    // Darunter, was gerade passiert.
    hud::drawlist::TextStyle status = style(22 * u, MUTED, 0.06f);
    std::string line = progress.label();
    float sw = text_width(status, line);
    hud::drawlist::text(f, cx - sw * 0.5f, H - 74 * u, status, line);
}

void CoverScreen::step(const char *what, float fraction, float ceiling)
{
    std::lock_guard<std::mutex> g(lock);
    progress.step(what, fraction, GetTickCount64(), ceiling);
}

Snapshot CoverScreen::snapshot()
{
    Snapshot out;
    if (!armed.load())
        return out;
    std::lock_guard<std::mutex> g(lock);
    out.active = true;
    out.label = progress.label();
    out.percent = progress.target_percent();
    return out;
}

void CoverScreen::set_subtitle(const char *text)
{
    if (!text || !*text)
        return;
    std::lock_guard<std::mutex> g(lock);
    subtitle = text;
}

bool CoverScreen::setup()
{
    char env[1024] = {};
    DWORD n = GetEnvironmentVariableA("SWOFTY_DIRECT_START", env, sizeof env);
    std::string value = n && n < sizeof env ? std::string(env) : std::string(), text;
    if (value.empty())
        text = runtime::read_file(runtime::swofty_root() + L"direct_start.json", size_t(1) << 16);
    else if (value[0] == '@')
        text = runtime::read_file(runtime::wide(value.c_str() + 1), size_t(1) << 16);
    Kind k = request_kind(value, text);
    if (k == Kind::none)
        return false;
    std::lock_guard<std::mutex> g(lock);
    kind = k;
    if (k == Kind::session) {
        json::Value v;
        bool tt = json::parse(text, v) && v["mode"].str() == "time_trial";
        subtitle = tt ? "Zeitfahren" : "Grand-Prix-Sitzung";
    }
    cover.arm(k == Kind::session ? SESSION_CAP_MS : CAP_MS);
    armed.store(true);
    return true;
}

void CoverScreen::paint(hud::drawlist::Frame &f, uint64_t now)
{
    if (!armed.load())
        return;
    std::lock_guard<std::mutex> g(lock);
    if (now - polled_at >= 100) {
        polled_at = now;
        if (kind == Kind::session) {
            int sig = runtime::direct_start_cover_signal();
            last = sig < 0 ? Signal::failed : sig > 0 ? Signal::shown : Signal::waiting;
        } else {
            // ERC-BEGIN netplay-cover-signal
            // Unser Netplay-Weg beantwortet die Frage selbst; alles andere fragt den Sitzungsdienst.
            int sig = runtime::session_host_cover_signal();
            if (runtime::netplay_room_requested()) {
                switch (runtime::netplay_room::cover_signal()) {
                case runtime::netplay_room::CoverSignal::failed: sig = -1; break;
                case runtime::netplay_room::CoverSignal::shown: sig = 1; break;
                default: sig = 0; break;
                }
            }
            // ERC-END netplay-cover-signal
            last = sig < 0 ? Signal::failed : sig > 0 ? Signal::shown : Signal::waiting;
        }
    }
    Event ev = Event::none;
    float alpha = cover.frame(now, last, &ev);
    const char *what = kind == Kind::session ? "test session" : "room launch";
    if (ev == Event::went_up)
        swofty::log("[cover] %s cover up (%.0fx%.0f)", what, double(f.width), double(f.height));
    else if (ev == Event::came_off)
        swofty::log("[cover] %s cover off: %s", what, why_name(cover.why()));
    if (!cover.drawing()) {
        armed.store(false);
        return;
    }
    if (alpha <= 0)
        return;
    if (last == Signal::shown)
        progress.complete("Ready", now);
    draw(f, now, alpha);
}

}

void step(const char *what, float fraction, float ceiling) { screen.step(what, fraction, ceiling); }

Snapshot snapshot() { return screen.snapshot(); }

void set_subtitle(const char *text) { screen.set_subtitle(text); }

bool setup() { return screen.setup(); }

bool active() { return screen.active(); }

void paint(hud::drawlist::Frame &f, uint64_t now) { screen.paint(f, now); }

}
