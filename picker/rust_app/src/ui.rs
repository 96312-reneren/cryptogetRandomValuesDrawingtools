//! 子控件层：EDIT 输入框（深色 + 字符过滤 + 自绘边框）、
//! 自绘按钮（主按钮/分段开关/常用场景/操作按钮）、以及整体布局计算。

#![allow(dead_code)]

use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::SetWindowTheme;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::paint;
use crate::theme::{sc, widen};

// ---------- 布局常量（96 DPI 基准，使用处统一 sc() 缩放） ----------
pub const MARGIN_X: i32 = 28;   // 页面左右边距
pub const MARGIN_TOP: i32 = 18; // 卡片与 header 的间距
pub const GAP: i32 = 18;        // 左右两列间隙
pub const LEFT_W: i32 = 400;    // 左卡片宽
pub const CARD_R: i32 = 14;     // 卡片圆角
pub const PAD: i32 = 16;        // 卡片内边距
pub const HDR_H: i32 = 86;      // header 高
pub const EDIT_H: i32 = 38;     // 输入框高
pub const SEG_H: i32 = 40;      // 分段开关高
pub const GO_H: i32 = 46;       // 主按钮高
pub const QUICK_H: i32 = 32;    // 常用场景按钮高
pub const OP_H: i32 = 34;       // 操作按钮高

// ---------- 控件 ID ----------
pub const ID_EDIT_MIN: u32 = 1;
pub const ID_EDIT_MAX: u32 = 2;
pub const ID_EDIT_COUNT: u32 = 3;
pub const ID_EDIT_EXCLUDE: u32 = 4;
pub const ID_EDIT_LIST: u32 = 5;
pub const ID_BTN_GO: u32 = 10;
pub const ID_BTN_SEG: u32 = 11;
pub const ID_BTN_MODE: u32 = 12;
pub const ID_BTN_DICE: u32 = 20;
pub const ID_BTN_CALL: u32 = 21;
pub const ID_BTN_SIX: u32 = 22;
pub const ID_BTN_DLB: u32 = 23;
pub const ID_BTN_COPY: u32 = 30;
pub const ID_BTN_HIST: u32 = 31;

// EDIT 过滤使用：字符按键只放行数字/负号/常用控制字符
const VK_TAB: u32 = 0x09;
const WM_MOUSELEAVE: u32 = 0x02A3;
const EM_SETLIMITTEXT: u32 = 0x00C5;
const EM_GETSEL: u32 = 0x00B0;

const BTN_CLASS_NAME: &str = "PickerBtn";

// 分段开关子窗口自己再分成左右两半：zone 1 = 左（不重复），zone 2 = 右（允许重复）
pub const SEG_ZONE_LEFT: isize = 1;
pub const SEG_ZONE_RIGHT: isize = 2;

// ---------- 输入框角色 ----------
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditRole {
    Min,
    Max,
    Count,
    Exclude,
    /// 候选数字列表：放行数字、负号与各类分隔符（逗号/分号/空格，含中文全角）
    List,
}

// ---------- 按钮种类 ----------
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BtnKind {
    Primary, // 主按钮：随机抽取
    Seg,     // 分段开关（自绘两半）
    Quick,   // 常用场景小按钮
    Op,      // 操作按钮（复制/历史）
}

/// 每个自绘按钮窗口的实例数据（泄露在堆上，窗口存活期间有效）。
#[repr(C)]
pub struct BtnData {
    pub hwnd: HWND,
    pub parent: HWND,
    pub id: u32,
    pub kind: BtnKind,
    pub text: String,
    pub hover: bool,
    pub pressed: bool,
    pub focus: bool,
    // seg 自绘两半需要记住按下/悬停落在左还是右
    pub hover_zone: isize,
    pub press_zone: isize,
}

/// 每个 EDIT 子类窗口的实例数据。
#[repr(C)]
pub struct EdData {
    pub prev: usize, // 原始 EDIT 窗口过程
    pub role: EditRole,
    pub focus: bool,
}

// ---------- 左卡片完整几何（布局与绘制共用同一份） ----------
#[derive(Clone)]
pub struct LeftGeo {
    pub card: RECT,        // 左卡片外框（含边框）
    pub h2: RECT,          // “抽取配置”
    pub lab_mode: RECT,    // “输入方式”
    pub seg_mode: RECT,    // 区间 / 指定列表
    pub lab_min: RECT,
    pub ed_min: RECT,
    pub ed_max: RECT,
    pub ed_list: RECT,
    pub hint: RECT,
    pub lab_count: RECT,
    pub ed_count: RECT,
    pub lab_exclude: RECT,
    pub ed_exclude: RECT,
    pub lab_uniq: RECT,
    pub seg: RECT,
    pub go: RECT,
    pub lab_quick: RECT,
    pub quick: [RECT; 4],
    pub card_bottom: i32, // 卡片外框下缘（用于确定右卡片与最小窗口高度）
}

