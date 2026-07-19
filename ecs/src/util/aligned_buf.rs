use std::alloc::{self, Layout};
use std::ptr::{self, NonNull};
use std::slice;

pub trait Alignment: Clone + Copy {
    fn get_alignment(self) -> usize;
}

#[derive(Clone, Copy)]
pub struct ConstAlignment<const N: usize>;

impl<const N: usize> Alignment for ConstAlignment<N> {
    fn get_alignment(self) -> usize {
        N
    }
}

impl Alignment for usize {
    fn get_alignment(self) -> usize {
        self
    }
}

/// aligned buffer
/// 支持自定义对齐的缓冲区
pub struct ABuf<Align = usize>
where
    Align: Alignment,
{
    align: Align,
    cap: usize,
    len: usize,
    buf: Option<NonNull<u8>>,
}

pub type ABufC<const N: usize> = ABuf<ConstAlignment<N>>;

impl<const N: usize> ABuf<ConstAlignment<N>> {
    pub fn new() -> Self {
        Self::new_with_align(ConstAlignment::<N>)
    }
}

impl<Align: Alignment> ABuf<Align> {
    pub fn new_with_align(align: Align) -> Self {
        let alignment = align.get_alignment();
        assert!(
            alignment.is_power_of_two(),
            "ABuf alignment must be a non-zero power of two, got {alignment}",
        );

        Self {
            align,
            cap: 0,
            len: 0,
            buf: None,
        }
    }

    pub fn with_capacity_in(capacity: usize, align: Align) -> Self {
        let mut buf = Self::new_with_align(align);
        buf.reserve_exact(capacity);
        buf
    }

    pub fn align(&self) -> usize {
        self.align.get_alignment()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.buf
            .map_or(NonNull::<u8>::dangling().as_ptr() as *const u8, |p| {
                p.as_ptr() as *const u8
            })
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.buf
            .map_or(NonNull::<u8>::dangling().as_ptr(), NonNull::as_ptr)
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len) }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn reserve(&mut self, additional: usize) {
        let required = self
            .len
            .checked_add(additional)
            .expect("ABuf capacity overflow");
        if required <= self.cap {
            return;
        }

        let doubled = self.cap.max(1).saturating_mul(2);
        self.grow(required.max(doubled));
    }

    pub fn reserve_exact(&mut self, additional: usize) {
        let required = self
            .len
            .checked_add(additional)
            .expect("ABuf capacity overflow");
        if required > self.cap {
            self.grow(required);
        }
    }

    pub fn resize(&mut self, new_len: usize, value: u8) {
        if new_len > self.len {
            let old_len = self.len;
            self.reserve(new_len - old_len);
            unsafe {
                ptr::write_bytes(self.as_mut_ptr().add(old_len), value, new_len - old_len);
            }
        }
        self.len = new_len;
    }

    pub fn extend_from_slice(&mut self, src: &[u8]) {
        if src.is_empty() {
            return;
        }

        let old_len = self.len;
        self.reserve(src.len());
        unsafe {
            ptr::copy_nonoverlapping(src.as_ptr(), self.as_mut_ptr().add(old_len), src.len());
        }
        self.len = old_len + src.len();
    }

    /// Sets the buffer length without initializing or dropping bytes.
    ///
    /// # Safety
    /// `new_len` must be less than or equal to `capacity()`, and bytes in
    /// `old_len..new_len` must already be initialized before being read.
    pub unsafe fn set_len(&mut self, new_len: usize) {
        assert!(
            new_len <= self.cap,
            "ABuf::set_len: new length {new_len} exceeds capacity {}",
            self.cap,
        );
        self.len = new_len;
    }

    fn grow(&mut self, new_cap: usize) {
        debug_assert!(new_cap > self.cap);
        let new_layout = self.layout(new_cap);

        let new_ptr = match self.buf {
            Some(ptr) => {
                let old_layout = self.layout(self.cap);
                unsafe { alloc::realloc(ptr.as_ptr(), old_layout, new_layout.size()) }
            }
            None => unsafe { alloc::alloc(new_layout) },
        };

        let new_ptr =
            NonNull::new(new_ptr).unwrap_or_else(|| alloc::handle_alloc_error(new_layout));
        self.buf = Some(new_ptr);
        self.cap = new_cap;
    }

    fn layout(&self, size: usize) -> Layout {
        Layout::from_size_align(size, self.align()).expect("ABuf layout exceeds addressable memory")
    }
}

impl<Align: Alignment> Drop for ABuf<Align> {
    fn drop(&mut self) {
        if let Some(ptr) = self.buf {
            if self.cap == 0 {
                return;
            }
            unsafe {
                alloc::dealloc(ptr.as_ptr(), self.layout(self.cap));
            }
        }
    }
}

impl Default for ABuf<usize> {
    fn default() -> Self {
        Self::new_with_align(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{ABuf, ConstAlignment};

    #[test]
    fn allocates_with_requested_alignment() {
        let mut buf = ABuf::new_with_align(4096);
        buf.resize(1, 0);

        assert_eq!(buf.align(), 4096);
        assert_eq!(buf.as_ptr() as usize % 4096, 0);
    }

    #[test]
    fn extend_resize_and_truncate() {
        let mut buf = ABuf::new_with_align(16);

        buf.extend_from_slice(&[1, 2, 3]);
        buf.resize(6, 9);
        assert_eq!(buf.as_slice(), &[1, 2, 3, 9, 9, 9]);

        buf.resize(2, 0);
        assert_eq!(buf.as_slice(), &[1, 2]);
    }

    #[test]
    fn supports_const_alignment_marker() {
        let mut buf = ABuf::with_capacity_in(8, ConstAlignment::<32>);
        buf.extend_from_slice(&[1, 2, 3, 4]);

        assert_eq!(buf.capacity(), 8);
        assert_eq!(buf.as_ptr() as usize % 32, 0);
        assert_eq!(buf.as_slice(), &[1, 2, 3, 4]);
    }
}
