// Copyright 2019-2020 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Provides an interface around the vector used to store elements of the
//! [`BinaryHeap`](`super::BinaryHeap`) in storage. This is necessary since
//! we don't just store each element in it's own storage cell, but rather
//! optimize storage access by putting children together in one storage cell.

use super::{
    children,
    children::Children,
    Iter,
    IterMut,
    StorageVec,
};
use crate::storage2::{
    traits::{
        KeyPtr,
        PackedLayout,
        SpreadLayout,
    },
    Lazy,
};

/// Provides an interface for accessing elements in the `BinaryHeap`.
///
/// Elements of the heap are stored in a vector of `Children` objects, whereby
/// each `Children` object contains two elements. When operating on indices of
/// the `BinaryHeap` this interface transposes heap indices to the child inside
/// the `Children` object, in which the element is stored.
#[derive(Default, PartialEq, Eq, Debug)]
pub(crate) struct ChildrenVector<T>
where
    T: PackedLayout + Ord,
{
    /// The number of elements stored in the heap.
    /// We cannot use the length of the storage vector, since each entry (i.e. each
    /// `Children` object) in the vector contains two child elements (except the root
    /// element which occupies a `Children` object on its own.
    len: Lazy<u32>,
    /// The underlying storage vec containing the `Children`.
    children: StorageVec<Children<T>>,
}

/// Encapsulates information regarding a particular child.
pub(crate) struct ChildInfo<'a, T> {
    /// A reference to the value in this child, if existent.
    pub(crate) child: &'a Option<T>,
}

impl<'a, T> ChildInfo<'a, T> {
    /// Creates a new `ChildInfo` object.
    fn new(child: &'a Option<T>) -> Self {
        Self { child }
    }
}

/// Encapsulates information regarding a particular child.
pub(crate) struct ChildInfoMut<'a, T> {
    /// A mutable reference to the value in this child, if existent.
    pub(crate) child: &'a mut Option<T>,
    /// The number of children which are set in this `Children` object.
    pub(crate) child_count: usize,
}

impl<'a, T> ChildInfoMut<'a, T> {
    /// Creates a new `ChildInfoMut` object.
    fn new(child: &'a mut Option<T>, child_count: usize) -> Self {
        Self { child, child_count }
    }
}