/// 鼠标坐标归一化（Windows 打包在 LPARAM 里）
pub fn lparam_pt(lp: LPARAM) -> (i32, i32) {
    let v = lp.0 as u32;
    let x = (v & 0xffff) as i16 as i32;
    let y = ((v >> 16) & 0xffff) as i16 as i32;
    (x, y)
}

fn in_rect(rc: &RECT, x: i32, y: i32) -> bool {
    x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom
}

fn set_ud(hwnd: HWND, p: *mut c_void) {
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, p as isize);
    }
}

fn ud_of<T>(hwnd: HWND) -> *mut T {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut T }
}

// ============================================================ 按钮
fn btn_text_w(bd: &BtnData) -> Vec<u16> {
    bd.text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn btn_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let bd_ptr = ud_of::<BtnData>(hwnd);
    if bd_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let bd = &mut *bd_ptr;
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_invalid() {
                let app = crate::app_of(bd.parent);
                paint::paint_button(hwnd, bd, app, hdc);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            let (x, _y) = lparam_pt(lparam);
            bd.hover = true;
            if bd.kind == BtnKind::Seg {
                bd.hover_zone = seg_zone_of(bd, x);
            }
            track_leave(hwnd);
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            bd.hover = false;
            bd.pressed = false;
            bd.hover_zone = 0;
            let _ = ReleaseCapture();
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let _ = SetFocus(hwnd);
            let _ = SetCapture(hwnd);
            bd.pressed = true;
            if bd.kind == BtnKind::Seg {
                let (x, _y) = lparam_pt(lparam);
                bd.press_zone = seg_zone_of(bd, x);
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let was_pressed = bd.pressed;
            let mut fired_zone = bd.press_zone;
            bd.pressed = false;
            if GetCapture() == hwnd {
                let _ = ReleaseCapture();
            }
            let (x, y) = lparam_pt(lparam);
            if was_pressed {
                let mut inside = in_rect(&client_rect_of(hwnd), x, y);
                if inside && bd.kind == BtnKind::Seg {
                    let z = seg_zone_of(bd, x);
                    if z != fired_zone {
                        inside = false;
                    } else {
                        fired_zone = z;
                    }
                }
                if inside {
                    let side = if bd.kind == BtnKind::Seg { fired_zone } else { 0 };
                    let _ = SendMessageW(bd.parent, WM_COMMAND, WPARAM(bd.id as usize), LPARAM(side));
                }
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if let Ok(cur) = LoadCursorW(None::<&HINSTANCE>, IDC_HAND) {
                let _ = SetCursor(cur);
            }
            LRESULT(1)
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            bd.focus = msg == WM_SETFOCUS;
            let _ = InvalidateRect(hwnd, None, false);
            // 继续让系统正常处理焦点（不拦截）
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_KEYDOWN => {
            let key = wparam.0 as u32;
            if key == VK_TAB {
                // Tab 由 Edit 各自处理；按钮收到时把焦点还给第一个输入框
                if let Some(app) = crate::app_mut_of(bd.parent) {
                    crate::focus_edit_index(app, 0);
                }
                LRESULT(0)
            } else if key == 0x0D || key == 0x20 {
                // Enter / Space 触发点击
                let side = if bd.kind == BtnKind::Seg {
                    bd.press_zone
                } else {
                    0
                };
                let _ = SendMessageW(bd.parent, WM_COMMAND, WPARAM(bd.id as usize), LPARAM(side));
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn seg_zone_of(bd: &BtnData, x: i32) -> isize {
    let rc = client_rect_of(bd.hwnd);
    let w = (rc.right - rc.left).max(1);
    if x < w / 2 {
        SEG_ZONE_LEFT
    } else {
        SEG_ZONE_RIGHT
    }
}

fn client_rect_of(hwnd: HWND) -> RECT {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    rc
}

fn track_leave(hwnd: HWND) {
    let mut tme = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe {
        let _ = TrackMouseEvent(&mut tme);
    }
}

pub fn register_classes(hinst: HINSTANCE) -> bool {
    let mut wc = WNDCLASSW::default();
    let name = widen(BTN_CLASS_NAME);
    wc.lpfnWndProc = Some(btn_proc);
    wc.hInstance = hinst;
    wc.lpszClassName = PCWSTR(name.as_ptr());
    wc.hCursor = match unsafe { LoadCursorW(None::<&HINSTANCE>, IDC_ARROW) } {
        Ok(c) => c,
        Err(_) => HCURSOR::default(),
    };
    wc.hbrBackground = HBRUSH::default();
    let atom = unsafe { RegisterClassW(&wc) };
    atom != 0
}

pub fn class_name_ptr() -> Vec<u16> {
    widen(BTN_CLASS_NAME)
}

// ============================================================ 创建控件

/// 创建一个自绘按钮子窗口（并挂上 BtnData）。
pub fn create_btn(
    parent: HWND,
    hinst: HINSTANCE,
    id: u32,
    kind: BtnKind,
    text: &str,
) -> HWND {
    let class = widen(BTN_CLASS_NAME);
    let title = widen(text);
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            parent,
            HMENU(id as usize as *mut c_void),
            hinst,
            None,
        )
    }
    .expect("create button failed");
    let bd = Box::leak(Box::new(BtnData {
        hwnd,
        parent,
        id,
        kind,
        text: text.to_string(),
        hover: false,
        pressed: false,
        focus: false,
        hover_zone: 0,
        press_zone: 0,
    }));
    set_ud(hwnd, bd as *mut BtnData as *mut c_void);
    hwnd
}

/// 创建深色单行输入框。
pub fn create_edit(
    parent: HWND,
    hinst: HINSTANCE,
    id: u32,
    role: EditRole,
    max_len: u32,
) -> HWND {
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
            0,
            0,
            0,
            0,
            parent,
            HMENU(id as usize as *mut c_void),
            hinst,
            None,
        )
    }
    .expect("create edit failed");
    // 关闭视觉主题，由 WM_CTLCOLOREDIT 完全接管配色
    unsafe {
        let _ = SetWindowTheme(hwnd, PCWSTR::null(), PCWSTR::null());
    }
    // 子类化：拦截 WM_CHAR 做输入过滤、WM_NCPAINT 自绘边框
    let prev = unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            edit_subproc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT
                as usize as isize,
        )
    };
    let ed = Box::leak(Box::new(EdData {
        prev: prev as usize,
        role,
        focus: false,
    }));
    set_ud(hwnd, ed as *mut EdData as *mut c_void);
    unsafe {
        let _ = SendMessageW(hwnd, EM_SETLIMITTEXT, WPARAM(max_len as usize), LPARAM(0));
    }
    hwnd
}

