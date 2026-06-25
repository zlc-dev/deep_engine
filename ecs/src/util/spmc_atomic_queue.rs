// ── MPMC 无锁队列封装 ──────────────────────────────────────
//
// 用成熟的 crossbeam_queue::SegQueue 替代手写 Treiber stack，避免 ABA 和
// 无锁内存回收问题。当前 ID 池只需要一个并发 free-list。

use crossbeam_queue::SegQueue;

pub struct SpmcAtomicQueue<T> {
    queue: SegQueue<T>,
}

impl<T> SpmcAtomicQueue<T> {
    pub const fn new() -> Self {
        Self {
            queue: SegQueue::new(),
        }
    }

    pub fn push(&self, value: T) {
        self.queue.push(value);
    }

    pub fn pop(&self) -> Option<T> {
        self.queue.pop()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl<T> Default for SpmcAtomicQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::SpmcAtomicQueue;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn pop_returns_none_for_empty_stack() {
        let stack = SpmcAtomicQueue::<usize>::new();

        assert_eq!(stack.pop(), None);
        assert!(stack.is_empty());
    }

    #[test]
    fn push_and_pop_return_all_values() {
        let stack = SpmcAtomicQueue::new();

        stack.push(1);
        stack.push(2);
        stack.push(3);

        let mut values = [stack.pop(), stack.pop(), stack.pop()];
        values.sort();

        assert_eq!(values, [Some(1), Some(2), Some(3)]);
        assert_eq!(stack.pop(), None);
        assert!(stack.is_empty());
    }

    #[test]
    fn multiple_consumers_pop_each_value_once() {
        const VALUES: usize = 10_000;
        const CONSUMERS: usize = 8;

        let stack = Arc::new(SpmcAtomicQueue::new());
        for value in 0..VALUES {
            stack.push(value);
        }

        let popped = Arc::new(Mutex::new(Vec::with_capacity(VALUES)));
        let count = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::with_capacity(CONSUMERS);

        for _ in 0..CONSUMERS {
            let stack = Arc::clone(&stack);
            let popped = Arc::clone(&popped);
            let count = Arc::clone(&count);

            handles.push(thread::spawn(move || {
                loop {
                    match stack.pop() {
                        Some(value) => {
                            popped.lock().unwrap().push(value);
                            count.fetch_add(1, Ordering::Relaxed);
                        }
                        None if count.load(Ordering::Acquire) >= VALUES => break,
                        None => thread::yield_now(),
                    }
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let popped = popped.lock().unwrap();
        let unique = popped.iter().copied().collect::<HashSet<_>>();

        assert_eq!(popped.len(), VALUES);
        assert_eq!(unique.len(), VALUES);
        assert!((0..VALUES).all(|value| unique.contains(&value)));
        assert!(stack.is_empty());
    }
}
