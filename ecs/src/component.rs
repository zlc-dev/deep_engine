use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

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
pub trait Component: Copy + 'static {}

/// 为所有满足约束的类型自动实现。
impl<T: Copy + 'static> Component for T {}

/// 组件类型的运行时元信息。
///
/// `default_value` 为 `None` 时，该组件不支持默认填零；
/// 为 `Some(bytes)` 时，`bytes` 长度必须等于 `size`。
#[derive(Clone)]
pub struct ComponentTypeInfo {
    pub id: ComponentId,
    pub size: usize,
    pub alignment: usize,
    pub default_value: Option<Box<[u8]>>,
}

impl ComponentTypeInfo {
    /// 从具体类型 `T` 构造 `ComponentTypeInfo`。
    ///
    /// `default_value` 接受引用切片，内部自动拷贝到堆上。
    /// 传入 `None` 即为零初始化默认值。
    pub fn new<T: Component>(id: ComponentId, default_value: Option<&[u8]>) -> Self {
        assert!(
            default_value.as_ref().map_or(true, |d| d.len() == std::mem::size_of::<T>()),
            "ComponentTypeInfo::new: default_value length must match component size"
        );
        Self {
            id,
            size: std::mem::size_of::<T>(),
            alignment: std::mem::align_of::<T>(),
            default_value: default_value.map(|d| d.to_vec().into_boxed_slice()),
        }
    }
}

/// 全局组件类型注册表。
static COMPONENT_REGISTRY: LazyLock<RwLock<HashMap<ComponentId, Arc<ComponentTypeInfo>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// 向全局注册表注册一个组件类型。
///
/// # Panics
/// 当 `id` 已存在时 panic。
pub fn register<T: Component>(id: ComponentId, default_value: Option<&[u8]>) {
    let info = Arc::new(ComponentTypeInfo::new::<T>(id, default_value));
    let mut registry = COMPONENT_REGISTRY.write().unwrap();
    assert!(
        registry.insert(id, info).is_none(),
        "Component ID {:?} is already registered",
        id,
    );
}

/// 按 `ComponentId` 查询组件类型信息。
pub fn get_component_type_info(id: ComponentId) -> Option<Arc<ComponentTypeInfo>> {
    COMPONENT_REGISTRY.read().unwrap().get(&id).map(Arc::clone)
}