unsafe extern "system" fn default_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn edit_subproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ed_ptr = ud_of::<EdData>(hwnd);
    if ed_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let ed = &mut *ed_ptr;
    let prev: WNDPROC = std::mem::transmute::<usize, WNDPROC>(ed.prev as usize);
    let prev_proc = prev.unwrap_or(default_proc);

    match msg {
        WM_CHAR => {
            let code = wparam.0 as u32;
            if code >= 0x20 {
                match ed.role {
                    // 排除数字：完全不干预，交给解析层报错
                    EditRole::Exclude => {}
                    // 候选列表：放行数字 / 负号 / 分隔符
                    EditRole::List => {
                        let is_digit = (0x30..=0x39).contains(&code);
                        let is_minus = code == b'-' as u32;
                        // ',', ';', ' ', '，'(U+FF0C), '；'(U+FF1B)
                        let is_sep = matches!(code, 0x2C | 0x3B | 0x20 | 0xFF0C | 0xFF1B);
                        if !is_digit && !is_minus && !is_sep {
                            return LRESULT(0);
                        }
                    }
                    // 纯整数：只放行数字 / 负号
                    _ => {
                        let is_digit = (0x30..=0x39).contains(&code);
                        let is_minus = code == b'-' as u32;
                        if !is_digit && !is_minus {
                            return LRESULT(0);
                        }
                        if is_minus {
                            if ed.role == EditRole::Count || !can_type_minus(hwnd) {
                                return LRESULT(0);
                            }
                        }
                    }
                }
                // 其余正常字符交给 EDIT
            } else if code == VK_TAB {
                return LRESULT(0); // Tab 在 WM_KEYDOWN 里处理
            }
            prev_proc(hwnd, msg, wparam, lparam)
        }
        WM_KEYDOWN => {
            let key = wparam.0 as u32;
            if key == VK_TAB {
                if let Some(app) = crate::app_mut_of(GetParent(hwnd).unwrap_or_default()) {
                    let shift = ((GetKeyState(0x10) as u16) & 0x8000) != 0;
                    let idx = app.edits.iter().position(|&h| h == hwnd).unwrap_or(0);
                    let next = if shift {
                        (idx + app.edits.len() - 1) % app.edits.len()
                    } else {
                        (idx + 1) % app.edits.len()
                    };
                    crate::focus_edit_index(app, next);
                }
                LRESULT(0)
            } else if key == 0x0D {
                // Enter：直接触发「随机抽取」，等价于点主按钮
                let _ = SendMessageW(
                    GetParent(hwnd).unwrap_or_default(),
                    WM_COMMAND,
                    WPARAM(ID_BTN_GO as usize),
                    LPARAM(0),
                );
                LRESULT(0)
            } else {
                prev_proc(hwnd, msg, wparam, lparam)
            }
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            ed.focus = msg == WM_SETFOCUS;
            let r = prev_proc(hwnd, msg, wparam, lparam);
            // 刷新自绘边框颜色（focus 时高亮）
            redraw_frame(hwnd);
            r
        }
        WM_NCPAINT => {
            // 自绘 1px 边框（配合 WS_BORDER）
            paint_edit_border(hwnd, ed.focus);
            LRESULT(0)
        }
        _ => prev_proc(hwnd, msg, wparam, lparam),
    }
}

