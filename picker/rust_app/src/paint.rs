//! 绘制层：主窗口整体界面 + 自绘按钮四态。
//!
//! 所有界面都画在双缓冲 `Canvas` 上（见 `theme.rs`），避免闪烁。
//! 配色、圆角、字号与网页版 CSS 对齐。

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::theme::{self, col, fonts, rgb, sc};
use crate::ui::{self, BtnData, BtnKind, LeftGeo};
use crate::{App, RightGeo, View};

// ============================================================ 基础图形

/// 只描边、不填充的圆角矩形。
fn stroke_rr(dc: HDC, rc: &RECT, radius: i32, color: COLORREF) {
    let p = theme::pen(color);
    let old_p = unsafe { SelectObject(dc, p.0) };
    let nb = unsafe { GetStockObject(NULL_BRUSH) };
    let old_b = unsafe { SelectObject(dc, nb) };
    unsafe {
        let el = if radius <= 0 { 1 } else { radius * 2 };
        let _ = RoundRect(dc, rc.left, rc.top, rc.right, rc.bottom, el, el);
        let _ = SelectObject(dc, old_b);
        let _ = SelectObject(dc, old_p);
    }
}

/// 面板卡片：竖向渐变填充 + 1px 边框。
fn card(dc: HDC, rc: &RECT, radius: i32) {
    {
        let _clip = theme::clip_rr(dc, rc, radius);
        theme::vgrad(dc, rc, col::PANEL, col::PANEL2);
    }
    stroke_rr(dc, rc, radius, col::LINE);
}

fn centered(dc: HDC, font: HFONT, text: &str, rc: &RECT, color: COLORREF) {
    theme::draw_text(
        dc,
        font,
        text,
        rc,
        color,
        DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_NOPREFIX,
    );
}

fn lerp_col(a: COLORREF, b: COLORREF, t: f32) -> COLORREF {
    let f = |sh: u32| {
        (((a.0 >> sh) & 0xff) as f32) * (1.0 - t) + (((b.0 >> sh) & 0xff) as f32) * t
    };
    rgb(f(0) as u8, f(8) as u8, f(16) as u8)
}

/// 子按钮所在卡片在该高度处的背景色。
///
/// 主窗口带 WS_CLIPCHILDREN，父窗口不会在子窗口区域绘制，因此每个按钮
/// 必须先把自己的整块背景补上（否则圆角外会露出未绘制的区域）。
fn card_bg_for(app: &App, parent: HWND, btn: HWND) -> COLORREF {
    let Some(geo) = app.geo.as_ref() else {
        return col::PANEL;
    };
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(btn, &mut r);
    }
    let mut pts = [
        POINT {
            x: r.left,
            y: r.top,
        },
        POINT {
            x: r.right,
            y: r.bottom,
        },
    ];
    unsafe {
        let _ = MapWindowPoints(HWND::default(), parent, &mut pts);
    }
    let card = if pts[0].x >= geo.right.card.left {
        &geo.right.card
    } else {
        &geo.left.card
    };
    let h = (card.bottom - card.top).max(1);
    let cy = (pts[0].y + pts[1].y) / 2;
    let t = (((cy - card.top) as f32) / (h as f32)).clamp(0.0, 1.0);
    lerp_col(col::PANEL, col::PANEL2, t)
}

// ============================================================ 按钮

