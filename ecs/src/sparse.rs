// ── 稀疏数组分页 ──────────────────────────────────────

use std::{marker::PhantomData, mem::replace};

use crate::id::{GenId, IsUnsignedInteger, Id};

struct AssertFitsUsize<U: IsUnsignedInteger>(PhantomData<U>);

impl<U: IsUnsignedInteger> AssertFitsUsize<U> {
    const OK: () = assert!(
        size_of::<U>() <= size_of::<usize>(),
        "ID inner type is wider than usize — would truncate on this target"
    );
}

/// 每页元素数（4096 个 slot，一页 = 4096 × size_of::<T>() 字节）。
const PAGE_SHIFT: usize = 12;
const PAGE_SIZE: usize = 1 << PAGE_SHIFT;
const PAGE_MASK: usize = PAGE_SIZE - 1;

/// 编译期指定稀疏数组的默认值。
pub trait SparseDefault<T: Copy> {
    const SPARSE_VALUE: T;
}


/// 分页的稀疏数组，按需分配页面。
///
/// `index → (page, offset)` 其中 `page = index >> PAGE_SHIFT`, `offset = index & PAGE_MASK`。
///
/// 类型参数 `T` 必须为 `Copy`，用于页面初始化。
/// `default` 是稀疏槽位的默认值——页面不存在或槽位未写入时返回该值。
pub struct SparseArray<T: Copy, D: SparseDefault<T>> {
    pages: Vec<Option<Box<[T; PAGE_SIZE]>>>,
    _marker: PhantomData<D>,
}

impl<T: Copy, D: SparseDefault<T>> SparseArray<T, D> {

    pub fn new() -> Self {
        Self { pages: Vec::new(), _marker: PhantomData }
    }

    /// 读取 `index` 处的值。页面不存在返回 `D::SPARSE_VALUE`。
    #[inline]
    pub fn get(&self, index: usize) -> T {
        let page_idx = index >> PAGE_SHIFT;
        let offset = index & PAGE_MASK;
        self.pages
            .get(page_idx)
            .and_then(|p| p.as_ref())
            .map_or(D::SPARSE_VALUE, |p| p[offset])
    }

    /// 写入 `index` 处的值。页面不存在时自动分配（全部初始化为 `default`）。
    pub fn set(&mut self, index: usize, value: T) {
        let page_idx = index >> PAGE_SHIFT;
        let offset = index & PAGE_MASK;

        if page_idx >= self.pages.len() {
            self.pages.resize_with(page_idx + 1, || None);
        }

        let page = self.pages[page_idx].get_or_insert_with(|| {
            Box::new([D::SPARSE_VALUE; PAGE_SIZE])
        });
        page[offset] = value;
    }

    /// 清空指定槽位（重置为 `default`），不回收页面内存。
    pub fn clear_slots(&mut self, keys: &[usize]) {
        for &key in keys {
            let page_idx = key >> PAGE_SHIFT;
            let offset = key & PAGE_MASK;
            if let Some(Some(page)) = self.pages.get_mut(page_idx) {
                page[offset] = D::SPARSE_VALUE;
            }
        }
    }
}


impl<T: Copy + PartialEq, D: SparseDefault<T>> SparseArray<T, D>  {
    /// 检查 `index` 处的值是否是默认值
    #[inline]
    pub fn is_default(&self, index: usize) -> bool {
        let page_idx = index >> PAGE_SHIFT;
        if page_idx >= self.pages.len() || self.pages[page_idx].is_none() {
            return true;
        }
        let offset = index & PAGE_MASK;
        D::SPARSE_VALUE == unsafe {
            self.pages
                .get_unchecked(page_idx)
                .as_ref()
                .unwrap_unchecked()[offset] 
        }
    }
}


/// 稀疏集：`usize → T` 的 O(1) 关联容器。
///
/// - 插入、删除、查找均为 O(1)
/// - 使用 swap-remove 删除，不保证顺序
/// - 迭代时内存连续（只遍历 dense 数组），缓存友好
pub struct SparseSet<K: Id, V> {
    sparse: SparseArray<usize, Self>,
    keys: Vec<K>,
    data: Vec<V>,
}

