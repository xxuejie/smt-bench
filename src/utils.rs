use gw_types::{packed, prelude::*};
use sparse_merkle_tree::{
    merge::MergeValue,
    tree::{BranchKey, BranchNode},
    H256,
};

// Simulating Pack/Unpack trait impls
pub fn pack_key(key: &BranchKey) -> packed::SMTBranchKey {
    let height = key.height.into();
    let node_key: [u8; 32] = key.node_key.into();

    packed::SMTBranchKey::new_builder()
        .height(height)
        .node_key(node_key.pack())
        .build()
}

pub fn unpack_h256(value: &packed::Byte32Reader) -> H256 {
    let ptr = value.as_slice().as_ptr() as *const [u8; 32];
    let r = unsafe { *ptr };
    r.into()
}

pub fn unpack_merge_value(value: &packed::SMTMergeValueReader) -> MergeValue {
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

pub fn pack_merge_value(value: &MergeValue) -> packed::SMTMergeValue {
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

pub fn pack_branch(branch: &BranchNode) -> packed::SMTBranchNode {
    packed::SMTBranchNode::new_builder()
        .left(pack_merge_value(&branch.left))
        .right(pack_merge_value(&branch.right))
        .build()
}

pub fn unpack_branch(branch: &packed::SMTBranchNodeReader) -> BranchNode {
    BranchNode {
        left: unpack_merge_value(&branch.left()),
        right: unpack_merge_value(&branch.right()),
    }
}
