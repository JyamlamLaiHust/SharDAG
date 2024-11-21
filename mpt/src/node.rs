// This file is part of Darwinia.
//
// Copyright (C) 2018-2021 Darwinia Network
// SPDX-License-Identifier: GPL-3.0
//
// Darwinia is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Darwinia is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Darwinia. If not, see <https://www.gnu.org/licenses/>.

use std::sync::Mutex;
use std::sync::Arc;
use sp_std::prelude::*;

use crate::nibbles::Nibbles;

#[derive(Debug, Clone)]
pub enum Node {
	Empty,
	Leaf(Arc<Mutex<LeafNode>>),
	Extension(Arc<Mutex<ExtensionNode>>),
	Branch(Arc<Mutex<BranchNode>>),
	Hash(Arc<Mutex<HashNode>>),

  // Leaf(Rc<RefCell<LeafNode>>),
	// Extension(Rc<RefCell<ExtensionNode>>),
	// Branch(Rc<RefCell<BranchNode>>),
	// Hash(Rc<RefCell<HashNode>>),
}

impl Node {
	pub fn from_leaf(key: Nibbles, value: Vec<u8>) -> Self {
		let leaf = Arc::new(Mutex::new(LeafNode { key, value }));
		Node::Leaf(leaf)
	}

	pub fn from_branch(children: [Node; 16], value: Option<Vec<u8>>) -> Self {
		let branch = Arc::new(Mutex::new(BranchNode { children, value }));
		Node::Branch(branch)
	}

	pub fn from_extension(prefix: Nibbles, node: Node) -> Self {
		let ext = Arc::new(Mutex::new(ExtensionNode { prefix, node }));
		Node::Extension(ext)
	}

	pub fn from_hash(hash: Vec<u8>) -> Self {
		let hash_node = Arc::new(Mutex::new(HashNode { hash }));
		Node::Hash(hash_node)
	}
}

#[derive(Debug)]
pub struct LeafNode {
	pub key: Nibbles,
	pub value: Vec<u8>,
}

#[derive(Debug)]
pub struct BranchNode {
	pub children: [Node; 16],
	pub value: Option<Vec<u8>>,
}

impl BranchNode {
	pub fn insert(&mut self, i: usize, n: Node) {
		if i == 16 {
			match n {
				Node::Leaf(leaf) => {
          self.value = Some(leaf.lock().unwrap().value.clone());
					// self.value = Some(leaf.borrow().value.clone());
				}
				_ => panic!("The n must be leaf node"),
			}
		} else {
			self.children[i] = n
		}
	}
}

#[derive(Debug)]
pub struct ExtensionNode {
	pub prefix: Nibbles,
	pub node: Node,
}

#[derive(Debug)]
pub struct HashNode {
	pub hash: Vec<u8>,
}

pub fn empty_children() -> [Node; 16] {
	[
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
		Node::Empty,
	]
}
