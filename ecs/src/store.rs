use std::hash::{Hash, Hasher};
use std::collections::HashMap;
use smallvec::SmallVec;

use crate::component::ComponentInfo;
use crate::id::{AtomicAllocIdPool, DefaultIdAllocator, IdAllocator};
use crate::{Entity, component::ComponentId, sparse::{GenSparseSet, SparseSet}, table::{Table, TableId}};

struct EntityRecord {
    id: Entity,
    table_id: TableId,
    index: usize,
}

/// Table 的组件签名：排序后的 ComponentId 列表。
///
/// 用作 `HashMap<TableType, TableId>` 的 key。
/// 组件 ID 必须排序，保证相同集合产生相同的 `TableType`。
///
/// 对象不可变，因此构造时预计算并缓存 hash 值。
#[derive(Clone, Debug)]
pub struct TableType {
    components: Box<[ComponentId]>,
    hash: u64,
}

impl TableType {
    /// 从已排序的组件 ID 切片构造。
    ///
    /// 调用者保证 `ids` 已排序且无重复。
    /// 内部拷贝到堆上以生成 `Box<[T]>`。
    pub fn from_sorted(ids: &[ComponentId]) -> Self {
        debug_assert!(
            ids.windows(2).all(|w| w[0] < w[1]),
            "TableType: component IDs must be sorted and unique"
        );
        let components: Box<[ComponentId]> = ids.to_vec().into_boxed_slice();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        components.hash(&mut hasher);
        let hash = hasher.finish();
        Self { components, hash }
    }

    /// 从任意组件 ID 迭代器构造（自动排序）。
    pub fn from_iter(ids: impl IntoIterator<Item = ComponentId>) -> Self {
        let mut v: Vec<ComponentId> = ids.into_iter().collect();
        v.sort_unstable();
        debug_assert!(
            v.windows(2).all(|w| w[0] != w[1]),
            "TableType: duplicate component IDs detected"
        );
        let components: Box<[ComponentId]> = v.into_boxed_slice();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        components.hash(&mut hasher);
        let hash = hasher.finish();
        Self { components, hash }
    }

    /// 组件 ID 切片。
    #[inline]
    pub fn as_slice(&self) -> &[ComponentId] {
        &self.components
    }

    /// 组件数量。
    #[inline]
    pub fn len(&self) -> usize {
        self.components.len()
    }

    /// 是否无组件。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }
}

impl PartialEq for TableType {
    fn eq(&self, other: &Self) -> bool {
        self.components == other.components
    }
}

impl Eq for TableType {}

impl Hash for TableType {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

// 负责维护 Entity -> Components 的存储
pub struct Store {
    entity_index: GenSparseSet<Entity, EntityRecord>,
    table_id_alloc: DefaultIdAllocator<TableId>,
    tables: SparseSet<TableId, Box<Table>>,
    table_map: HashMap<TableType, TableId>,
    component_tables: SparseSet<ComponentId, SmallVec<[TableId; 4]>>,
}


impl Store {

    pub fn new() -> Self {
        Self {
            entity_index: GenSparseSet::new(),
            table_id_alloc: DefaultIdAllocator::new(),
            tables: SparseSet::new(),
            table_map: HashMap::new(),
            component_tables: SparseSet::new(),
        }
    }

    pub fn insert_entity(&mut self, entity: Entity, components: &[ComponentId]) {

    }

    /// 获取或创建与给定组件集合匹配的 Table，返回其 `TableId`。
    fn create_table(&mut self, components: &[ComponentInfo]) -> TableId {
        // 已存在则直接返回。
        let table_type = TableType::from_iter(components.iter().map(|c| c.id));
        if let Some(&tid) = self.table_map.get(&table_type) {
            return tid;
        }

        let tid = self.table_id_alloc.allocate();
        self.tables.insert(tid, Box::new(Table::new(tid, components)));
        self.table_map.insert(table_type, tid);
        components.iter().for_each(
            |c| {
                self.component_tables
                    .entry(c.id)
                    .or_insert_with(|| SmallVec::new())
                    .push(tid);
            }
        );
        tid
    }

    pub fn get_table(&self, components: &[ComponentId]) -> Option<TableId> {
        self.table_map.get(&TableType::from_iter(components.iter().cloned())).cloned()
    }

    pub fn find_tables(&self, with: &[ComponentId], without: &[ComponentId]) -> Box<[TableId]> {

        // If no `with` components are specified, check all tables.
        if with.is_empty() {
            return self.tables.keys().filter_map(|k| {
                self.tables.get(&k).and_then(|table| {
                    // Exclude tables that contain any `without` component.
                    if table.without_components(without) {
                        Some(k)
                    } else {
                        None
                    }
                })
            }).collect();
        }

        let mut smallest: &[TableId] = &self.component_tables[&with[0]];

        for &cid in with.iter().skip(1) {
            let tables = &self.component_tables[&cid];
            if tables.len() < smallest.len() {
                smallest = tables;
            }
        }

        let result = smallest.iter().filter(
            |&&table_id| {
                let table = self.tables.get(&table_id).unwrap().as_ref();
                table.with_components(with) && table.without_components(without)
            }
        )
        .map(|&tid| tid)
        .collect();

        result
    }
}
