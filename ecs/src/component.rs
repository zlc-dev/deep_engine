use crate::id::Id;

/// 组件 ID — 纯索引，不回收，无需分代。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ComponentId(u32);

impl From<u32> for ComponentId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<ComponentId> for u32 {
    fn from(value: ComponentId) -> Self {
        value.0
    }
}

impl Id for ComponentId {
    type Inner = u32;
}

/// 组件必须满足 `Copy + 'static`，保证可被 ECS 以裸字节方式安全搬移。
pub trait Component: 'static {}

/// 为所有满足约束的类型自动实现。
impl<T: 'static> Component for T {}

/// 组件类型的运行时元信息。
#[derive(Clone)]
pub struct ComponentInfo {
    pub id: ComponentId,
    pub size: usize,
    pub alignment: usize,
    pub default_fn: Option<fn(*mut u8)>,
    pub drop_fn: Option<fn(*mut u8)>,
}

impl ComponentInfo {
    pub fn new<T: Component>(id: ComponentId) -> Self {
        Self::new_dyn(
            id,
            std::mem::size_of::<T>(),
            std::mem::align_of::<T>(),
            None,
            get_drop_fn::<T>(),
        )
    }

    /// 从具体类型 `T` 构造 `ComponentTypeInfo`。
    ///
    /// `default_value` 接受引用切片，内部自动拷贝到堆上。
    /// 传入 `None` 即为零初始化默认值。
    pub fn new_with_default<T: Component + Default>(id: ComponentId) -> Self {
        Self::new_dyn(
            id,
            std::mem::size_of::<T>(),
            std::mem::align_of::<T>(),
            Some(init_default_component::<T>),
            get_drop_fn::<T>(),
        )
    }

    /// 动态构造 `ComponentTypeInfo`。
    ///
    /// `default_value` 接受引用切片，内部自动拷贝到堆上。
    /// 传入 `None` 即为零初始化默认值。
    pub fn new_dyn(
        id: ComponentId,
        size: usize,
        alignment: usize,
        default_fn: Option<fn(*mut u8)>,
        drop_fn: Option<fn(*mut u8)>,
    ) -> Self {
        Self {
            id,
            size,
            alignment,
            default_fn,
            drop_fn,
        }
    }
}

fn init_default_component<T: Component + Default>(ptr: *mut u8) {
    unsafe {
        std::ptr::write(ptr as *mut T, T::default());
    }
}

/// 根据 `T` 是否实现 `Drop` 返回对应的析构函数指针。
fn get_drop_fn<T: Component>() -> Option<fn(*mut u8)> {
    if std::mem::needs_drop::<T>() {
        Some(drop_component::<T>)
    } else {
        None
    }
}

/// 对裸指针调用 `T` 的 `Drop::drop`。
fn drop_component<T: Component>(ptr: *mut u8) {
    unsafe { std::ptr::drop_in_place(ptr as *mut T) }
}
