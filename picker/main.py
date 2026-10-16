# -*- coding: utf-8 -*-
"""
随机选数后端 —— 在用户自定义区间内随机抽取整数，数据类型一律为 int。

功能说明
========
1. 从闭区间 [min, max] 中随机抽取 count 个整数；
2. 用户可以自定义：
   - 选择区间：min、max（必填，禁止空值）
   - 抽取个数：count（必填，1 ~ 100，禁止空值）
   - 排除数字：exclude（可选，这些数字不会出现在结果中）
   - 是否允许重复：unique（默认 true = 不重复 / 无放回；false = 允许重复 / 有放回）
3. 熵源为 random.SystemRandom（os.urandom，操作系统 CSPRNG），结果不可预测。

输入约束（重要）
================
- min / max / count 必填，禁止空值：缺失、null、空字符串一律拒绝（返回 422）；
- count 必须为 1 ~ 100 的整数；
- min 不能大于 max；
- unique=true 时，count 不能超过「区间内排除后剩余」的数字个数；
- exclude 中的数字若超出 [min, max]，会被自动忽略；重复项自动去重；
- 全部入参必须是整数（int），不接受小数。

启动方式
========
    pip install -r requirements.txt
    python main.py          # 直接运行，监听 0.0.0.0:8000
    # 或
    uvicorn main:app --host 0.0.0.0 --port 8000 --reload   # 开发热重载

在线文档：http://127.0.0.1:8000/docs
"""

from __future__ import annotations

import random
import time

from fastapi import FastAPI, HTTPException, Query
from pydantic import BaseModel, Field

# 熵源单例：所有随机性都来自操作系统 CSPRNG
RNG = random.SystemRandom()

MAX_COUNT = 100  # 单次最多抽取 100 个数（对应需求「1 ~ 100 个数」）
# 区间宽度小于该值时直接构造候选数字池（保证唯一性严格正确）；
# 更大的区间（例如用户填了上亿的跨度）走拒绝采样，避免一次性占用巨量内存。
SPAN_MATERIALIZE_LIMIT = 200_000

app = FastAPI(
    title="随机选数后端",
    version="1.0.0",
    description=(
        "在用户自定义闭区间 [min, max] 中随机抽取 count 个整数（int）；"
        "可排除指定数字，可选允许/不允许重复。"
        "熵源：random.SystemRandom（os.urandom）。"
    ),
)


class RngError(ValueError):
    """业务参数错误：统一转为 HTTP 400。"""


# ============================================================ 核心逻辑


def _check_range(minimum: int, maximum: int) -> None:
    if minimum > maximum:
        raise RngError(f"min({minimum}) 不能大于 max({maximum})")


def _check_count(count: int) -> None:
    if not 1 <= count <= MAX_COUNT:
        raise RngError(f"count 需在 1 ~ {MAX_COUNT} 之间，当前为 {count}")


def _normalize_exclude(
    minimum: int, maximum: int, exclude: list[int] | None
) -> tuple[set[int], list[int]]:
    """规范化排除数：只保留区间内的值并去重。

    返回 (排除数集合, 区间内有效排除数列表)。
    """
    if not exclude:
        return set(), []
    in_range = {int(e) for e in exclude if minimum <= int(e) <= maximum}
    return in_range, sorted(in_range)


def pick_ints(
    minimum: int,
    maximum: int,
    count: int,
    exclude: list[int] | None = None,
    unique: bool = True,
) -> list[int]:
    """从 [minimum, maximum] 闭区间中随机抽取 count 个整数。

    - unique=True：无放回抽取，结果互不重复；
    - unique=False：有放回抽取，结果允许重复；
    - exclude 中的数字（区间内）不会出现在结果中。
    """
    _check_range(minimum, maximum)
    _check_count(count)

    ex_set, ex_in_range = _normalize_exclude(minimum, maximum, exclude)
    span = maximum - minimum + 1
    available = span - len(ex_set)
    if available <= 0:
        raise RngError("排除后 [min, max] 区间内已无可选数字，请调整区间或排除数")
    if unique and count > available:
        raise RngError(
            f"不重复抽取要求 count({count}) ≤ 可选数字数量({available})，"
            "请增大区间、减少排除数，或将 unique 设为 false"
        )

    # —— 区间宽度适中：直接构造候选池，用标准库保证严格正确 ——
    if span <= SPAN_MATERIALIZE_LIMIT:
        pool = [v for v in range(minimum, maximum + 1) if v not in ex_set]
        if unique:
            return RNG.sample(pool, count)  # 无放回：结果互不相同
        return RNG.choices(pool, k=count)  # 有放回：结果允许重复

    # —— 超大区间：排除数相对很少，用拒绝采样，避免构造巨型列表 ——
    if unique:
        picked: list[int] = []
        seen: set[int] = set()
        while len(picked) < count:
            v = RNG.randint(minimum, maximum)
            if v in ex_set or v in seen:
                continue
            seen.add(v)
            picked.append(v)
        return picked

    result: list[int] = []
    for _ in range(count):
        while True:
            v = RNG.randint(minimum, maximum)
            if v not in ex_set:
                break
        result.append(v)
    return result


