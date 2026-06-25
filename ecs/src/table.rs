use crate::id::Id;
use crate::{Entity, component::{self, ComponentId, ComponentTypeInfo, get_component_type_info}, sparse::SparseSet};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;

/// 表 ID — 纯索引，不回收，无需分代。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TableId(u32);

impl From<u32> for TableId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<TableId> for u32 {
    fn from(value: TableId) -> Self {
        value.0
    }
}

impl Id for TableId {
    type Inner = u32;
}

struct Column {
    component_id: ComponentId,
    stride: usize,
    component_size: usize,
    type_info: Arc<ComponentTypeInfo>,
    data: Vec<u8>,
}

impl Column {

    fn new(component_id: ComponentId) -> Self {
        let info = get_component_type_info(component_id)
            .expect("Column::new: component not registered");

        Self {
            component_id, 
            stride: info.size & !(info.alignment - 1),
            component_size: info.size,
            type_info: info,
            data: Vec::new(),
        }
    }

    /// 将组件字节追加到此列末尾。
    ///
    /// # Panics
    /// 当 `component.len() != self.component_size` 时 panic。
    fn push(&mut self, component: &[u8]) {
        assert_eq!(
            component.len(),
            self.component_size,
            "Column::push: component size mismatch (expected {}, got {})",
            self.component_size,
            component.len(),
        );
        self.data.extend_from_slice(component);
    }

    /// 返回当前列中的组件数量。
    fn len(&self) -> usize {
        debug_assert_eq!(self.data.len() % self.stride, 0);
        self.data.len() / self.stride
    }

    /// 获取第 `index` 个组件的字节切片。
    fn get(&self, index: usize) -> Option<&[u8]> {
        let offset = index.checked_mul(self.stride)?;
        self.data.get(offset..offset + self.component_size)
    }

    /// 追加一个默认初始化的组件。
    fn push_default(&mut self) {
        match &self.type_info.default_value {
            None => {
                let old_len = self.data.len();
                self.data.resize(old_len + self.stride, 0);
            }
            Some(d) => {
                self.data.extend_from_slice(d);
            }
        }
    }

    /// 遍历原始指针
    fn raw_iter(&self) -> impl Iterator<Item = *const ()> {
        let base = self.data.as_ptr();
        let stride = self.stride;
        let len = self.len();
        (0..len).map(move |i| unsafe { base.add(i * stride) as *const () })
    }

    /// 遍历原始指针
    fn raw_iter_mut(&mut self) -> impl Iterator<Item = *mut ()> {
        let base = self.data.as_mut_ptr();
        let stride = self.stride;
        let len = self.len();
        (0..len).map(move |i| unsafe { base.add(i * stride) as *mut () })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NonMaxIndex(NonZeroUsize);

impl NonMaxIndex {
    /// `usize::MAX` → `None`（标记组件）；其他值 → `Some(Self)`（数据列）。
    pub fn new(n: usize) -> Option<Self> {
        // !usize::MAX == 0，NonZeroUsize 拒绝 0，产生 None
        NonZeroUsize::new(n ^ usize::MAX).map(Self)
    }

    /// 还原原始列索引（保证 ≠ `usize::MAX`）。
    pub fn get(self) -> usize {
        self.0.get() ^ usize::MAX
    }
}

impl From<NonMaxIndex> for usize {
    fn from(value: NonMaxIndex) -> Self {
        value.get()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ComponentIndex {
    Data(NonMaxIndex),
    Tag
}

pub struct Table {
    id: TableId,
    entities: Vec<Entity>,
    components_info: HashMap<ComponentId, ComponentIndex>, // Component -> Column Index
    columns: Box<[Column]>,
}

impl Table {

    pub fn new(id: TableId, components: &[ComponentId]) -> Self {

        let mut components = components.to_owned();
        components.sort();

        let mut components_info = HashMap::with_capacity(components.len());

        // 默认全部标记为 Tag（ZST / 不存在数据列）
        components.iter().for_each(|&cid| { components_info.insert(cid, ComponentIndex::Tag); });

        let columns: Box<[Column]> = components
            .iter()
            .filter(|&&cid| get_component_type_info(cid)
                    .map_or(false, |info| info.size != 0)
            )
            .map(|&cid| Column::new(cid))
            .collect();

        // 覆盖：存在数据列的组件 → Data(idx)
        columns.iter().enumerate().for_each(|(i, c)| {
            // SAFETY: i < columns.len() ≤ components.len() < usize::MAX
            components_info.insert(c.component_id, ComponentIndex::Data(NonMaxIndex::new(i).unwrap()));
        });

        Self {
            id,
            entities: Vec::new(), 
            components_info,
            columns,
        }
    }

    pub fn with_component(&self, component: ComponentId) -> bool {
        self.components_info.contains_key(&component)
    }

    pub fn with_components(&self, components: &[ComponentId]) -> bool {
        components.iter().all(|&component| self.with_component(component))
    }

    pub fn without_components(&self, components: &[ComponentId]) -> bool {
        components.iter().all(|&component| !self.with_component(component))
    }

    /// 向 Table 追加一个实体及其组件数据。
    ///
    /// `components[i]` 对应 `columns[i]`：
    /// - `Some(bytes)` → 直接写入 bytes
    /// - `None` → 使用该列的默认值填充
    ///
    /// # Panics
    /// 当 `components.len() != self.columns.len()` 时 panic。
    pub fn push(&mut self, entity: Entity, components: &[Option<&[u8]>]) {
        assert_eq!(
            components.len(),
            self.columns.len(),
            "Table::push: expected {} components, got {}",
            self.columns.len(),
            components.len(),
        );

        for (col, comp) in self.columns.iter_mut().zip(components.iter()) {
            match comp {
                Some(bytes) => col.push(bytes),
                None => col.push_default(),
            }
        }

        self.entities.push(entity);
    }

    /// 查询组件在 Table 中的列索引。
    ///
    /// - `None` → 组件不在此 Table 中
    /// - `Some(ComponentIndex::Tag)` → ZST 标记组件（无数据列）
    /// - `Some(ComponentIndex::Data(idx))` → 数据列，用 `idx.get()` 索引 `self.columns`
    pub fn get_component_index(&self, component: ComponentId) -> Option<ComponentIndex> {
        self.components_info.get(&component).copied()
    }

    /// 同时遍历多个数据列。用于 ECS system 的批量组件访问。
    pub fn raw_components_iter(
        &self,
        indices: &[NonMaxIndex],
    ) -> Box<[impl Iterator<Item = *const ()>]> {
        indices
            .iter()
            .map(move |&i| {
                self.columns[i.get()].raw_iter()
            })
            .collect()
    }

    /// 同时可变遍历多个数据列。用于 ECS system 的批量组件访问。
    ///
    /// # Safety
    ///
    /// - `indices` 中的索引必须互不相同（否则产生 aliasing `&mut`）。
    /// - `indices` 中的每个值必须来自 `ComponentIndex::Data(idx)`。
    pub unsafe fn raw_components_iter_mut(
        &mut self,
        indices: &[NonMaxIndex],
    ) -> Box<[impl Iterator<Item = *mut ()>]> {
        let ptr = self.columns.as_mut_ptr();
        indices
            .iter()
            .map(move |&i| {
                // SAFETY: caller guarantees indices are unique and valid
                let col = unsafe { &mut *ptr.add(i.get()) };
                col.raw_iter_mut()
            })
            .collect()
    }

}