use std::{
    mem,
    ops::{Deref, DerefMut},
};

use slab::Slab;
use wasm_runtime_layer::backend::{
    AsContext, AsContextMut, WasmEngine, WasmStore, WasmStoreContext, WasmStoreContextMut,
};

use crate::{instance::InstanceInner, Engine, Instance};

/// Owns all the data for the wasm module
///
/// Can be cheaply cloned
///
/// The data is retained through the lifetime of the store, and no GC will collect data from
/// no-longer used modules. It is as such recommended to have the stores lifetime correspond to its
/// modules, and not repeatedly create and drop modules within an existing store, but rather create
/// a new store for it, to avoid unbounded memory use.
pub struct Store<T> {
    /// The internal store is kept behind a pointer.
    ///
    /// This is to allow referencing and reconstructing a calling context in exported functions,
    /// where it is not possible to prove the correct lifetime and borrowing rules statically nor
    /// dynamically using RefCells. This is because functions can be re-entrant with exclusive but
    /// stacked calling contexts. [`std::cell::RefCell`] and [`std::cell::RefMut`] do not allow
    /// for recursive usage by design (and it would be nigh impossible and quite expensive to enforce at runtime).
    ///
    /// The store is stored through a raw pointer, as using a `Pin<Box<T>>` would not be possible,
    /// despite the memory location of the Box contents technically being pinned in memory. This is
    /// because of the stacked borrows model.
    ///
    /// When the outer box is moved, it invalidates all tags in its borrow stack, even
    /// though the memory location remains. This invalidates all references and raw pointers to `T`
    /// created from the Box.
    ///
    /// See: <https://blog.nilstrieb.dev/posts/box-is-a-unique-type/> for more details.
    ///
    /// By using a box here, we would leave invalid pointers with revoked access permissions to the
    /// memory location of `T`.
    ///
    /// This creates undefined behavior as the Rust compiler will incorrectly optimize register
    /// accesses and memory loading and incorrect no-alias attributes.
    ///
    /// To circumvent this we can use a raw pointer obtained from unwrapping a Box.
    ///
    /// # Playground
    ///
    /// - `Pin<Box<T>>` solution (UB): <https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=685c984584bc0ca1faa780ca292f406c>
    /// - raw pointer solution (sound): <https://play.rust-lang.org/?version=stable&mode=release&edition=2021&gist=257841cb1675106d55c756ad59fde2fb>
    ///
    /// You can use `Tools > Miri` to test the validity
    inner: *mut StoreInner<T>,
}

impl<T> Store<T> {
    /// Creates a new store from the inner box
    fn from_inner(inner: Box<StoreInner<T>>) -> Self {
        Self {
            inner: Box::into_raw(inner),
        }
    }

    /// Returns a borrow of the store
    pub(crate) fn get(&self) -> StoreContext<T> {
        // Safety:
        //
        // A shared reference to the store signifies a non-mutable ownership, and is thus safe.
        let inner = unsafe { &*self.inner };
        StoreContext::from_ref(inner)
    }

    /// Returns a mutable borrow of the store
    pub(crate) fn get_mut(&mut self) -> StoreContextMut<T> {
        // Safety:
        //
        // &mut self
        let inner = unsafe { &mut *self.inner };
        StoreContextMut::from_ref(inner)
    }
}

impl<T> Drop for Store<T> {
    fn drop(&mut self) {
        unsafe { drop(Box::from_raw(self.inner)) }
    }
}

impl<T> WasmStore<T, Engine> for Store<T> {
    fn new(engine: &Engine, data: T) -> Self {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("Store::new").entered();
        Self::from_inner(Box::new(StoreInner {
            engine: engine.clone(),
            instances: Slab::new(),
            data,
        }))
    }

    fn engine(&self) -> &Engine {
        &self.get().store.engine
    }

    fn data(&self) -> &T {
        &self.get().store.data
    }

