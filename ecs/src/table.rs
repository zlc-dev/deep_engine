use std::alloc::Layout;
use std::num::NonZeroUsize;
use std::ops::Add;
use std::ptr::NonNull;

use crate::component;
use crate::id::Id;
use crate::table::ComponentIndex::{Data, Tag};
use crate::util::aligned_buf::ABuf;
use crate::util::itertools::IterTools;
use crate::{
    Entity,
    component::{ComponentId, ComponentInfo},
    sparse::SparseSet,
};

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
    component_size: usize,
    stride: usize,
    default_fn: Option<fn(*mut u8)>,
    drop_fn: Option<fn(*mut u8)>,
    data: ABuf,
}

impl Column {
    fn new(component: &ComponentInfo) -> Self {
        let size = component.size;
        let align = component.alignment;
        Self {
            component_id: component.id,
            component_size: size,
            stride: (size + align - 1) & !(align - 1),
            default_fn: component.default_fn,
            drop_fn: component.drop_fn,
            data: ABuf::new_with_align(align),
        }
    }

    fn get_component_id(&self) -> ComponentId {
        self.component_id
    }

    fn get_component_align(&self) -> usize {
        self.data.align()
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
        unsafe { self.push_unchecked(component) }
    }

    /// 将组件字节追加到此列末尾。
    ///
    /// # Safety
    /// 需要 `component.len() == self.component_size`
    unsafe fn push_unchecked(&mut self, component: &[u8]) {
        self.data.extend_from_slice(component);
        let pad = self.stride - self.component_size;
        if pad > 0 {
            self.data.resize(self.data.len() + pad, 0);
        }
    }

    /// 返回当前列中的组件数量。
    fn len(&self) -> usize {
        debug_assert_eq!(self.data.len() % self.stride, 0);
        self.data.len() / self.stride
    }

    /// 获取第 `index` 个组件的字节切片。
    fn get(&self, index: usize) -> Option<&[u8]> {
        let offset = index.checked_mul(self.stride)?;
        self.data
            .as_slice()
            .get(offset..offset + self.component_size)
    }

    /// 追加一个默认初始化的组件。
    fn push_default(&mut self) {
        let old_len = self.data.len();
        self.data.resize(old_len + self.stride, 0);
        if let Some(f) = self.default_fn {
            unsafe {
                f(self.data.as_mut_ptr().add(old_len));
            }
        }
    }

    /// 交换删除第 `row` 行的组件。与末行交换后截断，O(1)。
    ///
    /// 若 `out_buf` 为 `Some`，组件字节被拷贝到缓冲区而非析构；
    /// 缓冲区长度必须 ≥ `self.component_size`，否则 panic。
    /// 若 `out_buf` 为 `None` 且组件有 `drop_fn`，则调用析构；
    /// `!Drop` 类型无析构，直接覆盖。
    ///
    /// # Panics
    /// - `row` 越界
    /// - `out_buf` 长度不足
    fn swap_remove(&mut self, row: usize, out_buf: Option<&mut [u8]>) {
        assert!(
            row < self.len(),
            "Column::swap_remove: row {row} out of bounds"
        );

        let last = self.len() - 1;
        let removed_offset = row * self.stride;
        let last_offset = last * self.stride;

        if row != last {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.data.as_ptr().add(last_offset),
                    self.data.as_mut_ptr().add(removed_offset),
                    self.stride,
                );
            }
        }

        // Handle the last element (now moved to `row` if row != last)
        match out_buf {
            Some(buf) => {
                assert!(
                    buf.len() >= self.component_size,
                    "Column::swap_remove: out_buf too small (need {}, got {})",
                    self.component_size,
                    buf.len(),
                );
                buf[..self.component_size]
                    .copy_from_slice(&self.data.as_slice()[last_offset..][..self.component_size]);
            }
            None => {
                if let Some(f) = self.drop_fn {
                    unsafe {
                        f(self.data.as_mut_ptr().add(last_offset));
                    }
                }
                // !Drop 类型：位模式平凡，直接覆盖无副作用
            }
        };

        // SAFETY: last_offset is a valid stride boundary (verified by len() invariant)
        unsafe {
            self.data.set_len(last_offset);
        }
    }

    /// 遍历原始指针
    fn raw_iter(&self) -> impl Iterator<Item = *const ()> + '_ {
        let base = self.data.as_ptr();
        let stride = self.stride;
        let len = self.len();
        (0..len).map(move |i| unsafe { base.add(i * stride) as *const () })
    }

    /// 遍历原始指针
    fn raw_iter_mut(&mut self) -> impl Iterator<Item = *mut ()> + '_ {
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
        // usize::MAX ^ usize::MAX == 0，NonZeroUsize 拒绝 0，产生 None
        NonZeroUsize::new(n ^ usize::MAX).map(Self)
    }

    /// 还原原始列索引。
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
    Tag,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableErr {
    DuplicateComponent,
    ComponentSizeMismatch(ComponentId),
    ComponentNotInTable(ComponentId),
}