impl<K: Id, V> SparseDefault<usize> for SparseSet<K, V> {
    const SPARSE_VALUE: usize = usize::MAX;
}

impl<K: Id, V> SparseSet<K, V> {
    
    #[inline]
    fn static_check() {
        AssertFitsUsize::<K::Inner>::OK
    }
    
    pub fn new() -> Self {
        Self::static_check();
        Self {
            sparse: SparseArray::new(),
            keys: Vec::new(),
            data: Vec::new(),
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self::static_check();
        Self {
            sparse: SparseArray::new(),
            keys: Vec::with_capacity(cap),
            data: Vec::with_capacity(cap),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// 检查 key 是否存在。
    #[inline]
    pub fn contains_key(&self, key: &K) -> bool {
        let key = *key;
        let idx = key.get_id().as_usize();
        let d = self.sparse.get(idx);
        if d == Self::SPARSE_VALUE {
            return false;
        }
        debug_assert!(self.keys[d] == key, "SparseSet invariant broken");
        return true;
    }

    /// 获取 key 对应的不可变引用。
    #[inline]
    pub fn get(&self, key: &K) -> Option<&V> {
        let key = *key;
        let idx = key.get_id().as_usize();
        let d = self.sparse.get(idx);
        if d == Self::SPARSE_VALUE {
            return None;
        }
        debug_assert!(self.keys[d] == key, "SparseSet invariant broken");
        Some(&self.data[d])
    }

    /// 获取 key 对应的可变引用。
    #[inline]
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let key = *key;
        let idx = key.get_id().as_usize();
        let d = self.sparse.get(idx);
        if d == Self::SPARSE_VALUE {
            return None;
        }
        debug_assert!(self.keys[d] == key, "SparseSet invariant broken");
        Some(&mut self.data[d])
    }

    /// 获取 key 对应的入口，支持就地插入或修改。
    #[inline]
    pub fn entry(&mut self, key: K) -> Entry<'_, K, V> {
        let idx = key.get_id().as_usize();
        match self.sparse.get(idx) {
            d if d != Self::SPARSE_VALUE => {
                debug_assert!(self.keys[d] == key, "SparseSet invariant broken");
                Entry::Occupied(OccupiedEntry {
                    value: unsafe { self.data.as_mut_ptr().add(d) },
                    _phantom: PhantomData,
                })
            }
            _ => Entry::Vacant(VacantEntry {
                key,
                sparse_idx: idx,
                set: self,
            }),
        }
    }

    /// 插入 key-value。若 key 已存在则覆盖旧值。
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let idx = key.get_id().as_usize();
        match self.sparse.get(idx) {
            d if d != Self::SPARSE_VALUE => {
                debug_assert!(self.keys[d] == key, "SparseSet invariant broken");
                let dst = &mut self.data[d];
                let old = replace(dst, value);
                Some(old)
            }
            _ => {
                self.sparse.set(idx, self.keys.len());
                self.keys.push(key);
                self.data.push(value);
                None
            }
        }
    }

    /// 插入 key-value。若 key 已存在则覆盖旧值。返回新值的引用。
    pub fn insert_and_get(&mut self, key: K, value: V) -> (Option<V>, &V) {
        let idx = key.get_id().as_usize();
        match self.sparse.get(idx) {
            d if d != Self::SPARSE_VALUE => {
                debug_assert!(self.keys[d] == key, "SparseSet invariant broken");
                let dst = &mut self.data[d];
                let old = replace(dst, value);
                (Some(old), dst)
            }
            _ => {
                self.sparse.set(idx, self.keys.len());
                self.keys.push(key);
                self.data.push(value);
                (None, unsafe { self.data.last().unwrap_unchecked() })
            }
        }
    }