fn redraw_frame(hwnd: HWND) {
    unsafe {
        let _ = RedrawWindow(hwnd, None, None, RDW_FRAME | RDW_INVALIDATE);
    }
}

fn paint_edit_border(hwnd: HWND, focus: bool) {
    unsafe {
        let hdc = GetWindowDC(hwnd);
        if hdc.is_invalid() {
            return;
        }
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        // 把客户区坐标换算成窗口坐标（含边框的坐标）
        let mut p = POINT::default();
        let _ = ClientToScreen(hwnd, &mut p);
        let mut wrc = RECT::default();
        let _ = GetWindowRect(hwnd, &mut wrc);
        let left = p.x - wrc.left;
        let top = p.y - wrc.top;
        let border = crate::theme::col::LINE;
        let use_color = if focus {
            crate::theme::col::ACCENT
        } else {
            border
        };
        let b = crate::theme::brush(use_color);
        let old_b = SelectObject(hdc, b.0);
        let np = GetStockObject(NULL_BRUSH);
        let old_p = SelectObject(hdc, np);
        let x0 = left - 1;
        let y0 = top - 1;
        let w = rc.right - rc.left + 1;
        let h = rc.bottom - rc.top + 1;
        let _ = Rectangle(hdc, x0, y0, x0 + w, y0 + h);
        let _ = SelectObject(hdc, old_p);
        let _ = SelectObject(hdc, old_b);
        let _ = ReleaseDC(hwnd, hdc);
    }
}

fn can_type_minus(hwnd: HWND) -> bool {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len == 0 {
            return true;
        }
        // 全选状态下允许替换
        let sel = SendMessageW(hwnd, EM_GETSEL, WPARAM(0), LPARAM(0)).0 as u32;
        let start = (sel & 0xffff) as usize;
        let end = (sel >> 16) as usize;
        start == 0 && end == len as usize
    }
}

// ============================================================ 布局计算

/// 读取文本（含中文安全处理）。
pub fn edit_text(hwnd: HWND) -> String {
    let n = unsafe { GetWindowTextLengthW(hwnd) };
    if n <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; n as usize + 1];
    unsafe {
        let got = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..got as usize])
    }
}

pub fn set_edit_text(hwnd: HWND, s: &str) {
    let w = widen(s);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(w.as_ptr()));
    }
}

/// 把控件名从按钮实例取回（给主窗口读取已设置的自绘按钮文本用）。
pub fn btn_label(id: u32) -> &'static str {
    match id {
        ID_BTN_GO => "随机抽取",
        ID_BTN_DICE => "掷 5 颗骰子",
        ID_BTN_CALL => "点名 1~30",
        ID_BTN_SIX => "抽 6 个不重复",
        ID_BTN_DLB => "双色球",
        ID_BTN_COPY => "复制结果",
        ID_BTN_HIST => "查看历史记录",
        _ => "",
    }
}

