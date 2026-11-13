#include "runtime.hpp"

#include "netplay_room.hpp"
#include "room_cover.hpp"
#include "room_cover_logic.hpp"

#include <swofty/mixin.hpp>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <mutex>

namespace runtime::room_cover {

namespace {

std::mutex lock;
Cover cover;
Kind kind = Kind::none;
Progress progress;
std::string subtitle = "Sitzung wird gestartet";
std::atomic<bool> armed{false};
uint64_t polled_at = 0;
Signal last = Signal::waiting;

constexpr uint32_t BG_TOP = 0x0a0b0dffu;
constexpr uint32_t BG_BOTTOM = 0x16181cffu;
constexpr uint32_t TEXT = 0xf2f4f7ffu;
constexpr uint32_t MUTED = 0xb6babdffu;
constexpr uint32_t FAINT = 0x737988ffu;
constexpr uint32_t ACCENT = 0xe10600ffu;
constexpr uint32_t ACCENT_HI = 0xff2b1fff;
constexpr uint32_t TRACK = 0x22262dffu;
/// Startampel: dunkel, solange das Licht aus ist; gruen, wenn es losgeht.
constexpr uint32_t LIGHT_OFF = 0x3a1418ffu;
constexpr uint32_t READY = 0x00d26affu;

uint32_t with_alpha(uint32_t rgba, float k)
{
    uint32_t a = uint32_t(std::clamp(float(rgba & 0xff) * k, 0.0f, 255.0f));
    return (rgba & 0xffffff00u) | a;
}

hud::TextStyle style(float px, uint32_t rgba, float tracking = 0)
{
    hud::TextStyle s;
    s.px = px;
    s.rgba = rgba;
    s.tracking = px * tracking;
    return s;
}

float text_width(const hud::TextStyle &st, const std::string &t)
{
    float w = 0, h = 0;
    hud::measure(st, t, &w, &h);
    return w;
}

void draw(hud::Frame &f, uint64_t now, float alpha)
{
    hud::reset_state(f);
    f.opacity = alpha;
    float W = f.width, H = f.height, u = H / 1080.0f, cx = W * 0.5f;
    hud::rect_gradient(f, 0, 0, W, H, 0, BG_TOP, BG_BOTTOM, 0);

    // Die Wortmarke steht klein oben; der Bildschirm gehoert der Sitzung.
    hud::TextStyle word = style(56 * u, TEXT, 0.34f);
    std::string name = "ERC";
    float ww = text_width(word, name);
    float wy = H * 0.12f;
    hud::text(f, cx - ww * 0.5f + word.tracking * 0.5f, wy, word, name);

    hud::TextStyle tag = style(18 * u, ACCENT_HI, 0.62f);
    std::string tagline = "LAUNCHER";
    float tw = text_width(tag, tagline);
    float ty = wy + 64 * u;
    hud::text(f, cx - tw * 0.5f + tag.tracking * 0.5f, ty, tag, tagline);
    float rule = 56 * u, ry = ty + 9 * u;
    hud::rect_rounded(f, cx - tw * 0.5f - 20 * u - rule, ry, rule, 2 * u, u, with_alpha(ACCENT, 0.55f));
    hud::rect_rounded(f, cx + tw * 0.5f + 20 * u, ry, rule, 2 * u, u, with_alpha(ACCENT, 0.55f));

    hud::TextStyle sub = style(46 * u, TEXT, 0.10f);
    float sw = text_width(sub, subtitle);
    float sy = H * 0.34f;
    hud::text(f, cx - sw * 0.5f + sub.tracking * 0.5f, sy, sub, subtitle);

    // Die Startampel: fuenf Lichter, eins je angefangene 20 Prozent.
    constexpr int lights = 5;
    bool ready = progress.completed();
    int pct = progress.percent(now);
    int lit = ready ? lights : std::clamp(pct / 20, 0, lights);
    float lr = 17 * u, gap = 66 * u;
    float lx = cx - gap * (lights - 1) * 0.5f, ly = H * 0.49f;
    for (int i = 0; i < lights; ++i) {
        bool on = i < lit;
        uint32_t c = ready ? READY : on ? ACCENT : LIGHT_OFF;
        float x = lx + gap * i;
        if (on)
            hud::rect_rounded(f, x - lr * 1.9f, ly - lr * 1.9f, lr * 3.8f, lr * 3.8f, lr * 1.9f, with_alpha(c, 0.16f));
        hud::rect_rounded(f, x - lr, ly - lr, lr * 2, lr * 2, lr, c);
    }

    float bw = std::min(W * 0.34f, 720 * u * 1.6f), bh = 6 * u, bx = cx - bw * 0.5f, by = H * 0.66f;
    float shown = progress.shown(now);

    hud::TextStyle status = style(22 * u, MUTED);
    hud::TextStyle pctst = style(22 * u, TEXT, 0.04f);
    std::string pt = std::to_string(pct) + "%";
    float ly2 = by - 20 * u - 30 * u;
    hud::text(f, bx, ly2, status, progress.label());
    float pw = text_width(pctst, pt);
    hud::text(f, bx + bw - pw, ly2, pctst, pt);

    hud::rect_rounded(f, bx, by, bw, bh, bh * 0.5f, TRACK);
    float fw = std::max(bh, bw * shown);
    hud::rect_gradient(f, bx, by, fw, bh, bh * 0.5f, ACCENT, ACCENT_HI, SWOFTY_GRADIENT_HORIZONTAL);

    hud::TextStyle foot = style(18 * u, FAINT, 0.10f);
    std::string note = ready ? "Bereit" : "Einen Moment ...";
    float nw = text_width(foot, note);
    hud::text(f, cx - nw * 0.5f + foot.tracking * 0.5f, by + 34 * u, foot, note);
}

}

void step(const char *what, float fraction, float ceiling)
{
    std::lock_guard<std::mutex> g(lock);
    progress.step(what, fraction, GetTickCount64(), ceiling);
}

Snapshot snapshot()
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

void set_subtitle(const char *text)
{
    if (!text || !*text)
        return;
    std::lock_guard<std::mutex> g(lock);
    subtitle = text;
}

bool setup()
{
    char env[1024] = {};
    DWORD n = GetEnvironmentVariableA("SWOFTY_DIRECT_START", env, sizeof env);
    std::string value = n && n < sizeof env ? std::string(env) : std::string(), text;
    if (value.empty())
        text = read_file(swofty_root() + L"direct_start.json", size_t(1) << 16);
    else if (value[0] == '@')
        text = read_file(wide(value.c_str() + 1), size_t(1) << 16);
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

bool active()
{
    return armed.load();
}

void paint(hud::Frame &f, uint64_t now)
{
    if (!armed.load())
        return;
    std::lock_guard<std::mutex> g(lock);
    if (now - polled_at >= 100) {
        polled_at = now;
        if (kind == Kind::session) {
            int sig = direct_start_cover_signal();
            last = sig < 0 ? Signal::failed : sig > 0 ? Signal::shown : Signal::waiting;
        } else {
            auto sig = netplay_room::cover_signal();
            last = sig == netplay_room::CoverSignal::failed  ? Signal::failed
                   : sig == netplay_room::CoverSignal::shown ? Signal::shown
                                                             : Signal::waiting;
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
        progress.complete("Bereit", now);
    draw(f, now, alpha);
}

}