# ============================================================ 请求模型


class PickRequest(BaseModel):
    """POST /api/v1/pick 请求体。min / max / count 必填，禁止空值。"""

    min: int = Field(..., description="区间下界（必填，禁止空值）")
    max: int = Field(..., description="区间上界（必填，禁止空值）")
    count: int = Field(..., description="抽取个数（必填，1 ~ 100）")
    exclude: list[int] = Field(
        default_factory=list, description="要排除的数字（可空，超出区间会被忽略）"
    )
    unique: bool = Field(True, description="true=不重复（默认）；false=允许重复")


def _build_response(
    minimum: int,
    maximum: int,
    count: int,
    exclude_in: list[int],
    unique: bool,
    result: list[int],
    t0: float,
) -> dict:
    return {
        "ok": True,
        "type": "pick",
        "count": len(result),
        "min": minimum,
        "max": maximum,
        "unique": unique,
        "exclude": exclude_in,  # 已去重、且落在区间内的排除数
        "result": result,  # 全部为 int
        "elapsed_ms": round((time.perf_counter() - t0) * 1000, 3),
    }


def _run_pick(minimum: int, maximum: int, count: int, exclude: list[int], unique: bool) -> dict:
    t0 = time.perf_counter()
    try:
        result = pick_ints(minimum, maximum, count, exclude, unique)
    except RngError as exc:
        raise HTTPException(status_code=400, detail=str(exc))
    _, ex_in_range = _normalize_exclude(minimum, maximum, exclude)
    return _build_response(minimum, maximum, count, ex_in_range, unique, result, t0)


# ============================================================ 接口


@app.get("/", include_in_schema=False)
async def index():
    return {
        "service": "随机选数后端",
        "docs": "/docs",
        "redoc": "/redoc",
        "health": "/api/v1/health",
        "pick": "/api/v1/pick",
    }


@app.get("/api/v1/health", tags=["系统"], summary="健康检查")
async def health():
    return {"status": "healthy", "engine": "random.SystemRandom", "ts": time.time()}


@app.post("/api/v1/pick", tags=["抽取"], summary="随机选整数（POST，参数走 JSON 请求体）")
async def post_pick(body: PickRequest):
    """在 [min, max] 区间内随机抽取 count 个整数。

    请求体示例：
        {"min": 1, "max": 100, "count": 5}
        {"min": 1, "max": 10, "count": 10, "exclude": [2, 4, 6], "unique": true}
        {"min": 1, "max": 6, "count": 100, "unique": false}
    """
    return _run_pick(body.min, body.max, body.count, body.exclude, body.unique)


def _parse_exclude(raw: list[str] | None) -> list[int]:
    """把 GET 的 exclude 参数解析为整数列表。

    支持两种写法，也可混用：
        ?exclude=3,5,7            （英文逗号分隔）
        ?exclude=3&exclude=5&exclude=7   （重复传参）
    """
    if not raw:
        return []
    values: list[int] = []
    for chunk in raw:
        for part in chunk.replace("，", ",").split(","):
            part = part.strip()
            if not part:
                continue
            try:
                values.append(int(part))
            except ValueError:
                raise HTTPException(status_code=422, detail=f"exclude 中的 '{part}' 不是整数")
    return values


@app.get("/api/v1/pick", tags=["抽取"], summary="随机选整数（GET，参数走查询字符串）")
async def get_pick(
    min: int = Query(..., description="区间下界（必填，禁止空值）"),
    max: int = Query(..., description="区间上界（必填，禁止空值）"),
    count: int = Query(..., description="抽取个数（必填，1 ~ 100）"),
    exclude: list[str] | None = Query(
        None, description="要排除的数字：逗号分隔（exclude=3,5）或重复传参（exclude=3&exclude=5）"
    ),
    unique: bool = Query(True, description="true=不重复（默认）；false=允许重复"),
):
    """GET 便捷形式，例如：

        /api/v1/pick?min=1&max=100&count=10
        /api/v1/pick?min=1&max=100&count=10&exclude=3,7,13
        /api/v1/pick?min=1&max=6&count=20&unique=false
    """
    return _run_pick(min, max, count, _parse_exclude(exclude), unique)


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host="0.0.0.0", port=8000)
