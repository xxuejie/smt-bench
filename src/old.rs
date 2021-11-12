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

// RocksDB store leveraging existing code in godwoken, mostly unchanged,
// only adding read/write stats.
pub struct CountingStore<'a, DB: KVStore> {
    store: &'a DB,

    reads: Cell<usize>,
    writes: usize,
}

impl<'a, DB: KVStore> CountingStore<'a, DB> {
    pub fn new(store: &'a DB) -> Self {
        Self {
            store,
            reads: Cell::default(),
            writes: 0,
        }
    }

    // pub fn clear_stats(&mut self) {
    // self.reads.set(0);
    // self.writes = 0;
    // }

    pub fn stats(&self) -> String {
        format!("Reads: {}, writes: {}", self.reads.get(), self.writes)
    }
}

// Simulating Pack/Unpack trait impls
fn pack_key(key: &BranchKey) -> packed::SMTBranchKey {
    let height = key.height.into();
    let node_key: [u8; 32] = key.node_key.into();

    packed::SMTBranchKey::new_builder()
        .height(height)
        .node_key(node_key.pack())
        .build()
}

fn unpack_h256(value: &packed::Byte32Reader) -> H256 {
    let ptr = value.as_slice().as_ptr() as *const [u8; 32];
    let r = unsafe { *ptr };
    r.into()
}

fn unpack_merge_value(value: &packed::SMTMergeValueReader) -> MergeValue {
    match value.to_enum() {
        packed::SMTMergeValueUnionReader::SMTValue(smt_value) => {
            MergeValue::Value(unpack_h256(&smt_value.value()))
        }
        packed::SMTMergeValueUnionReader::SMTMergeWithZero(merge_with_zero) => {
            MergeValue::MergeWithZero {
                base_node: unpack_h256(&merge_with_zero.base_node()),
                zero_bits: unpack_h256(&merge_with_zero.zero_bits()),
                zero_count: merge_with_zero.zero_count().into(),
            }
        }
    }
}

fn pack_merge_value(value: &MergeValue) -> packed::SMTMergeValue {
    match value {
        MergeValue::Value(value) => {
            let smt_value = packed::SMTValue::new_builder()
                .value(Into::<[u8; 32]>::into(*value).pack())
                .build();

            packed::SMTMergeValue::new_builder()
                .set(packed::SMTMergeValueUnion::SMTValue(smt_value))
                .build()
        }
        MergeValue::MergeWithZero {
            base_node,
            zero_bits,
            zero_count,
        } => {
            let merge_with_zero = packed::SMTMergeWithZero::new_builder()
                .base_node(Into::<[u8; 32]>::into(*base_node).pack())
                .zero_bits(Into::<[u8; 32]>::into(*zero_bits).pack())
                .zero_count(Into::<packed::Byte>::into(*zero_count))
                .build();

            packed::SMTMergeValue::new_builder()
                .set(packed::SMTMergeValueUnion::SMTMergeWithZero(
                    merge_with_zero,
                ))
                .build()
        }
    }
}

fn pack_branch(branch: &BranchNode) -> packed::SMTBranchNode {
    packed::SMTBranchNode::new_builder()
        .left(pack_merge_value(&branch.left))
        .right(pack_merge_value(&branch.right))
        .build()
}

fn unpack_branch(branch: &packed::SMTBranchNodeReader) -> BranchNode {
    BranchNode {
        left: unpack_merge_value(&branch.left()),
        right: unpack_merge_value(&branch.right()),
    }
}

impl<'a, DB: KVStore> Store<H256> for CountingStore<'a, DB> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        self.reads.set(self.reads.get() + 1);
        let branch_key: packed::SMTBranchKey = pack_key(branch_key);
        match self.store.get(0, branch_key.as_slice()) {
            Some(slice) => {
                let branch = packed::SMTBranchNodeReader::from_slice_should_be_ok(slice.as_ref());
                Ok(Some(unpack_branch(&branch)))
            }
            None => Ok(None),
        }
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
        let branch_key: packed::SMTBranchKey = pack_key(&branch_key);
        let branch: packed::SMTBranchNode = pack_branch(&branch);

        self.writes += 1;
        self.store
            .insert_raw(0, branch_key.as_slice(), branch.as_slice())
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
        let branch_key: packed::SMTBranchKey = pack_key(branch_key);

        self.writes += 1;
        self.store
            .delete(0, branch_key.as_slice())
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }

    fn remove_leaf(&mut self, leaf_key: &H256) -> Result<(), SMTError> {
        self.store
            .delete(1, leaf_key.as_slice())
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }
}