    /// 插入 key-value。若 key 已存在则覆盖旧值。返回新值的可变引用。
    pub fn insert_and_get_mut(&mut self, key: K, value: V) -> (Option<V>, &mut V) {
        let idx = key.get_id().as_usize();
        match self.sparse.get(idx) {
            d if d != Self::SPARSE_VALUE => {
                debug_assert!(self.keys[d] == key, "SparseSet invariant broken");
                let dst = &mut self.data[d];
                let old = replace(dst, value);
                (Some(old), dst)
            }
            _ => {
                self.sparse.set(idx, self.keys.len());
                self.keys.push(key);
                self.data.push(value);
                (None, unsafe { self.data.last_mut().unwrap_unchecked() })
            }
        }
    }

    /// 移除 key（swap-remove）。返回被移除的值。
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let key = *key;
        let idx = key.get_id().as_usize();
        let d = self.sparse.get(idx);

        if d == Self::SPARSE_VALUE {
            return None;
        }
        debug_assert!(self.keys[d] == key, "SparseSet invariant broken");

        let last = self.keys.len() - 1;
        self.keys.swap(d, last);
        self.data.swap(d, last);
        self.keys.pop();
        let removed = self.data.pop().unwrap();

        if d != last {
            let swapped_key = self.keys[d];
            self.sparse.set(swapped_key.get_id().as_usize(), d);
        }

        self.sparse.set(idx, Self::SPARSE_VALUE);

