use std::{collections::HashMap, fs::File};
use crate::{messages::{Address, Amount}, RWSet, Frame, Account2Shard, acc_shard::{AccToShardItem, ActAccToShardItem}};
use async_trait::async_trait;
use config::ShardId;
use csv::DeserializeRecordsIter;
use hex::FromHex;
use log::info;
use mpt::{MPTStore, MPTStoreTrait, RootHash, Key, Proof, MerklePatriciaTrie, Trie, Value, MMPTStore};
use serde::{Deserialize, Serialize};
use tokio::{time::Instant, sync::{oneshot, mpsc}};
use std::thread;
use futures::executor::block_on;
use std::sync::Arc;
use num_enum::TryFromPrimitive;
use tokio::sync::mpsc::{channel, Sender, Receiver};


pub const INIT_BALANCE: f64 = 100000000000000000000000000000000000000000000.0;

#[derive(TryFromPrimitive, Debug, Clone)]
#[repr(usize)]
pub enum StateStoreType {
    TStore,
    MStore,
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct RawState {
  pub tx_index: u64,
  pub block_number: u64,
  pub account: String,
  pub balance: Amount,
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct BrokerItem {
  pub account: String,
  pub freq: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Account {
    pub nonce: i64, 
    pub balance: Amount,
}


#[derive(Debug)]
pub struct AvatartStore {
  pub avatars: HashMap<ShardId, HashMap<Address, Amount>>
}

impl AvatartStore {
  pub fn new() -> Self {
    Self{
      avatars: HashMap::default()
    }
  }

  pub fn insert(&mut self, frame: &Frame) {
    let avatars = self.avatars.entry(frame.shardid).or_insert(HashMap::new());
    for rwset in &frame.rwset {
      avatars.insert(rwset.addr.clone(), rwset.value);
    }
  }
}


#[async_trait]
pub trait StateStore {
  async fn insert(&mut self, key: Vec<u8>, value: Vec<u8>);
  async fn get(&mut self, key: &[u8]) -> Option<Vec<u8>>;
  async fn root(&mut self) -> (Option<RootHash>, Option<RootHash>);
  
  async fn test_migration(
    &mut self, 
    out_act_accs: Vec<Vec<u8>>,
    out_dor_accs: Vec<Vec<u8>>,
    epoch: u64,
    shard_id: ShardId,
    target_shard_id: ShardId,    
    act_root_hash: Vec<u8>,
    dor_root_hash: Vec<u8>, 
    act_accs: Vec<Vec<u8>>, 
    dor_accs: Vec<Vec<u8>>,
  ) -> (usize, u128);

  /// output info for test_exec
  async fn test(&self) -> (usize, usize, f64, f64);
}



// Initialize state store
pub async fn new_primary_store(
  shard_id: ShardId,
  acc2shard_file: &str, 
  actacc2shard_file: &str,
  acc2shard: &Box<dyn Account2Shard + Send>,
  state_store_type: StateStoreType, 
  full_t_path: &str, 
) -> Box<dyn StateStore + Send> {
  let store: Box<dyn StateStore + Send>;
  match state_store_type {
    StateStoreType::MStore => {
      println!("initialize MStore");
      store = Box::new(MStore::new(shard_id, &acc2shard_file, &acc2shard, full_t_path).await);
    }
    StateStoreType::TStore => {
      println!("initialize TStore");
      store = Box::new(TStore::new(shard_id, &acc2shard_file, &actacc2shard_file, &acc2shard, full_t_path).await);
    }
  }
  store
}

// load initial account state csv
pub async fn load_accs(
  shard_id: ShardId,
  acc2shard_file: &str, 
  acc2shard: &Box<dyn Account2Shard + Send>,
  full_t_path: &str,
) -> MPTStore{

  let mut full_t = MPTStore::new(full_t_path);

  info!("Begin loading account...");
  println!("Begin loading account...");
  let mut loaded_local_accs = 0;
  let mut loaded_accs = 0;
  let before_load = Instant::now();

  let mut reader = csv::Reader::from_path(acc2shard_file).unwrap();
  let mut state_iter: DeserializeRecordsIter<File, AccToShardItem> = reader.deserialize().into_iter();
  while let Some(Ok(acc_shard)) = state_iter.next(){
    loaded_accs += 1;
    let addr = Vec::from_hex(&acc_shard.account[2..]).unwrap();
    // get account's shardid according to acc2shard policy
    let _shard_id = acc2shard.get_shard(&addr);
    if _shard_id == shard_id { // local account
      let acc = Account { nonce: 0, balance: INIT_BALANCE};
      let serialized = bincode::serialize(&acc).expect("Failed to serialize account");
      if full_t.insert(addr, serialized).await.unwrap(){
        loaded_local_accs+=1;
      }
    }
  }
  let dur = before_load.elapsed().as_millis();
  info!(
    "Total {} accounts for initializing current epoch from acc2shard_file! load {} local accounts, take {} ms",
    loaded_accs, loaded_local_accs, dur
  );
  println!(
    "Total {} accounts for initializing current epoch from acc2shard_file! load {} local accounts, take {} ms",
    loaded_accs, loaded_local_accs, dur
  );
  let _ = full_t.root().await;   
  full_t
}


// load initial act account state csv
pub async fn load_act_accs(
  shard_id: ShardId,
  actacc2shard_file: &str, 
  acc2shard: &Box<dyn Account2Shard + Send>, // get account's shardid according to acc2shard policy
) -> MMPTStore{

  let mut act_t = MMPTStore::new();
  info!("Begin loading act account...");
  println!("Begin loading act account...");
  let mut loaded_local_accs = 0;
  let mut loaded_accs = 0;
  let before_load = Instant::now();

  let mut reader = csv::Reader::from_path(actacc2shard_file).unwrap();
  let mut state_iter: DeserializeRecordsIter<File, ActAccToShardItem> = reader.deserialize().into_iter();
  while let Some(Ok(acc_shard)) = state_iter.next(){
    loaded_accs += 1;
    let addr = Vec::from_hex(&acc_shard.act_account[2..]).unwrap();
    let _shard_id = acc2shard.get_shard(&addr);
    if _shard_id == shard_id { // local account
      let acc = Account { nonce: 0, balance: INIT_BALANCE};
      let serialized = bincode::serialize(&acc).expect("Failed to serialize account");
      if act_t.insert(addr, serialized).await.unwrap(){
        loaded_local_accs+=1;
      }
    }
  }
  let dur = before_load.elapsed().as_millis();
  info!(
    "Total {} act accounts for initializing current epoch from actacc2shard_file! load {} local act accounts, take {} ms",
    loaded_accs, loaded_local_accs, dur
  );
  println!(
    "Total {} accounts for initializing current epoch from acc2shard_file! load {} local accounts, take {} ms",
    loaded_accs, loaded_local_accs, dur
  );
  let _ = act_t.root().await;   
  act_t
}


pub struct TStore {
  pub shard_id: ShardId,
  pub act_t: MMPTStore,
  pub full_t: MPTStore,
  pub insert_dur: Vec<u128>,
  pub get_act_dur: Vec<u128>,
  pub get_full_dur: Vec<u128>,
}


impl TStore {
  pub async fn new(
    shard_id: ShardId,
    acc2shard_file: &str,
    actacc2shard_file: &str,
    acc2shard: &Box<dyn Account2Shard + Send>,
    full_t_path: &str, 
  ) -> Self {

    info!("Initialize TStore!");
    let full_t = load_accs(shard_id, acc2shard_file, acc2shard, full_t_path).await;
    let act_t = load_act_accs(shard_id, actacc2shard_file, acc2shard).await;
    Self { 
      shard_id,
      act_t,
      full_t,
      insert_dur: Vec::default(),
      get_act_dur: Vec::default(),
      get_full_dur: Vec::default(),
    }
  }
}


#[async_trait]
impl StateStore for TStore {
  // insert the acc into active T
  async fn insert(&mut self, key: Vec<u8>, value: Vec<u8>) {
    let begin_insert = Instant::now();
    self.act_t.insert(key.clone(), value.clone()).await.unwrap();
    self.insert_dur.push(begin_insert.elapsed().as_micros());
  }

  async fn get(&mut self, key: &[u8]) -> Option<Vec<u8>> {
    let begin_get = Instant::now();
    //get acc from active T first
    let res = self.act_t.get(key).await.unwrap();
    if res.is_some() {
      self.get_act_dur.push(begin_get.elapsed().as_micros());
      return res;
    }
    // get acc from full_t
    let mut res_full_t = self.full_t.get(key).await.unwrap();
    match res_full_t {
      Some(value) => {
        let _ = self.act_t.insert(key.to_vec(), value.clone()).await.unwrap();
        res_full_t =  Some(value);
      },
      None => {
        res_full_t = None;
      }
    }
    self.get_full_dur.push(begin_get.elapsed().as_micros());
    res_full_t
  }

  async fn root(&mut self) -> (Option<RootHash>, Option<RootHash>){
    let root_hash_full = self.full_t.root().await.unwrap();
    let root_hash_act = self.act_t.root().await.unwrap();
    (Some(root_hash_act), Some(root_hash_full))
  }

  async fn test_migration(
    &mut self, 
    out_act_accs: Vec<Vec<u8>>,
    out_dor_accs: Vec<Vec<u8>>,
    epoch: u64,
    shard_id: ShardId,
    target_shard_id: ShardId,    
    act_root_hash: Vec<u8>,
    dor_root_hash: Vec<u8>,
    act_accs: Vec<Vec<u8>>, 
    dor_accs: Vec<Vec<u8>>,
  ) -> (usize, u128) {
    println!("[TStore_test_migration] out_act_accs: {:?}, out_dor_accs: {:?}", out_act_accs.len(), out_dor_accs);

    let before_test = Instant::now();

    let out_act_accs = Arc::new(out_act_accs);
    let out_dor_accs = Arc::new(out_dor_accs);

    let mut act_t_new = MMPTStore::new();

    let (act_tx, act_rx) = oneshot::channel();
    let (full_tx, full_rx) = oneshot::channel();
    let (delete_tx, delete_rx) = oneshot::channel();
    let (move_act_acc_tx, move_act_acc_rx) = oneshot::channel();

    // [6] get act accs' proof
    let mut act_t = self.act_t.clone();
    let out_act_accs_clone = out_act_accs.clone();
    let handle_act_proof = thread::spawn(move || {
      let before = Instant::now();
      let act_acc_proof = block_on(act_t.get_proof_batch(out_act_accs_clone.to_vec())).unwrap();
      println!("geting {} act accs' proof takes: {} ms", act_acc_proof.len(), before.elapsed().as_millis());
      let _ = act_tx.send(true);
      let _ = move_act_acc_tx.send(true);
      act_acc_proof
    });

    // [5] get dor accs' proof
    let mut full_t= self.full_t.clone();
    let out_dor_accs_clone = out_dor_accs.clone();
    let handle_dor_proof = thread::spawn(move || {
      let before = Instant::now();
      let dor_acc_proof = block_on(full_t.get_proof_batch(out_dor_accs_clone.to_vec())).unwrap();
      println!("geting dor accs' proof takes: {} ms", before.elapsed().as_millis());
      let _ = full_tx.send(true);
      let _ = delete_tx.send(true);
      dor_acc_proof
    });

    // [1] delete dor accs and old snapshot of act accs from full_t
    let mut full_t= self.full_t.clone();
    let out_act_accs_clone = out_act_accs.clone();
    let out_dor_accs_clone = out_dor_accs.clone();
    let handle_delete = thread::spawn(move || {
      block_on(delete_rx).expect("Failed to delete accs");
      let before = Instant::now();
      println!("delete dor accs and old snapshot of act accs from full_t");
      let _ = block_on(full_t.remove_batch(out_act_accs_clone.to_vec())).unwrap();
      let _ = block_on(full_t.remove_batch(out_dor_accs_clone.to_vec())).unwrap();
      println!("deleting dor accs and old snapshot of act accs from full_t takes {} ms", before.elapsed().as_millis());
    });

    // [2] clear the dormant accs    
    let mut act_t = self.act_t.clone();
    let mut full_t= self.full_t.clone();
    let handle_move_dor = thread::spawn(move || {
      block_on(act_rx).expect("Failed to delete accs");
      block_on(full_rx).expect("Failed to delete accs");
      let before = Instant::now();
      println!("begin to move {} dormant accs!", dor_accs.len());
      let (tx, mut rx): (Sender<(Key, Value)>, Receiver<(Key, Value)>) = mpsc::channel(100);
      // active t
      thread::spawn(move || {
        for addr in dor_accs {
          let value = block_on(act_t.get(&addr)).unwrap().unwrap();
            if let Err(_) = block_on(tx.send((addr, value))) {
              return;
          }
        }
      });
      // full T
      let mut cleared_dormant_accs = 0;
      while let Some((key, value)) = block_on(rx.recv()) {
          if block_on(full_t.insert(key, value)).unwrap(){
            cleared_dormant_accs += 1;
          }
      }
      println!("move total {} dormant accs! {} ms", cleared_dormant_accs, before.elapsed().as_millis());
    });

    // [3] move act accs to new act_t
    let mut act_t= self.act_t.clone();
    let mut act_t_new_clone = act_t_new.clone();
    let handle_move_act = thread::spawn(move || {
      block_on(move_act_acc_rx).expect("Failed to move acc accs");
      let before = Instant::now();
      println!("begin to move {} act accs!", act_accs.len());
      let (tx, mut rx): (Sender<(Key, Value)>, Receiver<(Key, Value)>) = mpsc::channel(100);
      // active t
      thread::spawn(move || {
        for addr in act_accs {
          let value = block_on(act_t.get(&addr)).unwrap().unwrap();
            if let Err(_) = block_on(tx.send((addr, value))) {
              return;
          }
        }
      });
      // full T
      let mut moved_act_accs = 0;
      while let Some((key, value)) = block_on(rx.recv()) {
          if block_on(act_t_new_clone.insert(key, value)).unwrap(){
            moved_act_accs += 1;
          }
      }
      println!("move total {} act accs! {} ms", moved_act_accs,  before.elapsed().as_millis());
    });

    let act_acc_proof = handle_act_proof.join().unwrap();
    let dor_acc_proof = handle_dor_proof.join().unwrap();

    // serialize 
    let before_s = Instant::now();
    let migration_msg = Migration::new(shard_id, target_shard_id, epoch, act_acc_proof, dor_acc_proof);
    let serialized = bincode::serialize(&migration_msg).expect("Failed to serialize account");
    let migration_data_size = serialized.len();
    println!("[TStore_test_migration] migration_data_size: {} B", migration_data_size);
    // deserialize
    let migration_msg : Migration = bincode::deserialize(&serialized).unwrap();   
    println!("serializing and deserializing takes {} ms", before_s.elapsed().as_millis());


    // [4] verify -> insert ingoing active accounts
    let mut invalid_act_accs: HashMap<Address, bool> = HashMap::default();
    let (ingoing_act_tx, mut ingoing_act_rx): (Sender<(Key, Value)>, Receiver<(Key, Value)>) = mpsc::channel(1000);
    let active_acc_proof_map = migration_msg.active_acc_proof_map;
    let handle_verify_act = thread::spawn(move || {
      for (addr, proof) in active_acc_proof_map {
        let res = MerklePatriciaTrie::verify_proof(act_root_hash.clone(), &addr, proof);
        match res {
          Ok(r) => {
            match r {
              Some(value) => { // valid proof
                block_on(ingoing_act_tx.send((addr, value))).expect("Failed to insert ingoing acc");
              }, 
              None => { // invalid
                invalid_act_accs.insert(addr, false);
              }            
            }
          },
          Err(_) => { // invalid
            invalid_act_accs.insert(addr, false);
          }    
        }
      } 
    });
    let handle_insert_ingoing_act = thread::spawn(move || {
      let mut inserted_act_accs = 0;
      while let Some((key, value)) = block_on(ingoing_act_rx.recv()) {
        let _ = block_on(act_t_new.insert(key, value)).unwrap();
        inserted_act_accs += 1;
      }
      println!("insert total {} ingoing act accs!", inserted_act_accs);
    });

    // [7] verify -> insert ingoing dormant accounts
    let mut invalid_dor_accs: HashMap<Address, bool> = HashMap::default();
    let (ingoing_dor_tx, mut ingoing_dor_rx): (Sender<(Key, Value)>, Receiver<(Key, Value)>) = mpsc::channel(1000);
    let dormant_acc_proof_map = migration_msg.dormant_acc_proof_map;
    let handle_verify_dor = thread::spawn(move || {
      for (addr, proof) in dormant_acc_proof_map {
        let res = MerklePatriciaTrie::verify_proof(dor_root_hash.clone(), &addr, proof);
        match res {
          Ok(r) => {
            match r {
              Some(value) => { // valid proof
                block_on(ingoing_dor_tx.send((addr, value))).expect("Failed to insert ingoing acc");
              }, 
              None => { // invalid
                invalid_dor_accs.insert(addr, false);
              }
            }
          },
          Err(_) => { // invalid
            invalid_dor_accs.insert(addr, false);
          }
        }
      }
    });
    let mut full_t = self.full_t.clone();
    let handle_insert_ingoing_dor = thread::spawn(move || {
      let mut inserted_dor_accs = 0;
      while let Some((key, value)) = block_on(ingoing_dor_rx.recv()) {
        let _ = block_on(full_t.insert(key, value)).unwrap();
        inserted_dor_accs += 1;
      }
      println!("insert total {} ingoing dor accs!", inserted_dor_accs);
    });

    handle_delete.join().unwrap();
    handle_move_dor.join().unwrap();
    handle_move_act.join().unwrap();

    handle_verify_act.join().unwrap();
    handle_insert_ingoing_act.join().unwrap();
    handle_verify_dor.join().unwrap();
    handle_insert_ingoing_dor.join().unwrap();
  
    let total_dur = before_test.elapsed().as_millis();
    println!("[TStore_test_migration] total_dur: {:?} ms", total_dur);

    (migration_data_size, total_dur)
  }

  async fn test(&self) -> (usize, usize, f64, f64) {

    let mut total_insert: u128 = 0;
    for get in &self.insert_dur {
      total_insert += *get;
    }
    let mean_insert = total_insert as f64 / self.insert_dur.len() as f64;
    println!("total insert: {} times, mean of insert_dur: {} us", self.insert_dur.len(),  mean_insert);

    let mut total_act_get: u128 = 0;
    for get in &self.get_act_dur {
      total_act_get += *get;
    }
    let mean_act_get = total_act_get as f64 / self.get_act_dur.len() as f64;
    println!("act get: {} times, mean of act_dur: {} us", self.get_act_dur.len(),  mean_act_get);

    let mut total_full_get: u128 = 0;
    for get in &self.get_full_dur {
      total_full_get += *get;
    }
    let mean_full_get = total_full_get as f64 / self.get_full_dur.len() as f64;
    println!("full get: {} times, mean of full_dur: {} us", self.get_full_dur.len(), mean_full_get);

    let total_get = self.get_act_dur.len() + self.get_full_dur.len();
    let mean_total_get = (total_act_get + total_full_get) as f64 / total_get as f64;
    println!("total get: {} times, mean of get_dur: {} us",total_get,  mean_total_get);

    (total_get, self.get_act_dur.len(), mean_insert, mean_total_get)
  }
}

pub struct MStore {
  pub shard_id: ShardId,
  pub full_t: MPTStore,
  pub insert_dur: Vec<u128>,
  pub get_dur: Vec<u128>,
}

impl MStore {
  pub async fn new(
    shard_id: ShardId,
    acc2shard_file: &str, 
    acc2shard: &Box<dyn Account2Shard + Send>,
    full_t_path: &str, 
  ) -> Self {

    info!("Initialize MStore!");
    let full_t = load_accs(shard_id, acc2shard_file, acc2shard, full_t_path).await;

    Self { 
      shard_id,
      full_t,
      insert_dur: Vec::default(),
      get_dur: Vec::default(),
    }
  }
}

#[async_trait]
impl StateStore for MStore {
  // insert the acc into full T
  async fn insert(&mut self, key: Vec<u8>, value: Vec<u8>) {
    let begin_insert = Instant::now();
    self.full_t.insert(key, value).await.unwrap();
    self.insert_dur.push(begin_insert.elapsed().as_micros());
  }

  async fn get(&mut self, key: &[u8]) -> Option<Vec<u8>> {
    let begin_get = Instant::now();
    let value = self.full_t.get(key).await.unwrap();
    self.get_dur.push(begin_get.elapsed().as_micros());
    value
  }

  async fn root(&mut self) -> (Option<RootHash>, Option<RootHash>){
    let root_hash_full = self.full_t.root().await.unwrap();
    (Some(Vec::default()), Some(root_hash_full))
  }

  async fn test_migration(
    &mut self, 
    out_act_accs: Vec<Vec<u8>>,
    out_dor_accs: Vec<Vec<u8>>,
    epoch: u64,
    shard_id: ShardId,
    target_shard_id: ShardId,
    _act_root_hash: Vec<u8>,
    full_root_hash: Vec<u8>,  
    _act_accs: Vec<Vec<u8>>, 
    _dor_accs: Vec<Vec<u8>>,
  ) -> (usize, u128) {

    println!("[MStore_test_migration] out_act_accs: {:?}, out_dor_accs: {:?}", out_act_accs.len(), out_dor_accs);
    let before_test = Instant::now();

    let out_act_accs = Arc::new(out_act_accs);
    let out_dor_accs = Arc::new(out_dor_accs);

    // outgoing
    // 1. get proof
    let act_acc_proof = self.full_t.get_proof_batch(out_act_accs.to_vec()).await.unwrap();
    let dor_acc_proof = self.full_t.get_proof_batch(out_dor_accs.to_vec()).await.unwrap();


    // 2. remove act acc and dor acc
    let mut full_t_clone = self.full_t.clone(); 
    let out_act_accs_clone = out_act_accs.clone();
    let out_dor_accs_clone = out_dor_accs.clone();
    let handle_remove = thread::spawn(move || {
      let before = Instant::now();
      let removed_act_acc_num = block_on(full_t_clone.remove_batch(out_act_accs_clone.to_vec())).unwrap();
      let removed_dor_acc_num = block_on(full_t_clone.remove_batch(out_dor_accs_clone.to_vec())).unwrap();
      println!("removing outgoing accs takes {} ms", before.elapsed().as_millis());
      (removed_act_acc_num, removed_dor_acc_num)
    });

    // 3. serialize 
    let before_3 = Instant::now();
    let migration_msg = Migration::new(shard_id, target_shard_id, epoch, act_acc_proof, dor_acc_proof);
    let serialized = bincode::serialize(&migration_msg).expect("Failed to serialize account");

    let migration_data_size = serialized.len();
    println!("[MStore_test_migration] migration_data_size: {} B", migration_data_size);
    // 4. deserialize
    let migration_msg : Migration = bincode::deserialize(&serialized).unwrap();   
    println!("serializing and deserializing takes {} ms", before_3.elapsed().as_millis());

    // verify -> insert pipeline
    let mut invalid_accs: HashMap<Address, bool> = HashMap::default();
    let (tx, mut rx) = channel(1000);
    // verify proof
    let handle_verify = thread::spawn(move || {
      for (addr, proof) in migration_msg.active_acc_proof_map {
        let res = MerklePatriciaTrie::verify_proof(full_root_hash.clone(), &addr, proof);
        match res {
          Ok(r) => {
            match r {
              Some(value) => { // valid proof
                block_on(tx.send((addr, value))).expect("Failed to insert ingoing acc");
              }, 
              None => { // invalid
                invalid_accs.insert(addr, false);
              }            
            }
          },
          Err(_) => { // invalid
            invalid_accs.insert(addr, false);
          }    
        }
      }    
      for (addr, proof) in migration_msg.dormant_acc_proof_map {
        let res = MerklePatriciaTrie::verify_proof(full_root_hash.clone(), &addr, proof);
        match res {
          Ok(r) => {
            match r {
              Some(value) => { // valid proof
                block_on(tx.send((addr, value))).expect("Failed to insert ingoing acc");
              }, 
              None => { // invalid
                invalid_accs.insert(addr, false);
              }            
            }
          },
          Err(_) => { // invalid
            invalid_accs.insert(addr, false);
          }    
        }
      }
      println!("verify finished!");
    });

    // insert
    while let Some((key, value)) = block_on(rx.recv()) {
      self.full_t.insert(key, value).await.unwrap();
    }
    println!("insert finished!");

    let (_, _) = handle_remove.join().unwrap();
    handle_verify.join().unwrap();

    let total_dur = before_test.elapsed().as_millis();
    println!("[MStore_test_migration] total_dur: {:?} ms", total_dur);

    (migration_data_size, total_dur)
  }

  async fn test(&self) -> (usize, usize, f64, f64) {
    let mut total_insert: u128 = 0;
    for get in &self.insert_dur {
      total_insert += *get;
    }
    let mean_insert = total_insert as f64 / self.insert_dur.len() as f64;
    println!("total insert: {} times, mean of insert_dur: {} us", self.insert_dur.len(),  mean_insert);

    let mut total_get: u128 = 0;
    for get in &self.get_dur {
      total_get += *get;
    }
    let mean_total_get = total_get as f64 / self.get_dur.len() as f64;
    println!("total get: {} times, mean of get_dur: {} us", self.get_dur.len(),  mean_total_get);

    (self.get_dur.len(), 0, mean_insert, mean_total_get)
  }  
}

pub fn _assemle_rwset(avatars: HashMap<Address, Amount>) -> Vec<RWSet> {
  let mut rwset: Vec<RWSet> = Vec::new();
  for (addr, value) in avatars {
    rwset.push(RWSet{addr, value});
  }
  rwset    
}




#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct Migration {
    pub source_shard_id: ShardId,
    pub target_shard_id: ShardId,
    pub epoch: u64,
    pub active_acc_proof_map : HashMap<Address, Proof>,
    pub dormant_acc_proof_map : HashMap<Address, Proof>,
}

impl Migration {
  pub fn new(
    source_shard_id: ShardId, target_shard_id: ShardId, epoch: u64, 
    active_acc_proof_map: HashMap<Address, Proof>, dormant_acc_proof_map : HashMap<Address, Proof>
  ) -> Self {
    Self { 
      source_shard_id, 
      target_shard_id, 
      epoch, 
      active_acc_proof_map,
      dormant_acc_proof_map
    }
  }
}