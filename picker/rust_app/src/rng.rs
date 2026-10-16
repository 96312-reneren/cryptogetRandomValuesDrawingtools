//! 系统级 CSPRNG（对应 Python 侧 random.SystemRandom / os.urandom）。
//!
//! 所有随机性都来自 Windows BCryptGenRandom（操作系统 CSPRNG），
//! 不可复现、不可预测；本模块不提供任何 seed 接口。

use windows::Win32::Foundation::NTSTATUS;
use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_ALG_HANDLE, BCRYPTGENRANDOM_FLAGS};

/// BCRYPT_RNG_USE_SYSTEM_PREFERRED_RNG：使用系统首选 RNG（即 CSPRNG）。
const RNG_USE_SYSTEM_PREFERRED: u32 = 0x2;

/// 向操作系统取 `buf.len()` 字节随机数（CSPRNG）。
pub fn fill_bytes(buf: &mut [u8]) {
    if buf.is_empty() {
        return;
    }
    let status: NTSTATUS = unsafe {
        BCryptGenRandom(
            BCRYPT_ALG_HANDLE::default(), // NULL 句柄 + SYSTEM_PREFERRED 标志即可
            buf,
            BCRYPTGENRANDOM_FLAGS(RNG_USE_SYSTEM_PREFERRED),
        )
    };
    // NT_SUCCESS(status) == status >= 0
    if status.0 < 0 {
        panic!("BCryptGenRandom failed with status {:#x}", status.0);
    }
}

/// 取一个 64 位无符号随机数（均匀）。
#[inline]
pub fn rand_u64() -> u64 {
    let mut b = [0u8; 8];
    fill_bytes(&mut b);
    u64::from_le_bytes(b)
}

/// 在闭区间 [lo, hi]（hi >= lo）内均匀抽取一个 u64，无模偏差。
///
/// 采用标准拒绝采样：阈值 t = (2^64 mod range)，
/// 丢弃落在 [0, t) 的样本，其余对 range 取模，保证均匀。
pub fn uniform_u64(lo: u64, hi: u64) -> u64 {
    debug_assert!(lo <= hi);
    if lo == hi {
        return lo;
    }
    let range = hi - lo; // 取值范围个数 = range + 1
    if range == u64::MAX {
        // [lo, hi] 覆盖整个 u64 空间
        return rand_u64();
    }
    let range = range + 1;
    // 2^64 mod range（在 u64 环上计算：(-range) mod range）
    let threshold = range.wrapping_neg() % range;
    loop {
        let x = rand_u64();
        if x >= threshold {
            return lo + (x % range);
        }
    }
}

/// 在闭区间 [lo, hi]（i64，hi >= lo）内均匀抽取一个 i64。
pub fn i64_in(lo: i64, hi: i64) -> i64 {
    debug_assert!(lo <= hi);
    // 用无符号做区间定位：value = lo + uniform[0, hi-lo]
    let offset = uniform_u64(0, (hi as u64).wrapping_sub(lo as u64));
    // lo + offset 必然落在 [lo, hi]，用 i128 中转避免中间溢出，再安全截断。
    (lo as i128 + offset as i128) as i64
}

/// 洗牌辅助：在 [0, n) 中均匀抽取下标。
pub fn index_in(n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    uniform_u64(0, n as u64 - 1) as usize
}
