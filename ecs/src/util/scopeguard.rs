use std::mem::ManuallyDrop;

pub trait DeferFunction {
    fn defer(self);
}

impl<F: FnOnce()> DeferFunction for F {
    fn defer(self) {
        self()
    }
}

pub struct Defer<F: DeferFunction> {
    f: ManuallyDrop<F>,
}

impl<F: DeferFunction> Defer<F> {
    pub fn new(f: F) -> Self {
        Self { f: ManuallyDrop::new(f) }
    }
}

impl<F: DeferFunction> Drop for Defer<F> {
    fn drop(&mut self) {
        // SAFETY: This is Drop — `self.f` will never be accessed again.
        // ManuallyDrop prevents the compiler from running F's destructor twice.
        let f = unsafe { std::ptr::read(&*self.f) };
        f.defer();
    }
}

/// Go 风格的 `defer` — 作用域结束时执行表达式或语句块。
///
/// ```
/// use ecs::defer;
/// 
/// defer!(println!("单条表达式"));
/// defer! {
///     println!("表达式1");
///     println!("表达式2");
/// }
/// ```
#[macro_export]
macro_rules! defer {
    ($expr:expr) => {
        let __defer_guard = $crate::util::scopeguard::Defer::new(|| $expr);
    };
    { $($stmt:stmt);* $(;)? } => {
        let __defer_guard = $crate::util::scopeguard::Defer::new(|| { $($stmt);* });
    };
}


