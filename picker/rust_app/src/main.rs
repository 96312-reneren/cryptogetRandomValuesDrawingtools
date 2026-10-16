//! 随机选数 · 原生桌面版（Win32 + GDI，无第三方运行时依赖）
//!
//! 单文件、零依赖 exe：核心随机逻辑见 `pick.rs`（对齐 `main.py`），
//! 熵源见 `rng.rs`（BCryptGenRandom），界面自绘见 `paint.rs` / `ui.rs`。
//!
//! 构建：
//!     cargo build --release            # 产物 target/release/随机选数.exe

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod paint;
mod pick;
mod rng;
mod theme;
mod ui;

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::*;
use windows::Win32::UI::HiDpi::{
    GetDpiForSystem, GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

use theme::{col, fonts, sc, widen};
use ui::{BtnKind, EditRole};

const MAIN_CLASS: &str = "PickerMainWindow";
const CF_UNICODETEXT: u32 = 13;
const TIMER_COPY: usize = 1;

// ============================================================ 状态

pub enum View {
    Normal,
    Ball { reds: Vec<i64>, blue: i64 },
}

pub struct ResultData {
    pub view: View,
    pub nums: Vec<i64>,
    pub meta: String,
    pub text: String,
}

pub struct HistItem {
    pub label: String,
    pub text: String,
}

#[derive(Clone, Default)]
pub struct RightGeo {
    pub card: RECT,
    pub h2: RECT,
    pub nums: RECT,
    pub meta: RECT,
    pub err: RECT,
    pub copy: RECT,
    pub hist: RECT,
}

pub struct Geo {
    pub client: RECT,
    pub left: ui::LeftGeo,
    pub right: RightGeo,
}

pub struct App {
    pub hwnd: HWND,
    pub hinst: HINSTANCE,
    pub dpi: u32,
    pub ready: bool,
    /// 当前模式下参与 Tab 循环的输入框（顺序即 Tab 顺序）
    pub edits: Vec<HWND>,
    pub ed_min: HWND,
    pub ed_max: HWND,
    pub ed_list: HWND,
    pub ed_count: HWND,
    pub ed_exclude: HWND,
    pub btn_go: HWND,
    pub btn_seg: HWND,
    pub btn_mode: HWND,
    pub quick: [HWND; 4],
    pub btn_copy: HWND,
    pub btn_hist: HWND,
    /// false = 区间模式；true = 指定列表模式
    pub list_mode: bool,
    pub unique: bool,
    pub result: Option<ResultData>,
    pub history: Vec<HistItem>,
    pub show_history: bool,
    pub error: Option<String>,
    pub elapsed_ms: f64,
    pub copy_flash: bool,
    pub geo: Option<Geo>,
}

impl App {
    fn new(hwnd: HWND, hinst: HINSTANCE, dpi: u32) -> App {
        App {
            hwnd,
            hinst,
            dpi,
            ready: false,
            edits: Vec::new(),
            ed_min: HWND::default(),
            ed_max: HWND::default(),
            ed_list: HWND::default(),
            ed_count: HWND::default(),
            ed_exclude: HWND::default(),
            btn_go: HWND::default(),
            btn_seg: HWND::default(),
            btn_mode: HWND::default(),
            quick: [HWND::default(); 4],
            btn_copy: HWND::default(),
            btn_hist: HWND::default(),
            list_mode: false,
            unique: true,
            result: None,
            history: Vec::new(),
            show_history: false,
            error: None,
            elapsed_ms: 0.0,
            copy_flash: false,
            geo: None,
        }
    }

    /// 切换「区间 / 指定列表」输入方式。
    ///
    /// 两套输入框共用同一行位置，只切换可见性与 Tab 顺序。
    fn set_mode(&mut self, list: bool) {
        if self.list_mode == list && !self.edits.is_empty() {
            return;
        }
        self.list_mode = list;
        unsafe {
            let _ = ShowWindow(self.ed_min, if list { SW_HIDE } else { SW_SHOW });
            let _ = ShowWindow(self.ed_max, if list { SW_HIDE } else { SW_SHOW });
            let _ = ShowWindow(self.ed_list, if list { SW_SHOW } else { SW_HIDE });
        }
        self.edits.clear();
        if list {
            self.edits.push(self.ed_list);
        } else {
            self.edits.push(self.ed_min);
            self.edits.push(self.ed_max);
        }
        self.edits.push(self.ed_count);
        self.edits.push(self.ed_exclude);
        self.invalidate_btn(ui::ID_BTN_MODE);
        self.invalidate_btn(ui::ID_BTN_SEG);
        self.relayout();
    }

    // ---------- 布局 ----------

    fn relayout(&mut self) {
        if !self.ready {
            return;
        }
        let dpi = self.dpi;
        let mut cr = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut cr);
        }
        let cw = cr.right.max(sc(600, dpi));
        let chh = cr.bottom.max(sc(400, dpi));

        let hdc = unsafe { GetDC(self.hwnd) };
        let base = fonts(dpi).base;
        let measure = |s: &str| theme::text_width(hdc, base, s);
        let left = ui::left_layout(dpi, &measure);

        let pad = sc(ui::PAD, dpi);
        let right_x = sc(ui::MARGIN_X, dpi) + sc(ui::LEFT_W, dpi) + sc(ui::GAP, dpi);
        let card_top = sc(ui::HDR_H, dpi) + sc(ui::MARGIN_TOP, dpi);
        let card_bottom = left.card_bottom.max(card_top + sc(240, dpi));
        let right_w = (cw - right_x - sc(ui::MARGIN_X, dpi)).max(sc(300, dpi));
        let card = RECT {
            left: right_x,
            top: card_top,
            right: right_x + right_w,
            bottom: card_bottom,
        };
        let inner_r = card.right - pad;

        let h2 = RECT {
            left: card.left + pad,
            top: card.top + pad,
            right: inner_r,
            bottom: card.top + pad + sc(20, dpi),
        };

        // 底部操作按钮行
        let op_h = sc(ui::OP_H, dpi);
        let op_y = card.bottom - pad - op_h;
        let copy_w = theme::text_width(hdc, base, "复制结果") + sc(30, dpi);
        let hist_w = theme::text_width(hdc, base, "查看历史记录") + sc(30, dpi);
        let copy = RECT {
            left: card.left + pad,
            top: op_y,
            right: card.left + pad + copy_w,
            bottom: op_y + op_h,
        };
        let hist = RECT {
            left: copy.right + sc(8, dpi),
            top: op_y,
            right: copy.right + sc(8, dpi) + hist_w,
            bottom: op_y + op_h,
        };

        // meta 行
        let meta = RECT {
            left: card.left + pad,
            top: op_y - sc(8, dpi) - sc(18, dpi),
            right: inner_r,
            bottom: op_y - sc(8, dpi),
        };

        // 错误条（高度随文本自适应）
        let err = if let Some(e) = &self.error {
            let w = (right_w - pad * 2 - sc(20, dpi)).max(1);
            let h = theme::wrap_height(hdc, fonts(dpi).small, e, w).max(sc(16, dpi)) + sc(16, dpi);
            let bottom = meta.top - sc(8, dpi);
            RECT {
                left: card.left + pad,
                top: bottom - h,
                right: inner_r,
                bottom,
            }
        } else {
            RECT {
                left: card.left + pad,
                top: meta.top,
                right: inner_r,
                bottom: meta.top,
            }
        };

        // 结果区
        let nums_top = card.top + pad + sc(20, dpi) + sc(6, dpi);
        let nums_bottom = (if self.error.is_some() { err.top } else { meta.top }) - sc(10, dpi);
        let nums = RECT {
            left: card.left + pad,
            top: nums_top,
            right: inner_r,
            bottom: nums_bottom.max(nums_top + sc(120, dpi)),
        };

        let right = RightGeo {
            card,
            h2,
            nums,
            meta,
            err,
            copy,
            hist,
        };

        unsafe {
            ReleaseDC(self.hwnd, hdc);
            move_ctl(self.ed_min, &left.ed_min);
            move_ctl(self.ed_max, &left.ed_max);
            move_ctl(self.ed_list, &left.ed_list);
            move_ctl(self.ed_count, &left.ed_count);
            move_ctl(self.ed_exclude, &left.ed_exclude);
            move_ctl(self.btn_mode, &left.seg_mode);
            move_ctl(self.btn_seg, &left.seg);
            move_ctl(self.btn_go, &left.go);
            for i in 0..4 {
                move_ctl(self.quick[i], &left.quick[i]);
            }
            move_ctl(self.btn_copy, &copy);
            move_ctl(self.btn_hist, &hist);
        }

        self.geo = Some(Geo {
            client: RECT {
                left: 0,
                top: 0,
                right: cw,
                bottom: chh,
            },
            left,
            right,
        });
        // 子控件移动后，原位置需要重绘（WS_CLIPCHILDREN 会排除现子控件区域）
        self.invalidate();
    }

    fn min_client(&self) -> (i32, i32) {
        let dpi = self.dpi;
        let w = sc(ui::MARGIN_X, dpi) * 2
            + sc(ui::LEFT_W, dpi)
            + sc(ui::GAP, dpi)
            + sc(330, dpi);
        let h = self
            .geo
            .as_ref()
            .map(|g| g.left.card_bottom + sc(ui::MARGIN_X, dpi))
            .unwrap_or_else(|| sc(700, dpi));
        (w, h)
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    fn invalidate_btn(&self, id: u32) {
        let h = match id {
            ui::ID_BTN_GO => self.btn_go,
            ui::ID_BTN_SEG => self.btn_seg,
            ui::ID_BTN_MODE => self.btn_mode,
            ui::ID_BTN_COPY => self.btn_copy,
            ui::ID_BTN_HIST => self.btn_hist,
            _ => HWND::default(),
        };
        if !h.is_invalid() {
            unsafe {
                let _ = InvalidateRect(h, None, false);
            }
        }
    }

    // ---------- 业务 ----------

    fn fail(&mut self, msg: &str) {
        self.error = Some(msg.to_string());
        self.result = None;
        self.elapsed_ms = 0.0;
        // 错误条高度参与布局，必须重新布局（relayout 内部会 invalidate）
        self.relayout();
    }

    fn push_history(&mut self, label: String, text: String) {
        self.history.insert(0, HistItem { label, text });
        if self.history.len() > 50 {
            self.history.truncate(50);
        }
    }

    fn do_pick(&mut self) {
        if self.list_mode {
            self.do_pick_list();
        } else {
            self.do_pick_range();
        }
    }

    fn do_pick_range(&mut self) {
        let min_s = ui::edit_text(self.ed_min);
        let max_s = ui::edit_text(self.ed_max);
        let cnt_s = ui::edit_text(self.ed_count);
        let ex_s = ui::edit_text(self.ed_exclude);
        if min_s.trim().is_empty() {
            self.fail("请填写区间下界 min（整数）");
            return;
        }
        if max_s.trim().is_empty() {
            self.fail("请填写区间上界 max（整数）");
            return;
        }
        if cnt_s.trim().is_empty() {
            self.fail("请填写抽取个数 count（1~100）");
            return;
        }
        let Ok(min) = min_s.trim().parse::<i64>() else {
            self.fail("区间下界 min 不是整数");
            return;
        };
        let Ok(max) = max_s.trim().parse::<i64>() else {
            self.fail("区间上界 max 不是整数");
            return;
        };
        let Ok(count) = cnt_s.trim().parse::<i64>() else {
            self.fail("抽取个数 count 不是整数");
            return;
        };
        if min > max {
            self.fail(&format!("下界 {min} 不能大于上界 {max}"));
            return;
        }
        if !(1..=100).contains(&count) {
            self.fail("count 需在 1 ~ 100 之间");
            return;
        }
        let exclude = match pick::parse_exclude_text(&ex_s) {
            Ok(v) => v,
            Err(msg) => {
                self.fail(&msg);
                return;
            }
        };
        let unique = self.unique;
        self.run_pick(min, max, count as u32, exclude, unique);
    }

    fn run_pick(&mut self, min: i64, max: i64, count: u32, exclude: Vec<i64>, unique: bool) {
        let t0 = std::time::Instant::now();
        match pick::pick_ints(min, max, count, &exclude, unique) {
            Ok(nums) => {
                self.elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
                self.error = None;
                let ex = pick::normalize_exclude(min, max, &exclude);
                let ex_txt = if ex.is_empty() {
                    String::new()
                } else {
                    format!(" · 排除 {{{}}}", join_i64(&ex, ","))
                };
                let meta = format!(
                    "区间 [{min}, {max}] · {count} 个 · {}{ex_txt} · 耗时 {:.3} ms",
                    if unique { "不重复" } else { "允许重复" },
                    self.elapsed_ms
                );
                let text = join_i64(&nums, ", ");
                self.push_history(meta.clone(), text.clone());
                self.result = Some(ResultData {
                    view: View::Normal,
                    nums,
                    meta,
                    text,
                });
            }
            Err(msg) => {
                self.error = Some(msg);
                self.result = None;
                self.elapsed_ms = 0.0;
            }
        }
        self.show_history = false;
        // 结果 / 错误状态会改变结果区布局，这里统一走 relayout
        self.relayout();
    }

    fn do_pick_list(&mut self) {
        let list_s = ui::edit_text(self.ed_list);
        let cnt_s = ui::edit_text(self.ed_count);
        let ex_s = ui::edit_text(self.ed_exclude);
        if list_s.trim().is_empty() {
            self.fail("请填写候选数字列表（整数，用逗号 / 空格分隔）");
            return;
        }
        if cnt_s.trim().is_empty() {
            self.fail("请填写抽取个数 count（1~100）");
            return;
        }
        let items = match pick::parse_int_list(&list_s, "候选列表") {
            Ok(v) => v,
            Err(msg) => {
                self.fail(&msg);
                return;
            }
        };
        let Ok(count) = cnt_s.trim().parse::<i64>() else {
            self.fail("抽取个数 count 不是整数");
            return;
        };
        if !(1..=100).contains(&count) {
            self.fail("count 需在 1 ~ 100 之间");
            return;
        }
        let exclude = match pick::parse_exclude_text(&ex_s) {
            Ok(v) => v,
            Err(msg) => {
                self.fail(&msg);
                return;
            }
        };
        let unique = self.unique;
        self.run_list_pick(&items, count as u32, exclude, unique);
    }

    /// 指定列表抽取：值优先语义（先去重再抽），随机性作用在索引上。
    fn run_list_pick(&mut self, items: &[i64], count: u32, exclude: Vec<i64>, unique: bool) {
        let t0 = std::time::Instant::now();
        match pick::pick_from_list(items, count, &exclude, unique) {
            Ok(nums) => {
                self.elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
                self.error = None;
                // 去重后的候选个数（仅用于展示）
                let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
                let uniq_n = items.iter().filter(|v| seen.insert(**v)).count();
                let ex_txt = if exclude.is_empty() {
                    String::new()
                } else {
                    format!(" · 排除 {{{}}}", join_i64(&exclude, ","))
                };
                let meta = format!(
                    "候选列表 {uniq_n} 个 · {count} 个 · {}{ex_txt} · 耗时 {:.3} ms",
                    if unique { "不重复" } else { "允许重复" },
                    self.elapsed_ms
                );
                let text = join_i64(&nums, ", ");
                self.push_history(meta.clone(), text.clone());
                self.result = Some(ResultData {
                    view: View::Normal,
                    nums,
                    meta,
                    text,
                });
            }
            Err(msg) => {
                self.error = Some(msg);
                self.result = None;
                self.elapsed_ms = 0.0;
            }
        }
        self.show_history = false;
        // 结果 / 错误状态会改变结果区布局，这里统一走 relayout
        self.relayout();
    }

    fn quick_scene(&mut self, min: i64, max: i64, count: u32, unique: bool) {
        self.set_mode(false);
        set_edit_i64(self.ed_min, min);
        set_edit_i64(self.ed_max, max);
        set_edit_i64(self.ed_count, count as i64);
        ui::set_edit_text(self.ed_exclude, "");
        self.unique = unique;
        self.invalidate_btn(ui::ID_BTN_SEG);
        self.run_pick(min, max, count, Vec::new(), unique);
    }

    fn do_ball(&mut self) {
        self.set_mode(false);
        let t0 = std::time::Instant::now();
        let reds = pick::pick_ints(1, 33, 6, &[], true);
        let blue = pick::pick_ints(1, 16, 1, &[], true);
        match (reds, blue) {
            (Ok(mut r), Ok(b)) => {
                r.sort_unstable();
                self.elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
                self.error = None;
                let blue_n = b[0];
                let meta = "双色球 · 红球 6 个（1~33，不重复） + 蓝球 1 个（1~16）".to_string();
                let reds_txt: Vec<String> = r.iter().map(|v| format!("{v:02}")).collect();
                let text = format!("红球 {} + 蓝球 {blue_n:02}", reds_txt.join(" "));
                self.push_history(meta.clone(), text.clone());
                self.result = Some(ResultData {
                    view: View::Ball {
                        reds: r,
                        blue: blue_n,
                    },
                    nums: Vec::new(),
                    meta,
                    text,
                });
            }
            (Err(m), _) | (_, Err(m)) => {
                self.error = Some(m);
                self.result = None;
                self.elapsed_ms = 0.0;
            }
        }
        self.show_history = false;
        // 结果 / 错误状态会改变结果区布局，这里统一走 relayout
        self.relayout();
    }

    fn do_copy(&mut self) {
        let text = match &self.result {
            Some(r) => r.text.clone(),
            None => {
                self.fail("还没有结果可复制");
                return;
            }
        };
        copy_to_clipboard(self.hwnd, &text);
        self.copy_flash = true;
        unsafe {
            let _ = SetTimer(self.hwnd, TIMER_COPY, 1200, None);
        }
        self.invalidate_btn(ui::ID_BTN_COPY);
    }
}

