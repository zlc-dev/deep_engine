use std::hash::Hash;
use std::sync::atomic::Ordering;

use crate::{
    sparse::{SparseArray, SparseDefault},
    util::spmc_atomic_queue::SpmcAtomicQueue,
};

pub trait IsUnsignedInteger: Clone + Copy + PartialEq + Eq + PartialOrd + Ord + Hash {
    const ZERO: Self;
    const ONE: Self;
    const MAX: Self;
    fn as_usize(self) -> usize;
    fn from_usize(n: usize) -> Self;
    fn wrapping_add(self, other: Self) -> Self;
}

macro_rules! impl_unsigned {
    ($($t:ty),* $(,)?) => {
        $(
            impl IsUnsignedInteger for $t {
                const ZERO: Self = 0;
                const ONE: Self = 1;
                const MAX: Self = <$t>::MAX;
                #[inline]
                fn as_usize(self) -> usize { self as usize }
                #[inline]
                fn from_usize(n: usize) -> Self { n as Self }
                #[inline]
                fn wrapping_add(self, other: Self) -> Self {
                    <$t>::wrapping_add(self, other)
                }
            }

        )*
    };
}

impl_unsigned!(u8, u16, u32, u64, u128, usize);

impl<T: IsUnsignedInteger> Id for T {
    type Inner = T;
}

pub trait AtomicInteger: IsUnsignedInteger {
    type Atomic;
    fn atomic_new(val: Self) -> Self::Atomic;
    fn atomic_load(atomic: &Self::Atomic, order: Ordering) -> Self;
    fn atomic_fetch_add(atomic: &Self::Atomic, val: Self, order: Ordering) -> Self;

    fn atomic_zero() -> Self::Atomic {
        Self::atomic_new(Self::ZERO)
    }
    fn atomic_one() -> Self::Atomic {
        Self::atomic_new(Self::ONE)
    }
}

macro_rules! impl_atomic_integer {
    ($int:ty => $atomic:ty) => {
        impl AtomicInteger for $int {
            type Atomic = $atomic;
            fn atomic_new(val: Self) -> Self::Atomic {
                <$atomic>::new(val)
            }
            fn atomic_load(atomic: &Self::Atomic, order: Ordering) -> Self {
                atomic.load(order)
            }
            fn atomic_fetch_add(atomic: &Self::Atomic, val: Self, order: Ordering) -> Self {
                atomic.fetch_add(val, order)
            }
        }
    };
}

impl_atomic_integer!(u8  => std::sync::atomic::AtomicU8);
impl_atomic_integer!(u16  => std::sync::atomic::AtomicU16);
impl_atomic_integer!(u32  => std::sync::atomic::AtomicU32);
impl_atomic_integer!(u64  => std::sync::atomic::AtomicU64);
impl_atomic_integer!(usize => std::sync::atomic::AtomicUsize);

// 普通ID
pub trait Id: Clone + Copy + PartialEq + Eq + Hash + From<Self::Inner> + Into<Self::Inner> {
    type Inner: IsUnsignedInteger;

    fn new(id: Self::Inner) -> Self {
        id.into()
    }

    fn get_id(self) -> Self::Inner {
        self.into()
    }
}

// 分代ID，Generational ID
pub trait GenId: Id {
    type Index: IsUnsignedInteger;
    type Generation: IsUnsignedInteger;

    fn new_with_gen(index: Self::Index, generation: Self::Generation) -> Self;

    fn get_index(&self) -> Self::Index;

    fn get_gen(&self) -> Self::Generation;
}

pub trait IdAllocator {
    type IdType: Id;

    fn allocate(&mut self) -> Self::IdType;

    fn is_valid(&self, id: Self::IdType) -> bool;
}

pub trait AtomicIdAllocator: IdAllocator {
    fn allocate_atomic(&self) -> Self::IdType;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdPoolError {
    NotAllocated,
    AlreadyDeallocated,
}

pub trait IdPool: IdAllocator<IdType: GenId> {
    fn deallocate(&mut self, id: Self::IdType) -> Result<(), IdPoolError>;
}

pub trait AtomicIdPool: IdPool {
    fn deallocate_atomic(&self, id: Self::IdType) -> Result<(), IdPoolError>;
}

pub struct DefaultIdAllocator<T: Id> {
    next_id: T::Inner,
}

impl<T: Id> DefaultIdAllocator<T> {
    pub fn new() -> Self {
        Self {
            next_id: T::Inner::ZERO,
        }
    }
}

impl<T: Id> IdAllocator for DefaultIdAllocator<T> {
    type IdType = T;

    fn allocate(&mut self) -> Self::IdType {
        let id = self.next_id.into();
        self.next_id = self.next_id.wrapping_add(T::Inner::ONE);
        id
    }

