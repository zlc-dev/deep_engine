use std::sync::{LazyLock, Mutex};

use crate::id::{GenId, Id, IdAllocator, IdPool, IdPoolError};

/// Lightweight entity identifier.
///
/// An `Entity` is a pair of `(index, generation)`:
/// - `index` locates the entity's slot in the sparse/dense storage.
/// - `generation` is incremented each time the slot is reused,
///   so stale references to a deleted entity can be detected.
///
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Entity {
    /// Slot index in the entity storage.
    index: u32,
    /// Generation counter; incremented on slot reuse.
    generation: u32,
}

impl Entity {
    /// A sentinel value representing "no entity" (index = u32::MAX, generation = 0).
    pub const PLACEHOLDER: Self = Self {
        index: u32::MAX,
        generation: 0,
    };

    /// Create an `Entity` from raw `index` and `generation`.
    #[inline]
    pub const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Pack the entity into a single `u64`.
    ///
    /// Layout (matching Bevy): upper 32 bits = generation, lower 32 bits = index.
    #[inline]
    pub const fn to_bits(self) -> u64 {
        ((self.generation as u64) << 32) | (self.index as u64)
    }

    #[inline]
    pub const fn from_bits(bits: u64) -> Self {
        Self {
            index: (bits & 0xFFFF_FFFF) as u32,
            generation: ((bits >> 32) & 0xFFFF_FFFF) as u32,
        }
    }
}

impl Default for Entity {
    fn default() -> Self {
        Self::PLACEHOLDER
    }
}

impl std::fmt::Display for Entity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}

impl From<Entity> for u64 {
    #[inline]
    fn from(e: Entity) -> u64 {
        e.to_bits()
    }
}

impl From<u64> for Entity {
    #[inline]
    fn from(bits: u64) -> Entity {
        Entity::from_bits(bits)
    }
}

impl From<(u32, u32)> for Entity {
    #[inline]
    fn from((index, generation): (u32, u32)) -> Entity {
        Entity::new(index, generation)
    }
}

impl Id for Entity {
    type Inner = u64;
}

impl GenId for Entity {
    type Index = u32;
    type Generation = u32;

    fn new_with_gen(index: Self::Index, generation: Self::Generation) -> Self {
        Self { index, generation }
    }

    fn get_index(&self) -> Self::Index {
        self.index
    }

    fn get_gen(&self) -> Self::Generation {
        self.generation
    }
}
