//! 主题：调色板（对齐网页版 CSS）、GDI RAII、字体与双缓冲画布。

#![allow(dead_code)]

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;

#[inline]
pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

/// 把 96 DPI 基准尺寸缩放到当前 DPI。
#[inline]
pub fn sc(px: i32, dpi: u32) -> i32 {
    (px * dpi as i32) / 96
}

// ---------- 调色板（与网页版 CSS 一致） ----------
pub mod col {
    use super::*;
    pub const BG: COLORREF = rgb(0x0b, 0x10, 0x20);
    pub const PANEL: COLORREF = rgb(0x14, 0x1b, 0x32);
    pub const PANEL2: COLORREF = rgb(0x1b, 0x24, 0x40);
    pub const LINE: COLORREF = rgb(0x2a, 0x35, 0x57);
    pub const TEXT: COLORREF = rgb(0xe6, 0xec, 0xff);
    pub const MUTED: COLORREF = rgb(0x8b, 0x98, 0xc4);
    pub const ACCENT: COLORREF = rgb(0x4f, 0x8c, 0xff);
    pub const ACCENT2: COLORREF = rgb(0x37, 0xd6, 0xa3);
    pub const DANGER: COLORREF = rgb(0xff, 0x6b, 0x81);
    pub const INPUT_BG: COLORREF = rgb(0x0e, 0x15, 0x30);
    pub const CHIP_BG: COLORREF = rgb(0x22, 0x30, 0x5c);
    pub const CHIP_LINE: COLORREF = rgb(0x3a, 0x4c, 0x8f);
    pub const RED_BG: COLORREF = rgb(0x5a, 0x1f, 0x2a);
    pub const RED_LINE: COLORREF = rgb(0xa0, 0x4a, 0x56);
    pub const RED_TXT: COLORREF = rgb(0xff, 0xb4, 0xbd);
    pub const BLUE_BG: COLORREF = rgb(0x1c, 0x2f, 0x5e);
    pub const BLUE_LINE: COLORREF = rgb(0x4a, 0x6f, 0xd0);
    pub const BLUE_TXT: COLORREF = rgb(0xa9, 0xc0, 0xff);
    pub const SEG_ON: COLORREF = rgb(0x1e, 0x2a, 0x55);
    pub const QUICK_BG: COLORREF = rgb(0x1e, 0x2a, 0x4d);
    pub const EMPTY: COLORREF = rgb(0x5a, 0x67, 0x96);
    pub const ERR_BG: COLORREF = rgb(0x2a, 0x15, 0x20);
    pub const ERR_TXT: COLORREF = rgb(0xff, 0xb3, 0xbd);
    pub const BTN_TEXT: COLORREF = rgb(0xff, 0xff, 0xff);
}

// ---------- 字体 ----------
pub struct FontSet {
    pub title: HFONT,  // 19px 半粗（窗口标题）
    pub base: HFONT,   // 14px 常规
    pub small: HFONT,  // 12px
    pub label: HFONT,  // 12px（卡片小节标签/标题条小字）
    pub bold: HFONT,   // 13px 加粗（卡片小节标题）
    pub mono: HFONT,   // 16px 数字徽章常规
    pub big: HFONT,    // 30px 单结果放大
    pub huge: HFONT,   // 40px
    pub chip: HFONT,   // 17px 徽章数字
    pub redb: HFONT,   // 20px 双色球大号
}

// GDI 句柄是裸指针，默认不具备 Send/Sync；字体集只在 UI 线程创建与使用，
// 这里显式声明以便放进 static 缓存。
unsafe impl Send for FontSet {}
unsafe impl Sync for FontSet {}

fn make_font(face: &[u16], px: i32, weight: i32, dpi: u32) -> HFONT {
    // cheight 为负表示按像素指定字符高度
    let h = -sc(px, dpi);
    unsafe {
        CreateFontW(
            h,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            1, // DEFAULT_CHARSET
            0, // OUT_DEFAULT_PRECIS
            0, // CLIP_DEFAULT_PRECIS
            0, // DEFAULT_QUALITY
            0, // FF_DONTCARE | DEFAULT_PITCH
            PCWSTR(face.as_ptr()),
        )
    }
}

/// 按 DPI 缓存字体集；同一进程可能跨多个显示器（不同 DPI），
/// 用 `Box::leak` 让每个 DPI 的字体集在进程生命周期内有效（数量极少）。
static FONTS: OnceLock<Mutex<HashMap<u32, &'static FontSet>>> = OnceLock::new();

pub fn fonts(dpi: u32) -> &'static FontSet {
    let map = FONTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap();
    if let Some(f) = guard.get(&dpi) {
        return f;
    }
    let leaked: &'static FontSet = Box::leak(Box::new(build_fontset(dpi)));
    guard.insert(dpi, leaked);
    leaked
}

