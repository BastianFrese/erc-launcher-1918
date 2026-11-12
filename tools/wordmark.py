# -*- coding: utf-8 -*-
"""Zeichnet die Wortmarke des Launchers neu: ERC / ERDI'S RACING COMMUNITY.

    python -I tools/wordmark.py

Schreibt `apps/launcher/assets/wordmark.png` (440x238, durchsichtig).
Vorbild ist die Marke im eigenen Fenster (`ui/app.css`, `.marke`):
Saira Semi Condensed 600, weite Sperrung, darunter der Vereinsname.
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

W, H = 440, 238
SKALA = 3
WEISS = (242, 244, 247, 255)
GEDAEMPFT = (169, 173, 179, 255)
SCHRIFT = r"C:\Windows\Fonts\bahnschrift.ttf"


def schrift(groesse, variante):
    f = ImageFont.truetype(SCHRIFT, groesse)
    f.set_variation_by_name(variante)
    return f


def gesperrt(draw, xy, text, font, fuellung, sperrung):
    """Setzt Text mit Sperrung; PIL kennt keine Laufweite."""
    x, y = xy
    for zeichen in text:
        draw.text((x, y), zeichen, font=font, fill=fuellung)
        x += draw.textlength(zeichen, font=font) + sperrung


def breite(draw, text, font, sperrung):
    return sum(draw.textlength(z, font=font) + sperrung for z in text) - sperrung


def main():
    gross = Image.new("RGBA", (W * SKALA, H * SKALA), (0, 0, 0, 0))
    d = ImageDraw.Draw(gross)

    kuerzel = schrift(150 * SKALA, "SemiBold Condensed")
    name = schrift(26 * SKALA, "SemiCondensed")

    # ERC
    b = breite(d, "ERC", kuerzel, 14 * SKALA)
    d_links = (W * SKALA - b) / 2
    gesperrt(d, (d_links, 6 * SKALA), "ERC", kuerzel, WEISS, 14 * SKALA)

    # Der Vereinsname darunter, auf dieselbe Breite gezogen.
    text = "ERDI'S RACING COMMUNITY"
    b = breite(d, text, name, 2 * SKALA)
    gesperrt(d, ((W * SKALA - b) / 2, 186 * SKALA), text, name, GEDAEMPFT, 2 * SKALA)

    ziel = Path(__file__).resolve().parent.parent / "apps" / "launcher" / "assets" / "wordmark.png"
    gross.resize((W, H), Image.LANCZOS).save(ziel)
    print("geschrieben: %s" % ziel)


if __name__ == "__main__":
    main()