/// 计算左卡片及其内部控件几何。measure 用于测量快捷按钮文本宽度。
pub fn left_layout(dpi: u32, measure: &dyn Fn(&str) -> i32) -> LeftGeo {
    let s = dpi;
    let card_x = sc(MARGIN_X, s);
    let card_w = sc(LEFT_W, s);
    let card_top = sc(HDR_H, s) + sc(MARGIN_TOP, s);
    let inner_x = card_x + sc(PAD, s);
    let inner_w = card_w - sc(PAD, s) * 2;
    let card = RECT {
        left: card_x,
        top: card_top,
        right: card_x + card_w,
        bottom: 0,
    };

    let mut y = card_top + sc(PAD, s);

    let h2 = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(20, s),
    };
    y += sc(20, s) + sc(6, s);

    // 输入方式：区间 / 指定列表
    let lab_mode = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(22, s),
    };
    y += sc(22, s) + sc(4, s);
    let seg_mode = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(SEG_H, s),
    };
    y += sc(SEG_H, s) + sc(8, s);

    // 区间下界 / 上界（或：候选数字列表）
    let lab_min = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(22, s),
    };
    y += sc(22, s) + sc(4, s);
    let half = (inner_w - sc(GAP, s)) / 2;
    let ed_min = RECT {
        left: inner_x,
        top: y,
        right: inner_x + half,
        bottom: y + sc(EDIT_H, s),
    };
    let ed_max = RECT {
        left: inner_x + half + sc(GAP, s),
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(EDIT_H, s),
    };
    // 列表输入框占满整行；与 min/max 互斥显示，共用同一行位置
    let ed_list = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(EDIT_H, s),
    };
    y += sc(EDIT_H, s) + sc(2, s);
    let hint = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(16, s),
    };
    y += sc(16, s) + sc(4, s);

    // 抽取个数
    let lab_count = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(22, s),
    };
    y += sc(22, s) + sc(4, s);
    let ed_count = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(EDIT_H, s),
    };
    y += sc(EDIT_H, s) + sc(2, s);

    // 排除数字
    let lab_exclude = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(22, s),
    };
    y += sc(22, s) + sc(4, s);
    let ed_exclude = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(EDIT_H, s),
    };
    y += sc(EDIT_H, s) + sc(6, s);

    // 是否允许重复
    let lab_uniq = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(22, s),
    };
    y += sc(22, s) + sc(4, s);
    let seg = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(SEG_H, s),
    };
    y += sc(SEG_H, s) + sc(16, s);

    // 主按钮
    let go = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(GO_H, s),
    };
    y += sc(GO_H, s) + sc(18, s);

    // 常用场景
    let lab_quick = RECT {
        left: inner_x,
        top: y,
        right: card.right - sc(PAD, s),
        bottom: y + sc(22, s),
    };
    y += sc(22, s) + sc(6, s);

    // 快捷按钮自动换行放置：cursor 为当前行下一个按钮可放的 x。
    // 放不下则另起一行（qy 下移、cursor 回到 inner_x）。
    let labels = [
        crate::ui::btn_label(ID_BTN_DICE),
        crate::ui::btn_label(ID_BTN_CALL),
        crate::ui::btn_label(ID_BTN_SIX),
        crate::ui::btn_label(ID_BTN_DLB),
    ];
    let quick_gap = sc(7, s);
    let mut quick = [RECT::default(); 4];
    let mut qy = y;
    let max_x = card.right - sc(PAD, s);
    let mut cursor = inner_x;
    for (i, label) in labels.iter().enumerate() {
        let w = measure(label) + sc(22, s);
        if cursor > inner_x && cursor + quick_gap + w > max_x + 1 {
            qy += sc(QUICK_H, s) + quick_gap; // 换行
            cursor = inner_x;
        }
        let x = if cursor > inner_x {
            cursor + quick_gap
        } else {
            inner_x
        };
        quick[i] = RECT {
            left: x,
            top: qy,
            right: x + w,
            bottom: qy + sc(QUICK_H, s),
        };
        cursor = x + w;
    }
    let last_quick = quick[3];
    y = last_quick.bottom + sc(PAD, s);

    let mut geo = LeftGeo {
        card,
        h2,
        lab_mode,
        seg_mode,
        lab_min,
        ed_min,
        ed_max,
        ed_list,
        hint,
        lab_count,
        ed_count,
        lab_exclude,
        ed_exclude,
        lab_uniq,
        seg,
        go,
        lab_quick,
        quick,
        card_bottom: y,
    };
    geo.card.bottom = geo.card_bottom;
    geo
}
