//! 随机选数核心逻辑：完整移植 Python 版 `main.py` 的
//! `pick_ints` / 校验 / 排除解析 行为与文案。
//!
//! 语义：
//! - 从闭区间 [min, max] 中抽取 count 个整数；
//! - unique=true 无放回（结果互不重复）；false 有放回（允许重复）；
//! - exclude 中超出区间的自动忽略，重复项去重；
//! - 区间宽度 <= SPAN_MATERIALIZE_LIMIT 时构造候选池（严格正确）；
//!   更大区间走拒绝采样，避免占用巨量内存。
//! - 所有 span 计算用 i128，避免 i64 中间溢出。

use std::collections::HashSet;

use crate::rng;

pub const MAX_COUNT: u32 = 100;
pub const SPAN_MATERIALIZE_LIMIT: i128 = 200_000;

/// 规范化排除数：仅保留 [min,max] 区间内、去重并排序（用于回显 meta）。
pub fn normalize_exclude(min: i64, max: i64, exclude: &[i64]) -> Vec<i64> {
    let mut set: HashSet<i64> = HashSet::new();
    for &e in exclude {
        if min <= e && e <= max {
            set.insert(e);
        }
    }
    let mut list: Vec<i64> = set.into_iter().collect();
    list.sort_unstable();
    list
}