fn build_fontset(dpi: u32) -> FontSet {
    let face = widen("Microsoft YaHei UI");
    let face_mono = widen("Consolas");
    let mk = |px: i32, w: i32| make_font(&face, px, w, dpi);
    let mk_mono = |px: i32, w: i32| make_font(&face_mono, px, w, dpi);
    FontSet {
        title: mk(19, 600),
        base: mk(14, 400),
        small: mk(12, 400),
        label: mk(12, 400),
        bold: mk(13, 600),
        mono: mk_mono(16, 400),
        big: mk_mono(30, 700),
        huge: mk_mono(40, 700),
        chip: mk_mono(17, 700),
        redb: mk_mono(20, 700),
    }
}

/// 生成 UTF-16（带结尾 \0）文本，用于宽字符 API。
pub fn widen(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------- GDI RAII ----------
pub struct Brush(pub HBRUSH);
impl Drop for Brush {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0);
        }
    }
}
pub fn brush(c: COLORREF) -> Brush {
    Brush(unsafe { CreateSolidBrush(c) })
}

/// 长生命周期画刷缓存：用于 WM_CTLCOLOR* 这类必须把 HBRUSH 交还给系统的场合。
/// 句柄在进程生命周期内保持有效（数量极少，不做回收）。
static BRUSHES: OnceLock<Mutex<HashMap<u32, isize>>> = OnceLock::new();

pub fn cached_brush(c: COLORREF) -> HBRUSH {
    let m = BRUSHES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = m.lock().unwrap();
    if let Some(h) = guard.get(&c.0) {
        return HBRUSH(*h as *mut c_void);
    }
    let b = unsafe { CreateSolidBrush(c) };
    guard.insert(c.0, b.0 as isize);
    b
}

pub struct Pen(pub HPEN);
impl Drop for Pen {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0);
        }
    }
}
pub fn pen(c: COLORREF) -> Pen {
    Pen(unsafe { CreatePen(PS_SOLID, 1, c) })
}

pub struct Selected(pub HGDIOBJ);
impl Drop for Selected {
    fn drop(&mut self) {
        // 恢复旧对象由调用方在统一位置执行（见 Canvas::finish / ctx 封装）
    }
}

// ---------- 双缓冲画布 ----------
/// 一次性内存 DC，绘制完由调用方 `present(hdc_dst)` 送至目标。
pub struct Canvas {
    pub dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    pub w: i32,
    pub h: i32,
}

impl Canvas {
    pub fn new(target: HDC, w: i32, h: i32) -> Option<Canvas> {
        unsafe {
            let mem = CreateCompatibleDC(target);
            if mem.is_invalid() {
                return None;
            }
            let bmp = CreateCompatibleBitmap(target, w, h);
            if bmp.is_invalid() {
                let _ = DeleteDC(mem);
                return None;
            }
            let old = SelectObject(mem, bmp);
            Some(Canvas {
                dc: mem,
                bmp,
                old,
                w,
                h,
            })
        }
    }

    pub fn present(&self, dst: HDC) {
        unsafe {
            let _ = BitBlt(dst, 0, 0, self.w, self.h, self.dc, 0, 0, SRCCOPY);
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp);
            let _ = DeleteDC(self.dc);
        }
    }
}

// ---------- 常用绘制小函数（都在画布 dc 上操作） ----------
pub fn fill_rect(dc: HDC, r: &RECT, c: COLORREF) {
    let b = brush(c);
    unsafe {
        FillRect(dc, r, b.0);
    }
}

pub fn fill_rr(dc: HDC, x0: i32, y0: i32, x1: i32, y1: i32, radius: i32, fill: COLORREF, border: Option<COLORREF>) {
    let b = brush(fill); // 析构时才 DeleteObject，先保持选中
    let old_b = unsafe { SelectObject(dc, b.0) };
    let p = border.map(|c| pen(c));
    let sel = match &p {
        Some(pn) => unsafe { SelectObject(dc, pn.0) },
        None => {
            let np = unsafe { GetStockObject(NULL_PEN) }; // 替代默认黑笔，避免无边框黑边
            unsafe { SelectObject(dc, np) }
        }
    };
    unsafe {
        let el = if radius <= 0 { 1 } else { radius * 2 };
        let _ = RoundRect(dc, x0, y0, x1, y1, el, el);
        // 按相反顺序恢复旧画笔、旧画刷
        let _ = SelectObject(dc, sel);
        let _ = SelectObject(dc, old_b);
    }
    // 此处 b / p 已被恢复出 DC，Drop 删除句柄安全
}

