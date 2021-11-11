mod old;
mod trie;
mod utils;

// extern crate cpuprofiler;

use crate::{old::CountingStore, trie::TrieStore};
use gw_config::StoreConfig;
use gw_db::RocksDB;
use gw_store::Store as GwStore;
use rand_chacha::{
    rand_core::{RngCore, SeedableRng},
    ChaCha20Rng,
};
use sparse_merkle_tree::{blake2b::Blake2bHasher, SparseMerkleTree, H256};
use std::path::PathBuf;

fn random_h256(rng: &mut impl RngCore) -> H256 {
    let mut buf = [0u8; 32];
    rng.fill_bytes(&mut buf);
    buf.into()
}

type SMT<'a, DB> = SparseMerkleTree<Blake2bHasher, H256, CountingStore<'a, DB>>;
type SMT2<'a, DB> = SparseMerkleTree<Blake2bHasher, H256, TrieStore<'a, DB>>;

fn main() {
    // use cpuprofiler::PROFILER;
    // PROFILER.lock().unwrap().start("./my-prof.profile").unwrap();

    let mut rng = ChaCha20Rng::seed_from_u64(0);

    // let store = GwStore::open_tmp().unwrap();
    let config2 = StoreConfig{path: PathBuf::from("./store2.db".to_string()), ..Default::default()};
    let db2 = RocksDB::open(&config2, 10);
    let store2 = GwStore::new(db2);

    // Initializing
    let root = {
        // let tx = store.begin_transaction();
        // let store = CountingStore::new(&tx);
        // let mut smt = SMT::new(H256::default(), store);

        let tx2 = store2.begin_transaction();
        let store2 = TrieStore::new(&tx2);
        let mut smt2 = SMT2::new(H256::default(), store2);

        for _ in 0..200 {
            let key = random_h256(&mut rng);
            let value = random_h256(&mut rng);
            // smt.update(key, value).unwrap();
            smt2.update(key, value).unwrap();
        }
        // assert_eq!(smt.root(), smt2.root());
        let root = smt2.root().clone();

        // tx.commit().unwrap();
        tx2.commit().unwrap();

        root
    };

    // Testing
    let mut pairs = vec![];
    for _ in 0..10000 {
        let key = random_h256(&mut rng);
        let value = random_h256(&mut rng);
        pairs.push((key, value));
    }

    // let tx = store.begin_transaction();
    // let store = CountingStore::new(&tx);
    // let mut smt = SMT::new(root, store);
    // smt.update_all(pairs.clone()).unwrap();
    // smt.store().stats();
    // tx.commit().unwrap();

    println!("Begin transaction");
    let tx2 = store2.begin_transaction();
    let store2 = TrieStore::new(&tx2);
    let mut smt2 = SMT2::new(root, store2);
    println!("Update all");
    smt2.update_all(pairs).unwrap();
    smt2.store().stats();
    tx2.commit().unwrap();

    // assert_eq!(smt.root(), smt2.root());
}