pub struct Table {
    id: TableId,
    entities: Vec<Entity>,
    components_info: SparseSet<ComponentId, ComponentIndex>, // Component -> Column Index
    columns: Box<[Column]>,
}

impl Table {
    pub fn new(id: TableId, components: &[&ComponentInfo]) -> Self {
        let mut components = components.to_owned();
        components.sort_by(|l, r| l.id.cmp(&r.id));

        let mut components_info = SparseSet::with_capacity(components.len());

        // 默认全部标记为 Tag（ZST / 不存在数据列）
        components.iter().for_each(|comp| {
            components_info.insert(comp.id, ComponentIndex::Tag);
        });

        let columns: Box<[Column]> = components
            .iter()
            .filter(|&comp| comp.size != 0)
            .map(|comp| Column::new(comp))
            .collect();

        // 覆盖：存在数据列的组件 → Data(idx)
        columns.iter().enumerate().for_each(|(i, c)| {
            // SAFETY: i < columns.len() ≤ components.len() < usize::MAX
            components_info.insert(
                c.component_id,
                ComponentIndex::Data(NonMaxIndex::new(i).unwrap()),
            );
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
        components
            .iter()
            .all(|&component| self.with_component(component))
    }

    pub fn without_components(&self, components: &[ComponentId]) -> bool {
        components
            .iter()
            .all(|&component| !self.with_component(component))
    }

    /// 向 Table 追加一个新实体及其组件数据。
    /// 返回行号或错误
    pub fn push(
        &mut self,
        entity: Entity,
        components: &[Option<&[u8]>],
    ) -> Result<usize, TableErr> {

        for (col, &component) in self.columns.iter().zip(components.iter()) {
            if let Some(component) = component {
                if col.component_size != component.len() {
                    return Err(TableErr::ComponentSizeMismatch(col.component_id));
                }
            }
        }

        Ok(unsafe { self.push_unchecked(entity, components) })
    }

    /// 向 Table 追加一个新实体及其组件数据。
    /// 返回行号
    pub unsafe fn push_unchecked(
        &mut self,
        entity: Entity,
        components: &[Option<&[u8]>],
    ) -> usize {
        let row = self.entities.len();

        self.columns.iter_mut()
            .zip_left(components.iter())
            .for_each(|(col, component)| {
                if let Some(component) = component.and_then(|c| c.as_deref()) {
                    col.push(component);
                } else {
                    col.push_default();
                }
            });

        self.entities.push(entity);

        row
    }

    /// 查询组件在 Table 中的列索引。
    ///
    /// - `None` → 组件不在此 Table 中
    /// - `Some(ComponentIndex::Tag)` → ZST 标记组件（无数据列）
    /// - `Some(ComponentIndex::Data(idx))` → 数据列，用 `idx.get()` 索引 `self.columns`
    pub fn get_component_index(&self, component: ComponentId) -> Option<ComponentIndex> {
        self.components_info.get(&component).copied()
    }

    pub fn row_count(&self) -> usize {
        self.entities.len()
    }

    pub fn swap_remove(&mut self, row: usize, out_bufs: &mut [Option<&mut [u8]>]) {
        self.columns
            .iter_mut()
            .zip_left(out_bufs.iter_mut())
            .for_each(|(col, buf_slot)| {
                col.swap_remove(row, buf_slot.and_then(|slot| slot.as_deref_mut()));
            });
    }

    /// 同时遍历多个数据列。用于 ECS system 的批量组件访问。
    pub fn raw_components_iter(
        &self,
        indices: &[NonMaxIndex],
    ) -> Box<[impl Iterator<Item = *const ()>]> {
        indices
            .iter()
            .map(move |&i| self.columns[i.get()].raw_iter())
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
