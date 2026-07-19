use std::fmt;
use std::iter::FusedIterator;

pub trait IterTools: Iterator + Sized {
    /// 以 self 为基准的 zip — 左边始终有值，右边耗尽后填 None。
    fn zip_left<Iter: Iterator>(self, iter: Iter) -> ZipLeft<Self, Iter> {
        ZipLeft {
            left: self,
            right: iter,
        }
    }

    /// 以 other 为基准的 zip — 右边始终有值，左边耗尽后填 None。
    fn zip_right<Iter: Iterator>(self, iter: Iter) -> ZipRight<Self, Iter> {
        ZipRight {
            left: self,
            right: iter,
        }
    }

    /// 两边都耗尽后停止 — 超出的填 None。
    fn zip_longest<Iter: Iterator>(self, iter: Iter) -> ZipLongest<Self, Iter> {
        ZipLongest {
            left: self,
            right: iter,
        }
    }
}

impl<I: Iterator> IterTools for I {}

// ── ZipLeft ──────────────────────────────────────────────

pub struct ZipLeft<L, R> {
    left: L,
    right: R,
}

impl<L: Clone, R: Clone> Clone for ZipLeft<L, R> {
    fn clone(&self) -> Self {
        Self {
            left: self.left.clone(),
            right: self.right.clone(),
        }
    }
}

impl<L: fmt::Debug, R: fmt::Debug> fmt::Debug for ZipLeft<L, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZipLeft")
            .field("left", &self.left)
            .field("right", &self.right)
            .finish()
    }
}

impl<L: Iterator, R: Iterator> Iterator for ZipLeft<L, R> {
    type Item = (L::Item, Option<R::Item>);

    fn next(&mut self) -> Option<Self::Item> {
        let left = self.left.next()?;
        let right = self.right.next();
        Some((left, right))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.left.size_hint()
    }
}

impl<L: ExactSizeIterator, R: Iterator> ExactSizeIterator for ZipLeft<L, R> {
    fn len(&self) -> usize {
        self.left.len()
    }
}

impl<L: FusedIterator, R: Iterator> FusedIterator for ZipLeft<L, R> {}

// ── ZipRight ─────────────────────────────────────────────

pub struct ZipRight<L, R> {
    left: L,
    right: R,
}

impl<L: Clone, R: Clone> Clone for ZipRight<L, R> {
    fn clone(&self) -> Self {
        Self {
            left: self.left.clone(),
            right: self.right.clone(),
        }
    }
}

impl<L: fmt::Debug, R: fmt::Debug> fmt::Debug for ZipRight<L, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZipRight")
            .field("left", &self.left)
            .field("right", &self.right)
            .finish()
    }
}

impl<L: Iterator, R: Iterator> Iterator for ZipRight<L, R> {
    type Item = (Option<L::Item>, R::Item);

    fn next(&mut self) -> Option<Self::Item> {
        let right = self.right.next()?;
        let left = self.left.next();
        Some((left, right))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.right.size_hint()
    }
}

impl<L: Iterator, R: ExactSizeIterator> ExactSizeIterator for ZipRight<L, R> {
    fn len(&self) -> usize {
        self.right.len()
    }
}

impl<L: Iterator, R: FusedIterator> FusedIterator for ZipRight<L, R> {}

// ── ZipLongest ───────────────────────────────────────────

pub struct ZipLongest<L, R> {
    left: L,
    right: R,
}

impl<L: Clone, R: Clone> Clone for ZipLongest<L, R> {
    fn clone(&self) -> Self {
        Self {
            left: self.left.clone(),
            right: self.right.clone(),
        }
    }
}

impl<L: fmt::Debug, R: fmt::Debug> fmt::Debug for ZipLongest<L, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZipLongest")
            .field("left", &self.left)
            .field("right", &self.right)
            .finish()
    }
}

impl<L: Iterator, R: Iterator> Iterator for ZipLongest<L, R> {
    type Item = (Option<L::Item>, Option<R::Item>);

    fn next(&mut self) -> Option<Self::Item> {
        let left = self.left.next();
        let right = self.right.next();
        if left.is_none() && right.is_none() {
            return None;
        }
        Some((left, right))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (l_lower, l_upper) = self.left.size_hint();
        let (r_lower, r_upper) = self.right.size_hint();
        let lower = l_lower.max(r_lower);
        let upper = match (l_upper, r_upper) {
            (Some(l), Some(r)) => Some(l.max(r)),
            _ => None,
        };
        (lower, upper)
    }
}

impl<L: ExactSizeIterator, R: ExactSizeIterator> ExactSizeIterator for ZipLongest<L, R> {
    fn len(&self) -> usize {
        self.left.len().max(self.right.len())
    }
}

impl<L: FusedIterator, R: FusedIterator> FusedIterator for ZipLongest<L, R> {}