/// 解析一段整数文本（对齐网页版 parseExclude）。
///
/// 分隔符：英文逗号 / 中文逗号 / 分号 / 空白（可混用）。
/// 每个 token 必须是 `-?\d+`，否则返回错误信息（`what` 用于文案，
/// 例如 "排除数字" / "候选列表"）；
/// 数值超出 i64 可表示范围时按「落在区间外 → 自动忽略」处理，直接丢弃。
pub fn parse_int_list(text: &str, what: &str) -> Result<Vec<i64>, String> {
    let mut out: Vec<i64> = Vec::new();
    let toks: Vec<&str> = text
        .split(|c: char| c == ',' || c == '，' || c == ';' || c == '；' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    for t in toks {
        if !(t.starts_with('-')
            && t.len() > 1
            && t[1..].chars().all(|c| c.is_ascii_digit()))
            && !(t.chars().all(|c| c.is_ascii_digit()) && !t.is_empty())
        {
            return Err(format!("{what}中的 \"{t}\" 不是整数"));
        }
        // 已确认形如 -?\d+；用 i128 解析防止溢出 panic
        let big: i128 = t
            .parse()
            .map_err(|_| format!("{what}中的 \"{t}\" 不是整数"))?;
        if (i64::MIN as i128) <= big && big <= (i64::MAX as i128) {
            out.push(big as i64);
        }
        // 超出 i64 范围 → 必然在 [min,max] 之外 → 自动忽略（与网页行为一致）
    }
    Ok(out)
}

/// 解析排除输入框文本（现有行为/文案保持不变）。
pub fn parse_exclude_text(text: &str) -> Result<Vec<i64>, String> {
    parse_int_list(text, "排除数字")
}

fn check_count(count: u32) -> Result<(), String> {
    if !(1..=MAX_COUNT).contains(&count) {
        return Err(format!("count 需在 1 ~ {MAX_COUNT} 之间，当前为 {count}"));
    }
    Ok(())
}

fn check_range(minimum: i64, maximum: i64) -> Result<(), String> {
    if minimum > maximum {
        return Err(format!("min({minimum}) 不能大于 max({maximum})"));
    }
    Ok(())
}

/// 从 [minimum, maximum] 闭区间随机抽取 count 个整数。
pub fn pick_ints(
    minimum: i64,
    maximum: i64,
    count: u32,
    exclude: &[i64],
    unique: bool,
) -> Result<Vec<i64>, String> {
    check_range(minimum, maximum)?;
    check_count(count)?;

    let ex_set: HashSet<i64> = normalize_exclude(minimum, maximum, exclude).into_iter().collect();
    // span 用 u128 计算，避免 i64 溢出（例：min=i64::MIN, max=i64::MAX）
    let span: i128 = (maximum as i128) - (minimum as i128) + 1;
    let available: i128 = span - ex_set.len() as i128;
    if available <= 0 {
        return Err("排除后 [min, max] 区间内已无可选数字，请调整区间或排除数".to_string());
    }
    if unique && (count as i128) > available {
        return Err(format!(
            "不重复抽取要求 count({count}) ≤ 可选数字数量({available})，\
             请增大区间、减少排除数，或将 unique 设为 false"
        ));
    }

    // —— 区间宽度适中：直接构造候选池 ——
    if span <= SPAN_MATERIALIZE_LIMIT {
        let mut pool: Vec<i64> = Vec::with_capacity(span as usize);
        for off in 0..span {
            let v = (minimum as i128 + off) as i64;
            if !ex_set.contains(&v) {
                pool.push(v);
            }
        }
        if unique {
            // 部分 Fisher–Yates：只需洗前 count 个
            let n = pool.len();
            for i in 0..(count as usize).min(n) {
                let j = i + rng::index_in(n - i);
                pool.swap(i, j);
            }
            Ok(pool[..(count as usize).min(n)].to_vec())
        } else {
            // 有放回：均匀抽样，排除集合内重抽
            let mut result = Vec::with_capacity(count as usize);
            while result.len() < count as usize {
                let off = rng::uniform_u64(0, span as u64 - 1);
                let v = (minimum as i128 + off as i128) as i64;
                if !ex_set.contains(&v) {
                    result.push(v);
                }
            }
            Ok(result)
        }
    } else {
        // —— 超大区间：拒绝采样，避免构造巨型列表 ——
        if unique {
            let mut picked: Vec<i64> = Vec::with_capacity(count as usize);
            let mut seen: HashSet<i64> = HashSet::with_capacity(count as usize * 2);
            while picked.len() < count as usize {
                let v = rng::i64_in(minimum, maximum);
                if ex_set.contains(&v) || seen.contains(&v) {
                    continue;
                }
                seen.insert(v);
                picked.push(v);
            }
            Ok(picked)
        } else {
            let mut result = Vec::with_capacity(count as usize);
            for _ in 0..count {
                loop {
                    let v = rng::i64_in(minimum, maximum);
                    if !ex_set.contains(&v) {
                        result.push(v);
                        break;
                    }
                }
            }
            Ok(result)
        }
    }
}

/// 从「用户指定的数字列表」中随机抽取 count 个（值优先语义）。
///
/// 语义：
/// - 候选列表先按值去重（重复写法只算一个），保留首次出现顺序；
/// - exclude 按值排除，去掉所有匹配项；
/// - unique=true 无放回（结果值互不相同）；false 有放回（可重复）；
/// - 随机性作用在**索引**上：索引与值一一对应，映射是确定性的，
///   因此不引入任何偏差，等概率等价于直接对原值抽样。
pub fn pick_from_list(
    items: &[i64],
    count: u32,
    exclude: &[i64],
    unique: bool,
) -> Result<Vec<i64>, String> {
    check_count(count)?;

    // 按值去重（保持首次出现顺序）
    let mut seen: HashSet<i64> = HashSet::with_capacity(items.len());
    let mut pool: Vec<i64> = Vec::with_capacity(items.len());
    for &v in items {
        if seen.insert(v) {
            pool.push(v);
        }
    }
    if pool.is_empty() {
        return Err("候选列表为空，请填写要抽取的数字".to_string());
    }

    if !exclude.is_empty() {
        let ex: HashSet<i64> = exclude.iter().copied().collect();
        pool.retain(|v| !ex.contains(v));
    }
    if pool.is_empty() {
        return Err("候选列表去重并排除后已无可选数字，请调整列表或排除数".to_string());
    }

    let n = pool.len();
    if unique && (count as usize) > n {
        return Err(format!(
            "不重复抽取要求 count({count}) ≤ 可选数字数量({n})，\
             请减少抽取个数、减少排除数，或将 unique 设为 false"
        ));
    }

    if unique {
        // 部分 Fisher–Yates：只需洗前 count 个
        let k = (count as usize).min(n);
        for i in 0..k {
            let j = i + rng::index_in(n - i);
            pool.swap(i, j);
        }
        Ok(pool[..k].to_vec())
    } else {
        // 有放回：均匀抽索引（index_in 已无模偏）
        let mut out = Vec::with_capacity(count as usize);
        for _ in 0..count {
            out.push(pool[rng::index_in(n)]);
        }
        Ok(out)
    }
}