impl<T> ChildrenVector<T>
where
    T: PackedLayout + Ord,
{
    /// Creates a new empty storage heap.
    pub fn new() -> Self {
        Self {
            len: Lazy::new(0),
            children: StorageVec::new(),
        }
    }

    /// Returns the number of elements in the heap, also referred to as its 'length'.
    pub fn len(&self) -> u32 {
        *self.len
    }

    /// Returns the amount of `Children` objects stored in the vector.
    #[allow(dead_code)]
    #[cfg(all(test, feature = "ink-fuzz-tests"))]
    pub fn children_count(&self) -> u32 {
        self.children.len()
    }

    /// Returns `true` if the heap contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a shared reference to the indexed element.
    ///
    /// Returns `None` if `index` is out of bounds.
    pub fn get(&self, index: u32) -> Option<&T> {
        self.get_child(index)?.child.as_ref()
    }

    /// Returns an exclusive reference to the indexed element.
    /// The element in a `Children` is an `Option<T>`.
    ///
    /// Returns `None` if `index` is out of bounds.
    pub fn get_mut(&mut self, index: u32) -> Option<&mut T> {
        let child_info = self.get_child_mut(index)?;
        child_info.child.as_mut()
    }

    /// Swaps the elements at the given indices.
    ///
    /// # Panics
    ///
    /// If one or both indices are out of bounds.
    pub fn swap(&mut self, a: u32, b: u32) {
        if a == b {
            return
        }
        assert!(a < self.len(), "a is out of bounds");
        assert!(b < self.len(), "b is out of bounds");

        let child_info_a = self.get_child_mut(a).expect("index a must exist");
        let a_opt = child_info_a.child.take();

        let child_info_b = self.get_child_mut(b).expect("index b must exist");
        let b_opt = core::mem::replace(child_info_b.child, a_opt);

        let child_info_a = self.get_child_mut(a).expect("index a must exist");
        *child_info_a.child = b_opt;
    }

    /// Removes the element at `index` from the heap and returns it.
    ///
    /// The last element of the heap is put into the slot at `index`.
    /// Returns `None` and does not mutate the heap is empty.
    pub fn swap_remove(&mut self, index: u32) -> Option<T> {
        if self.is_empty() {
            return None
        }
        self.swap(index, self.len() - 1);
        self.pop()
    }

    /// Returns an iterator yielding shared references to all elements of the heap.
    ///
    /// # Note
    ///
    /// Avoid unbounded iteration over big storage heaps.
    /// Prefer using methods like `Iterator::take` in order to limit the number
    /// of yielded elements.
    pub fn iter(&self) -> Iter<T> {
        Iter::new(&self)
    }

    /// Returns an iterator yielding exclusive references to all elements of the heap.
    ///
    /// # Note
    ///
    /// Avoid unbounded iteration over big storage heaps.
    /// Prefer using methods like `Iterator::take` in order to limit the number
    /// of yielded elements.
    pub fn iter_mut(&mut self) -> IterMut<T> {
        IterMut::new(self)
    }

    /// Returns a shared reference to the first element if any.
    pub fn first(&self) -> Option<&T> {
        if self.is_empty() {
            return None
        }
        self.get(0)
    }

    /// Returns an exclusive reference to the first element if any.
    pub fn first_mut(&mut self) -> Option<&mut T> {
        if self.is_empty() {
            return None
        }
        self.get_mut(0)
    }

    /// Removes all elements from this heap.
    ///
    /// # Note
    ///
    /// Use this method to clear the heap instead of e.g. iterative `pop()`.
    /// This method performs significantly better and does not actually read
    /// any of the elements (whereas `pop()` does).
    pub fn clear(&mut self) {
        if self.is_empty() {
            return
        }
        self.children.clear();
        self.len = Lazy::new(0);
    }

    /// Appends an element to the back of the heap.
    pub fn push(&mut self, value: T) {
        assert!(
            self.len() < core::u32::MAX,
            "cannot push more elements into the storage heap"
        );
        let last_index = self.len();
        *self.len += 1;
        self.push_to(last_index, Some(value));
    }

    /// Returns information about the child at the heap index if any.
    pub fn get_child(&self, index: u32) -> Option<ChildInfo<T>> {
        let storage_index = children::get_children_storage_index(index);
        let child_pos = children::get_child_pos(index);
        let children = self.children.get(storage_index)?;
        let child = children.child(child_pos);
        Some(ChildInfo::new(child))
    }

    /// Returns information about the child at the heap index if any.
    ///
    /// The returned `ChildInfoMut` contains a mutable reference to the value `T`.
    pub fn get_child_mut(&mut self, index: u32) -> Option<ChildInfoMut<T>> {
        let storage_index = children::get_children_storage_index(index);
        let child_pos = children::get_child_pos(index);
        let children = self.children.get_mut(storage_index)?;
        let count = children.count();
        let child = children.child_mut(child_pos);
        Some(ChildInfoMut::new(child, count))
    }

    /// Pushes `value` to the heap index `index`.
    ///
    /// If there is already a child in storage which `index` resolves to
    /// then `value` is inserted there. Otherwise a new child is created.
    fn push_to(&mut self, index: u32, value: Option<T>) {
        let info = self.get_child_mut(index);
        if let Some(info) = info {
            *info.child = value;
            return
        }

        self.children.push(Children::new(value, None));
        debug_assert!(
            {
                let storage_index = children::get_children_storage_index(index);
                self.children.get(storage_index).is_some()
            },
            "the new children were not placed at children_index!"
        );
    }

    /// Pops the last element from the heap and returns it.
    //
    /// Returns `None` if the heap is empty.
    fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None
        }
        let last_index = self.len() - 1;
        *self.len = last_index;

        let info = self
            .get_child_mut(last_index)
            .expect("children must exist at last_index");
        let popped_val = info.child.take();
        if info.child_count == 1 {
            // if both children are non-existent the entire children object can be removed
            self.children.pop();
        }
        popped_val
    }
}

impl<T> SpreadLayout for ChildrenVector<T>
where
    T: SpreadLayout + Ord + PackedLayout,
{
    const FOOTPRINT: u64 = 1 + <StorageVec<Children<T>> as SpreadLayout>::FOOTPRINT;

    fn pull_spread(ptr: &mut KeyPtr) -> Self {
        let len = SpreadLayout::pull_spread(ptr);
        let children = SpreadLayout::pull_spread(ptr);
        Self { len, children }
    }

    fn push_spread(&self, ptr: &mut KeyPtr) {
        SpreadLayout::push_spread(&self.len, ptr);
        SpreadLayout::push_spread(&self.children, ptr);
    }

    fn clear_spread(&self, ptr: &mut KeyPtr) {
        SpreadLayout::clear_spread(&self.len, ptr);
        SpreadLayout::clear_spread(&self.children, ptr);
    }
}
