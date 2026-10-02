use std::collections::VecDeque;
use std::sync::Mutex;

pub struct Queue {
    q: Mutex<VecDeque<String>>,
}

impl Queue {
    pub const fn new() -> Queue {
        Queue { q: Mutex::new(VecDeque::new()) }
    }

    pub fn push(&self, s: impl Into<String>) {
        if let Ok(mut q) = self.q.lock() {
            q.push_back(s.into());
        }
    }

    pub fn pop(&self) -> Option<String> {
        self.q.lock().ok()?.pop_front()
    }

    pub fn drain(&self) -> Vec<String> {
        self.q.lock().map_or_else(|_| Vec::new(), |mut q| q.drain(..).collect())
    }

    pub fn len(&self) -> usize {
        self.q.lock().map_or(0, |q| q.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for Queue {
    fn default() -> Self {
        Queue::new()
    }
}

#[cfg(test)]
mod t {
    use super::*;

    static Q: Queue = Queue::new();

    #[test]
    fn fifo() {
        Q.push("a");
        Q.push(String::from("b"));
        assert_eq!(Q.len(), 2);
        assert_eq!(Q.pop().as_deref(), Some("a"));
        assert_eq!(Q.drain(), ["b"]);
        assert!(Q.is_empty() && Q.pop().is_none());
    }
}
