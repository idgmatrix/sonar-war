//! 버퍼 길이 기준 순환 읽기. usize 비트 폭에 의존하지 않는다.
#[inline]
pub(crate) fn read(history: &[f32], next: usize, offset: f32) -> f32 {
    let capacity = history.len();
    let delay = offset.max(0.0).min(capacity as f32 - 2.0);
    let whole = delay.floor() as usize;
    let fraction = delay - whole as f32;
    let newest = if next == 0 { capacity - 1 } else { next - 1 };
    let first = (newest + capacity - whole) % capacity;
    let second = (first + capacity - 1) % capacity;
    history[first] * (1.0 - fraction) + history[second] * fraction
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ramp_is_continuous_across_multiple_wraps() {
        for capacity in [250, 264, 272, 286] {
            let mut history = vec![0.0; capacity];
            let mut next = 0;
            for n in 0..capacity * 5 {
                history[next] = n as f32;
                next = (next + 1) % capacity;
                if n >= capacity {
                    for delay in [0.0, 1.0, 123.48, capacity as f32 - 2.0] {
                        assert!((read(&history, next, delay) - (n as f32 - delay)).abs() < 0.001);
                    }
                }
            }
        }
    }
}