pub fn paint_button(hwnd: HWND, bd: &BtnData, app: &App, hdc: HDC) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    if w <= 0 || h <= 0 {
        return;
    }
    let dpi = app.dpi;
    // 补上本按钮所在位置的卡片背景（父窗口因 WS_CLIPCHILDREN 不会画到这里）
    theme::fill_rect(hdc, &rc, card_bg_for(app, bd.parent, hwnd));
    match bd.kind {
        BtnKind::Primary => {
            let (c1, c2) = if bd.pressed {
                (rgb(0x3d, 0x71, 0xd8), rgb(0x55, 0x86, 0xea))
            } else if bd.hover {
                (rgb(0x63, 0x9c, 0xff), rgb(0x84, 0xb2, 0xff))
            } else {
                (col::ACCENT, rgb(0x6a, 0xa1, 0xff))
            };
            {
                let _clip = theme::clip_rr(hdc, &rc, sc(10, dpi));
                theme::vgrad(hdc, &rc, c1, c2);
            }
            let label = if bd.text.is_empty() {
                "随机抽取"
            } else {
                bd.text.as_str()
            };
            centered(hdc, fonts(dpi).bold, label, &rc, col::BTN_TEXT);
            if bd.focus {
                stroke_rr(hdc, &rc, sc(10, dpi), col::ACCENT2);
            }
        }
        BtnKind::Seg => {
            // 两个分段开关：输入方式（区间/指定列表）、是否允许重复
            let (left_on, left_text, right_text) = if bd.id == ui::ID_BTN_MODE {
                (!app.list_mode, "区间", "指定列表")
            } else {
                (app.unique, "不重复（无放回）", "允许重复（有放回）")
            };
            let rad = sc(9, dpi);
            theme::fill_rr(
                hdc,
                rc.left,
                rc.top,
                rc.right,
                rc.bottom,
                rad,
                col::INPUT_BG,
                Some(col::LINE),
            );
            let half = rc.left + w / 2;
            let inset = sc(3, dpi);
            // 选中态底色
            let active = if left_on {
                RECT {
                    left: rc.left + inset,
                    top: rc.top + inset,
                    right: half + inset,
                    bottom: rc.bottom - inset,
                }
            } else {
                RECT {
                    left: half - inset,
                    top: rc.top + inset,
                    right: rc.right - inset,
                    bottom: rc.bottom - inset,
                }
            };
            theme::fill_rr(
                hdc,
                active.left,
                active.top,
                active.right,
                active.bottom,
                rad - 1,
                col::SEG_ON,
                None,
            );
            // 分隔线
            theme::fill_rect(
                hdc,
                &RECT {
                    left: half,
                    top: inset,
                    right: half + 1,
                    bottom: rc.bottom - inset,
                },
                col::LINE,
            );
            let f = fonts(dpi).small;
            let lr = RECT {
                left: rc.left,
                top: rc.top,
                right: half,
                bottom: rc.bottom,
            };
            let rr = RECT {
                left: half,
                top: rc.top,
                right: rc.right,
                bottom: rc.bottom,
            };
            let (lc, rcx) = if left_on {
                (col::TEXT, col::MUTED)
            } else {
                (col::MUTED, col::TEXT)
            };
            centered(hdc, f, left_text, &lr, lc);
            centered(hdc, f, right_text, &rr, rcx);
        }
        BtnKind::Quick => {
            let rad = sc(8, dpi);
            let bg = if bd.pressed {
                rgb(0x27, 0x35, 0x60)
            } else {
                col::QUICK_BG
            };
            let line = if bd.hover || bd.focus {
                col::ACCENT
            } else {
                col::LINE
            };
            theme::fill_rr(hdc, rc.left, rc.top, rc.right, rc.bottom, rad, bg, Some(line));
            let txt = if bd.hover || bd.pressed {
                rgb(0xff, 0xff, 0xff)
            } else {
                col::TEXT
            };
            centered(hdc, fonts(dpi).small, &bd.text, &rc, txt);
        }
        BtnKind::Op => {
            let rad = sc(8, dpi);
            let line = if bd.hover || bd.focus {
                col::ACCENT
            } else {
                col::LINE
            };
            theme::fill_rr(
                hdc,
                rc.left,
                rc.top,
                rc.right,
                rc.bottom,
                rad,
                col::PANEL,
                Some(line),
            );
            let label = if bd.id == ui::ID_BTN_COPY && app.copy_flash {
                "已复制 ✓"
            } else {
                bd.text.as_str()
            };
            centered(hdc, fonts(dpi).base, label, &rc, col::TEXT);
        }
    }
}

// ============================================================ 主界面

pub fn paint_main(app: &App, dc: HDC) {
    let Some(geo) = app.geo.as_ref() else {
        return;
    };
    let dpi = app.dpi;
    let bg = RECT {
        left: 0,
        top: 0,
        right: geo.client.right,
        bottom: geo.client.bottom,
    };
    theme::vgrad(dc, &bg, rgb(0x16, 0x1f, 0x3e), col::BG);

    paint_header(app, dc, geo.client.right);
    card(dc, &geo.left.card, sc(ui::CARD_R, dpi));
    paint_left_labels(app, dc, &geo.left);
    paint_right_card(app, dc, &geo.right);
    if app.show_history {
        paint_history(app, dc, &geo.right);
    }
}