    fn data_mut(&mut self) -> &mut T {
        &mut self.get_mut().store.data
    }

    fn into_data(self) -> T {
        // Safety:
        //
        // Ownership of `self` signifies that no guest stack is currently active
        let ptr = unsafe { Box::from_raw(self.inner) };

        // Don't execute drop for `Store`. This impl deallocates the whole box, which we don't
        // want.
        //
        // The box will be deallocated at the end of this scope
        mem::forget(self);

        ptr.data
    }
}

impl<T> AsContext<Engine> for Store<T> {
    type UserState = T;

    fn as_context(&self) -> <Engine as WasmEngine>::StoreContext<'_, Self::UserState> {
        self.get()
    }
}

impl<T> AsContextMut<Engine> for Store<T> {
    fn as_context_mut(&mut self) -> StoreContextMut<T> {
        self.get_mut()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Store<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

#[derive(Debug)]
/// Holds the inner state of the store
pub struct StoreInner<T> {
    /// The engine used
    pub(crate) engine: Engine,
    /// Instances are not Send + Sync
    pub(crate) instances: Slab<InstanceInner>,
    /// The user data
    pub(crate) data: T,
}

impl<T> StoreInner<T> {
    /// Inserts a new instance and returns its id
    pub(crate) fn insert_instance(&mut self, instance: InstanceInner) -> Instance {
        Instance {
            id: self.instances.insert(instance),
        }
    }
}

/// Immutable context to the store
pub struct StoreContext<'a, T: 'a> {
    /// The store
    store: &'a StoreInner<T>,
}

impl<'a, T: 'a> StoreContext<'a, T> {
    /// Provides a store context from a reference
    pub fn from_ref(store: &'a StoreInner<T>) -> Self {
        Self { store }
    }
}

impl<'a, T> Deref for StoreContext<'a, T> {
    type Target = StoreInner<T>;

    fn deref(&self) -> &Self::Target {
        self.store
    }
}

/// Mutable context to the store
pub struct StoreContextMut<'a, T: 'a> {
    /// The store
    store: &'a mut StoreInner<T>,
}

impl<'a, T: 'a> StoreContextMut<'a, T> {
    /// Provides a mutable store context from a reference
    pub(crate) fn from_ref(store: &'a mut StoreInner<T>) -> Self {
        Self { store }
    }
}

impl<'a, T> Deref for StoreContextMut<'a, T> {
    type Target = StoreInner<T>;

    fn deref(&self) -> &Self::Target {
        &*self.store
    }
}

impl<'a, T> DerefMut for StoreContextMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.store
    }
}

impl<'a, T: 'a> WasmStoreContext<'a, T, Engine> for StoreContext<'a, T> {
    fn engine(&self) -> &Engine {
        &self.engine
    }

    fn data(&self) -> &T {
        &self.data
    }
}

impl<'a, T: 'a> AsContext<Engine> for StoreContext<'a, T> {
    type UserState = T;

    fn as_context(&self) -> StoreContext<'_, T> {
        StoreContext { store: self.store }
    }
}

impl<'a, T: 'a> WasmStoreContext<'a, T, Engine> for StoreContextMut<'a, T> {
    fn engine(&self) -> &Engine {
        &self.engine
    }

    fn data(&self) -> &T {
        &self.data
    }
}

impl<'a, T: 'a> WasmStoreContextMut<'a, T, Engine> for StoreContextMut<'a, T> {
    fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

impl<'a, T: 'a> AsContext<Engine> for StoreContextMut<'a, T> {
    type UserState = T;

    fn as_context(&self) -> <Engine as WasmEngine>::StoreContext<'_, T> {
        StoreContext { store: self.store }
    }
}

impl<'a, T: 'a> AsContextMut<Engine> for StoreContextMut<'a, T> {
    fn as_context_mut(&mut self) -> StoreContextMut<'_, T> {
        StoreContextMut { store: self.store }
    }
}
