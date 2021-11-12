mod old;

extern crate cpuprofiler;

use crate::old::CountingStore;
use gw_config::StoreConfig;
use gw_db::RocksDB;
use gw_store::Store as GwStore;
use rand_chacha::{
    rand_core::{RngCore, SeedableRng},
    ChaCha20Rng,
};
use sparse_merkle_tree::{blake2b::Blake2bHasher, SparseMerkleTree, H256};
use std::path::PathBuf;
use std::time::Instant;

fn random_h256(rng: &mut impl RngCore) -> H256 {
    let mut buf = [0u8; 32];
    rng.fill_bytes(&mut buf);
    buf.into()
}

type SMT<'a, DB> = SparseMerkleTree<Blake2bHasher, H256, CountingStore<'a, DB>>;

fn main() {
    use cpuprofiler::PROFILER;
    PROFILER.lock().unwrap().start("./my-prof.profile").unwrap();

    let mut rng = ChaCha20Rng::seed_from_u64(0);

    let config = StoreConfig {
        path: PathBuf::from("./store.db".to_string()),
        ..Default::default()
    };
    let db = RocksDB::open(&config, 10);
    let store = GwStore::new(db);

    // Initializing
    let mut root = {
        let tx = store.begin_transaction();
        let store = CountingStore::new(&tx);
        let mut smt = SMT::new(H256::default(), store);
        for _ in 0..200 {
            let key = random_h256(&mut rng);
            let value = random_h256(&mut rng);
            smt.update(key, value).unwrap();
        }
        let root = smt.root().clone();
        tx.commit().unwrap();
        root
    };

    // Testing
    let a = Instant::now();
    for i in 0..100 {
        let b = Instant::now();
        let mut pairs = vec![];
        for _ in 0..1000 {
            let key = random_h256(&mut rng);
            let value = random_h256(&mut rng);
            pairs.push((key, value));
        }
        let tx = store.begin_transaction();
        let store = CountingStore::new(&tx);
        let mut smt = SMT::new(root, store);
        smt.update_all(pairs).unwrap();
        tx.commit().unwrap();
        println!(
            "Round #{}, elapsed time: {:?}, stats: {}",
            i,
            b.elapsed(),
            smt.store().stats()
        );
        root = smt.root().clone();
    }
    println!("\nRunning time: {:?}", a.elapsed());
    println!("Final root: {:?}", root);
}