fn chip_width(dc: HDC, dpi: u32, label: &str, value: &str) -> i32 {
    let f = fonts(dpi).small;
    let pad = sc(11, dpi);
    pad * 2 + theme::text_width(dc, f, label) + theme::text_width(dc, f, value) + sc(4, dpi)
}

fn draw_chip(dc: HDC, dpi: u32, x: i32, y: i32, h: i32, label: &str, value: &str) {
    let f = fonts(dpi).small;
    let pad = sc(11, dpi);
    let w = chip_width(dc, dpi, label, value);
    let rc = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    theme::fill_rr(
        dc,
        rc.left,
        rc.top,
        rc.right,
        rc.bottom,
        h / 2,
        col::PANEL,
        Some(col::LINE),
    );
    let lw = theme::text_width(dc, f, label);
    let lt = RECT {
        left: rc.left + pad,
        top: rc.top,
        right: rc.left + pad + lw,
        bottom: rc.bottom,
    };
    theme::draw_text(
        dc,
        f,
        label,
        &lt,
        col::MUTED,
        DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
    );
    let vt = RECT {
        left: lt.right,
        top: rc.top,
        right: rc.right - pad,
        bottom: rc.bottom,
    };
    theme::draw_text(
        dc,
        f,
        value,
        &vt,
        col::ACCENT2,
        DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
    );
}

fn paint_header(app: &App, dc: HDC, cw: i32) {
    let dpi = app.dpi;
    let pad = sc(ui::MARGIN_X, dpi);
    let band = RECT {
        left: 0,
        top: 0,
        right: cw,
        bottom: sc(ui::HDR_H, dpi),
    };
    theme::vgrad(dc, &band, rgb(0x14, 0x1d, 0x3c), rgb(0x0e, 0x15, 0x2e));
    theme::fill_rect(
        dc,
        &RECT {
            left: 0,
            top: band.bottom - 1,
            right: cw,
            bottom: band.bottom,
        },
        col::LINE,
    );

    let title = RECT {
        left: pad,
        top: sc(18, dpi),
        right: cw - pad,
        bottom: sc(18, dpi) + sc(28, dpi),
    };
    theme::draw_text(
        dc,
        fonts(dpi).title,
        "随机选数器 · 原生桌面版",
        &title,
        col::TEXT,
        DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
    );
    let sub = RECT {
        left: pad,
        top: sc(46, dpi),
        right: cw - pad,
        bottom: sc(46, dpi) + sc(18, dpi),
    };
    theme::draw_text(
        dc,
        fonts(dpi).small,
        "闭区间 [min, max] 内随机抽取整数 · 熵源 BCryptGenRandom（系统 CSPRNG）",
        &sub,
        col::MUTED,
        DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
    );

    let entries: [(&str, String); 3] = [
        ("引擎", "BCryptGenRandom".to_string()),
        ("服务", "正常".to_string()),
        (
            "耗时",
            if app.elapsed_ms > 0.0 {
                format!("{:.3} ms", app.elapsed_ms)
            } else {
                "-".to_string()
            },
        ),
    ];
    let ch = sc(26, dpi);
    let gap = sc(8, dpi);
    let y = sc(22, dpi);
    let widths: Vec<i32> = entries
        .iter()
        .map(|(l, v)| chip_width(dc, dpi, l, v))
        .collect();
    let total: i32 = widths.iter().sum::<i32>() + gap * (entries.len() as i32 - 1);
    let mut x = cw - pad - total;
    for (i, (l, v)) in entries.iter().enumerate() {
        draw_chip(dc, dpi, x, y, ch, l, v);
        x += widths[i] + gap;
    }
}

