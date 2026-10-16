# -*- coding: utf-8 -*-
"""
随机选数 · 桌面版（单文件 UI 入口）
====================================
在不改动 main.py（后端逻辑）的前提下，把 UI 页面作为 / 路由挂在同一个 FastAPI
应用上；启动时自动选空闲端口拉起服务，并自动用系统浏览器打开界面。
整个程序可被 PyInstaller 打成单个 exe，无需任何外部资源文件。

用法
----
    python desktop.py              # 启动服务并自动打开浏览器
    python desktop.py --no-browser # 只启动服务（测试用，不弹浏览器）
    python desktop.py --port 8000  # 指定端口

打包为单 exe（详见 build.bat）：
    pyinstaller --noconfirm --clean --onefile --name 随机选数 desktop.py \
        --collect-all uvicorn --collect-submodules fastapi
"""

from __future__ import annotations

import sys
import threading
import time
import urllib.request
import webbrowser
from socket import socket

from starlette.routing import Route
from fastapi.responses import HTMLResponse
import uvicorn

from main import app  # 复用现有后端：路由、校验、SystemRandom 逻辑全部不变

# ============================================================ 内嵌 UI 页面
# 说明：全部样式/脚本内联，打包后不依赖任何外部 html/css/js 文件。

UI_HTML = r"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>随机选数器</title>
<style>
  :root{
    --bg:#0b1020; --panel:#141b32; --panel2:#1b2440; --line:#2a3557;
    --text:#e6ecff; --muted:#8b98c4; --accent:#4f8cff; --accent2:#37d6a3;
    --danger:#ff6b81; --warn:#ffb454;
  }
  *{box-sizing:border-box}
  body{margin:0;min-height:100vh;background:radial-gradient(1200px 600px at 20% -10%,#1a2444 0%,var(--bg) 60%);
       color:var(--text);font:14px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif}
  header{padding:22px 28px 12px;border-bottom:1px solid var(--line);display:flex;flex-wrap:wrap;align-items:center;gap:12px}
  h1{margin:0;font-size:19px;letter-spacing:.4px}
  .sub{color:var(--muted);font-size:12px;margin-top:3px;width:100%}
  .bar{display:flex;flex-wrap:wrap;gap:8px;margin-left:auto}
  .chip{background:var(--panel);border:1px solid var(--line);border-radius:999px;padding:4px 11px;font-size:12px;color:var(--muted)}
  .chip b{color:var(--accent2);font-weight:600}
  main{display:grid;grid-template-columns:400px 1fr;gap:18px;padding:18px 28px 40px;align-items:start}
  @media(max-width:920px){main{grid-template-columns:1fr}}
  .card{background:linear-gradient(180deg,var(--panel) 0%,var(--panel2) 100%);
        border:1px solid var(--line);border-radius:14px;padding:16px}
  .card h2{margin:0 0 12px;font-size:13px;color:var(--muted);font-weight:600;letter-spacing:.5px}
  label{display:block;font-size:12px;color:var(--muted);margin:10px 0 4px}
  input[type=number],input[type=text]{width:100%;background:#0e1530;color:var(--text);border:1px solid var(--line);
        border-radius:8px;padding:8px 10px;font-size:14px;outline:none}
  input:focus{border-color:var(--accent)}
  .row{display:flex;gap:8px}
  .row>*{flex:1}
  .hint{font-size:11px;color:var(--muted);margin-top:4px}
  .seg{display:flex;gap:0;border:1px solid var(--line);border-radius:9px;overflow:hidden;margin-top:4px}
  .seg button{flex:1;background:#0e1530;color:var(--muted);border:none;padding:8px 0;font-size:13px;cursor:pointer}
  .seg button.on{background:#1e2a55;color:var(--text);font-weight:600}
  button.primary{width:100%;margin-top:16px;background:linear-gradient(90deg,var(--accent),#6aa1ff);
        color:#fff;border:none;border-radius:10px;padding:11px;font-size:15px;font-weight:700;cursor:pointer;transition:.15s}
  button.primary:hover{filter:brightness(1.1)}
  button.primary:disabled{opacity:.5;cursor:not-allowed}
  .quick{display:flex;flex-wrap:wrap;gap:7px;margin-top:14px}
  .quick button{background:#1e2a4d;color:var(--text);border:1px solid var(--line);
                font-weight:500;font-size:12.5px;padding:6px 11px;border-radius:8px;cursor:pointer}
  .quick button:hover{border-color:var(--accent);color:#fff}
  .result{min-height:240px}
  .nums{background:#0b1226;border:1px solid var(--line);border-radius:10px;padding:16px;min-height:120px;
        display:flex;flex-wrap:wrap;gap:8px;align-content:flex-start}
  .num{background:#22305c;border:1px solid #3a4c8f;border-radius:9px;padding:6px 12px;font-family:ui-monospace,Menlo,Consolas,monospace;
       font-size:18px;font-weight:700;color:var(--accent2);animation:pop .18s ease-out both}
  .num.redball{background:#5a1f2a;border-color:#a04a56;color:#ffb4bd}
  .num.blueball{background:#1c2f5e;border-color:#4a6fd0;color:#a9c0ff}
  .num.big{font-size:44px;padding:12px 22px;border-radius:14px}
  @keyframes pop{0%{transform:scale(.6);opacity:0}100%{transform:scale(1);opacity:1}}
  .empty{color:#5a6796;font-size:13px;align-self:center;margin:auto}
  .meta{color:var(--muted);font-size:12px;margin-top:10px}
  .meta b{color:var(--text)}
  .err{display:none;background:#2a1520;border:1px solid var(--danger);color:#ffb3bd;border-radius:8px;
        padding:8px 10px;font-size:12.5px;margin-top:10px}
  .ops{display:flex;gap:8px;margin-top:12px}
  button.op{background:transparent;border:1px solid var(--line);color:var(--text);border-radius:8px;padding:7px 12px;font-size:13px;cursor:pointer}
  button.op:hover{border-color:var(--accent)}
  button.op:disabled{opacity:.4;cursor:not-allowed}
  .histbox{background:var(--panel);border:1px solid var(--line);border-radius:14px;padding:12px 16px;margin:0 28px 40px;font-size:12.5px}
  .histbox h3{margin:0 0 8px;font-size:12px;color:var(--muted);font-weight:600}
  .histitem{display:flex;gap:10px;padding:3px 0;border-bottom:1px dashed var(--line);color:var(--muted)}
  .histitem:last-child{border-bottom:none}
  .histitem .idx{color:var(--accent);width:16px;flex-shrink:0}
  .histitem .nums2{color:var(--text);font-family:ui-monospace,Menlo,Consolas,monospace;word-break:break-all}
  footer{color:#5a6796;font-size:11.5px;text-align:center;padding-bottom:24px}
</style>
</head>
<body>

<header>
  <div style="flex:1;min-width:240px">
    <h1>随机选数器</h1>
    <div class="sub">闭区间 [min, max] 内随机抽取整数 · 熵源 random.SystemRandom (os.urandom) · 后端 FastAPI</div>
  </div>
  <div class="bar">
    <span class="chip">引擎 <b id="c-engine">-</b></span>
    <span class="chip">服务 <b id="c-health">-</b></span>
    <span class="chip">耗时 <b id="c-ms">-</b></span>
  </div>
</header>

<main>
  <section class="card">
    <h2>抽取配置</h2>
    <label>区间下界 / 上界</label>
    <div class="row">
      <input id="min" type="number" value="1" step="1">
      <input id="max" type="number" value="100" step="1">
    </div>
    <div class="hint">闭区间，两端都会包含；只接受整数。</div>

    <label>抽取个数（1 ~ 100）</label>
    <input id="count" type="number" value="5" min="1" max="100" step="1">

    <label>排除数字（可选，逗号分隔，超出区间自动忽略）</label>
    <input id="exclude" type="text" value="" placeholder="例如：3,7,13">

    <label>是否允许重复</label>
    <div class="seg" id="seg">
      <button type="button" data-unique="1" class="on">不重复（无放回）</button>
      <button type="button" data-unique="0">允许重复（有放回）</button>
    </div>

    <button class="primary" id="go">随机抽取</button>

    <label style="margin-top:18px">常用场景</label>
    <div class="quick">
      <button data-min="1" data-max="6" data-count="5" data-unique="0">掷 5 颗骰子</button>
      <button data-min="1" data-max="30" data-count="1" data-unique="1">点名 1~30</button>
      <button data-min="1" data-max="99" data-count="6" data-unique="1">抽 6 个不重复</button>
      <button id="dlb">双色球</button>
    </div>
  </section>

  <section class="card result">
    <h2>抽取结果</h2>
    <div class="nums" id="nums"><div class="empty">点击「随机抽取」或常用场景开始 ✨</div></div>
    <div class="meta" id="meta"></div>
    <div class="err" id="err"></div>
    <div class="ops">
      <button class="op" id="copy">复制结果</button>
      <button class="op" id="history">查看历史记录</button>
    </div>
  </section>
</main>

<div class="histbox" id="histbox" style="display:none">
  <h3>历史记录（本次运行）</h3>
  <div id="histlist"><div class="empty" style="color:#5a6796">暂无记录</div></div>
</div>

<footer>提示：关闭本窗口 / 按 Ctrl+C 即可退出服务。</footer>

<script>
"use strict";
const $ = id => document.getElementById(id);
let currentResult = null;      // {nums:[], meta:'', text:''}
let historyArr = [];           // [{label, text}]
let lastBallSet = null;        // 双色球：{reds:[], blue:null}

let engineEl = $("c-engine"), healthEl = $("c-health"), msEl = $("c-ms");

function showErr(msg){
  const e = $("err");
  if(msg){ e.style.display = "block"; e.textContent = "出错了：" + msg; }
  else e.style.display = "none";
}

function parseExclude(){
  const raw = $("exclude").value.trim();
  if(!raw) return [];
  const parts = raw.split(/[,，;；\s]+/).map(s=>s.trim()).filter(Boolean);
  const out = [];
  for(const p of parts){
    if(!/^-?\d+$/.test(p)){ showErr(`排除数字中的 "${p}" 不是整数`); return null; }
    out.push(parseInt(p,10));
  }
  return out;
}

function validateForm(){
  const min = parseInt($("min").value,10);
  const max = parseInt($("max").value,10);
  const cnt = parseInt($("count").value,10);
  if(!Number.isInteger(min) || $("min").value.trim()==="") { showErr("请填写区间下界 min（整数）"); return null; }
  if(!Number.isInteger(max) || $("max").value.trim()==="") { showErr("请填写区间上界 max（整数）"); return null; }
  if(!Number.isInteger(cnt) || $("count").value.trim()==="") { showErr("请填写抽取个数 count（1~100）"); return null; }
  if(min > max){ showErr(`下界 ${min} 不能大于上界 ${max}`); return null; }
  if(cnt < 1 || cnt > 100){ showErr(`count 需在 1 ~ 100 之间`); return null; }
  const unique = currentUnique === 1;
  return { min, max, count: cnt, unique };
}

let currentUnique = 1;
document.querySelectorAll("#seg button").forEach(btn=>{
  btn.onclick = () => {
    document.querySelectorAll("#seg button").forEach(b=>b.classList.remove("on"));
    btn.classList.add("on");
    currentUnique = parseInt(btn.dataset.unique,10);
  };
});

async function doPick(payload){
  showErr(null);
  $("go").disabled = true;
  const t0 = performance.now();
  try{
    const resp = await fetch("/api/v1/pick",{
      method:"POST",
      headers:{"Content-Type":"application/json"},
      body: JSON.stringify(payload)
    });
    const data = await resp.json();
    const ms = Math.round(performance.now()-t0);
    msEl.textContent = ms + " ms";
    if(!resp.ok || data.ok !== true){
      showErr(data.detail || `HTTP ${resp.status}`);
      return null;
    }
    return data;
  }catch(e){
    showErr("请求失败：" + e.message);
    return null;
  }finally{
    $("go").disabled = false;
  }
}

function renderNums(arr, cls){
  const box = $("nums");
  box.innerHTML = "";
  if(!arr || !arr.length){
    box.innerHTML = '<div class="empty">（无结果）</div>';
    return;
  }
  // 单个数放大展示
  if(arr.length === 1){
    const d = document.createElement("div");
    d.className = "num big" + (cls?" "+cls:"");
    d.textContent = arr[0];
    box.appendChild(d);
    return;
  }
  arr.forEach((n,i)=>{
    const d = document.createElement("div");
    d.className = "num" + (cls?" "+cls:"");
    d.textContent = n;
    d.style.animationDelay = (i*12)+"ms";
    box.appendChild(d);
  });
}

function pushHistory(label, text){
  historyArr.unshift({label, text});
  if(historyArr.length > 50) historyArr.pop();
}

function paintResult(data){
  const { min, max, count, unique, exclude, result, elapsed_ms } = data;
  renderNums(result);
  currentResult = {
    nums: result,
    meta: `区间 [${min}, ${max}] · ${count} 个 · ${unique?"不重复":"允许重复"}` + (exclude.length? ` · 排除 {${exclude.join(",")}}`:"") + ` · 服务端 ${elapsed_ms}ms`,
    text: result.join(", ")
  };
  $("meta").textContent = currentResult.meta;
  pushHistory(currentResult.meta, currentResult.text);
}

async function run(payload, label){
  const data = await doPick(payload);
  if(!data) return;
  paintResult(data);
  renderHistory();
  if(label && historyArr.length) historyArr[0].label = label; // 覆盖最新一条说明文字
}

// 主按钮
$("go").onclick = async () => {
  const p = validateForm();
  if(!p) return;
  const ex = parseExclude();
  if(ex === null) return;
  p.exclude = ex;
  await run(p, "");
};

// 常用场景
document.querySelectorAll(".quick button[data-min]").forEach(b=>{
  b.onclick = async () => {
    const payload = {
      min: parseInt(b.dataset.min,10),
      max: parseInt(b.dataset.max,10),
      count: parseInt(b.dataset.count,10),
      unique: b.dataset.unique === "1",
      exclude: []
    };
    // 让表单与场景保持一致
    $("min").value = payload.min;
    $("max").value = payload.max;
    $("count").value = payload.count;
    $("exclude").value = "";
    document.querySelectorAll("#seg button").forEach(x=>{
      x.classList.toggle("on", parseInt(x.dataset.unique,10) === (payload.unique?1:0));
    });
    currentUnique = payload.unique ? 1 : 0;
    await run(payload, b.textContent);
  };
});

// 双色球：红球 1~33 取 6（不重复）+ 蓝球 1~16 取 1
$("dlb").onclick = async () => {
  showErr(null);
  $("go").disabled = true;
  try{
    const [red, blue] = await Promise.all([
      fetch("/api/v1/pick",{method:"POST",headers:{"Content-Type":"application/json"},
        body:JSON.stringify({min:1,max:33,count:6,unique:true,exclude:[]})}),
      fetch("/api/v1/pick",{method:"POST",headers:{"Content-Type":"application/json"},
        body:JSON.stringify({min:1,max:16,count:1,unique:true,exclude:[]})})
    ]);
    const rd = await red.json(), bd = await blue.json();
    msEl.textContent = "-";
    if(!red.ok || !rd.ok) { showErr(rd.detail||"红球抽取失败"); return; }
    if(!blue.ok || !bd.ok) { showErr(bd.detail||"蓝球抽取失败"); return; }
    const reds = [...rd.result].sort((a,b)=>a-b);
    const blueN = bd.result[0];
    lastBallSet = {reds, blue: blueN};
    renderNums(reds, "redball");
    // 追加蓝球
    const box = $("nums");
    const sep = document.createElement("div");
    sep.style.cssText = "align-self:center;color:#8b98c4;font-weight:700;";
    sep.textContent = "+";
    const bnum = document.createElement("div");
    bnum.className = "num blueball big";
    bnum.textContent = String(blueN).padStart(2,"0");
    box.appendChild(sep); box.appendChild(bnum);
    const meta = "双色球 · 红球 6 个（1~33，不重复） + 蓝球 1 个（1~16）";
    const text = "红球 " + reds.map(n=>String(n).padStart(2,"0")).join(" ") + " + 蓝球 " + String(blueN).padStart(2,"0");
    currentResult = { nums: reds.concat(blueN), meta, text };
    $("meta").textContent = meta;
    pushHistory(meta, text);
    renderHistory();
  }catch(e){ showErr("双色球请求失败："+e.message); }
  finally{ $("go").disabled = false; }
};

// 复制
$("copy").onclick = async () => {
  if(!currentResult){ showErr("还没有结果可复制"); return; }
  try{ await navigator.clipboard.writeText(currentResult.text); }
  catch(e){
    // 回退方案
    const ta = document.createElement("textarea");
    ta.value = currentResult.text; document.body.appendChild(ta);
    ta.select(); document.execCommand("copy"); ta.remove();
  }
  showErr(null);
  const b = $("copy"); const old = b.textContent;
  b.textContent = "已复制 ✓"; b.disabled = true;
  setTimeout(()=>{ b.textContent = old; b.disabled = false; }, 1200);
};

// 历史记录面板
function renderHistory(){
  const list = $("histlist");
  if(!historyArr.length){
    list.innerHTML = '<div class="empty" style="color:#5a6796">暂无记录</div>';
    return;
  }
  list.innerHTML = "";
  historyArr.forEach((h,i)=>{
    const d = document.createElement("div");
    d.className = "histitem";
    const idx = document.createElement("span"); idx.className="idx"; idx.textContent = (i+1)+".";
    const n = document.createElement("span"); n.className="nums2"; n.textContent = h.text;
    d.appendChild(idx); d.appendChild(n);
    d.title = h.label;
    list.appendChild(d);
  });
}
$("history").onclick = () => {
  const box = $("histbox");
  box.style.display = box.style.display === "none" ? "block" : "none";
  renderHistory();
};

// 健康检查 & 引擎信息
async function refreshHealth(){
  try{
    const r = await fetch("/api/v1/health");
    const h = await r.json();
    engineEl.textContent = h.engine || "SystemRandom";
    healthEl.textContent = h.status === "healthy" ? "正常" : "异常";
  }catch(e){
    healthEl.textContent = "离线";
    engineEl.textContent = "-";
  }
}
refreshHealth();
</script>
</body>
</html>
"""

# ============================================================ 路由挂载
# main.py 已在 "/" 注册了 JSON 服务索引；这里把 UI 页面插到路由最前，
# 使访问根路径 "/" 直接打开界面，其余 API(/docs、/api/v1/...) 保持不变。


async def _ui_index(request):
    return HTMLResponse(UI_HTML)


app.router.routes.insert(0, Route("/", _ui_index, methods=["GET"]))

# ============================================================ 启动逻辑


def _find_free_port() -> int:
    with socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _open_browser_after_delay(url: str) -> None:
    """等服务真正就绪后再打开浏览器，避免页面抢先加载失败。"""
    for _ in range(50):  # 最多等 10 秒（单文件 exe 冷启动较慢）
        try:
            with urllib.request.urlopen(url + "api/v1/health", timeout=1):
                break
        except Exception:
            time.sleep(0.2)
    try:
        webbrowser.open(url)
    except Exception as exc:  # pragma: no cover - 打不开浏览器也不致命
        print(f"[警告] 自动打开浏览器失败：{exc}")


def main() -> None:
    no_browser = "--no-browser" in sys.argv
    port = _find_free_port()  # 默认自动选空闲端口，避免 8000 被占用导致启动失败
    if "--port" in sys.argv:
        try:
            port = int(sys.argv[sys.argv.index("--port") + 1])
        except (IndexError, ValueError):
            port = 0
    if port == 0:
        port = _find_free_port()

    url = f"http://127.0.0.1:{port}/"
    if not no_browser:
        threading.Thread(target=_open_browser_after_delay, args=(url,), daemon=True).start()

    print("=" * 56)
    print("  随机选数器 · 桌面版")
    print(f"  服务地址：{url}")
    print("  界面已{}自动打开浏览器，手动访问上面地址亦可。".format("触发，" if not no_browser else "跳过，"))
    print("  按 Ctrl+C 或关闭本窗口退出。")
    print("=" * 56)

    try:
        uvicorn.run(app, host="127.0.0.1", port=port, log_level="info")
    except KeyboardInterrupt:
        print("\n已退出，再见。")


if __name__ == "__main__":
    main()
