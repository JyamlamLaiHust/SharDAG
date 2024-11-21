use log::info;
use num_enum::TryFromPrimitive;
use std::{collections::HashMap, fs::File};
use csv::DeserializeRecordsIter;
use serde::{Deserialize, Serialize};
use hex::FromHex;

use config::{Import, ShardId};
use crate::{Address};



#[derive(TryFromPrimitive, Debug)]
#[repr(usize)]
pub enum Account2ShardType {
  HashPolicy,
  GraphPolicy,
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct AccToShardItem {
  pub account: String,
  pub shard: ShardId,
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct ActAccToShardItem {
  pub act_account: String,
  pub shard: ShardId,
}

pub trait Account2Shard {
  fn get_shard_num(&self) -> ShardId;
  fn get_shard(&self, addr: &Address) -> ShardId;
}


#[derive(Clone, Deserialize, Debug, Default)]
pub struct  Account2ShardHash {
    pub shard_num: ShardId,
}

impl Import for Account2ShardHash {}

impl Account2ShardHash {
    pub fn new(shard_num: ShardId) -> Self {
      info!("Initialize Account2ShardHash...");
      info!("Done...");
      Self{
        shard_num,
      }
    }
}


impl Account2Shard for Account2ShardHash {
  fn get_shard_num(&self) -> ShardId {
    self.shard_num
  }

  fn get_shard(&self, addr: &Address) -> ShardId{
    // *self.account_shard.get(&acc).unwrap()
    // let shardid = u32::from_str_radix(&acc[34..], 16).unwrap() % self.shard_num;
    let shardid = addr.last().unwrap() % self.shard_num as u8;
    shardid as ShardId
  } 
}


#[derive(Clone, Deserialize, Debug, Default)]
pub struct  Account2ShardGraph {
    pub shard_num: ShardId,
    pub acc2shard: HashMap<Address, ShardId>,
}

impl Import for Account2ShardGraph {}

impl Account2ShardGraph {
    pub fn new(shard_num: ShardId, acc2shard_file: &str) -> Self {
      info!("Initialize Account2ShardGraph...");
      let mut acc2shard: HashMap<Address, ShardId> = HashMap::new();
      // load acc2shard map from acc2shard_file
      let mut loaded_accs = 0;

      let mut reader = csv::Reader::from_path(acc2shard_file).unwrap();
      let mut state_iter: DeserializeRecordsIter<File, AccToShardItem> = reader.deserialize().into_iter();
      while let Some(Ok(acc2shard_item)) = state_iter.next(){
        let addr = Vec::from_hex(&acc2shard_item.account[2..]).unwrap();
        let shardid = acc2shard_item.shard;
        acc2shard.insert(addr, shardid);
        loaded_accs += 1;
      }
      info!(
        "Load total {} accounts for initializing current epoch from acc2shard_file!",
        loaded_accs
      );

      Self{
        shard_num,
        acc2shard
      }
    }
}


impl Account2Shard for Account2ShardGraph {
  fn get_shard_num(&self) -> ShardId {
    self.shard_num
  }

  fn get_shard(&self, addr: &Address) -> ShardId{
    let res = self.acc2shard.get(addr);
    match res {
      Some(shardid) => {
        *shardid
      }
      None => {// the first-appearing account
        let shardid = addr.last().unwrap() % self.shard_num as u8;
        shardid as ShardId    
      }
    }
  }
}