    fn is_valid(&self, id: Self::IdType) -> bool {
        <T::Inner as Ord>::cmp(&id.into(), &self.next_id.into()).is_lt()
    }
}

impl<T: Id> Default for DefaultIdAllocator<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DefaultIdPool<T: GenId, A = DefaultIdAllocator<<T as GenId>::Index>>
where
    A: IdAllocator<IdType = T::Index>,
{
    index_allocator: A,
    free_list: Vec<T::Index>,
    /// 分页稀疏存储的 generation。未写入的槽位隐式返回 0。
    generations: SparseArray<T::Generation, Self>,
}

impl<T: GenId, A> SparseDefault<T::Generation> for DefaultIdPool<T, A>
where
    A: IdAllocator<IdType = T::Index>,
{
    const SPARSE_VALUE: T::Generation = T::Generation::ZERO;
}

impl<T: GenId, A> DefaultIdPool<T, A>
where
    A: IdAllocator<IdType = T::Index>,
{
    pub fn new_with_alloc(alloc: A) -> Self {
        Self {
            index_allocator: alloc,
            free_list: Vec::new(),
            generations: SparseArray::new(),
        }
    }
}

impl<T: GenId, A> DefaultIdPool<T, A>
where
    A: IdAllocator<IdType = T::Index> + Default,
{
    pub fn new() -> Self {
        Self {
            index_allocator: A::default(),
            free_list: Vec::new(),
            generations: SparseArray::new(),
        }
    }
}

impl<T: GenId, A> IdAllocator for DefaultIdPool<T, A>
where
    A: IdAllocator<IdType = T::Index>,
{
    type IdType = T;

    fn allocate(&mut self) -> Self::IdType {
        if let Some(index) = self.free_list.pop() {
            let generation = self.generations.get(index.as_usize());
            T::new_with_gen(index, generation)
        } else {
            let index = self.index_allocator.allocate();
            T::new_with_gen(index, T::Generation::ZERO)
        }
    }

    fn is_valid(&self, id: Self::IdType) -> bool {
        if !self.index_allocator.is_valid(id.get_index()) {
            return false;
        }
        let idx = id.get_index().as_usize();
        id.get_gen() == self.generations.get(idx)
    }
}

impl<T: GenId, A> IdPool for DefaultIdPool<T, A>
where
    A: IdAllocator<IdType = T::Index>,
{
    fn deallocate(&mut self, id: Self::IdType) -> Result<(), IdPoolError> {
        let index = id.get_index();
        if !self.index_allocator.is_valid(id.get_index()) {
            return Err(IdPoolError::NotAllocated);
        }
        let idx = index.as_usize();
        let stored = self.generations.get(idx);
        if stored != id.get_gen() {
            return Err(IdPoolError::AlreadyDeallocated);
        }
        self.generations
            .set(idx, id.get_gen().wrapping_add(T::Generation::ONE));
        self.free_list.push(index);
        Ok(())
    }
}

impl<T: GenId, A> Default for DefaultIdPool<T, A>
where
    A: IdAllocator<IdType = T::Index> + Default,
{
    fn default() -> Self {
        Self::new()
    }
}

pub struct DefaultAtomicIdAllocator<T: Id>
where
    T::Inner: AtomicInteger,
{
    next_id: <T::Inner as AtomicInteger>::Atomic,
}

impl<T: Id> DefaultAtomicIdAllocator<T>
where
    T::Inner: AtomicInteger,
{
    pub fn new() -> Self {
        Self {
            next_id: T::Inner::atomic_zero(),
        }
    }
}

impl<T: Id> IdAllocator for DefaultAtomicIdAllocator<T>
where
    T::Inner: AtomicInteger,
{
    type IdType = T;

    fn allocate(&mut self) -> Self::IdType {
        <Self as AtomicIdAllocator>::allocate_atomic(&self)
    }

    fn is_valid(&self, id: Self::IdType) -> bool {
        let next_id = <T::Inner as AtomicInteger>::atomic_load(&self.next_id, Ordering::Relaxed);
        <T::Inner as Ord>::cmp(&id.into(), &next_id.into()).is_lt()
    }
}

impl<T: Id> AtomicIdAllocator for DefaultAtomicIdAllocator<T>
where
    T::Inner: AtomicInteger,
{
    fn allocate_atomic(&self) -> Self::IdType {
        T::Inner::atomic_fetch_add(&self.next_id, T::Inner::ONE, Ordering::Relaxed).into()
    }
}

impl<T: Id> Default for DefaultAtomicIdAllocator<T>
where
    T::Inner: AtomicInteger,
{
    fn default() -> Self {
        Self::new()
    }
}

/// 原子分配 + 串行回收的 ID 池。
pub struct AtomicAllocIdPool<T: GenId, A = DefaultAtomicIdAllocator<<T as GenId>::Index>>
where
    T::Index: AtomicInteger,
    A: AtomicIdAllocator<IdType = T::Index>,
{
    index_allocator: A,
    free_list: SpmcAtomicQueue<T::Index>,
    generations: SparseArray<T::Generation, Self>,
}

impl<T: GenId, A> SparseDefault<T::Generation> for AtomicAllocIdPool<T, A>
where
    T::Index: AtomicInteger,
    A: AtomicIdAllocator<IdType = T::Index>,
{
    const SPARSE_VALUE: T::Generation = T::Generation::ZERO;
}

impl<T: GenId, A> AtomicAllocIdPool<T, A>
where
    T::Index: AtomicInteger,
    A: AtomicIdAllocator<IdType = T::Index>,
{
    pub fn new_with_alloc(alloc: A) -> Self {
        Self {
            index_allocator: alloc,
            free_list: SpmcAtomicQueue::new(),
            generations: SparseArray::new(),
        }
    }
}

impl<T: GenId, A> AtomicAllocIdPool<T, A>
where
    T::Index: AtomicInteger,
    A: AtomicIdAllocator<IdType = T::Index> + Default,
{
    pub fn new() -> Self {
        Self {
            index_allocator: A::default(),
            free_list: SpmcAtomicQueue::new(),
            generations: SparseArray::new(),
        }
    }
}

impl<T: GenId, A> IdAllocator for AtomicAllocIdPool<T, A>
where
    T::Index: AtomicInteger,
    A: AtomicIdAllocator<IdType = T::Index>,
{
    type IdType = T;

    fn allocate(&mut self) -> Self::IdType {
        if let Some(index) = self.free_list.pop() {
            let generation = self.generations.get(index.as_usize());
            T::new_with_gen(index, generation)
        } else {
            let index = self.index_allocator.allocate();
            T::new_with_gen(index, T::Generation::ZERO)
        }
    }

    fn is_valid(&self, id: Self::IdType) -> bool {
        if !self.index_allocator.is_valid(id.get_index()) {
            return false;
        }
        let idx = id.get_index().as_usize();
        id.get_gen() == self.generations.get(idx)
    }
}

impl<T: GenId, A> AtomicIdAllocator for AtomicAllocIdPool<T, A>
where
    T::Index: AtomicInteger,
    A: AtomicIdAllocator<IdType = T::Index>,
{
    fn allocate_atomic(&self) -> Self::IdType {
        if let Some(index) = self.free_list.pop() {
            let generation = self.generations.get(index.as_usize());
            T::new_with_gen(index, generation)
        } else {
            let index = self.index_allocator.allocate_atomic();
            T::new_with_gen(index, T::Generation::ZERO)
        }
    }
}

impl<T: GenId, A> IdPool for AtomicAllocIdPool<T, A>
where
    T::Index: AtomicInteger,
    A: AtomicIdAllocator<IdType = T::Index>,
{
    fn deallocate(&mut self, id: Self::IdType) -> Result<(), IdPoolError> {
        let index = id.get_index();
        if !self.index_allocator.is_valid(id.get_index()) {
            return Err(IdPoolError::NotAllocated);
        }
        let idx = index.as_usize();
        let stored = self.generations.get(idx);
        if stored != id.get_gen() {
            return Err(IdPoolError::AlreadyDeallocated);
        }
        self.generations
            .set(idx, id.get_gen().wrapping_add(T::Generation::ONE));
        self.free_list.push(index);
        Ok(())
    }
}

impl<T: GenId, A> Default for AtomicAllocIdPool<T, A>
where
    T::Index: AtomicInteger,
    A: AtomicIdAllocator<IdType = T::Index> + Default,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fmt::Debug;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::{
        AtomicAllocIdPool, AtomicIdAllocator, DefaultAtomicIdAllocator, DefaultIdAllocator,
        DefaultIdPool, GenId, Id, IdAllocator, IdPool, IdPoolError,
    };
    use crate::Entity;

