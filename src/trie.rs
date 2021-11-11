use crate::utils::*;
use gw_store::traits::KVStore;
use gw_types::{packed, prelude::*};
use sparse_merkle_tree::{
    error::Error as SMTError,
    merge::MergeValue,
    traits::Store,
    tree::{BranchKey, BranchNode},
    H256,
};
use std::cell::Cell;

const BYTE_SIZE: usize = 8;
const NODES_PER_TRIE: usize = (1 << BYTE_SIZE) - 1;
const MERGE_VALUE_SIZE: usize = 32 + 32 + 2;
const NODE_SIZE: usize = MERGE_VALUE_SIZE * 2;
const TRIE_SIZE: usize = NODES_PER_TRIE * NODE_SIZE;

struct BranchTrie {
    data: Vec<u8>,
    rounded_path: BranchKey,
}

impl BranchTrie {
    fn empty(rounded_path: BranchKey) -> Self {
        BranchTrie {
            data: vec![0u8; TRIE_SIZE],
            rounded_path,
        }
    }

    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        let index = self.calculate_index(branch_key);
        Ok(Some(self.load_branch_node(index)))
    }

    fn insert_branch(
        &mut self,
        branch_key: &BranchKey,
        branch: &BranchNode,
    ) -> Result<(), SMTError> {
        let index = self.calculate_index(branch_key);
        self.save_branch_node(index, branch);
        Ok(())
    }

    fn remove_branch(&mut self, branch_key: &BranchKey) -> Result<bool, SMTError> {
        let index = self.calculate_index(branch_key);
        let offset = index * NODE_SIZE;
        self.data[offset..offset + NODE_SIZE].fill(0);
        // TODO: we return true if current Trie contains no valid branches. For now
        // we always return false but this is an optimization that can be used to reduce
        // storage.
        Ok(false)
    }

    fn calculate_index(&self, branch_key: &BranchKey) -> usize {
        let index_byte =
            branch_key.node_key.as_slice()[self.rounded_path.height as usize / BYTE_SIZE];
        let inner_height: u8 = branch_key.height % BYTE_SIZE as u8;
        let base_index: usize = (1 << (8 - inner_height - 1)) - 1;
        let index = index_byte >> (inner_height + 1);
        base_index as usize + index as usize
    }

    fn load_branch_node(&self, index: usize) -> BranchNode {
        let offset = index * NODE_SIZE;
        BranchNode {
            left: self.load_merge_value(offset),
            right: self.load_merge_value(offset + MERGE_VALUE_SIZE),
        }
    }

    fn load_merge_value(&self, offset: usize) -> MergeValue {
        if self.data[offset] == 1 {
            // merge with zero type
            MergeValue::MergeWithZero {
                base_node: self.load_h256(offset + 2),
                zero_bits: self.load_h256(offset + 2 + 32),
                zero_count: self.data[offset + 1],
            }
        } else {
            // value type
            MergeValue::Value(self.load_h256(offset + 2))
        }
    }

    fn load_h256(&self, offset: usize) -> H256 {
        let mut buffer = [0u8; 32];
        buffer.copy_from_slice(&self.data[offset..offset + 32]);
        buffer.into()
    }

    fn save_branch_node(&mut self, index: usize, branch: &BranchNode) {
        let offset = index * NODE_SIZE;
        self.save_merge_value(offset, &branch.left);
        self.save_merge_value(offset + MERGE_VALUE_SIZE, &branch.right);
    }

    fn save_merge_value(&mut self, offset: usize, merge_value: &MergeValue) {
        match merge_value {
            MergeValue::Value(value) => {
                self.data[offset] = 0;
                self.save_h256(offset + 2, value);
            }
            MergeValue::MergeWithZero {
                base_node,
                zero_bits,
                zero_count,
            } => {
                self.data[offset] = 1;
                self.data[offset + 1] = *zero_count;
                self.save_h256(offset + 2, base_node);
                self.save_h256(offset + 2 + 32, zero_bits);
            }
        }
    }

    fn save_h256(&mut self, offset: usize, h: &H256) {
        self.data[offset..offset + 32].copy_from_slice(h.as_slice());
    }
}

pub struct TrieStore<'a, DB: KVStore> {
    store: &'a DB,

    reads: Cell<usize>,
    writes: usize,
    // cache: Cell<Option<BranchTrie>>,
}