        Some(removed)
    }

    /// 清空所有元素，保留已分配内存。
    pub fn clear(&mut self) {
        let indices: Vec<usize> = self.keys.iter().map(|k| k.get_id().as_usize()).collect();
        self.sparse.clear_slots(&indices);
        self.keys.clear();
        self.data.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = (K, &V)> {
        self.keys.iter().copied().zip(self.data.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (K, &mut V)> {
        self.keys.iter().copied().zip(self.data.iter_mut())
    }

    pub fn keys(&self) -> impl Iterator<Item = K> + '_ {
        self.keys.iter().copied()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.data.iter()
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.data.iter_mut()
    }
}

impl<K: Id, V> Default for SparseSet<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Id, V> std::ops::Index<&K> for SparseSet<K, V> {
    type Output = V;

    fn index(&self, key: &K) -> &V {
        self.get(key).expect("SparseSet: key not found")
    }
}

/// 带 generation 校验的实体稀疏集：`K → V`。
///
/// 在 `SparseSet` 之上增加 generation 验证——即使 entity index 被复用，
/// 旧 handle 的 generation 不匹配也会被拒绝。
///
/// - `entity.index` → sparse 定位
/// - `entity.generation` → 与存储的 generation 比对
pub struct GenSparseSet<K: GenId, V> {
    sparse: SparseArray<usize, Self>,
    keys: Vec<K>,
    data: Vec<V>,
}

impl<K: GenId, V> SparseDefault<usize> for GenSparseSet<K, V> {
    const SPARSE_VALUE: usize = usize::MAX;
}

impl<K: GenId, V> GenSparseSet<K, V> {
    
    #[inline]
    fn static_check() {
        AssertFitsUsize::<K::Index>::OK
    }

    pub fn new() -> Self {
        Self::static_check();
        Self {
            sparse: SparseArray::new(),
            keys: Vec::new(),
            data: Vec::new(),
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self::static_check();
        Self {
            sparse: SparseArray::new(),
            keys: Vec::with_capacity(cap),
            data: Vec::with_capacity(cap),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// 检查实体是否存在（含 generation 校验）。
    #[inline]
    pub fn contains(&self, entity: &K) -> bool {
        let entity = *entity;
        let idx = entity.get_index().as_usize();
        let d = self.sparse.get(idx);
        d != Self::SPARSE_VALUE && self.keys[d] == entity
    }

    /// 获取实体对应的不可变引用。
    #[inline]
    pub fn get(&self, entity: &K) -> Option<&V> {
        let entity = *entity;
        let idx = entity.get_index().as_usize();
        let d = self.sparse.get(idx);
        if d == Self::SPARSE_VALUE || self.keys[d] != entity {
            return None;
        }
        Some(&self.data[d])
    }

    /// 获取实体对应的可变引用。
    #[inline]
    pub fn get_mut(&mut self, entity: &K) -> Option<&mut V> {
        let entity = *entity;
        let idx = entity.get_index().as_usize();
        let d = self.sparse.get(idx);
        if d == Self::SPARSE_VALUE || self.keys[d] != entity {
            return None;
        }
        Some(&mut self.data[d])
    }

    /// 获取实体对应的入口，支持就地插入或修改。
    #[inline]
    pub fn entry(&mut self, entity: K) -> GenEntry<'_, K, V> {
        let idx = entity.get_index().as_usize();
        match self.sparse.get(idx) {
            d if d != Self::SPARSE_VALUE && self.keys[d] == entity => {
                GenEntry::Occupied(OccupiedEntry {
                    value: unsafe { self.data.as_mut_ptr().add(d) },
                    _phantom: PhantomData,
                })
            }
            _ => GenEntry::Vacant(GenVacantEntry {
                key: entity,
                sparse_idx: idx,
                set: self,
            }),
        }
    }

    /// 插入实体及值。若实体已存在则覆盖。
    pub fn insert(&mut self, entity: K, value: V) -> Option<V> {
        let idx = entity.get_index().as_usize();

        match self.sparse.get(idx) {
            d if d != Self::SPARSE_VALUE && self.keys[d] == entity => {
                let dst = &mut self.data[d];
                let old = replace(dst, value);
                Some(old)
            }
            _ => {
                self.sparse.set(idx, self.keys.len());
                self.keys.push(entity);
                self.data.push(value);
                None
            }
        }
    }

    /// 插入实体及值。若实体已存在则覆盖旧值。返回新值的引用。
    pub fn insert_and_get(&mut self, entity: K, value: V) -> (Option<V>, &V) {
        let idx = entity.get_index().as_usize();

        match self.sparse.get(idx) {
            d if d != Self::SPARSE_VALUE && self.keys[d] == entity => {
                let dst = &mut self.data[d];
                let old = replace(dst, value);
                (Some(old), dst)
            }
            _ => {
                self.sparse.set(idx, self.keys.len());
                self.keys.push(entity);
                self.data.push(value);
                (None, unsafe { self.data.last().unwrap_unchecked() })
            }
        }
    }

    /// 插入实体及值。若实体已存在则覆盖旧值。返回新值的可变引用。
    pub fn insert_and_get_mut(&mut self, entity: K, value: V) -> (Option<V>, &mut V) {
        let idx = entity.get_index().as_usize();

        match self.sparse.get(idx) {
            d if d != Self::SPARSE_VALUE && self.keys[d] == entity => {
                let dst = &mut self.data[d];
                let old = replace(dst, value);
                (Some(old), dst)
            }
            _ => {
                self.sparse.set(idx, self.keys.len());
                self.keys.push(entity);
                self.data.push(value);
                (None, unsafe { self.data.last_mut().unwrap_unchecked() })
            }
        }
    }

    /// 移除实体（swap-remove + generation 校验）。
    pub fn remove(&mut self, entity: &K) -> Option<V> {
        let entity = *entity;
        let idx = entity.get_index().as_usize();
        let d = self.sparse.get(idx);

        if d == Self::SPARSE_VALUE || self.keys[d] != entity {
            return None;
        }

        let last = self.keys.len() - 1;
        self.keys.swap(d, last);
        self.data.swap(d, last);
        self.keys.pop();
        let removed = self.data.pop().unwrap();

        if d != last {
            let swapped = self.keys[d];
            self.sparse.set(swapped.get_index().as_usize(), d);
        }

        self.sparse.set(idx, Self::SPARSE_VALUE);

        Some(removed)
    }

    /// 清空所有元素，保留已分配内存。
    pub fn clear(&mut self) {
        for &entity in &self.keys {
            self.sparse.set(entity.get_index().as_usize(), Self::SPARSE_VALUE);
        }
        self.keys.clear();
        self.data.clear();
    }

    // ── 迭代器 ────────────────────────────────────

    pub fn iter(&self) -> impl Iterator<Item = (K, &V)> {
        self.keys.iter().copied().zip(self.data.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (K, &mut V)> {
        self.keys.iter().copied().zip(self.data.iter_mut())
    }

    pub fn entities(&self) -> impl Iterator<Item = K> + '_ {
        self.keys.iter().copied()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.data.iter()
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.data.iter_mut()
    }
}

impl<K: GenId, V> Default for GenSparseSet<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: GenId, V> std::ops::Index<&K> for GenSparseSet<K, V> {
    type Output = V;

    fn index(&self, key: &K) -> &V {
        self.get(key).expect("GenerationSparseSet: key not found")
    }
}



// ── Entry API ────────────────────────────────────

/// `SparseSet::entry` 的视图，类似 `HashMap::Entry`。
pub enum Entry<'a, K: Id, V> {
    Occupied(OccupiedEntry<'a, V>),
    Vacant(VacantEntry<'a, K, V>),
}

impl<'a, K: Id, V> Entry<'a, K, V> {
    /// 若为 `Vacant`，插入默认值；无论如何返回 `&mut V`。
    pub fn or_insert(self, default: V) -> &'a mut V {
        match self {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => e.insert(default),
        }
    }

    /// 若为 `Vacant`，调用 `f` 生成值并插入；无论如何返回 `&mut V`。
    pub fn or_insert_with<F: FnOnce() -> V>(self, f: F) -> &'a mut V {
        match self {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => e.insert(f()),
        }
    }

    /// 若为 `Occupied`，对值应用 `f`。
    pub fn and_modify<F: FnOnce(&mut V)>(mut self, f: F) -> Self {
        if let Entry::Occupied(ref mut e) = self {
            f(e.get_mut());
        }
        self
    }
}

pub struct OccupiedEntry<'a, V> {
    value: *mut V,
    _phantom: PhantomData<&'a mut V>,
}

impl<'a, V> OccupiedEntry<'a, V> {
    pub fn get(&self) -> &V {
        unsafe { &*self.value }
    }

    pub fn get_mut(&mut self) -> &mut V {
        unsafe { &mut *self.value }
    }

    pub fn into_mut(self) -> &'a mut V {
        unsafe { &mut *self.value }
    }
}

pub struct VacantEntry<'a, K: Id, V> {
    key: K,
    sparse_idx: usize,
    set: &'a mut SparseSet<K, V>,
}

