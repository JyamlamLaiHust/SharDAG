
use std::collections::HashMap;
use std::sync::Arc;
use worker::{CoreTx, Transaction, Frame, RWSet, Account2Shard};
// use crate::messages::CoreTx;
// use crate::{RWSet, Account2Shard};
// use crate::{Transaction, Frame};
use config::ShardId;
use std::time::{SystemTime, UNIX_EPOCH};


// convert rawTx to TransferTx
pub struct ConvertTx {}



impl ConvertTx {
  pub fn new() -> Self {
    Self {}
  }

  pub fn get_payload(&self, acc_shard: Arc<dyn Account2Shard + Send>, payload: Vec<RWSet>) -> (Vec<Frame>, usize) {

    let shard_num = acc_shard.get_shard_num();
    let mut shard_rwset_map_sub: HashMap<ShardId, Vec<RWSet>> = HashMap::new();
    let mut shard_rwset_map_add: HashMap<ShardId, Vec<RWSet>> = HashMap::new();

    let mut flag_sub = vec![false;shard_num as usize];
    let mut flag_add = vec![false;shard_num as usize];


    for rwset in payload {
      let shardid  = acc_shard.get_shard(&rwset.addr) as usize;
      if rwset.value < 0.0 {
        let rwset_list = shard_rwset_map_sub.entry(shardid).or_insert(Vec::default());
        rwset_list.push(rwset);
        flag_sub[shardid] = true;
      } else {
        let rwset_list = shard_rwset_map_add.entry(shardid).or_insert(Vec::default());
        rwset_list.push(rwset);
        flag_add[shardid] = true;
      }
    }

    let mut involved_shar_num = 0;
    for i in 0..=shard_num-1 {
      if flag_sub[i] || flag_add[i] {
        involved_shar_num += 1;
      }
    }


    // merge
    // find the shard involving both add and sub op
    let mut sp_shard_id = usize::MAX;
    for i in 0..=shard_num-1 {
      if flag_sub[i] && flag_add[i] {
        sp_shard_id = i;
        break;
      }
    }

    let mut sp_rwset: Vec<RWSet> = Vec::new();
    if sp_shard_id != usize::MAX {  
      sp_rwset = shard_rwset_map_sub.remove(&sp_shard_id).unwrap();
      let tmp_add_reset = shard_rwset_map_add.remove(&(sp_shard_id)).unwrap();
      for rwset in tmp_add_reset {
        sp_rwset.push(rwset);
      }
    }

    let mut payload: Vec<Frame> = Vec::new();
    // sub rwset
    for (shardid, rwset_list) in shard_rwset_map_sub {
      payload.push(Frame { shardid, rwset: rwset_list });
    }

    if sp_shard_id != usize::MAX {
      payload.push(Frame { shardid: sp_shard_id, rwset: sp_rwset });
    }

    // add rwset
    for (shardid, rwset_list) in shard_rwset_map_add {
      payload.push(Frame { shardid, rwset: rwset_list });
    }

    (payload, involved_shar_num)
  } 

  pub fn rawtx2tx(&self, acc_shard: Arc<dyn Account2Shard + Send>, raw_tx: CoreTx) -> (Transaction, ShardId) {
    let payload_len = raw_tx.payload.len();
    let (payload, involved_shard_num)= self.get_payload(acc_shard, raw_tx.payload);
    let first_shard = payload.get(0).unwrap().shardid;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros();


    let tx = Transaction::new(raw_tx.sender, raw_tx.receiver, raw_tx.amount, payload, payload_len, involved_shard_num,  raw_tx.sample, raw_tx.counter, timestamp, None, None);

    // info!(" {}, {}, ", tx.sender, shardid_sender);
    (tx,first_shard)
  }
}