// ============================================================ 全局访问

pub fn app_of(hwnd: HWND) -> &'static App {
    unsafe {
        let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        debug_assert!(!p.is_null(), "app_of: GWLP_USERDATA 未设置");
        &*p
    }
}

pub fn app_mut_of(hwnd: HWND) -> Option<&'static mut App> {
    unsafe {
        let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if p.is_null() {
            None
        } else {
            Some(&mut *p)
        }
    }
}

pub fn focus_edit_index(app: &mut App, idx: usize) {
    let n = app.edits.len();
    if n == 0 {
        return;
    }
    let i = idx % n;
    unsafe {
        let _ = SetFocus(app.edits[i]);
    }
}

// ============================================================ 工具

fn join_i64(v: &[i64], sep: &str) -> String {
    v.iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(sep)
}

fn move_ctl(hwnd: HWND, rc: &RECT) {
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        let _ = MoveWindow(
            hwnd,
            rc.left,
            rc.top,
            rc.right - rc.left,
            rc.bottom - rc.top,
            true,
        );
    }
}

fn set_edit_i64(hwnd: HWND, v: i64) {
    ui::set_edit_text(hwnd, &v.to_string());
}

fn copy_to_clipboard(hwnd: HWND, text: &str) {
    unsafe {
        if OpenClipboard(hwnd).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        let mut utf16: Vec<u16> = text.encode_utf16().collect();
        utf16.push(0);
        let bytes = utf16.len() * 2;
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
            let p = GlobalLock(h);
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(utf16.as_ptr() as *const u8, p as *mut u8, bytes);
                let _ = GlobalUnlock(h);
                let _ = SetClipboardData(CF_UNICODETEXT, HANDLE(h.0));
            }
        }
        let _ = CloseClipboard();
    }
}