fn paint_left_labels(app: &App, dc: HDC, g: &LeftGeo) {
    let dpi = app.dpi;
    let f = fonts(dpi);
    theme::draw_text(
        dc,
        f.bold,
        "抽取配置",
        &g.h2,
        col::MUTED,
        DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
    );
    let lab = |rc: &RECT, s: &str, c: COLORREF| {
        theme::draw_text(
            dc,
            f.small,
            s,
            rc,
            c,
            DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
        )
    };
    lab(&g.lab_mode, "输入方式", col::MUTED);
    if app.list_mode {
        lab(&g.lab_min, "候选数字列表", col::MUTED);
        lab(
            &g.hint,
            "分隔符：逗号 / 空格 / 分号；重复值只算一次。",
            col::EMPTY,
        );
        lab(
            &g.lab_exclude,
            "排除数字（可选；按值去掉列表中的该数字）",
            col::MUTED,
        );
    } else {
        lab(&g.lab_min, "区间下界 / 上界", col::MUTED);
        lab(&g.hint, "闭区间，两端都会包含；只接受整数。", col::EMPTY);
        lab(
            &g.lab_exclude,
            "排除数字（可选，逗号分隔，超出区间自动忽略）",
            col::MUTED,
        );
    }
    lab(&g.lab_count, "抽取个数（1 ~ 100）", col::MUTED);
    lab(&g.lab_uniq, "是否允许重复", col::MUTED);
    lab(&g.lab_quick, "常用场景", col::MUTED);
}