fn round_branch_key(branch_key: &BranchKey) -> BranchKey {
    let rounded_height = (((branch_key.height as usize) / BYTE_SIZE + 1) * BYTE_SIZE - 1) as u8;
    BranchKey::new(
        rounded_height,
        branch_key.node_key.parent_path(rounded_height),
    )
}

impl<'a, DB: KVStore> TrieStore<'a, DB> {
    pub fn new(store: &'a DB) -> Self {
        Self {
            store,
            reads: Cell::default(),
            writes: 0,
        }
    }

    pub fn clear_stats(&mut self) {
        self.reads.set(0);
        self.writes = 0;
    }

    pub fn stats(&self) {
        println!("Reads: {}, writes: {}", self.reads.get(), self.writes);
    }
}

impl<'a, DB: KVStore> Store<H256> for TrieStore<'a, DB> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        let rounded_key = round_branch_key(branch_key);
        let packed_rounded_key: packed::SMTBranchKey = pack_key(&rounded_key);

        self.reads.set(self.reads.get() + 1);
        // TODO: cache
        let trie = match self.store.get(0, packed_rounded_key.as_slice()) {
            Some(slice) => {
                if slice.len() != TRIE_SIZE {
                    return Err(SMTError::Store("corrupted trie".to_string()));
                }
                BranchTrie {
                    data: slice.to_vec(),
                    rounded_path: rounded_key,
                }
            }
            None => return Ok(None),
        };

        trie.get_branch(branch_key)
    }

    fn get_leaf(&self, leaf_key: &H256) -> Result<Option<H256>, SMTError> {
        self.reads.set(self.reads.get() + 1);
        match self.store.get(1, leaf_key.as_slice()) {
            Some(slice) if 32 == slice.len() => {
                let mut leaf = [0u8; 32];
                leaf.copy_from_slice(slice.as_ref());
                Ok(Some(H256::from(leaf)))
            }
            Some(_) => Err(SMTError::Store("get corrupted leaf".to_string())),
            None => Ok(None),
        }
    }

    fn insert_branch(&mut self, branch_key: BranchKey, branch: BranchNode) -> Result<(), SMTError> {
        let rounded_key = round_branch_key(&branch_key);
        let packed_rounded_key: packed::SMTBranchKey = pack_key(&rounded_key);

        self.reads.set(self.reads.get() + 1);
        // TODO: cache
        let mut trie = match self.store.get(0, packed_rounded_key.as_slice()) {
            Some(slice) => {
                if slice.len() != TRIE_SIZE {
                    return Err(SMTError::Store("corrupted trie".to_string()));
                }
                BranchTrie {
                    data: slice.to_vec(),
                    rounded_path: rounded_key,
                }
            }
            None => BranchTrie::empty(rounded_key),
        };

        trie.insert_branch(&branch_key, &branch)?;
        self.writes += 1;
        self.store
            .insert_raw(0, packed_rounded_key.as_slice(), trie.data.as_slice())
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn insert_leaf(&mut self, leaf_key: H256, leaf: H256) -> Result<(), SMTError> {
        self.writes += 1;
        self.store
            .insert_raw(1, leaf_key.as_slice(), leaf.as_slice())
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn remove_branch(&mut self, branch_key: &BranchKey) -> Result<(), SMTError> {
        let rounded_key = round_branch_key(branch_key);
        let packed_rounded_key: packed::SMTBranchKey = pack_key(&rounded_key);

        self.reads.set(self.reads.get() + 1);
        // TODO: cache
        let mut trie = match self.store.get(0, packed_rounded_key.as_slice()) {
            Some(slice) => {
                if slice.len() != TRIE_SIZE {
                    return Err(SMTError::Store("corrupted trie".to_string()));
                }
                BranchTrie {
                    data: slice.to_vec(),
                    rounded_path: rounded_key,
                }
            }
            None => BranchTrie::empty(rounded_key),
        };

        let should_remove = trie.remove_branch(branch_key)?;
        self.writes += 1;
        if should_remove {
            self.store
                .delete(0, packed_rounded_key.as_slice())
                .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;
        } else {
            self.store
                .insert_raw(0, packed_rounded_key.as_slice(), trie.data.as_slice())
                .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;
        }

        Ok(())
    }

    fn remove_leaf(&mut self, leaf_key: &H256) -> Result<(), SMTError> {
        self.store
            .delete(1, leaf_key.as_slice())
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }
}
