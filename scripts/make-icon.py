"""Generate the source icon for `tauri icon`.

Design: accent-blue rounded square, a white 9:16 "screen" in the middle.
Portrait on purpose — that is what this app is for.
"""

from PIL import Image, ImageDraw

SIZE = 1024
BG = (79, 140, 255, 255)  # --accent #4f8cff
FG = (255, 255, 255, 255)
SHADOW = (0, 0, 0, 40)


def rounded(size, radius, fill):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=fill)
    return img


def main() -> None:
    canvas = rounded(SIZE, 224, BG)

    # 9:16 screen, optically centered (slightly above true center).
    w, h = 300, 534
    x = (SIZE - w) // 2
    y = (SIZE - h) // 2 - 18

    # Soft drop shadow so the glyph reads on light backgrounds too.
    shadow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        [x + 10, y + 14, x + w + 10, y + h + 14], radius=56, fill=SHADOW
    )
    shadow = shadow.filter(__import__("PIL.ImageFilter", fromlist=["GaussianBlur"]).GaussianBlur(18))
    canvas = Image.alpha_composite(canvas, shadow)

    draw = ImageDraw.Draw(canvas)
    draw.rounded_rectangle([x, y, x + w, y + h], radius=56, fill=FG)

    # Home indicator: one small bar, the only detail.
    bar_w, bar_h = 96, 12
    bx = x + (w - bar_w) // 2
    by = y + h - 44
    draw.rounded_rectangle([bx, by, bx + bar_w, by + bar_h], radius=6, fill=BG)

    canvas.save("assets/app-icon.png")
    print("wrote assets/app-icon.png", canvas.size)


if __name__ == "__main__":
    main()