    fn check_id_allocator<A, I>(mut allocator: A)
    where
        A: IdAllocator<IdType = I>,
        I: Id + Debug,
        I::Inner: From<u8> + Debug,
    {
        let first = allocator.allocate();
        let second = allocator.allocate();
        let third = allocator.allocate();

        assert_eq!(first.get_id(), I::Inner::from(0));
        assert_eq!(second.get_id(), I::Inner::from(1));
        assert_eq!(third.get_id(), I::Inner::from(2));
        assert!(allocator.is_valid(first));
        assert!(allocator.is_valid(second));
        assert!(allocator.is_valid(third));
        assert!(!allocator.is_valid(I::new(I::Inner::from(3))));
    }

    fn check_atomic_id_allocator<A, I>(allocator: A)
    where
        A: AtomicIdAllocator<IdType = I> + Send + Sync + 'static,
        I: Id + Debug,
        I::Inner: From<u8> + Debug + Send + 'static,
    {
        let allocator = Arc::new(allocator);
        let allocated = Arc::new(Mutex::new(Vec::new()));
        let mut handles = Vec::new();

        for _ in 0..8 {
            let allocator = Arc::clone(&allocator);
            let allocated = Arc::clone(&allocated);

            handles.push(thread::spawn(move || {
                let mut local = Vec::new();
                for _ in 0..1_000 {
                    local.push(allocator.allocate_atomic().get_id());
                }
                allocated.lock().unwrap().extend(local);
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let allocated = allocated.lock().unwrap();
        let unique = allocated.iter().copied().collect::<HashSet<_>>();

        assert_eq!(allocated.len(), 8_000);
        assert_eq!(unique.len(), allocated.len());
        assert!(allocated.iter().all(|&id| allocator.is_valid(I::new(id))));
    }

    fn check_id_pool<P, I>(mut pool: P)
    where
        P: IdPool<IdType = I>,
        I: GenId + Debug,
        I::Index: From<u8> + Debug,
        I::Generation: From<u8> + Debug,
    {
        let first = pool.allocate();
        let second = pool.allocate();

        assert_eq!(first.get_index(), I::Index::from(0));
        assert_eq!(second.get_index(), I::Index::from(1));
        assert_eq!(first.get_gen(), I::Generation::from(0));
        assert_eq!(second.get_gen(), I::Generation::from(0));
        assert!(pool.is_valid(first));
        assert!(pool.is_valid(second));

        assert_eq!(pool.deallocate(first), Ok(()));
        assert!(!pool.is_valid(first));
        assert_eq!(pool.deallocate(first), Err(IdPoolError::AlreadyDeallocated));

        let reused = pool.allocate();
        assert_eq!(reused.get_index(), first.get_index());
        assert_eq!(reused.get_gen(), I::Generation::from(1));
        assert!(pool.is_valid(reused));
        assert!(pool.is_valid(second));

        let never_allocated = I::new_with_gen(I::Index::from(9), I::Generation::from(0));
        assert_eq!(
            pool.deallocate(never_allocated),
            Err(IdPoolError::NotAllocated)
        );
    }

    fn check_atomic_id_pool<P, I>(pool: P)
    where
        P: AtomicIdAllocator<IdType = I> + IdPool<IdType = I> + Send + Sync + 'static,
        I: GenId + Debug + Send + 'static,
        I::Index: From<u8> + Debug + Send + 'static,
        I::Generation: From<u8> + Debug,
    {
        let pool = Arc::new(pool);
        let allocated = Arc::new(Mutex::new(Vec::new()));
        let mut handles = Vec::new();

        for _ in 0..8 {
            let pool = Arc::clone(&pool);
            let allocated = Arc::clone(&allocated);

            handles.push(thread::spawn(move || {
                let mut local = Vec::new();
                for _ in 0..1_000 {
                    local.push(pool.allocate_atomic());
                }
                allocated.lock().unwrap().extend(local);
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let allocated = allocated.lock().unwrap();
        let unique = allocated
            .iter()
            .map(|id| id.get_index())
            .collect::<HashSet<_>>();

        assert_eq!(allocated.len(), 8_000);
        assert_eq!(unique.len(), allocated.len());
        assert!(allocated.iter().all(|&id| pool.is_valid(id)));
    }

    #[test]
    fn default_id_allocator_allocates_sequential_ids() {
        check_id_allocator::<DefaultIdAllocator<u32>, u32>(DefaultIdAllocator::new());
    }

    #[test]
    fn default_atomic_id_allocator_allocates_sequential_ids() {
        check_id_allocator::<DefaultAtomicIdAllocator<u32>, u32>(DefaultAtomicIdAllocator::new());
        check_atomic_id_allocator::<DefaultAtomicIdAllocator<u32>, u32>(
            DefaultAtomicIdAllocator::new(),
        );
    }

    #[test]
    fn default_id_pool_reuses_indices_with_new_generation() {
        check_id_pool::<DefaultIdPool<Entity>, Entity>(DefaultIdPool::new());
    }

    #[test]
    fn atomic_alloc_id_pool_allocates_from_shared_reference() {
        check_id_pool::<AtomicAllocIdPool<Entity>, Entity>(AtomicAllocIdPool::new());
        check_atomic_id_pool::<AtomicAllocIdPool<Entity>, Entity>(AtomicAllocIdPool::new());
    }
}
