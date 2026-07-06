pub trait IterTools: Iterator + Sized {
    /// 以 self 为基准的 zip — 左边始终有值，右边耗尽后填 None。
    fn zip_left<Iter: Iterator>(self, iter: Iter) -> impl Iterator<Item = (Self::Item, Option<Iter::Item>)> {
        ZipLeft { left: self, right: iter }
    }

    /// 以 other 为基准的 zip — 右边始终有值，左边耗尽后填 None。
    fn zip_right<Iter: Iterator>(self, iter: Iter) -> impl Iterator<Item = (Option<Self::Item>, Iter::Item)> {
        iter.zip_left(self).map(|(l, r)| (r, l))
    }

    /// 两边都耗尽后停止 — 超出的填 None。
    fn zip_longest<Iter: Iterator>(self, iter: Iter) -> impl Iterator<Item = (Option<Self::Item>, Option<Iter::Item>)> {
        ZipLongest { left: self, right: iter }
    }
}

impl<I: Iterator> IterTools for I {}

// ── ZipLeft ──────────────────────────────────────────────

struct ZipLeft<L, R> {
    left: L,
    right: R,
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

// ── ZipLongest ───────────────────────────────────────────

pub struct ZipLongest<L, R> {
    left: L,
    right: R,
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