fn paint_right_card(app: &App, dc: HDC, g: &RightGeo) {
    let dpi = app.dpi;
    card(dc, &g.card, sc(ui::CARD_R, dpi));
    theme::draw_text(
        dc,
        fonts(dpi).bold,
        "抽取结果",
        &g.h2,
        col::MUTED,
        DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
    );

    let nbg = rgb(0x0b, 0x12, 0x26);
    {
        let _clip = theme::clip_rr(dc, &g.nums, sc(10, dpi));
        theme::fill_rect(dc, &g.nums, nbg);
    }
    stroke_rr(dc, &g.nums, sc(10, dpi), col::LINE);
    paint_numbers(app, dc, &g.nums);

    if let Some(r) = &app.result {
        theme::draw_text(
            dc,
            fonts(dpi).small,
            &r.meta,
            &g.meta,
            col::MUTED,
            DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }

    if let Some(e) = &app.error {
        theme::fill_rr(
            dc,
            g.err.left,
            g.err.top,
            g.err.right,
            g.err.bottom,
            sc(8, dpi),
            col::ERR_BG,
            Some(col::DANGER),
        );
        let text = format!("出错了：{e}");
        let mut tr = g.err;
        tr.left += sc(10, dpi);
        tr.right -= sc(10, dpi);
        tr.top += sc(8, dpi);
        tr.bottom -= sc(8, dpi);
        theme::draw_text(
            dc,
            fonts(dpi).small,
            &text,
            &tr,
            col::ERR_TXT,
            DT_WORDBREAK | DT_LEFT | DT_NOPREFIX,
        );
    }
}

fn big_chip(
    dc: HDC,
    dpi: u32,
    x: i32,
    y: i32,
    text: &str,
    bg: COLORREF,
    line: COLORREF,
    txt: COLORREF,
) {
    let f = fonts(dpi).huge;
    let padx = sc(22, dpi);
    let w = theme::text_width(dc, f, text) + padx * 2;
    let h = sc(62, dpi);
    theme::fill_rr(dc, x, y, x + w, y + h, sc(14, dpi), bg, Some(line));
    let tr = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    centered(dc, f, text, &tr, txt);
}

fn paint_numbers(app: &App, dc: HDC, area: &RECT) {
    let dpi = app.dpi;
    let Some(r) = app.result.as_ref() else {
        let msg = if app.error.is_some() {
            "（无结果）"
        } else {
            "点击「随机抽取」或常用场景开始"
        };
        centered(dc, fonts(dpi).base, msg, area, col::EMPTY);
        return;
    };
    let pad = sc(16, dpi);
    let inner = RECT {
        left: area.left + pad,
        top: area.top + pad,
        right: area.right - pad,
        bottom: area.bottom - pad,
    };
    let _clip = theme::clip_rect(dc, &inner);
    let ch = sc(34, dpi);
    let gap = sc(8, dpi);
    let padx = sc(12, dpi);
    let f = fonts(dpi).mono;

    match &r.view {
        View::Normal => {
            if r.nums.len() == 1 {
                big_chip(
                    dc,
                    dpi,
                    inner.left,
                    inner.top,
                    &r.nums[0].to_string(),
                    col::CHIP_BG,
                    col::CHIP_LINE,
                    col::ACCENT2,
                );
                return;
            }
            let mut x = inner.left;
            let mut y = inner.top;
            for n in &r.nums {
                let t = n.to_string();
                let w = theme::text_width(dc, f, &t) + padx * 2;
                if x > inner.left && x + w > inner.right {
                    x = inner.left;
                    y += ch + gap;
                }
                if y + ch > inner.bottom {
                    break;
                }
                theme::fill_rr(
                    dc,
                    x,
                    y,
                    x + w,
                    y + ch,
                    sc(9, dpi),
                    col::CHIP_BG,
                    Some(col::CHIP_LINE),
                );
                let tr = RECT {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + ch,
                };
                centered(dc, f, &t, &tr, col::ACCENT2);
                x += w + gap;
            }
        }
        View::Ball { reds, blue } => {
            let mut x = inner.left;
            let y = inner.top;
            for n in reds {
                let t = format!("{n:02}");
                let w = theme::text_width(dc, f, &t) + padx * 2;
                if x + w > inner.right {
                    break;
                }
                theme::fill_rr(
                    dc,
                    x,
                    y,
                    x + w,
                    y + ch,
                    sc(9, dpi),
                    col::RED_BG,
                    Some(col::RED_LINE),
                );
                let tr = RECT {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + ch,
                };
                centered(dc, f, &t, &tr, col::RED_TXT);
                x += w + gap;
            }
            let pw = sc(18, dpi);
            let plus = RECT {
                left: x,
                top: y,
                right: x + pw,
                bottom: y + ch,
            };
            centered(dc, fonts(dpi).bold, "+", &plus, col::MUTED);
            x += pw + gap;
            big_chip(
                dc,
                dpi,
                x,
                y,
                &format!("{blue:02}"),
                col::BLUE_BG,
                col::BLUE_LINE,
                col::BLUE_TXT,
            );
        }
    }
}

fn paint_history(app: &App, dc: HDC, g: &RightGeo) {
    let dpi = app.dpi;
    let rc = g.card;
    theme::fill_rr(
        dc,
        rc.left,
        rc.top,
        rc.right,
        rc.bottom,
        sc(ui::CARD_R, dpi),
        rgb(0x10, 0x16, 0x2c),
        Some(col::ACCENT),
    );
    let pad = sc(ui::PAD, dpi);
    let title = RECT {
        left: rc.left + pad,
        top: rc.top + pad,
        right: rc.right - pad,
        bottom: rc.top + pad + sc(20, dpi),
    };
    theme::draw_text(
        dc,
        fonts(dpi).bold,
        "历史记录（本次运行）",
        &title,
        col::MUTED,
        DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
    );
    // 提示行放在操作按钮上方，避免被子按钮遮挡
    let hint = RECT {
        left: rc.left + pad,
        top: g.copy.top - sc(8, dpi) - sc(18, dpi),
        right: rc.right - pad,
        bottom: g.copy.top - sc(8, dpi),
    };
    let hint_top = hint.top;
    theme::draw_text(
        dc,
        fonts(dpi).small,
        "点击任意位置，或再次点「查看历史记录」关闭",
        &hint,
        col::EMPTY,
        DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
    );

    let f = fonts(dpi).small;
    let item_h = sc(22, dpi);
    let inner_r = rc.right - pad;
    let idx_w = sc(24, dpi);
    let mut y = title.bottom + sc(8, dpi);
    if app.history.is_empty() {
        let er = RECT {
            left: rc.left + pad,
            top: y,
            right: inner_r,
            bottom: y + item_h,
        };
        theme::draw_text(
            dc,
            f,
            "暂无记录",
            &er,
            col::EMPTY,
            DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
        );
        return;
    }
    for (i, h) in app.history.iter().enumerate() {
        if y + item_h > hint_top - sc(6, dpi) {
            break;
        }
        let ir = RECT {
            left: rc.left + pad,
            top: y,
            right: rc.left + pad + idx_w,
            bottom: y + item_h,
        };
        theme::draw_text(
            dc,
            f,
            &format!("{}.", i + 1),
            &ir,
            col::ACCENT,
            DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_NOPREFIX,
        );
        let tr = RECT {
            left: ir.right,
            top: y,
            right: inner_r,
            bottom: y + item_h,
        };
        theme::draw_text(
            dc,
            f,
            &h.text,
            &tr,
            col::TEXT,
            DT_SINGLELINE | DT_LEFT | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        theme::fill_rect(
            dc,
            &RECT {
                left: tr.left,
                top: y + item_h - 1,
                right: inner_r,
                bottom: y + item_h,
            },
            col::LINE,
        );
        y += item_h;
    }
}
