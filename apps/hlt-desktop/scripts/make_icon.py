"""앱 아이콘(1024px 원본)을 그린다: 남색 둥근 사각 위 명세서 한 장(【 】 표제 · [0001] 문단 · 본문 줄) + 펜촉.
쓰는 법: python3 scripts/make_icon.py src-tauri/icons/source.png && npx tauri icon src-tauri/icons/source.png
글꼴은 맥 기본 AppleMyungjo(시스템 글꼴 — 아이콘 그림에만 쓰고 앱에 싣지 않는다)."""

import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont

S = 4  # 4배로 그려 줄인다(가장자리 매끈하게)
W = 1024 * S


def px(value):
    return int(value * S)


def main(out):
    canvas = Image.new("RGBA", (W, W), (0, 0, 0, 0))

    # 맥 아이콘 격자: 1024 안에 824 둥근 사각(반경 ~185), 아래로 그림자.
    shadow = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        [px(100), px(112), px(924), px(936)], radius=px(185), fill=(0, 0, 0, 110)
    )
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(px(14))))

    # 바탕: 위 밝은 남색 → 아래 짙은 남색.
    top, bottom = (43, 84, 140), (18, 40, 77)
    gradient = Image.new("RGBA", (W, W))
    draw = ImageDraw.Draw(gradient)
    for y in range(W):
        t = y / W
        draw.line(
            [(0, y), (W, y)],
            fill=tuple(int(a + (b - a) * t) for a, b in zip(top, bottom)) + (255,),
        )
    mask = Image.new("L", (W, W), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [px(100), px(100), px(924), px(924)], radius=px(185), fill=255
    )
    canvas.paste(gradient, (0, 0), mask)

    # 종이: 그림자 · 흰 장 · 오른쪽 위 접힌 귀.
    paper = [px(250), px(190), px(730), px(830)]
    fold = px(110)
    sheet_shadow = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    ImageDraw.Draw(sheet_shadow).rounded_rectangle(
        [paper[0] + px(10), paper[1] + px(16), paper[2] + px(10), paper[3] + px(16)],
        radius=px(18),
        fill=(0, 0, 0, 90),
    )
    canvas.alpha_composite(sheet_shadow.filter(ImageFilter.GaussianBlur(px(12))))
    d = ImageDraw.Draw(canvas)
    d.rounded_rectangle(paper, radius=px(18), fill=(252, 251, 247, 255))
    # 접힌 귀: 귀 자리를 바탕색으로 덮고 삼각형을 그린다.
    corner = [(paper[2] - fold, paper[1]), (paper[2], paper[1]), (paper[2], paper[1] + fold)]
    d.polygon(corner, fill=(0, 0, 0, 0))
    canvas_px = canvas.load()
    region = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    ImageDraw.Draw(region).polygon(corner, fill=(255, 255, 255, 255))
    canvas.paste(gradient, (0, 0), Image.composite(mask, Image.new("L", (W, W), 0), region.split()[3]))
    d = ImageDraw.Draw(canvas)
    d.polygon(
        [(paper[2] - fold, paper[1]), (paper[2] - fold, paper[1] + fold), (paper[2], paper[1] + fold)],
        fill=(222, 218, 207, 255),
    )
    del canvas_px

    # 표제 【 】 — 앱 강조색(적갈색).
    accent = (166, 61, 45, 255)
    font = ImageFont.truetype("/System/Library/Fonts/Supplemental/AppleMyungjo.ttf", px(118))
    d.text((px(282), px(238)), "【", font=font, fill=accent)
    d.text((px(548), px(238)), "】", font=font, fill=accent)
    d.rounded_rectangle([px(392), px(292), px(548), px(316)], radius=px(10), fill=(120, 120, 120, 255))

    # [0001] 번호 칸 + 본문 줄(양쪽 맞춤처럼 같은 길이 · 문단 끝 줄만 짧게).
    gray = (150, 150, 150, 255)
    line = (88, 88, 88, 255)
    rows = [
        (410, True, 680),
        (466, False, 680),
        (522, False, 560),
        (600, True, 680),
        (656, False, 680),
        (712, False, 470),
    ]
    for y, numbered, end in rows:
        if numbered:
            d.rounded_rectangle([px(282), px(y), px(330), px(y + 22)], radius=px(6), fill=gray)
            start = 352
        else:
            start = 352
        d.rounded_rectangle([px(start), px(y), px(end), px(y + 22)], radius=px(11), fill=line)

    # 펜촉: 오른쪽 아래에서 종이 위로 비스듬히.
    pen = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    p = ImageDraw.Draw(pen)
    body = (226, 178, 74, 255)
    dark = (150, 105, 30, 255)
    # 축(세로로 그리고 돌린다).
    cx = W // 2
    p.rounded_rectangle([cx - px(46), px(400), cx + px(46), px(600)], radius=px(28), fill=(30, 30, 34, 255))
    p.rectangle([cx - px(46), px(570), cx + px(46), px(615)], fill=(200, 200, 205, 255))
    p.polygon([(cx - px(46), px(615)), (cx + px(46), px(615)), (cx, px(760))], fill=body)
    p.line([(cx, px(640)), (cx, px(740))], fill=dark, width=px(7))
    p.ellipse([cx - px(10), px(650), cx + px(10), px(670)], fill=dark)
    pen = pen.rotate(-38, resample=Image.BICUBIC, center=(cx, px(760)))
    shifted = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    shifted.paste(pen, (px(178), px(-30)))
    shade = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    shade.paste((0, 0, 0, 90), (0, 0), shifted.split()[3])
    canvas.alpha_composite(shade.filter(ImageFilter.GaussianBlur(px(10))), (px(8), px(14)))
    canvas.alpha_composite(shifted)

    canvas.resize((1024, 1024), Image.LANCZOS).save(out)


if __name__ == "__main__":
    main(sys.argv[1])