fn window_dpi(hwnd: HWND) -> u32 {
    let d = unsafe { GetDpiForWindow(hwnd) };
    if d == 0 {
        96
    } else {
        d
    }
}

fn module_instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into() }
}

// ============================================================ 消息处理

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            on_create(hwnd);
            LRESULT(0)
        }
        WM_SIZE => {
            if let Some(app) = app_mut_of(hwnd) {
                app.relayout();
            }
            LRESULT(0)
        }
        WM_PAINT => {
            on_paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_CTLCOLOREDIT => on_ctlcolor(wparam),
        WM_COMMAND => {
            on_command(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_TIMER => {
            on_timer(hwnd, wparam);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if let Some(app) = app_mut_of(hwnd) {
                if app.show_history {
                    app.show_history = false;
                    app.invalidate();
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 as u32 == 0x1B {
                if let Some(app) = app_mut_of(hwnd) {
                    if app.show_history {
                        app.show_history = false;
                        app.invalidate();
                    }
                }
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            on_dpi_changed(hwnd, lparam);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            on_minmax(hwnd, lparam);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn on_create(hwnd: HWND) {
    let hinst = module_instance();
    let dpi = window_dpi(hwnd);
    let app: &'static mut App = Box::leak(Box::new(App::new(hwnd, hinst, dpi)));
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as *mut App as isize);
    }

    let f = fonts(dpi).base;
    app.ed_min = ui::create_edit(hwnd, hinst, ui::ID_EDIT_MIN, EditRole::Min, 20);
    app.ed_max = ui::create_edit(hwnd, hinst, ui::ID_EDIT_MAX, EditRole::Max, 20);
    app.ed_list = ui::create_edit(hwnd, hinst, ui::ID_EDIT_LIST, EditRole::List, 512);
    app.ed_count = ui::create_edit(hwnd, hinst, ui::ID_EDIT_COUNT, EditRole::Count, 3);
    app.ed_exclude = ui::create_edit(hwnd, hinst, ui::ID_EDIT_EXCLUDE, EditRole::Exclude, 512);
    app.btn_go = ui::create_btn(hwnd, hinst, ui::ID_BTN_GO, BtnKind::Primary, "随机抽取");
    app.btn_mode = ui::create_btn(hwnd, hinst, ui::ID_BTN_MODE, BtnKind::Seg, "");
    app.btn_seg = ui::create_btn(hwnd, hinst, ui::ID_BTN_SEG, BtnKind::Seg, "");
    app.quick[0] = ui::create_btn(
        hwnd,
        hinst,
        ui::ID_BTN_DICE,
        BtnKind::Quick,
        ui::btn_label(ui::ID_BTN_DICE),
    );
    app.quick[1] = ui::create_btn(
        hwnd,
        hinst,
        ui::ID_BTN_CALL,
        BtnKind::Quick,
        ui::btn_label(ui::ID_BTN_CALL),
    );
    app.quick[2] = ui::create_btn(
        hwnd,
        hinst,
        ui::ID_BTN_SIX,
        BtnKind::Quick,
        ui::btn_label(ui::ID_BTN_SIX),
    );
    app.quick[3] = ui::create_btn(
        hwnd,
        hinst,
        ui::ID_BTN_DLB,
        BtnKind::Quick,
        ui::btn_label(ui::ID_BTN_DLB),
    );
    app.btn_copy = ui::create_btn(
        hwnd,
        hinst,
        ui::ID_BTN_COPY,
        BtnKind::Op,
        ui::btn_label(ui::ID_BTN_COPY),
    );
    app.btn_hist = ui::create_btn(
        hwnd,
        hinst,
        ui::ID_BTN_HIST,
        BtnKind::Op,
        ui::btn_label(ui::ID_BTN_HIST),
    );

    for &e in [
        app.ed_min,
        app.ed_max,
        app.ed_list,
        app.ed_count,
        app.ed_exclude,
    ]
    .iter()
    {
        unsafe {
            SendMessageW(e, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
        }
    }
    ui::set_edit_text(app.ed_min, "1");
    ui::set_edit_text(app.ed_max, "100");
    ui::set_edit_text(app.ed_count, "5");
    ui::set_edit_text(app.ed_list, "");
    app.ready = true;
    app.set_mode(false);
    app.relayout();
}

fn on_paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    if !hdc.is_invalid() {
        if let Some(app) = app_mut_of(hwnd) {
            if let Some(geo) = app.geo.as_ref() {
                let w = geo.client.right;
                let h = geo.client.bottom;
                if w > 0 && h > 0 {
                    if let Some(cv) = theme::Canvas::new(hdc, w, h) {
                        paint::paint_main(app, cv.dc);
                        cv.present(hdc);
                    }
                }
            }
        }
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
    }
}

fn on_ctlcolor(wparam: WPARAM) -> LRESULT {
    let hdc = theme::hdc_from_ptr(wparam.0 as *mut c_void);
    unsafe {
        SetTextColor(hdc, col::TEXT);
        SetBkColor(hdc, col::INPUT_BG);
    }
    LRESULT(theme::cached_brush(col::INPUT_BG).0 as isize)
}

fn on_command(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let id = (wparam.0 & 0xffff) as u32;
    let side = lparam.0;
    let Some(app) = app_mut_of(hwnd) else {
        return;
    };
    match id {
        ui::ID_BTN_GO => app.do_pick(),
        ui::ID_BTN_SEG => {
            app.unique = if side == ui::SEG_ZONE_LEFT {
                true
            } else if side == ui::SEG_ZONE_RIGHT {
                false
            } else {
                !app.unique
            };
            app.invalidate_btn(ui::ID_BTN_SEG);
        }
        ui::ID_BTN_MODE => {
            // 左 = 区间，右 = 指定列表
            let list = if side == ui::SEG_ZONE_LEFT {
                false
            } else if side == ui::SEG_ZONE_RIGHT {
                true
            } else {
                !app.list_mode
            };
            app.set_mode(list);
        }
        ui::ID_BTN_DICE => app.quick_scene(1, 6, 5, false),
        ui::ID_BTN_CALL => app.quick_scene(1, 30, 1, true),
        ui::ID_BTN_SIX => app.quick_scene(1, 99, 6, true),
        ui::ID_BTN_DLB => app.do_ball(),
        ui::ID_BTN_COPY => app.do_copy(),
        ui::ID_BTN_HIST => {
            app.show_history = !app.show_history;
            app.invalidate();
        }
        _ => {}
    }
}

fn on_timer(hwnd: HWND, wparam: WPARAM) {
    if wparam.0 != TIMER_COPY {
        return;
    }
    let Some(app) = app_mut_of(hwnd) else {
        return;
    };
    app.copy_flash = false;
    unsafe {
        let _ = KillTimer(hwnd, TIMER_COPY);
    }
    app.invalidate_btn(ui::ID_BTN_COPY);
}

fn on_dpi_changed(hwnd: HWND, lparam: LPARAM) {
    let Some(app) = app_mut_of(hwnd) else {
        return;
    };
    app.dpi = window_dpi(hwnd);
    let pr = lparam.0 as *const RECT;
    if !pr.is_null() {
        let r = unsafe { *pr };
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                HWND::default(),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
    let f = fonts(app.dpi).base;
    for &e in app.edits.iter() {
        unsafe {
            SendMessageW(e, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
        }
    }
    app.relayout();
}

fn on_minmax(hwnd: HWND, lparam: LPARAM) {
    let Some(app) = app_mut_of(hwnd) else {
        return;
    };
    let mmi = lparam.0 as *mut MINMAXINFO;
    if mmi.is_null() {
        return;
    }
    let (cw, ch) = app.min_client();
    let mut r = RECT {
        left: 0,
        top: 0,
        right: cw,
        bottom: ch,
    };
    unsafe {
        let _ = AdjustWindowRect(&mut r, WS_OVERLAPPEDWINDOW, false);
        (*mmi).ptMinTrackSize.x = r.right - r.left;
        (*mmi).ptMinTrackSize.y = r.bottom - r.top;
    }
}

// ============================================================ 入口

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let hinst = module_instance();
    ui::register_classes(hinst);

    let cls = widen(MAIN_CLASS);
    let mut wc = WNDCLASSW::default();
    wc.lpfnWndProc = Some(wndproc);
    wc.hInstance = hinst;
    wc.lpszClassName = PCWSTR(cls.as_ptr());
    wc.hCursor = unsafe { LoadCursorW(None::<&HINSTANCE>, IDC_ARROW).unwrap_or_default() };
    wc.hbrBackground = theme::cached_brush(col::BG);
    if unsafe { RegisterClassW(&wc) } == 0 {
        let msg = widen("注册窗口类失败。");
        let cap = widen("随机选数器");
        unsafe {
            MessageBoxW(
                HWND::default(),
                PCWSTR(msg.as_ptr()),
                PCWSTR(cap.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
        return;
    }

    // 初始尺寸按系统 DPI 计算，避免高 DPI 屏上窗口过小、内容拥挤
    let dpi = {
        let d = unsafe { GetDpiForSystem() };
        if d == 0 {
            96
        } else {
            d
        }
    };
    let client_w = sc(ui::MARGIN_X, dpi) * 2
        + sc(ui::LEFT_W, dpi)
        + sc(ui::GAP, dpi)
        + sc(560, dpi);
    // 用屏幕 DC 量一次布局，保证初始高度刚好容纳左卡片（新增了模式开关一行）
    let client_h = {
        let screen = HWND::default();
        let hdc = unsafe { GetDC(screen) };
        let base = fonts(dpi).base;
        let measure = |s: &str| theme::text_width(hdc, base, s);
        let left = ui::left_layout(dpi, &measure);
        unsafe {
            ReleaseDC(screen, hdc);
        }
        left.card_bottom + sc(ui::MARGIN_X, dpi)
    };
    let mut wr = RECT {
        left: 0,
        top: 0,
        right: client_w,
        bottom: client_h,
    };
    unsafe {
        let _ = AdjustWindowRect(&mut wr, WS_OVERLAPPEDWINDOW, false);
    }
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    // 窗口不超过屏幕可用大小
    let ww = (wr.right - wr.left).min(sw);
    let wh = (wr.bottom - wr.top).min(sh - sc(48, dpi).max(0));
    let x = ((sw - ww) / 2).max(0);
    let y = ((sh - wh) / 2).max(0);

    let title = widen("随机选数器");
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(cls.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            x,
            y,
            ww,
            wh,
            None,
            None,
            hinst,
            None,
        )
    };
    let hwnd = match hwnd {
        Ok(h) => h,
        Err(e) => {
            let msg = widen(&format!("创建窗口失败：{e}"));
            let cap = widen("随机选数器");
            unsafe {
                MessageBoxW(
                    HWND::default(),
                    PCWSTR(msg.as_ptr()),
                    PCWSTR(cap.as_ptr()),
                    MB_OK | MB_ICONERROR,
                );
            }
            return;
        }
    };

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
    }

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