/// 在画布上绘制文本。
/// `format` 为 DT_* 位或值（如 `DT_SINGLELINE | DT_CENTER | DT_VCENTER`）。
#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    dc: HDC,
    font: HFONT,
    text: &str,
    rect: &RECT,
    color: COLORREF,
    format: DRAW_TEXT_FORMAT,
) {
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    if buf.is_empty() {
        return;
    }
    let fsel = unsafe { SelectObject(dc, font) };
    let mut rc = *rect;
    unsafe {
        SetTextColor(dc, color);
        SetBkMode(dc, TRANSPARENT);
        DrawTextW(dc, &mut buf, &mut rc, format);
        SelectObject(dc, fsel);
    }
}

/// 测量单行文本像素宽度（方便右对齐芯片等）。
pub fn text_width(dc: HDC, font: HFONT, text: &str) -> i32 {
    let buf: Vec<u16> = text.encode_utf16().collect();
    if buf.is_empty() {
        return 0;
    }
    let fsel = unsafe { SelectObject(dc, font) };
    let mut sz = SIZE::default();
    unsafe {
        let _ = GetTextExtentPoint32W(dc, &buf, &mut sz);
        SelectObject(dc, fsel);
    }
    sz.cx
}

/// 估算文本在宽度 `max_w` 内自动换行后的总高度（DT_CALCRECT，不实际绘制）。
pub fn wrap_height(dc: HDC, font: HFONT, text: &str, max_w: i32) -> i32 {
    if text.is_empty() {
        return 0;
    }
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: max_w.max(1),
        bottom: 0,
    };
    let fsel = unsafe { SelectObject(dc, font) };
    unsafe {
        let _ = DrawTextW(dc, &mut buf, &mut rc, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
        SelectObject(dc, fsel);
    }
    (rc.bottom - rc.top).max(0)
}

// ---------- 工具 ----------
pub trait HdcChecks {
    fn is_invalid(&self) -> bool;
}

impl HdcChecks for HDC {
    fn is_invalid(&self) -> bool {
        self.0.is_null()
    }
}

/// 构造 RECT（避免大量转换样板）。
#[inline]
pub fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

/// BOOL 成功判断
pub fn ok(b: BOOL) -> bool {
    b.as_bool()
}

/// 把裸指针包装成 HDC（供 WM_CTLCOLOR 等消息里的 wparam 使用）。
#[inline]
pub fn hdc_from_ptr(p: *mut c_void) -> HDC {
    HDC(p)
}

/// 垂直线性渐变填充（c1 → c2，逐行内插）。
pub fn vgrad(dc: HDC, rc: &RECT, c1: COLORREF, c2: COLORREF) {
    let (h, r) = (rc.bottom - rc.top, rc.left);
    if h <= 0 {
        return;
    }
    for y in 0..h {
        let t = y as f32 / h as f32;
        let inv = 1.0 - t;
        let cr = (((c1.0 >> 0) & 0xff) as f32 * inv + ((c2.0 >> 0) & 0xff) as f32 * t) as u8;
        let cg = (((c1.0 >> 8) & 0xff) as f32 * inv + ((c2.0 >> 8) & 0xff) as f32 * t) as u8;
        let cb = (((c1.0 >> 16) & 0xff) as f32 * inv + ((c2.0 >> 16) & 0xff) as f32 * t) as u8;
        let row = RECT {
            left: r,
            top: rc.top + y,
            right: rc.right,
            bottom: rc.top + y + 1,
        };
        fill_rect(dc, &row, rgb(cr, cg, cb));
    }
}

/// 圆角裁剪保护（Drop 时自动恢复原裁剪区）。
pub struct Clip {
    dc: HDC,
    saved: i32,
}
impl Drop for Clip {
    fn drop(&mut self) {
        unsafe {
            let _ = RestoreDC(self.dc, self.saved);
        }
    }
}
/// 将 DC 裁剪为圆角矩形区域后返回保护对象。
pub fn clip_rr(dc: HDC, rc: &RECT, radius: i32) -> Clip {
    let saved = unsafe { SaveDC(dc) };
    let rad = if radius <= 0 { 1 } else { radius * 2 };
    unsafe {
        let hrgn = CreateRoundRectRgn(rc.left, rc.top, rc.right, rc.bottom, rad, rad);
        if !hrgn.is_invalid() {
            let _ = SelectClipRgn(dc, hrgn);
            let _ = DeleteObject(hrgn);
        }
    }
    Clip { dc, saved }
}

/// 平面矩形裁剪保护。
pub fn clip_rect(dc: HDC, rc: &RECT) -> Clip {
    let saved = unsafe { SaveDC(dc) };
    unsafe {
        let _ = IntersectClipRect(dc, rc.left, rc.top, rc.right, rc.bottom);
    }
    Clip { dc, saved }
}
