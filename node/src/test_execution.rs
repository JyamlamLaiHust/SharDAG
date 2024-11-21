use client::rawtx2tx;
use config::ShardId;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::fs::{File, self};
use worker::{StateStoreType, TStore, MStore, StateStore, Account2Shard, Account2ShardGraph, RawTxOld, RWSet, Frame, StateTransition};
use csv::DeserializeRecordsIter;
use tokio::time::Instant;

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
struct ExectutionRes {
  pub method: String,
  pub epoch: u64,
  pub shard_num: ShardId,
  pub shard_id: ShardId,
  pub total_accs: u64,

  pub total_access: usize,
  pub total_act_hit: usize,
  pub hit_ratio: f64,

  pub mean_insert_dur_ns: f64,
  pub mean_get_dur_ns: f64,

  pub exec_dur_ms: u128,
}


#[tokio::main]
async fn main(){

  println!("Run main() in h_store_main.rs");

  let res_path = "../test-state-store/res-exec.csv";
  let mut wtr = csv::Writer::from_path(res_path).unwrap();

  let methods = vec![String::from("MStore"), String::from("TStore")];
  let test_method: usize = 1; // 0: MStore; 1: TStore
  let state_store_type = StateStoreType::try_from(test_method).unwrap();  

  // initial state store
  let shard_num = 8;
  let shard_id = 0;
  let epoch = 27;
  // test epoch：0, 6, 12, 18, 24, 29


  println!("shard_num: {}, epoch: {}", shard_num, epoch);

  let full_t_path = "test_db_full_t";
  
  let _ = fs::remove_dir_all(full_t_path);

  let acc2shard_file = format!("/root/SharDAG-WorkSpace/inputv2/acc2shard-e{}-s8.csv", epoch);
  println!("acc2shard file {}", acc2shard_file);
  let workload_file = format!("/root/SharDAG-WorkSpace/inputv2/input-e{}.csv", epoch);
  println!("workload file: {:?}", workload_file);
  let actacc2shard_file = format!("/root/SharDAG-WorkSpace/inputv2/act-acc2shard-e{}-s8.csv", epoch);
  println!("actacc2shard file {}", actacc2shard_file);

  
  println!("Account2Shard: GraphPolicy");
  let acc2shard: Box<dyn Account2Shard + Send> = Box::new(Account2ShardGraph::new(shard_num, &acc2shard_file));

  // create state store
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
  let mut state_transition = StateTransition::new(store);

  // execute txs
  let mut reader = csv::Reader::from_path(workload_file.clone()).unwrap();
  let mut workload_iter: DeserializeRecordsIter<File, RawTxOld> = reader.deserialize().into_iter();

  println!("Start sending transactions");
  let mut executed_txs = 0;
  let begin = Instant::now();

  while let Some(Ok(raw_tx_old)) = workload_iter.next() {
    let tx_sample: u8 = 0;
    let tx_counter: u64 = 0;
    let core_tx = rawtx2tx(raw_tx_old, tx_sample, tx_counter);

    let mut tx_rwset: Vec<RWSet> = Vec::default();
    for rwset in core_tx.payload {
      let _shard_id = acc2shard.get_shard(&rwset.addr);
      if _shard_id == shard_id {
        tx_rwset.push(rwset);
      }
    }
    if !tx_rwset.is_empty() { // local txs
      let frame = Frame{shardid: shard_id, rwset: tx_rwset};
      let mut latest_states = state_transition.get_latest_states(&frame.rwset).await;

      // balance check
      let mut pass_check = true;
      for rwset in &frame.rwset {
        let acc = latest_states.get_mut(&rwset.addr).unwrap();
        acc.balance += rwset.value;
        if rwset.value < 0.0 { // deduction
          if acc.balance < 0.0 { // the balance is negative after execution
            pass_check = false;
            break;
          }
          acc.nonce +=1;          
        }
      }
      if pass_check { // apply new states
        state_transition.apply_new_states(latest_states).await;
      }
      executed_txs += 1;
      if executed_txs % 1000 == 0 { 
        let _ = state_transition.store.root().await;
      }
    }
  }

  let total_dur = begin.elapsed().as_millis();

  println!("executed txs: {}", executed_txs);
  let (total, act_hit, mean_insert, mean_get) = state_transition.store.test().await;

  let hit_ratio = act_hit as f64 / total as f64;
  println!("total_access: {} times, act_hit {} times, hit_ratio: {}!", total, act_hit, hit_ratio);

  println!("finished {} ms", total_dur);
  let new_res = ExectutionRes{
    method: methods[test_method].clone(),
    epoch,
    shard_num,
    shard_id,
    total_accs: 0,
    total_access: total,
    total_act_hit: act_hit,
    hit_ratio,
    mean_insert_dur_ns: mean_insert,
    mean_get_dur_ns: mean_get,
    exec_dur_ms: total_dur,
  };

  wtr.serialize(new_res).unwrap();      
  wtr.flush().unwrap();
}