impl<'a, K: Id, V> VacantEntry<'a, K, V> {
    pub fn insert(self, value: V) -> &'a mut V {
        let set = self.set;
        let dense_idx = set.keys.len();
        set.sparse.set(self.sparse_idx, dense_idx);
        set.keys.push(self.key);
        set.data.push(value);
        unsafe { set.data.last_mut().unwrap_unchecked() }
    }
}

// ── Generation Entry API ────────────────────────

/// `GenerationSparseSet::entry` 的视图。
pub enum GenEntry<'a, K: GenId, V> {
    Occupied(OccupiedEntry<'a, V>),
    Vacant(GenVacantEntry<'a, K, V>),
}

impl<'a, K: GenId, V> GenEntry<'a, K, V> {
    pub fn or_insert(self, default: V) -> &'a mut V {
        match self {
            GenEntry::Occupied(e) => e.into_mut(),
            GenEntry::Vacant(e) => e.insert(default),
        }
    }

    pub fn or_insert_with<F: FnOnce() -> V>(self, f: F) -> &'a mut V {
        match self {
            GenEntry::Occupied(e) => e.into_mut(),
            GenEntry::Vacant(e) => e.insert(f()),
        }
    }

    pub fn and_modify<F: FnOnce(&mut V)>(mut self, f: F) -> Self {
        if let GenEntry::Occupied(ref mut e) = self {
            f(e.get_mut());
        }
        self
    }
}

pub struct GenVacantEntry<'a, K: GenId, V> {
    key: K,
    sparse_idx: usize,
    set: &'a mut GenSparseSet<K, V>,
}

impl<'a, K: GenId, V> GenVacantEntry<'a, K, V> {
    pub fn insert(self, value: V) -> &'a mut V {
        let set = self.set;
        let dense_idx = set.keys.len();
        set.sparse.set(self.sparse_idx, dense_idx);
        set.keys.push(self.key);
        set.data.push(value);
        unsafe { set.data.last_mut().unwrap_unchecked() }
    }
}
